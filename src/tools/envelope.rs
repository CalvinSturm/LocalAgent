use serde_json::json;

use crate::types::{Message, Role, ToolCall};

use super::{
    invalid_args_detail, tool_side_effects, ToolResultEnvelope, ToolResultMeta,
};

pub fn to_tool_result_envelope(
    tc: &ToolCall,
    source: &str,
    ok: bool,
    content: String,
    truncated: bool,
    meta: ToolResultMeta,
) -> ToolResultEnvelope {
    to_tool_result_envelope_with_error(tc, source, ok, content, truncated, None, meta)
}

pub fn to_tool_result_envelope_with_error(
    tc: &ToolCall,
    source: &str,
    ok: bool,
    content: String,
    truncated: bool,
    error: Option<super::ToolErrorDetail>,
    mut meta: ToolResultMeta,
) -> ToolResultEnvelope {
    meta.source = source.to_string();
    ToolResultEnvelope {
        schema_version: "openagent.tool_result.v1".to_string(),
        tool_name: tc.name.clone(),
        tool_call_id: tc.id.clone(),
        ok,
        content,
        truncated,
        truncate_reason: None,
        full_output_ref: None,
        error,
        meta,
    }
}

pub fn envelope_to_message(env: ToolResultEnvelope) -> Message {
    Message {
        role: Role::Tool,
        content: Some(serde_json::to_string(&env).unwrap_or_else(|e| {
            json!({"schema_version":"openagent.tool_result.v1","ok":false,"content":format!("failed to serialize tool result envelope: {e}")}).to_string()
        })),
        tool_call_id: Some(env.tool_call_id.clone()),
        tool_name: Some(env.tool_name.clone()),
        tool_calls: None,
    }
}

pub fn invalid_args_tool_message(
    tc: &ToolCall,
    source: &str,
    err: &str,
    execution_target: String,
) -> Message {
    envelope_to_message(to_tool_result_envelope_with_error(
        tc,
        source,
        false,
        format!("invalid tool arguments: {err}"),
        false,
        Some(invalid_args_detail(&tc.name, &tc.arguments, err)),
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
