use std::collections::BTreeSet;

use crate::agent_tool_exec::{contains_tool_wrapper_markers, extract_content_tool_calls};
use crate::types::{GenerateResponse, ToolCall};

pub(super) enum ToolWrapperParseState {
    Normalized(Vec<ToolCall>),
    MalformedWrapper,
    Unchanged,
}

pub(super) fn normalize_tool_calls_from_assistant(
    resp: &GenerateResponse,
    step: u32,
    allowed_tool_names: &BTreeSet<String>,
) -> ToolWrapperParseState {
    if !resp.tool_calls.is_empty() {
        return ToolWrapperParseState::Unchanged;
    }
    let assistant_content = resp.assistant.content.clone().unwrap_or_default();
    let normalized_calls = extract_content_tool_calls(&assistant_content, step, allowed_tool_names);
    if !normalized_calls.is_empty() {
        return ToolWrapperParseState::Normalized(normalized_calls);
    }
    if contains_tool_wrapper_markers(&assistant_content) {
        return ToolWrapperParseState::MalformedWrapper;
    }
    ToolWrapperParseState::Unchanged
}
