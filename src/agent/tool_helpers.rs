use crate::agent_impl_guard::normalize_tool_path;
use crate::agent_taint_helpers::compute_taint_spans_for_tool;
use crate::agent_tool_exec::{run_tool_once, tool_result_has_error};
use crate::agent_utils::sha256_hex;
use crate::events::EventKind;
use crate::hooks::protocol::{HookInvocationReport, ToolResultPayload};
use crate::hooks::runner::make_tool_result_input;
use crate::providers::ModelProvider;
use crate::agent_utils::provider_name;
use crate::tools::ToolErrorCode;
use crate::types::{Message, Role, ToolCall};

use super::Agent;
use super::INTERNAL_ENFORCE_IMPLEMENTATION_GUARD_FLAG;

pub(super) fn is_repairable_error_code(code: ToolErrorCode) -> bool {
    matches!(
        code,
        ToolErrorCode::ToolArgsInvalid
            | ToolErrorCode::ToolUnknown
            | ToolErrorCode::ToolArgsMalformedJson
            | ToolErrorCode::ToolPathDenied
    )
}

pub(super) fn failed_repeat_key(tc: &ToolCall) -> String {
    let canonical_args =
        crate::trust::approvals::canonical_json(&tc.arguments).unwrap_or_else(|_| "null".to_string());
    sha256_hex(format!("{}|{canonical_args}", tc.name).as_bytes())
}

pub(super) fn normalized_tool_path_from_args(tc: &ToolCall) -> Option<String> {
    tc.arguments
        .get("path")
        .and_then(|v| v.as_str())
        .map(normalize_tool_path)
}

pub(super) fn injected_messages_enforce_implementation_integrity_guard(messages: &[Message]) -> bool {
    messages.iter().any(|m| {
        matches!(m.role, Role::System | Role::Developer)
            && m.content
                .as_deref()
                .is_some_and(|c| c.trim() == INTERNAL_ENFORCE_IMPLEMENTATION_GUARD_FLAG)
    })
}

pub(super) struct ToolResultHookState {
    pub tool_msg: Message,
    pub input_digest: String,
    pub output_digest: String,
    pub input_len: usize,
    pub output_len: usize,
    pub final_truncated: bool,
}

pub(super) enum SchemaRepairDecision {
    RestartAgentStep,
    Exhausted,
}

pub(super) enum RetryLoopDecision {
    Break,
    ContinueWithToolMsg(Message, u32),
    Finalize(super::agent_types::AgentOutcome),
}

impl<P: ModelProvider> Agent<P> {
    pub(super) async fn run_tool_with_timeout_and_emit_mcp_events(
        &mut self,
        run_id: &str,
        step: u32,
        tc: &ToolCall,
        phase: &str,
    ) -> Message {
        let tool_exec_timeout_ms = self.effective_tool_exec_timeout_ms();
        let outcome = match tokio::time::timeout(
            std::time::Duration::from_millis(tool_exec_timeout_ms),
            run_tool_once(&self.tool_rt, tc, self.mcp_registry.as_ref()),
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(_) => {
                let reason = format!(
                    "runtime tool execution timeout: '{}' exceeded {}ms",
                    tc.name, tool_exec_timeout_ms
                );
                self.emit_event(
                    run_id,
                    step,
                    EventKind::Error,
                    serde_json::json!({
                        "error": reason,
                        "source": "runtime_tool_timeout",
                        "tool_call_id": tc.id,
                        "name": tc.name,
                        "timeout_ms": tool_exec_timeout_ms
                    }),
                );
                return self.tool_timeout_message(tc, tool_exec_timeout_ms);
            }
        };
        if let Some(meta) = outcome.mcp_meta {
            if meta.progress_ticks > 0 {
                self.emit_event(
                    run_id,
                    step,
                    EventKind::McpProgress,
                    serde_json::json!({
                        "tool_call_id": tc.id,
                        "name": tc.name,
                        "progress_ticks": meta.progress_ticks,
                        "elapsed_ms": meta.elapsed_ms,
                        "phase": phase
                    }),
                );
            }
            if meta.cancelled {
                self.emit_event(
                    run_id,
                    step,
                    EventKind::McpCancelled,
                    serde_json::json!({
                        "tool_call_id": tc.id,
                        "name": tc.name,
                        "reason": "timeout",
                        "elapsed_ms": meta.elapsed_ms
                    }),
                );
            }
        }
        outcome.message
    }

    pub(super) async fn apply_tool_result_hooks(
        &mut self,
        run_id: &str,
        step: u32,
        tc: &ToolCall,
        tool_msg: Message,
        hook_invocations: &mut Vec<HookInvocationReport>,
    ) -> Result<ToolResultHookState, String> {
        let original_content = tool_msg.content.clone().unwrap_or_default();
        let mut state = ToolResultHookState {
            tool_msg,
            input_digest: sha256_hex(original_content.as_bytes()),
            output_digest: sha256_hex(original_content.as_bytes()),
            input_len: original_content.chars().count(),
            output_len: original_content.chars().count(),
            final_truncated: crate::agent_tool_exec::infer_truncated_flag(&original_content),
        };

        if !self.hooks.enabled() {
            return Ok(state);
        }

        let payload = ToolResultPayload {
            tool_call_id: tc.id.clone(),
            tool_name: tc.name.clone(),
            ok: !tool_result_has_error(&original_content),
            content: original_content.clone(),
            truncated: state.final_truncated,
        };
        let hook_input = make_tool_result_input(
            run_id,
            step,
            provider_name(self.gate_ctx.provider),
            &self.model,
            &self.gate_ctx.workdir,
            match serde_json::to_value(payload) {
                Ok(v) => v,
                Err(e) => {
                    self.emit_event(
                        run_id,
                        step,
                        EventKind::HookError,
                        serde_json::json!({"stage":"tool_result","error": e.to_string()}),
                    );
                    return Err(format!("failed to encode tool_result hook payload: {e}"));
                }
            },
        );
        match self
            .hooks
            .run_tool_result_hooks(
                hook_input,
                &tc.name,
                &original_content,
                state.final_truncated,
            )
            .await
        {
            Ok(hook_out) => {
                for inv in &hook_out.invocations {
                    self.emit_event(
                        run_id,
                        step,
                        EventKind::HookStart,
                        serde_json::json!({
                            "hook_name": inv.hook_name,
                            "stage": inv.stage
                        }),
                    );
                    self.emit_event(
                        run_id,
                        step,
                        EventKind::HookEnd,
                        serde_json::json!({
                            "hook_name": inv.hook_name,
                            "stage": inv.stage,
                            "action": inv.action,
                            "modified": inv.modified,
                            "duration_ms": inv.duration_ms,
                            "input_digest": inv.input_digest,
                            "output_digest": inv.output_digest
                        }),
                    );
                }
                hook_invocations.extend(hook_out.invocations);
                if let Some(reason) = hook_out.abort_reason {
                    return Err(reason);
                }
                state.tool_msg.content = Some(hook_out.content);
                state.final_truncated = hook_out.truncated;
                state.input_digest = hook_out.input_digest;
                state.output_digest = hook_out.output_digest;
                state.input_len = hook_out.input_len;
                state.output_len = hook_out.output_len;
                Ok(state)
            }
            Err(e) => {
                self.emit_event(
                    run_id,
                    step,
                    EventKind::HookError,
                    serde_json::json!({"stage":"tool_result","error": e.message}),
                );
                Err(e.message)
            }
        }
    }

    pub(super) fn update_taint_for_tool_result(
        &mut self,
        run_id: &str,
        step: u32,
        tc: &ToolCall,
        content: &str,
        tool_message_index: usize,
        taint_state: &mut crate::taint::TaintState,
    ) {
        if !matches!(self.taint_toggle, crate::taint::TaintToggle::On) {
            return;
        }
        let spans = compute_taint_spans_for_tool(
            tc,
            content,
            self.policy_for_taint.as_ref(),
            self.taint_digest_bytes,
        );
        if spans.is_empty() {
            return;
        }
        taint_state.add_tool_spans(&tc.id, tool_message_index, spans.clone());
        self.emit_event(
            run_id,
            step,
            EventKind::TaintUpdated,
            serde_json::json!({
                "overall": taint_state.overall_str(),
                "new_spans": spans.len(),
                "sources": taint_state.sources_count_for_last_update()
            }),
        );
    }

    pub(super) async fn verify_post_write_path(
        &mut self,
        run_id: &str,
        step: u32,
        path: &str,
        post_write_verify_timeout_ms: u64,
    ) -> Result<crate::agent_impl_guard::ToolExecutionRecord, String> {
        self.emit_event(
            run_id,
            step,
            EventKind::PostWriteVerifyStart,
            serde_json::json!({
                "name": "read_file",
                "path": path,
                "source": "runtime_post_write_verify",
                "timeout_ms": post_write_verify_timeout_ms
            }),
        );
        let verify_started = std::time::Instant::now();
        let verify = match tokio::time::timeout(
            std::time::Duration::from_millis(post_write_verify_timeout_ms),
            self.tool_rt.exec_target.read_file(crate::target::ReadReq {
                workdir: self.tool_rt.workdir.clone(),
                path: path.to_string(),
                max_read_bytes: self.tool_rt.max_read_bytes,
            }),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                self.emit_event(
                    run_id,
                    step,
                    EventKind::PostWriteVerifyEnd,
                    serde_json::json!({
                        "name": "read_file",
                        "path": path,
                        "ok": false,
                        "status": "timeout",
                        "source": "runtime_post_write_verify",
                        "failure_class": "E_RUNTIME_POST_WRITE_VERIFY_TIMEOUT",
                        "elapsed_ms": verify_started.elapsed().as_millis() as u64,
                        "timeout_ms": post_write_verify_timeout_ms
                    }),
                );
                return Err(format!(
                    "implementation guard: runtime post-write verification timed out on read_file for '{path}' after {}ms",
                    post_write_verify_timeout_ms
                ));
            }
        };
        self.emit_event(
            run_id,
            step,
            EventKind::PostWriteVerifyEnd,
            serde_json::json!({
                "name": "read_file",
                "path": path,
                "ok": verify.ok,
                "status": if verify.ok { "ok" } else { "failed" },
                "source": "runtime_post_write_verify",
                "failure_class": if verify.ok { serde_json::Value::Null } else { serde_json::Value::String("E_RUNTIME_POST_WRITE_VERIFY_FAILED".to_string()) },
                "elapsed_ms": verify_started.elapsed().as_millis() as u64
            }),
        );
        if !verify.ok {
            return Err(format!(
                "implementation guard: runtime post-write verification failed read_file on '{path}': {}",
                verify.content
            ));
        }
        Ok(crate::agent_impl_guard::ToolExecutionRecord {
            name: "read_file".to_string(),
            path: Some(path.to_string()),
            ok: true,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_schema_repair_attempt(
        &mut self,
        run_id: &str,
        step: u32,
        tc: &ToolCall,
        error_code: ToolErrorCode,
        tool_retry_count: u32,
        current_content: &str,
        tool_msg: &Message,
        repeat_key: &str,
        messages: &mut Vec<Message>,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        schema_repair_attempts: &mut std::collections::BTreeMap<String, u32>,
    ) -> SchemaRepairDecision {
        let repair_key = format!("{}|{}", tc.name, error_code.as_str());
        let attempts = schema_repair_attempts
            .entry(repair_key)
            .and_modify(|n| *n = n.saturating_add(1))
            .or_insert(1);
        if *attempts <= super::MAX_SCHEMA_REPAIR_ATTEMPTS {
            self.emit_tool_retry_event(
                run_id,
                step,
                tc,
                *attempts,
                super::MAX_SCHEMA_REPAIR_ATTEMPTS,
                "E_SCHEMA",
                "repair",
                Some(error_code.as_str()),
            );
            self.emit_event(
                run_id,
                step,
                EventKind::ToolExecEnd,
                serde_json::json!({
                    "tool_call_id": tc.id,
                    "name": tc.name,
                    "ok": false,
                    "truncated": crate::agent_tool_exec::infer_truncated_flag(current_content),
                    "retry_count": tool_retry_count,
                    "failure_class": "E_SCHEMA",
                    "error_code": error_code.as_str()
                }),
            );
            messages.push(tool_msg.clone());
            if self.inject_post_tool_operator_messages(run_id, step, messages) {
                return SchemaRepairDecision::RestartAgentStep;
            }
            let n = failed_repeat_counts
                .entry(repeat_key.to_string())
                .or_insert(0);
            *n = n.saturating_add(1);
            messages.push(crate::agent_tool_exec::schema_repair_instruction_message(
                tc,
                error_code.as_str(),
            ));
            return SchemaRepairDecision::RestartAgentStep;
        }
        self.emit_tool_retry_event(
            run_id,
            step,
            tc,
            *attempts,
            super::MAX_SCHEMA_REPAIR_ATTEMPTS,
            "E_SCHEMA",
            "stop",
            Some(error_code.as_str()),
        );
        self.emit_schema_repair_exhausted_event(run_id, step, tc, *attempts);
        SchemaRepairDecision::Exhausted
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_generic_retry_iteration(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        current_content: &str,
        side_effects: crate::types::SideEffects,
        tool_retry_count: u32,
        tool_budget_usage: &mut crate::agent_budget::ToolCallBudgetUsage,
        approval_mode_meta: Option<String>,
        auto_scope_meta: Option<String>,
        approval_key_version_meta: Option<String>,
        tool_schema_hash_hex: Option<String>,
        hooks_config_hash_hex: Option<String>,
        planner_hash_hex: Option<String>,
        decision_exec_target: Option<String>,
        started_at: String,
        messages: Vec<Message>,
        observed_tool_calls: Vec<ToolCall>,
        observed_tool_decisions: Vec<super::ToolDecisionRecord>,
        request_context_chars: usize,
        last_compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &crate::types::TokenUsage,
        taint_state: &crate::taint::TaintState,
    ) -> RetryLoopDecision {
        let class = crate::agent_tool_exec::classify_tool_failure(tc, current_content, false);
        let retry_error_code =
            crate::agent_tool_exec::tool_result_error_code(current_content).map(|c| c.as_str());
        let max_retries = class.retry_limit_for(side_effects);
        if tool_retry_count >= max_retries {
            self.emit_tool_retry_event(
                &run_id,
                step,
                tc,
                tool_retry_count,
                max_retries,
                class.as_str(),
                "stop",
                retry_error_code,
            );
            return RetryLoopDecision::Break;
        }
        let next_retry_count = tool_retry_count.saturating_add(1);
        self.emit_tool_retry_event(
            &run_id,
            step,
            tc,
            next_retry_count,
            max_retries,
            class.as_str(),
            "retry",
            retry_error_code,
        );
        if let Some(reason) = crate::agent_budget::check_and_consume_tool_budget(
            &self.tool_call_budget,
            tool_budget_usage,
            side_effects,
        ) {
            self.emit_event(
                &run_id,
                step,
                EventKind::ToolDecision,
                serde_json::json!({
                    "tool_call_id": tc.id,
                    "name": tc.name,
                    "decision": "deny",
                    "reason": reason.clone(),
                    "source": "runtime_budget",
                    "side_effects": side_effects
                }),
            );
            return RetryLoopDecision::Finalize(self.finalize_runtime_budget_deny_with_end(
                run_id,
                step,
                tc,
                reason,
                approval_mode_meta,
                auto_scope_meta,
                approval_key_version_meta,
                tool_schema_hash_hex,
                hooks_config_hash_hex,
                planner_hash_hex,
                decision_exec_target,
                started_at,
                messages,
                observed_tool_calls,
                observed_tool_decisions,
                request_context_chars,
                last_compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
            ));
        }
        if let Some(reason) = crate::agent_budget::check_and_consume_mcp_budget(
            &self.tool_call_budget,
            tool_budget_usage,
            tc.name.starts_with("mcp."),
        ) {
            return RetryLoopDecision::Finalize(
                self.finalize_runtime_mcp_budget_exceeded_with_error(
                    run_id,
                    step,
                    reason,
                    started_at,
                    messages,
                    observed_tool_calls,
                    observed_tool_decisions,
                    request_context_chars,
                    last_compaction_report,
                    hook_invocations,
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                ),
            );
        }
        let tool_msg = self
            .run_tool_with_timeout_and_emit_mcp_events(&run_id, step, tc, "retry_await_result")
            .await;
        RetryLoopDecision::ContinueWithToolMsg(tool_msg, next_retry_count)
    }
}
