use crate::agent_impl_guard::normalize_tool_path;
use crate::agent_utils::sha256_hex;
use crate::tools::ToolErrorCode;
use crate::types::{Message, Role, ToolCall};

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
