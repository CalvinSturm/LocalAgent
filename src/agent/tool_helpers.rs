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
}
