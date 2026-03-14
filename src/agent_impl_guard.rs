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

pub(crate) fn prompt_required_validation_command(prompt: &str) -> Option<&'static str> {
    let p = prompt.to_ascii_lowercase();
    ["node --test", "cargo test", "npm test", "pnpm test"]
        .into_iter()
        .find(|needle| p.contains(needle))
}

fn extract_inline_exact_answer(rest: &str) -> Option<String> {
    let trimmed = rest.trim_start();
    let mut chars = trimmed.chars();
    let quote = chars.next()?;
    if !matches!(quote, '`' | '"' | '\'') {
        return None;
    }
    let body = &trimmed[quote.len_utf8()..];
    let end = body.find(quote)?;
    let extracted = body[..end].trim();
    (!extracted.is_empty()).then(|| extracted.to_string())
}

pub(crate) fn prompt_required_exact_final_answer(prompt: &str) -> Option<String> {
    let lower = prompt.to_ascii_lowercase();
    let markers = [
        ("your final answer must be exactly:", true),
        ("reply with exactly:", true),
        ("reply with exactly", false),
    ];
    let (idx, marker, requires_block) = markers
        .iter()
        .filter_map(|(marker, requires_block)| {
            lower
                .find(marker)
                .map(|idx| (idx, *marker, *requires_block))
        })
        .min_by_key(|(idx, _, _)| *idx)?;
    let rest = &prompt[idx + marker.len()..];
    if requires_block {
        let normalized = rest.trim();
        if normalized.is_empty() {
            None
        } else {
            Some(normalized.to_string())
        }
    } else {
        extract_inline_exact_answer(rest)
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
    fn extracts_required_exact_final_answer_from_inline_backticks() {
        let prompt = "Inspect the code and reply with exactly `fixed: src/math.rs` after the edit.";
        assert_eq!(
            prompt_required_exact_final_answer(prompt).as_deref(),
            Some("fixed: src/math.rs")
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
