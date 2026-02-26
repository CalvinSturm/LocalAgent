use crate::agent::AgentTaintRecord;
use crate::taint::{digest_prefix_hex, TaintMode, TaintSpan, TaintState, TaintToggle};
use crate::tools::tool_side_effects;
use crate::trust::policy::Policy;
use crate::types::ToolCall;

pub(crate) fn taint_record_from_state(
    toggle: TaintToggle,
    mode: TaintMode,
    digest_bytes: usize,
    state: &TaintState,
) -> Option<AgentTaintRecord> {
    if !matches!(toggle, TaintToggle::On) {
        return None;
    }
    Some(AgentTaintRecord {
        enabled: true,
        mode: match mode {
            TaintMode::Propagate => "propagate".to_string(),
            TaintMode::PropagateAndEnforce => "propagate_and_enforce".to_string(),
        },
        digest_bytes,
        overall: state.overall_str().to_string(),
        spans_by_tool_call_id: state.spans_by_tool_call_id.clone(),
    })
}

pub(crate) fn compute_taint_spans_for_tool(
    tc: &ToolCall,
    tool_message_content: &str,
    policy: Option<&Policy>,
    digest_bytes: usize,
) -> Vec<TaintSpan> {
    let mut spans = Vec::new();
    let side_effects = tool_side_effects(&tc.name);
    let content_for_digest = extract_tool_envelope_content(tool_message_content);
    let digest = digest_prefix_hex(&content_for_digest, digest_bytes);

    match side_effects {
        crate::types::SideEffects::Browser => spans.push(TaintSpan {
            source: "browser".to_string(),
            detail: tc.name.clone(),
            digest,
        }),
        crate::types::SideEffects::Network => spans.push(TaintSpan {
            source: "network".to_string(),
            detail: tc.name.clone(),
            digest,
        }),
        _ => {
            if tc.name == "read_file" {
                if let Some(path) = tc.arguments.get("path").and_then(|v| v.as_str()) {
                    if let Some(p) = policy.and_then(|p| p.taint_file_match(path)) {
                        spans.push(TaintSpan {
                            source: "file".to_string(),
                            detail: format!("matched taint glob: {p}"),
                            digest,
                        });
                    }
                }
            }
        }
    }
    spans
}

pub(crate) fn extract_tool_envelope_content(raw: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(v) => v
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or(raw)
            .to_string(),
        Err(_) => raw.to_string(),
    }
}
