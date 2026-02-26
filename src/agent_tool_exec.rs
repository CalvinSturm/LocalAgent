use crate::agent::ToolFailureClass;
use crate::mcp::registry::McpRegistry;
use crate::tools::{
    envelope_to_message, execute_tool, to_tool_result_envelope, tool_side_effects, ToolResultMeta,
    ToolRuntime,
};
use crate::types::{Message, ToolCall};

pub(crate) async fn run_tool_once(
    tool_rt: &ToolRuntime,
    tc: &ToolCall,
    mcp_registry: Option<&std::sync::Arc<McpRegistry>>,
) -> ToolRunOutcome {
    if tc.name.starts_with("mcp.") {
        match mcp_registry {
            Some(reg) => match reg.call_namespaced_tool(tc, tool_rt.tool_args_strict).await {
                Ok(outcome) => ToolRunOutcome {
                    message: outcome.message,
                    mcp_meta: Some(outcome.meta),
                },
                Err(e) => ToolRunOutcome {
                    message: envelope_to_message(to_tool_result_envelope(
                        tc,
                        "mcp",
                        false,
                        format!("mcp call failed: {e}"),
                        false,
                        ToolResultMeta {
                            side_effects: tool_side_effects(&tc.name),
                            bytes: None,
                            exit_code: None,
                            stderr_truncated: None,
                            stdout_truncated: None,
                            source: "mcp".to_string(),
                            execution_target: "host".to_string(),
                            docker: None,
                        },
                    )),
                    mcp_meta: None,
                },
            },
            None => ToolRunOutcome {
                message: envelope_to_message(to_tool_result_envelope(
                    tc,
                    "mcp",
                    false,
                    "mcp registry not available".to_string(),
                    false,
                    ToolResultMeta {
                        side_effects: tool_side_effects(&tc.name),
                        bytes: None,
                        exit_code: None,
                        stderr_truncated: None,
                        stdout_truncated: None,
                        source: "mcp".to_string(),
                        execution_target: "host".to_string(),
                        docker: None,
                    },
                )),
                mcp_meta: None,
            },
        }
    } else {
        ToolRunOutcome {
            message: execute_tool(tool_rt, tc).await,
            mcp_meta: None,
        }
    }
}

pub(crate) struct ToolRunOutcome {
    pub(crate) message: Message,
    pub(crate) mcp_meta: Option<crate::mcp::registry::McpCallMeta>,
}

pub(crate) fn classify_tool_failure(
    tc: &ToolCall,
    raw_content: &str,
    invalid_args_error: bool,
) -> ToolFailureClass {
    let text = tool_result_text(raw_content).to_ascii_lowercase();
    if invalid_args_error
        || text.contains("invalid tool arguments")
        || text.contains("missing required field")
        || text.contains("unknown field not allowed")
        || text.contains("must be a ")
        || text.contains("has invalid type")
    {
        return ToolFailureClass::Schema;
    }
    if text.contains("denied") || text.contains("not allowed") || text.contains("approval required")
    {
        return ToolFailureClass::Policy;
    }
    if text.contains("strict mode violation")
        || (text.contains("locator") && text.contains("multiple"))
        || (text.contains("selector") && text.contains("ambiguous"))
    {
        return ToolFailureClass::SelectorAmbiguous;
    }
    if text.contains("timed out") || text.contains("timeout") || text.contains("stream idle") {
        return ToolFailureClass::TimeoutTransient;
    }
    if text.contains("mcp call failed")
        || text.contains("connection refused")
        || text.contains("response channel closed")
        || text.contains("failed to spawn mcp")
        || text.contains("temporarily unavailable")
    {
        return ToolFailureClass::NetworkTransient;
    }
    let side_effects = tool_side_effects(&tc.name);
    if matches!(
        side_effects,
        crate::types::SideEffects::FilesystemWrite
            | crate::types::SideEffects::ShellExec
            | crate::types::SideEffects::Browser
            | crate::types::SideEffects::Network
    ) {
        return ToolFailureClass::NonIdempotent;
    }
    ToolFailureClass::Other
}

pub(crate) fn is_apply_patch_invalid_format_error(tc: &ToolCall, raw_content: &str) -> bool {
    if tc.name != "apply_patch" {
        return false;
    }
    let text = tool_result_text(raw_content).to_ascii_lowercase();
    text.contains("invalid patch format")
        || text.contains("invalid patch:")
        || text.contains("failed to apply patch:")
}

pub(crate) fn tool_result_text(raw: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(v) => v
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or(raw)
            .to_string(),
        Err(_) => raw.to_string(),
    }
}

pub(crate) fn tool_result_has_error(content: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(v) => {
            if let Some(ok) = v.get("ok").and_then(|x| x.as_bool()) {
                !ok
            } else {
                v.get("error").is_some()
            }
        }
        Err(_) => false,
    }
}

pub(crate) fn infer_truncated_flag(content: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(v) => v
            .get("truncated")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        Err(_) => false,
    }
}
