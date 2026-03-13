use crate::types::ToolCall;

#[derive(Debug, Clone)]
pub(crate) struct ToolExecutionRecord {
    pub name: String,
    pub path: Option<String>,
    pub ok: bool,
    /// For write tools: whether the tool actually changed the file.
    /// `None` means unknown (assume changed if ok). `Some(false)` means no-op.
    pub changed: Option<bool>,
}

#[cfg(test)]
pub(crate) fn implementation_integrity_violation(
    user_prompt: &str,
    final_output: &str,
    observed_tool_calls: &[ToolCall],
) -> Option<String> {
    let tool_executions = observed_tool_calls
        .iter()
        .map(|call| ToolExecutionRecord {
            name: call.name.clone(),
            path: call
                .arguments
                .get("path")
                .and_then(|v| v.as_str())
                .map(normalize_tool_path),
            ok: true,
            changed: None,
        })
        .collect::<Vec<_>>();
    implementation_integrity_violation_with_tool_executions(
        user_prompt,
        final_output,
        observed_tool_calls,
        &tool_executions,
        true,
    )
}

pub(crate) fn implementation_integrity_violation_with_tool_executions(
    user_prompt: &str,
    final_output: &str,
    observed_tool_calls: &[ToolCall],
    tool_executions: &[ToolExecutionRecord],
    enforce_implementation_integrity_guard: bool,
) -> Option<String> {
    let tool_facts = crate::agent::tool_facts_from_calls_and_executions(
        prompt_required_validation_command(user_prompt),
        observed_tool_calls,
        tool_executions,
    );
    crate::agent::implementation_integrity_violation_from_facts(
        user_prompt,
        final_output,
        &tool_facts,
        enforce_implementation_integrity_guard,
    )
}

pub(crate) fn pending_post_write_verification_paths(
    observed_tool_calls: &[ToolCall],
    tool_executions: &[ToolExecutionRecord],
) -> std::collections::BTreeSet<String> {
    let tool_facts = crate::agent::tool_facts_from_calls_and_executions(
        None,
        observed_tool_calls,
        tool_executions,
    );
    crate::agent::pending_post_write_verification_paths_from_facts(&tool_facts)
}

pub(crate) fn normalize_tool_path(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            if !out.is_empty() {
                out.pop();
            }
            continue;
        }
        out.push(part);
    }
    if out.is_empty() {
        ".".to_string()
    } else {
        out.join("/").to_ascii_lowercase()
    }
}

pub(crate) fn prompt_requires_tool_only(prompt: &str) -> bool {
    let p = prompt.to_ascii_lowercase();
    (p.contains("tool calls only")
        || p.contains("tool-only")
        || p.contains("exactly one tool call"))
        && (p.contains("no prose")
            || p.contains("do not output code")
            || p.contains("do not explain"))
}

pub(crate) fn prompt_requires_post_write_follow_on(prompt: &str) -> bool {
    let p = prompt.to_ascii_lowercase();
    let requires_validation = [
        "run test",
        "run tests",
        "cargo test",
        "node --test",
        "npm test",
        "pnpm test",
        "validate",
        "validation",
        "verify",
        "confirm",
        "check that",
        "check the",
        "before finishing",
        "before you finish",
        "before finalizing",
    ]
    .iter()
    .any(|needle| p.contains(needle));
    let requires_user_facing_closeout = [
        "final answer",
        "final response",
        "summarize what changed",
        "summarise what changed",
        "explain what changed",
        "tell me what changed",
        "describe what changed",
    ]
    .iter()
    .any(|needle| p.contains(needle));
    requires_validation || requires_user_facing_closeout
}

pub(crate) fn prompt_required_validation_command(prompt: &str) -> Option<&'static str> {
    let p = prompt.to_ascii_lowercase();
    ["node --test", "cargo test", "npm test", "pnpm test"]
        .into_iter()
        .find(|needle| p.contains(needle))
}

pub(crate) fn prompt_required_exact_final_answer(prompt: &str) -> Option<String> {
    let lower = prompt.to_ascii_lowercase();
    let markers = ["your final answer must be exactly:", "reply with exactly:"];
    let (idx, marker) = markers
        .iter()
        .filter_map(|marker| lower.find(marker).map(|idx| (idx, *marker)))
        .min_by_key(|(idx, _)| *idx)?;
    let rest = &prompt[idx + marker.len()..];
    let normalized = rest.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        implementation_integrity_violation_with_tool_executions,
        pending_post_write_verification_paths, prompt_required_exact_final_answer,
        ToolExecutionRecord,
    };
    use crate::types::ToolCall;

    #[test]
    fn extracts_required_exact_final_answer_block() {
        let prompt =
            "Do the task.\n\nYour final answer must be exactly:\n\nverified=yes\nfile=src/status.ts\nbytes=31\n";
        assert_eq!(
            prompt_required_exact_final_answer(prompt).as_deref(),
            Some("verified=yes\nfile=src/status.ts\nbytes=31")
        );
    }

    #[test]
    fn extracts_required_exact_final_answer_block_from_reply_with_exactly() {
        let prompt = "Fix it.\n\nReply with exactly:\n\nverified fix\n";
        assert_eq!(
            prompt_required_exact_final_answer(prompt).as_deref(),
            Some("verified fix")
        );
    }

    #[test]
    fn edit_counts_as_effective_write_and_requires_post_write_readback() {
        let prompt = "Update main.rs to return 2. Before finishing, run node --test successfully.";
        let calls = vec![
            ToolCall {
                id: "tc_read".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"main.rs"}),
            },
            ToolCall {
                id: "tc_edit".to_string(),
                name: "edit".to_string(),
                arguments: serde_json::json!({"path":"main.rs","old_string":"1","new_string":"2"}),
            },
        ];
        let execs = vec![
            ToolExecutionRecord {
                name: "read_file".to_string(),
                path: Some("main.rs".to_string()),
                ok: true,
                changed: None,
            },
            ToolExecutionRecord {
                name: "edit".to_string(),
                path: Some("main.rs".to_string()),
                ok: true,
                changed: Some(true),
            },
        ];
        let err = implementation_integrity_violation_with_tool_executions(
            prompt,
            "verified fix",
            &calls,
            &execs,
            true,
        )
        .expect("expected missing post-write verification");
        assert!(err.contains("post-write verification missing read_file"));
        assert!(!err.contains("without an effective write"));

        let pending = pending_post_write_verification_paths(&calls, &execs);
        assert!(pending.contains("main.rs"));
    }
}
