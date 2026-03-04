use crate::mcp::registry::McpRegistry;
use crate::tools::{
    envelope_to_message, execute_tool, invalid_args_tool_message, to_tool_result_envelope,
    tool_side_effects, ToolErrorCode, ToolResultMeta, ToolRuntime,
};
use crate::types::{Message, Role, ToolCall};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolFailureClass {
    Schema,
    Policy,
    TimeoutTransient,
    SelectorAmbiguous,
    NetworkTransient,
    NonIdempotent,
    Other,
}

impl ToolFailureClass {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Schema => "E_SCHEMA",
            Self::Policy => "E_POLICY",
            Self::TimeoutTransient => "E_TIMEOUT_TRANSIENT",
            Self::SelectorAmbiguous => "E_SELECTOR_AMBIGUOUS",
            Self::NetworkTransient => "E_NETWORK_TRANSIENT",
            Self::NonIdempotent => "E_NON_IDEMPOTENT",
            Self::Other => "E_OTHER",
        }
    }

    pub(crate) fn retry_limit_for(self, side_effects: crate::types::SideEffects) -> u32 {
        if matches!(
            side_effects,
            crate::types::SideEffects::FilesystemWrite
                | crate::types::SideEffects::ShellExec
                | crate::types::SideEffects::Network
                | crate::types::SideEffects::Browser
        ) {
            return 0;
        }
        match self {
            Self::Schema => 1,
            Self::TimeoutTransient => 1,
            Self::SelectorAmbiguous => 1,
            Self::NetworkTransient => 1,
            Self::Policy | Self::NonIdempotent | Self::Other => 0,
        }
    }
}

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
                            warnings: None,
                            warnings_max: None,
                            warnings_truncated: None,
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
                        warnings: None,
                        warnings_max: None,
                        warnings_truncated: None,
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
    is_apply_patch_parse_error_text(&text)
}

fn is_apply_patch_parse_error_text(text: &str) -> bool {
    text.contains("invalid patch format") || text.contains("invalid patch:")
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

pub(crate) fn tool_result_error_code(content: &str) -> Option<ToolErrorCode> {
    let v = serde_json::from_str::<serde_json::Value>(content).ok()?;
    let code = v.get("error")?.get("code")?.as_str()?;
    match code {
        "tool_args_invalid" => Some(ToolErrorCode::ToolArgsInvalid),
        "tool_unknown" => Some(ToolErrorCode::ToolUnknown),
        "tool_path_denied" => Some(ToolErrorCode::ToolPathDenied),
        "tool_disabled" => Some(ToolErrorCode::ToolDisabled),
        "tool_args_malformed_json" => Some(ToolErrorCode::ToolArgsMalformedJson),
        "shell_gate_deny" => Some(ToolErrorCode::ShellGateDeny),
        "shell_tool_unavailable" => Some(ToolErrorCode::ShellToolUnavailable),
        "shell_exec_not_found" => Some(ToolErrorCode::ShellExecNotFound),
        "shell_exec_os_error" => Some(ToolErrorCode::ShellExecOsError),
        "shell_exec_non_zero_exit" => Some(ToolErrorCode::ShellExecNonZeroExit),
        _ => None,
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

pub(crate) fn make_invalid_args_tool_message(
    tc: &ToolCall,
    err: &str,
    exec_target_kind: crate::target::ExecTargetKind,
) -> Message {
    let source = if tc.name.starts_with("mcp.") {
        "mcp"
    } else {
        "builtin"
    };
    let execution_target = if source == "mcp" {
        "host".to_string()
    } else {
        match exec_target_kind {
            crate::target::ExecTargetKind::Host => "host".to_string(),
            crate::target::ExecTargetKind::Docker => "docker".to_string(),
        }
    };
    invalid_args_tool_message(tc, source, err, execution_target)
}

pub(crate) fn schema_repair_instruction_message(tc: &ToolCall, err: &str) -> Message {
    let err_lower = err.to_ascii_lowercase();
    let guidance = if tc.name == "apply_patch" {
        " Use read_file first, then emit exactly one apply_patch with a minimal unified diff and a valid '@@ -old,+new @@' hunk header. Use a relative path within workdir (example: 'src/main.rs'), never an absolute path."
    } else if tc.name == "write_file"
        || err_lower.contains("overwrite_existing")
        || err_lower.contains("existing file")
    {
        " If the target file already exists, do not overwrite it. Read the file first and use apply_patch for in-place edits."
    } else if err_lower.contains("tool_path_denied")
        || err_lower.contains("path must stay within workdir")
        || err_lower.contains("absolute path")
    {
        " Use a relative path inside the current workdir only (no absolute paths and no '..' traversal)."
    } else {
        ""
    };
    Message {
        role: Role::Developer,
        content: Some(format!(
            "Schema repair required for tool '{}': {}. Re-emit exactly one corrected tool call for '{}' with valid arguments only.{}",
            tc.name, err, tc.name, guidance
        )),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
    }
}

pub(crate) fn parse_jsonish(raw: &str) -> Option<serde_json::Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Some(v);
    }
    if let Some(candidate) = fenced_json_candidate(trimmed) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&candidate) {
            return Some(v);
        }
    }
    if let Some((start, end)) = find_json_bounds(trimmed) {
        let candidate = &trimmed[start..=end];
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(candidate) {
            return Some(v);
        }
    }
    None
}

pub(crate) fn contains_tool_wrapper_markers(s: &str) -> bool {
    let u = s.to_ascii_uppercase();
    u.contains("[TOOL_CALL]") || u.contains("[END_TOOL_CALL]")
}

pub(crate) fn extract_content_tool_calls(
    raw: &str,
    step: u32,
    allowed_tool_names: &std::collections::BTreeSet<String>,
) -> Vec<ToolCall> {
    let wrapped = extract_wrapped_tool_calls(raw, step, allowed_tool_names);
    if !wrapped.is_empty() {
        return wrapped;
    }
    if let Some(tc) = extract_inline_tool_call(raw, step, allowed_tool_names) {
        return vec![tc];
    }
    Vec::new()
}

pub(crate) fn extract_inline_tool_call(
    raw: &str,
    step: u32,
    allowed_tool_names: &std::collections::BTreeSet<String>,
) -> Option<ToolCall> {
    let v = parse_jsonish(raw)?;
    let name = v.get("name").and_then(|x| x.as_str())?;
    if !allowed_tool_names.contains(name) {
        return None;
    }
    let arguments = v
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    Some(ToolCall {
        id: format!("inline_tc_{step}_0"),
        name: name.to_string(),
        arguments,
    })
}

pub(crate) fn extract_wrapped_tool_calls(
    raw: &str,
    step: u32,
    allowed_tool_names: &std::collections::BTreeSet<String>,
) -> Vec<ToolCall> {
    let upper = raw.to_ascii_uppercase();
    let start_tag = "[TOOL_CALL]";
    let end_tag = "[END_TOOL_CALL]";
    let mut out = Vec::new();
    let mut offset = 0usize;
    while let Some(rel_start) = upper[offset..].find(start_tag) {
        let start = offset + rel_start + start_tag.len();
        let Some(rel_end) = upper[start..].find(end_tag) else {
            break;
        };
        let end = start + rel_end;
        let body = raw[start..end].trim();
        if !body.is_empty() {
            if let Some(v) = parse_jsonish(body) {
                if let Some(name) = v.get("name").and_then(|x| x.as_str()) {
                    if !allowed_tool_names.contains(name) {
                        offset = end + end_tag.len();
                        continue;
                    }
                    let arguments = v
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({}));
                    out.push(ToolCall {
                        id: format!("wrapped_tc_{step}_{}", out.len()),
                        name: name.to_string(),
                        arguments,
                    });
                }
            }
        }
        offset = end + end_tag.len();
    }
    out
}

pub(crate) fn fenced_json_candidate(s: &str) -> Option<String> {
    if !s.starts_with("```") {
        return None;
    }
    let lines = s.lines().collect::<Vec<_>>();
    if lines.len() < 3 {
        return None;
    }
    if !lines.first()?.starts_with("```") || !lines.last()?.starts_with("```") {
        return None;
    }
    Some(lines[1..lines.len() - 1].join("\n"))
}

pub(crate) fn find_json_bounds(s: &str) -> Option<(usize, usize)> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end <= start {
        return None;
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::is_apply_patch_invalid_format_error;
    use crate::types::ToolCall;
    use serde_json::json;

    #[test]
    fn apply_patch_invalid_format_detects_parse_error() {
        let tc = ToolCall {
            id: "tc1".to_string(),
            name: "apply_patch".to_string(),
            arguments: json!({"path":"main.rs","patch":"bad"}),
        };
        let raw = json!({
            "ok": false,
            "content": "invalid patch: error parsing patch: Hunk header does not match hunk"
        })
        .to_string();
        assert!(is_apply_patch_invalid_format_error(&tc, &raw));
    }

    #[test]
    fn apply_patch_invalid_format_ignores_hunk_apply_error() {
        let tc = ToolCall {
            id: "tc1".to_string(),
            name: "apply_patch".to_string(),
            arguments: json!({"path":"main.rs","patch":"@@ -1 +1 @@\n-a\n+b\n"}),
        };
        let raw = json!({
            "ok": false,
            "content": "failed to apply patch: error applying hunk #1"
        })
        .to_string();
        assert!(!is_apply_patch_invalid_format_error(&tc, &raw));
    }
}
