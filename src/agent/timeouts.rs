use crate::providers::ModelProvider;
use crate::tools::{envelope_to_message, to_tool_result_envelope, tool_side_effects, ToolResultMeta};
use crate::types::{Message, ToolCall};

use super::{Agent, DEFAULT_POST_WRITE_VERIFY_TIMEOUT_MS, DEFAULT_TOOL_EXEC_TIMEOUT_MS};

impl<P: ModelProvider> Agent<P> {
    pub(super) fn effective_tool_exec_timeout_ms(&self) -> u64 {
        if self.tool_call_budget.tool_exec_timeout_ms == 0 {
            DEFAULT_TOOL_EXEC_TIMEOUT_MS
        } else {
            self.tool_call_budget.tool_exec_timeout_ms
        }
    }

    pub(super) fn effective_post_write_verify_timeout_ms(&self) -> u64 {
        if self.tool_call_budget.post_write_verify_timeout_ms == 0 {
            DEFAULT_POST_WRITE_VERIFY_TIMEOUT_MS
        } else {
            self.tool_call_budget.post_write_verify_timeout_ms
        }
    }

    pub(super) fn tool_timeout_message(&self, tc: &ToolCall, timeout_ms: u64) -> Message {
        let source = if tc.name.starts_with("mcp.") {
            "mcp"
        } else {
            "builtin"
        };
        let execution_target = if source == "mcp" {
            "host".to_string()
        } else {
            match self.tool_rt.exec_target_kind {
                crate::target::ExecTargetKind::Host => "host".to_string(),
                crate::target::ExecTargetKind::Docker => "docker".to_string(),
            }
        };
        envelope_to_message(to_tool_result_envelope(
            tc,
            source,
            false,
            format!(
                "tool execution timed out after {}ms (runtime timeout)",
                timeout_ms
            ),
            false,
            ToolResultMeta {
                side_effects: tool_side_effects(&tc.name),
                bytes: None,
                exit_code: None,
                stderr_truncated: None,
                stdout_truncated: None,
                source: source.to_string(),
                execution_target,
                warnings: None,
                warnings_max: None,
                warnings_truncated: None,
                docker: None,
            },
        ))
    }
}
