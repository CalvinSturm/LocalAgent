use std::collections::BTreeSet;

use crate::agent_tool_exec::{contains_tool_wrapper_markers, extract_content_tool_calls};
use crate::types::GenerateResponse;

pub(super) enum AssistantResponseNormalization {
    Ready,
    MalformedWrapper,
    MultipleToolCalls {
        count: usize,
    },
}

pub(super) fn normalize_assistant_response(
    resp: &mut GenerateResponse,
    step: u32,
    allowed_tool_names: &BTreeSet<String>,
) -> AssistantResponseNormalization {
    if resp.tool_calls.is_empty() {
        let assistant_content = resp.assistant.content.clone().unwrap_or_default();
        let normalized_calls =
            extract_content_tool_calls(&assistant_content, step, allowed_tool_names);
        if !normalized_calls.is_empty() {
            resp.tool_calls = normalized_calls;
            resp.assistant.content = None;
        } else if contains_tool_wrapper_markers(&assistant_content) {
            return AssistantResponseNormalization::MalformedWrapper;
        }
    }

    if resp.tool_calls.len() > 1 {
        return AssistantResponseNormalization::MultipleToolCalls {
            count: resp.tool_calls.len(),
        };
    }

    AssistantResponseNormalization::Ready
}

#[cfg(test)]
mod tests {
    use super::{normalize_assistant_response, AssistantResponseNormalization};
    use crate::types::{GenerateResponse, Message, Role};
    use std::collections::BTreeSet;

    fn empty_response(content: &str) -> GenerateResponse {
        GenerateResponse {
            assistant: Message {
                role: Role::Assistant,
                content: Some(content.to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
            tool_calls: Vec::new(),
            usage: None,
        }
    }

    #[test]
    fn normalizes_wrapped_tool_call_content_into_tool_calls() {
        let mut response = empty_response(
            "[TOOL_CALL] {\"name\":\"shell\",\"arguments\":{\"command\":\"cargo test\"}} [/TOOL_CALL]",
        );
        let mut allowed = BTreeSet::new();
        allowed.insert("shell".to_string());

        let result = normalize_assistant_response(&mut response, 1, &allowed);

        assert!(matches!(
            result,
            AssistantResponseNormalization::Ready
        ));
        assert_eq!(response.tool_calls.len(), 1);
        assert!(response.assistant.content.is_none());
    }

    #[test]
    fn flags_malformed_wrapped_tool_call_content() {
        let mut response = empty_response("[TOOL_CALL]");
        let result = normalize_assistant_response(&mut response, 1, &BTreeSet::new());
        assert!(matches!(
            result,
            AssistantResponseNormalization::MalformedWrapper
        ));
    }
}
