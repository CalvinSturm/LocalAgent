use crate::agent_impl_guard::normalize_tool_path;
use crate::agent_tool_exec::run_tool_once;
use crate::agent_utils::sha256_hex;
use crate::events::EventKind;
use crate::providers::ModelProvider;
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
}
