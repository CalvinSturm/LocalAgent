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

#[allow(dead_code)]
pub(crate) fn prompt_requires_effective_write(prompt: &str) -> bool {
    let p = prompt.to_ascii_lowercase();
    p.contains("apply_patch")
        || p.contains("write_file")
        || p.contains("edit ")
        || p.contains("fix ")
        || p.contains("modify ")
        || p.contains("update ")
        || p.contains("change ")
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

#[allow(dead_code)]
pub(crate) fn required_validation_command_satisfied(
    prompt: &str,
    observed_tool_calls: &[ToolCall],
    tool_executions: &[ToolExecutionRecord],
) -> bool {
    let tool_facts = crate::agent::tool_facts_from_calls_and_executions(
        prompt_required_validation_command(prompt),
        observed_tool_calls,
        tool_executions,
    );
    crate::agent::required_validation_command_satisfied_from_facts(
        prompt_required_validation_command(prompt),
        &tool_facts,
    )
}

#[allow(dead_code)]
pub(crate) fn required_validation_failure_needs_repair(
    prompt: &str,
    observed_tool_calls: &[ToolCall],
    tool_executions: &[ToolExecutionRecord],
) -> bool {
    let tool_facts = crate::agent::tool_facts_from_calls_and_executions(
        prompt_required_validation_command(prompt),
        observed_tool_calls,
        tool_executions,
    );
    crate::agent::required_validation_failure_needs_repair_from_facts(
        prompt_required_validation_command(prompt),
        &tool_facts,
    )
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

#[allow(dead_code)]
pub(crate) fn final_output_matches_required_exact_answer(prompt: &str, final_output: &str) -> bool {
    let Some(required) = prompt_required_exact_final_answer(prompt) else {
        return true;
    };
    final_output.trim() == required.trim()
}

#[allow(dead_code)]
pub(crate) fn recover_required_exact_final_answer(
    prompt: &str,
    final_output: &str,
) -> Option<String> {
    let required = prompt_required_exact_final_answer(prompt)?;
    let required_trimmed = required.trim();
    let output_trimmed = final_output.trim();
    if output_trimmed == required_trimmed {
        return Some(required);
    }

    let fenced_matches: Vec<&str> = fenced_code_blocks(output_trimmed)
        .into_iter()
        .filter(|block| block.trim() == required_trimmed)
        .collect();
    if fenced_matches.len() == 1 {
        return Some(required);
    }

    let mut substring_matches = 0usize;
    let mut search_start = 0usize;
    while let Some(rel_idx) = output_trimmed[search_start..].find(required_trimmed) {
        let idx = search_start + rel_idx;
        let before_ok = idx == 0
            || output_trimmed[..idx]
                .chars()
                .last()
                .is_some_and(|c| c == '\n' || c == '\r');
        let after_idx = idx + required_trimmed.len();
        let after_ok = after_idx == output_trimmed.len()
            || output_trimmed[after_idx..]
                .chars()
                .next()
                .is_some_and(|c| c == '\n' || c == '\r');
        if before_ok && after_ok {
            substring_matches += 1;
            if substring_matches > 1 {
                return None;
            }
        }
        search_start = idx + required_trimmed.len();
    }
    (substring_matches == 1).then_some(required)
}

#[allow(dead_code)]
fn fenced_code_blocks(text: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after_ticks = &rest[start + 3..];
        let after_lang = match after_ticks.find('\n') {
            Some(idx) => &after_ticks[idx + 1..],
            None => break,
        };
        let Some(end) = after_lang.find("```") else {
            break;
        };
        blocks.push(&after_lang[..end]);
        rest = &after_lang[end + 3..];
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::{
        final_output_matches_required_exact_answer,
        implementation_integrity_violation_with_tool_executions,
        pending_post_write_verification_paths, prompt_required_exact_final_answer,
        recover_required_exact_final_answer, required_validation_command_satisfied,
        required_validation_failure_needs_repair, ToolExecutionRecord,
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
    fn exact_final_answer_match_is_trim_tolerant() {
        let prompt = "Your final answer must be exactly:\n\nverified=yes\nfile=src/status.ts\n";
        assert!(final_output_matches_required_exact_answer(
            prompt,
            "verified=yes\nfile=src/status.ts\n"
        ));
        assert!(!final_output_matches_required_exact_answer(
            prompt,
            "verified=yes"
        ));
    }

    #[test]
    fn exact_final_answer_match_supports_reply_with_exactly() {
        let prompt = "Reply with exactly:\n\nverified fix\n";
        assert!(final_output_matches_required_exact_answer(
            prompt,
            "verified fix\n"
        ));
        assert!(!final_output_matches_required_exact_answer(
            prompt, "verified"
        ));
    }

    #[test]
    fn exact_final_answer_can_be_recovered_from_fenced_block() {
        let prompt =
            "Your final answer must be exactly:\n\nverified=yes\ncommand=node --test\nresult=passed\n";
        let wrapped =
            "Validation passed.\n\n```\nverified=yes\ncommand=node --test\nresult=passed\n```";
        assert_eq!(
            recover_required_exact_final_answer(prompt, wrapped).as_deref(),
            Some("verified=yes\ncommand=node --test\nresult=passed")
        );
    }

    #[test]
    fn exact_final_answer_recovery_requires_single_unambiguous_match() {
        let prompt = "Your final answer must be exactly:\n\nverified=yes\nfile=main.rs\n";
        let ambiguous = "verified=yes\nfile=main.rs\n\nextra\n\nverified=yes\nfile=main.rs\n";
        assert!(recover_required_exact_final_answer(prompt, ambiguous).is_none());
    }

    #[test]
    fn required_validation_command_requires_successful_matching_shell_call() {
        let prompt = "Before finishing, run node --test successfully.";
        let calls = vec![ToolCall {
            id: "tc_shell".to_string(),
            name: "shell".to_string(),
            arguments: serde_json::json!({"cmd":"node","args":["--test"]}),
        }];
        let ok_execs = vec![ToolExecutionRecord {
            name: "shell".to_string(),
            path: None,
            ok: true,
            changed: None,
        }];
        assert!(required_validation_command_satisfied(
            prompt, &calls, &ok_execs
        ));

        let failed_execs = vec![ToolExecutionRecord {
            name: "shell".to_string(),
            path: None,
            ok: false,
            changed: None,
        }];
        assert!(!required_validation_command_satisfied(
            prompt,
            &calls,
            &failed_execs
        ));
    }

    #[test]
    fn failed_required_validation_without_changed_write_needs_repair() {
        let prompt = "Before finishing, run node --test successfully.";
        let calls = vec![ToolCall {
            id: "tc_shell".to_string(),
            name: "shell".to_string(),
            arguments: serde_json::json!({"cmd":"node","args":["--test"]}),
        }];
        let execs = vec![ToolExecutionRecord {
            name: "shell".to_string(),
            path: None,
            ok: false,
            changed: None,
        }];
        assert!(required_validation_failure_needs_repair(
            prompt, &calls, &execs
        ));
    }

    #[test]
    fn changed_write_clears_failed_validation_repair_need() {
        let prompt = "Before finishing, run node --test successfully.";
        let calls = vec![
            ToolCall {
                id: "tc_shell".to_string(),
                name: "shell".to_string(),
                arguments: serde_json::json!({"cmd":"node","args":["--test"]}),
            },
            ToolCall {
                id: "tc_edit".to_string(),
                name: "edit".to_string(),
                arguments: serde_json::json!({"path":"main.rs","old_string":"1","new_string":"2"}),
            },
        ];
        let execs = vec![
            ToolExecutionRecord {
                name: "shell".to_string(),
                path: None,
                ok: false,
                changed: None,
            },
            ToolExecutionRecord {
                name: "edit".to_string(),
                path: Some("main.rs".to_string()),
                ok: true,
                changed: Some(true),
            },
        ];
        assert!(!required_validation_failure_needs_repair(
            prompt, &calls, &execs
        ));
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
