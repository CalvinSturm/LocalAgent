use crate::agent_impl_guard::normalize_tool_path;
use crate::agent_taint_helpers::compute_taint_spans_for_tool;
use crate::agent_tool_exec::{run_tool_once, tool_result_has_error};
use crate::agent_utils::provider_name;
use crate::agent_utils::sha256_hex;
use crate::events::EventKind;
use crate::hooks::protocol::{HookInvocationReport, ToolResultPayload};
use crate::hooks::runner::make_tool_result_input;
use crate::providers::ModelProvider;
use crate::tools::ToolErrorCode;
use crate::types::{Message, Role, ToolCall};

use super::run_events::ToolRetryEvent;
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
    let canonical_args = crate::trust::approvals::canonical_json(&tc.arguments)
        .unwrap_or_else(|_| "null".to_string());
    sha256_hex(format!("{}|{canonical_args}", tc.name).as_bytes())
}

pub(super) fn normalized_tool_path_from_args(tc: &ToolCall) -> Option<String> {
    tc.arguments
        .get("path")
        .and_then(|v| v.as_str())
        .map(normalize_tool_path)
}

pub(super) fn injected_messages_enforce_implementation_integrity_guard(
    messages: &[Message],
) -> bool {
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

pub(super) enum InvalidPatchFormatDecision {
    Continue,
    RestartAgentStep,
    Finalize(Box<super::agent_types::AgentOutcome>),
}

pub(super) enum RetryLoopDecision {
    Break,
    ContinueWithToolMsg(Message, u32),
    Finalize(Box<super::agent_types::AgentOutcome>),
}

pub(super) enum ToolRetryLoopOutcome {
    Completed {
        tool_msg: Message,
        tool_retry_count: u32,
    },
    RestartAgentStep,
    Finalize(Box<super::agent_types::AgentOutcome>),
}

pub(super) enum AllowedToolResultDecision {
    Continue,
    RestartAgentStep,
    Finalize(Box<super::agent_types::AgentOutcome>),
}

pub(super) enum MalformedToolCallDecision {
    ContinueToolLoop { invalid_args_error: Option<String> },
    RestartAgentStep,
    Finalize(Box<super::agent_types::AgentOutcome>),
}

pub(super) enum FailedRepeatGuardDecision {
    Continue,
    Finalize(Box<super::agent_types::AgentOutcome>),
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_malformed_tool_call(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        plan_tool_allowed: bool,
        malformed_tool_call_attempts: &mut u32,
        schema_repair_attempts: &mut std::collections::BTreeMap<String, u32>,
        messages: &mut Vec<Message>,
        started_at: String,
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
    ) -> MalformedToolCallDecision {
        let invalid_args_error = if tc.name.starts_with("mcp.") {
            self.mcp_registry.as_ref().and_then(|reg| {
                reg.validate_namespaced_tool_args(tc, self.tool_rt.tool_args_strict)
                    .err()
            })
        } else {
            crate::tools::validate_builtin_tool_args(
                &tc.name,
                &tc.arguments,
                self.tool_rt.tool_args_strict,
            )
            .err()
        };

        let Some(err) = invalid_args_error.as_ref() else {
            return MalformedToolCallDecision::ContinueToolLoop {
                invalid_args_error: None,
            };
        };

        *malformed_tool_call_attempts = malformed_tool_call_attempts.saturating_add(1);
        if *malformed_tool_call_attempts >= 2 {
            let reason = format!(
                "MODEL_TOOL_PROTOCOL_VIOLATION: repeated malformed tool calls (tool='{}', error='{}')",
                tc.name, err
            );
            self.emit_event(
                &run_id,
                step,
                EventKind::Error,
                serde_json::json!({
                    "error": reason,
                    "source": "tool_protocol_guard",
                    "tool_call_id": tc.id,
                    "name": tc.name,
                    "failure_class": "E_SCHEMA",
                    "attempt": *malformed_tool_call_attempts
                }),
            );
            return MalformedToolCallDecision::Finalize(Box::new(
                self.finalize_planner_error_with_output_with_end(
                    step,
                    run_id,
                    started_at,
                    reason,
                    messages.clone(),
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
            ));
        }

        let repair_key = format!("{}|{}", tc.name, err);
        let attempts = schema_repair_attempts
            .entry(repair_key)
            .and_modify(|n| *n = n.saturating_add(1))
            .or_insert(1);
        if *attempts <= super::MAX_SCHEMA_REPAIR_ATTEMPTS && plan_tool_allowed {
            self.emit_event(
                &run_id,
                step,
                EventKind::ToolRetry,
                serde_json::json!({
                    "tool_call_id": tc.id,
                    "name": tc.name,
                    "attempt": *attempts,
                    "max_retries": super::MAX_SCHEMA_REPAIR_ATTEMPTS,
                    "max_attempts": super::MAX_SCHEMA_REPAIR_ATTEMPTS,
                    "failure_class": "E_SCHEMA",
                    "action": "repair",
                    "error_code": crate::tools::ToolErrorCode::ToolArgsInvalid.as_str()
                }),
            );
            let tool_msg = crate::agent_tool_exec::make_invalid_args_tool_message(
                tc,
                err,
                self.tool_rt.exec_target_kind,
            );
            self.emit_event(
                &run_id,
                step,
                EventKind::ToolExecEnd,
                serde_json::json!({
                    "tool_call_id": tc.id,
                    "name": tc.name,
                    "ok": false,
                    "truncated": false,
                    "retry_count": 0,
                    "failure_class": "E_SCHEMA",
                    "source": "schema_repair",
                    "repair_attempted": true,
                    "repair_succeeded": false,
                    "error_code": crate::tools::ToolErrorCode::ToolArgsInvalid.as_str()
                }),
            );
            messages.push(tool_msg);
            if self.inject_post_tool_operator_messages(&run_id, step, messages) {
                return MalformedToolCallDecision::RestartAgentStep;
            }
            messages.push(crate::agent_tool_exec::schema_repair_instruction_message(
                tc, err,
            ));
            return MalformedToolCallDecision::RestartAgentStep;
        }

        self.emit_event(
            &run_id,
            step,
            EventKind::ToolRetry,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "attempt": *attempts,
                "max_retries": super::MAX_SCHEMA_REPAIR_ATTEMPTS,
                "max_attempts": super::MAX_SCHEMA_REPAIR_ATTEMPTS,
                "failure_class": "E_SCHEMA",
                "action": "stop",
                "error_code": crate::tools::ToolErrorCode::ToolArgsInvalid.as_str()
            }),
        );
        if *attempts > super::MAX_SCHEMA_REPAIR_ATTEMPTS {
            self.emit_event(
                &run_id,
                step,
                EventKind::Error,
                serde_json::json!({
                    "error": "schema repair attempts exhausted",
                    "source": "schema_repair",
                    "code": "TOOL_SCHEMA_REPAIR_EXHAUSTED",
                    "tool_call_id": tc.id,
                    "name": tc.name,
                    "attempt": *attempts,
                    "max_attempts": super::MAX_SCHEMA_REPAIR_ATTEMPTS
                }),
            );
        }
        MalformedToolCallDecision::ContinueToolLoop { invalid_args_error }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_failed_repeat_guard(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        failed_repeat_count: u32,
        failed_repeat_name_count: u32,
        repeat_key: &str,
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
    ) -> FailedRepeatGuardDecision {
        if failed_repeat_count < super::MAX_FAILED_REPEAT_PER_KEY
            && failed_repeat_name_count < super::MAX_FAILED_REPEAT_PER_TOOL_NAME
        {
            return FailedRepeatGuardDecision::Continue;
        }
        let reason = format!(
            "TOOL_REPEAT_BLOCKED: repeated failed tool call for '{}' exceeded repeat limit",
            tc.name
        );
        self.emit_event(
            &run_id,
            step,
            EventKind::StepBlocked,
            serde_json::json!({
                "source": "tool_repeat_guard",
                "code": "TOOL_REPEAT_BLOCKED",
                "tool_call_id": tc.id,
                "name": tc.name,
                "repeat_count": failed_repeat_count,
                "repeat_limit": super::MAX_FAILED_REPEAT_PER_KEY,
                "repeat_key_sha256": repeat_key
            }),
        );
        self.emit_event(
            &run_id,
            step,
            EventKind::Error,
            serde_json::json!({
                "error": reason.clone(),
                "source": "tool_repeat_guard",
                "tool_call_id": tc.id,
                "name": tc.name
            }),
        );
        FailedRepeatGuardDecision::Finalize(Box::new(
            self.finalize_planner_error_with_output_with_end(
                step,
                run_id,
                started_at,
                reason,
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
        ))
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
            changed: None,
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
                ToolRetryEvent {
                    attempt: *attempts,
                    max_retries: super::MAX_SCHEMA_REPAIR_ATTEMPTS,
                    failure_class: "E_SCHEMA",
                    action: "repair",
                    error_code: Some(error_code.as_str()),
                },
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
            let name_key = format!("name::{}", tc.name);
            let nn = failed_repeat_counts.entry(name_key).or_insert(0);
            *nn = nn.saturating_add(1);
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
            ToolRetryEvent {
                attempt: *attempts,
                max_retries: super::MAX_SCHEMA_REPAIR_ATTEMPTS,
                failure_class: "E_SCHEMA",
                action: "stop",
                error_code: Some(error_code.as_str()),
            },
        );
        self.emit_schema_repair_exhausted_event(run_id, step, tc, *attempts);
        SchemaRepairDecision::Exhausted
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_invalid_apply_patch_format(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        tool_retry_count: u32,
        current_content: &str,
        tool_msg: Message,
        repeat_key: &str,
        plan_tool_allowed: bool,
        invalid_patch_format_attempts: &mut std::collections::BTreeMap<String, u32>,
        messages: &mut Vec<Message>,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        started_at: String,
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
    ) -> InvalidPatchFormatDecision {
        if !crate::agent_tool_exec::is_apply_patch_invalid_format_error(tc, current_content) {
            return InvalidPatchFormatDecision::Continue;
        }

        let attempts = invalid_patch_format_attempts
            .entry(repeat_key.to_string())
            .and_modify(|n| *n = n.saturating_add(1))
            .or_insert(1);
        let invalid_patch_attempt = *attempts;

        if invalid_patch_attempt < 2 && plan_tool_allowed {
            self.emit_event(
                &run_id,
                step,
                EventKind::ToolExecEnd,
                serde_json::json!({
                    "tool_call_id": tc.id,
                    "name": tc.name,
                    "ok": false,
                    "truncated": crate::agent_tool_exec::infer_truncated_flag(current_content),
                    "retry_count": tool_retry_count,
                    "failure_class": "E_SCHEMA",
                    "error_code": "tool_args_invalid",
                    "attempt": invalid_patch_attempt
                }),
            );
            messages.push(tool_msg);
            if self.inject_post_tool_operator_messages(&run_id, step, messages) {
                return InvalidPatchFormatDecision::RestartAgentStep;
            }
            let n = failed_repeat_counts
                .entry(repeat_key.to_string())
                .or_insert(0);
            *n = n.saturating_add(1);
            let name_key = format!("name::{}", tc.name);
            let nn = failed_repeat_counts.entry(name_key).or_insert(0);
            *nn = nn.saturating_add(1);
            messages.push(crate::agent_tool_exec::schema_repair_instruction_message(
                tc,
                "invalid patch format",
            ));
            return InvalidPatchFormatDecision::RestartAgentStep;
        }

        if invalid_patch_attempt >= 2 {
            let reason =
                "MODEL_TOOL_PROTOCOL_VIOLATION: repeated invalid patch format for apply_patch"
                    .to_string();
            self.emit_event(
                &run_id,
                step,
                EventKind::ToolExecEnd,
                serde_json::json!({
                    "tool_call_id": tc.id,
                    "name": tc.name,
                    "ok": false,
                    "truncated": crate::agent_tool_exec::infer_truncated_flag(current_content),
                    "retry_count": tool_retry_count,
                    "failure_class": "E_PROTOCOL_PATCH_FORMAT",
                    "attempt": invalid_patch_attempt
                }),
            );
            self.emit_event(
                &run_id,
                step,
                EventKind::Error,
                serde_json::json!({
                    "error": reason,
                    "source": "tool_protocol_guard",
                    "tool_call_id": tc.id,
                    "name": tc.name,
                    "failure_class": "E_PROTOCOL_PATCH_FORMAT",
                    "attempt": invalid_patch_attempt
                }),
            );
            return InvalidPatchFormatDecision::Finalize(Box::new(
                self.finalize_planner_error_with_output_with_end(
                    step,
                    run_id,
                    started_at,
                    reason,
                    messages.clone(),
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
            ));
        }

        InvalidPatchFormatDecision::Continue
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
                ToolRetryEvent {
                    attempt: tool_retry_count,
                    max_retries,
                    failure_class: class.as_str(),
                    action: "stop",
                    error_code: retry_error_code,
                },
            );
            return RetryLoopDecision::Break;
        }
        let next_retry_count = tool_retry_count.saturating_add(1);
        self.emit_tool_retry_event(
            &run_id,
            step,
            tc,
            ToolRetryEvent {
                attempt: next_retry_count,
                max_retries,
                failure_class: class.as_str(),
                action: "retry",
                error_code: retry_error_code,
            },
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
            return RetryLoopDecision::Finalize(Box::new(
                self.finalize_runtime_budget_deny_with_end(
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
                ),
            ));
        }
        if let Some(reason) = crate::agent_budget::check_and_consume_mcp_budget(
            &self.tool_call_budget,
            tool_budget_usage,
            tc.name.starts_with("mcp."),
        ) {
            return RetryLoopDecision::Finalize(Box::new(
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
            ));
        }
        let tool_msg = self
            .run_tool_with_timeout_and_emit_mcp_events(&run_id, step, tc, "retry_await_result")
            .await;
        RetryLoopDecision::ContinueWithToolMsg(tool_msg, next_retry_count)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_tool_retry_loop(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        initial_tool_msg: Message,
        side_effects: crate::types::SideEffects,
        plan_tool_allowed: bool,
        repeat_key: &str,
        tool_budget_usage: &mut crate::agent_budget::ToolCallBudgetUsage,
        invalid_patch_format_attempts: &mut std::collections::BTreeMap<String, u32>,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        schema_repair_attempts: &mut std::collections::BTreeMap<String, u32>,
        messages: &mut Vec<Message>,
        approval_mode_meta: Option<String>,
        auto_scope_meta: Option<String>,
        approval_key_version_meta: Option<String>,
        tool_schema_hash_hex: Option<String>,
        hooks_config_hash_hex: Option<String>,
        planner_hash_hex: Option<String>,
        decision_exec_target: Option<String>,
        started_at: String,
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
    ) -> ToolRetryLoopOutcome {
        let mut tool_msg = initial_tool_msg;
        let mut tool_retry_count = 0u32;
        let mut total_retry_attempts = 0u32;
        const MAX_TOTAL_RETRY_ATTEMPTS: u32 = 4;
        loop {
            let current_content = tool_msg.content.clone().unwrap_or_default();
            if !tool_result_has_error(&current_content) {
                break;
            }
            total_retry_attempts = total_retry_attempts.saturating_add(1);
            if total_retry_attempts > MAX_TOTAL_RETRY_ATTEMPTS {
                break;
            }
            match self.handle_invalid_apply_patch_format(
                run_id.clone(),
                step,
                tc,
                tool_retry_count,
                &current_content,
                tool_msg.clone(),
                repeat_key,
                plan_tool_allowed,
                invalid_patch_format_attempts,
                messages,
                failed_repeat_counts,
                started_at.clone(),
                observed_tool_calls.clone(),
                observed_tool_decisions.clone(),
                request_context_chars,
                last_compaction_report.clone(),
                hook_invocations.clone(),
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
            ) {
                InvalidPatchFormatDecision::Continue => {}
                InvalidPatchFormatDecision::RestartAgentStep => {
                    return ToolRetryLoopOutcome::RestartAgentStep;
                }
                InvalidPatchFormatDecision::Finalize(outcome) => {
                    return ToolRetryLoopOutcome::Finalize(outcome);
                }
            }
            if let Some(error_code) =
                crate::agent_tool_exec::tool_result_error_code(&current_content)
            {
                if is_repairable_error_code(error_code) && plan_tool_allowed {
                    match self.handle_schema_repair_attempt(
                        &run_id,
                        step,
                        tc,
                        error_code,
                        tool_retry_count,
                        &current_content,
                        &tool_msg,
                        repeat_key,
                        messages,
                        failed_repeat_counts,
                        schema_repair_attempts,
                    ) {
                        SchemaRepairDecision::RestartAgentStep => {
                            return ToolRetryLoopOutcome::RestartAgentStep;
                        }
                        SchemaRepairDecision::Exhausted => {}
                    }
                }
            }
            match self
                .handle_generic_retry_iteration(
                    run_id.clone(),
                    step,
                    tc,
                    &current_content,
                    side_effects,
                    tool_retry_count,
                    tool_budget_usage,
                    approval_mode_meta.clone(),
                    auto_scope_meta.clone(),
                    approval_key_version_meta.clone(),
                    tool_schema_hash_hex.clone(),
                    hooks_config_hash_hex.clone(),
                    planner_hash_hex.clone(),
                    decision_exec_target.clone(),
                    started_at.clone(),
                    messages.clone(),
                    observed_tool_calls.clone(),
                    observed_tool_decisions.clone(),
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.clone(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                )
                .await
            {
                RetryLoopDecision::Break => break,
                RetryLoopDecision::ContinueWithToolMsg(next_msg, next_count) => {
                    tool_msg = next_msg;
                    tool_retry_count = next_count;
                }
                RetryLoopDecision::Finalize(outcome) => {
                    return ToolRetryLoopOutcome::Finalize(outcome);
                }
            }
        }

        ToolRetryLoopOutcome::Completed {
            tool_msg,
            tool_retry_count,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn finalize_allowed_tool_result(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        tool_msg: Message,
        tool_retry_count: u32,
        invalid_args_present: bool,
        approval_id: Option<String>,
        approval_key: Option<String>,
        reason: Option<String>,
        source: Option<String>,
        taint_enforced: bool,
        escalated: bool,
        escalation_reason: Option<String>,
        approval_mode_meta: Option<String>,
        auto_scope_meta: Option<String>,
        approval_key_version_meta: Option<String>,
        tool_schema_hash_hex: Option<String>,
        hooks_config_hash_hex: Option<String>,
        planner_hash_hex: Option<String>,
        decision_exec_target: Option<String>,
        repeat_key: &str,
        started_at: String,
        request_context_chars: usize,
        last_compaction_report: Option<crate::compaction::CompactionReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &crate::types::TokenUsage,
        hook_invocations: &mut Vec<crate::hooks::protocol::HookInvocationReport>,
        taint_state: &mut crate::taint::TaintState,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        invalid_patch_format_attempts: &mut std::collections::BTreeMap<String, u32>,
        successful_write_tool_ok_this_step: &mut bool,
        messages: &mut Vec<Message>,
        observed_tool_decisions: &mut Vec<super::ToolDecisionRecord>,
        observed_tool_executions: &mut Vec<crate::agent_impl_guard::ToolExecutionRecord>,
        observed_tool_calls: Vec<ToolCall>,
    ) -> AllowedToolResultDecision {
        let hook_state = match self
            .apply_tool_result_hooks(&run_id, step, tc, tool_msg, hook_invocations)
            .await
        {
            Ok(state) => state,
            Err(reason) => {
                return AllowedToolResultDecision::Finalize(Box::new(
                    self.finalize_hook_aborted_with_end(
                        step,
                        run_id,
                        started_at,
                        String::new(),
                        reason,
                        messages.clone(),
                        observed_tool_calls,
                        observed_tool_decisions.clone(),
                        request_context_chars,
                        last_compaction_report,
                        hook_invocations.clone(),
                        provider_retry_count,
                        provider_error_count,
                        saw_token_usage,
                        total_token_usage,
                        taint_state,
                    ),
                ));
            }
        };
        let tool_msg = hook_state.tool_msg;
        let input_digest = hook_state.input_digest;
        let output_digest = hook_state.output_digest;
        let input_len = hook_state.input_len;
        let output_len = hook_state.output_len;
        let final_truncated = hook_state.final_truncated;

        let content = tool_msg.content.clone().unwrap_or_default();
        let final_ok = !tool_result_has_error(&content);
        let final_error_code = crate::agent_tool_exec::tool_result_error_code(&content);
        let changed_flag = if matches!(tc.name.as_str(), "apply_patch" | "write_file" | "str_replace") {
            crate::agent_tool_exec::tool_result_changed_flag(&content)
        } else {
            None
        };
        observed_tool_executions.push(crate::agent_impl_guard::ToolExecutionRecord {
            name: tc.name.clone(),
            path: normalized_tool_path_from_args(tc),
            ok: final_ok,
            changed: changed_flag,
        });
        let final_failure_class = if tool_result_has_error(&content) {
            Some(crate::agent_tool_exec::classify_tool_failure(
                tc,
                &content,
                invalid_args_present,
            ))
        } else {
            None
        };
        self.update_taint_for_tool_result(&run_id, step, tc, &content, messages.len(), taint_state);
        self.record_allowed_tool_result(
            &run_id,
            step,
            tc,
            approval_id,
            approval_key,
            reason,
            source,
            taint_enforced,
            escalated,
            escalation_reason,
            approval_mode_meta,
            auto_scope_meta,
            approval_key_version_meta,
            tool_schema_hash_hex,
            hooks_config_hash_hex,
            planner_hash_hex,
            decision_exec_target,
            final_ok,
            content.clone(),
            Some(input_digest),
            Some(output_digest),
            Some(input_len),
            Some(output_len),
            tool_retry_count,
            final_truncated,
            final_failure_class,
            final_error_code,
            taint_state,
            repeat_key,
            failed_repeat_counts,
            invalid_patch_format_attempts,
            successful_write_tool_ok_this_step,
            tool_msg,
            messages,
            observed_tool_decisions,
        );
        if !final_ok
            && tc.name == "write_file"
            && content.contains("write_file blocked for existing file")
        {
            return AllowedToolResultDecision::Finalize(Box::new(
                self.finalize_existing_write_file_guard_with_end(
                    run_id,
                    step,
                    tc,
                    started_at,
                    messages.clone(),
                    observed_tool_calls,
                    observed_tool_decisions.clone(),
                    request_context_chars,
                    last_compaction_report,
                    hook_invocations.clone(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                ),
            ));
        }
        if self.inject_post_tool_operator_messages(&run_id, step, messages) {
            return AllowedToolResultDecision::RestartAgentStep;
        }
        AllowedToolResultDecision::Continue
    }
}
