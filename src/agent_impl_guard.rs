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
    if !enforce_implementation_integrity_guard {
        return None;
    }
    if observed_tool_calls.is_empty() {
        return Some(
            "implementation guard: file-edit task finalized without any tool calls".to_string(),
        );
    }
    if output_has_placeholder_artifacts(final_output) {
        return Some(
            "implementation guard: final answer contains placeholder artifacts instead of concrete implementation".to_string(),
        );
    }
    let mut successful_read_paths = std::collections::BTreeSet::<String>::new();
    let mut pending_post_write_verification = std::collections::BTreeSet::<String>::new();
    let mut saw_effective_write = false;
    let allow_new_file_without_read = prompt_allows_new_file_without_read(user_prompt);
    for execution in tool_executions {
        if !execution.ok {
            continue;
        }
        if matches!(
            execution.name.as_str(),
            "apply_patch" | "edit" | "write_file" | "str_replace"
        ) {
            let actually_changed = execution.changed.unwrap_or(true);
            if actually_changed {
                saw_effective_write = true;
            }
        }
        match execution.name.as_str() {
            "read_file" => {
                if let Some(path) = &execution.path {
                    successful_read_paths.insert(path.clone());
                    pending_post_write_verification.remove(path);
                }
            }
            "apply_patch" | "edit" | "str_replace" => {
                if let Some(path) = &execution.path {
                    if !successful_read_paths.contains(path) {
                        return Some(format!(
                            "implementation guard: {} on '{path}' requires prior read_file on the same path",
                            execution.name
                        ));
                    }
                    pending_post_write_verification.insert(path.clone());
                }
            }
            "write_file" => {
                if let Some(path) = &execution.path {
                    if !allow_new_file_without_read && !successful_read_paths.contains(path) {
                        return Some(format!(
                            "implementation guard: write_file on '{path}' requires prior read_file on the same path"
                        ));
                    }
                    pending_post_write_verification.insert(path.clone());
                }
            }
            _ => {}
        }
    }
    if prompt_requires_effective_write(user_prompt) && !saw_effective_write {
        return Some(
            "implementation guard: file-edit task finalized without an effective write (writes failed or write tool changed:false)".to_string(),
        );
    }
    if let Some(path) = pending_post_write_verification.iter().next() {
        return Some(format!(
            "implementation guard: post-write verification missing read_file on '{path}'"
        ));
    }
    None
}

pub(crate) fn pending_post_write_verification_paths(
    tool_executions: &[ToolExecutionRecord],
) -> std::collections::BTreeSet<String> {
    let mut pending_post_write_verification = std::collections::BTreeSet::<String>::new();
    for execution in tool_executions {
        if !execution.ok {
            continue;
        }
        match execution.name.as_str() {
            "read_file" => {
                if let Some(path) = &execution.path {
                    pending_post_write_verification.remove(path);
                }
            }
            "apply_patch" | "edit" | "write_file" | "str_replace" => {
                if let Some(path) = &execution.path {
                    pending_post_write_verification.insert(path.clone());
                }
            }
            _ => {}
        }
    }
    pending_post_write_verification
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

fn prompt_allows_new_file_without_read(prompt: &str) -> bool {
    let p = prompt.to_ascii_lowercase();
    p.contains("create a new file")
        || p.contains("create new file")
        || p.contains("new file at")
        || p.contains("add new file")
        || p.contains("create `")
        || p.contains("create the file")
}

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

fn output_has_placeholder_artifacts(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    let patterns = [
        "... (full implementation) ...",
        "...full implementation...",
        "same css as before",
        "same html structure",
        "additional improvements coming",
        "todo:",
        "coming soon",
    ];
    patterns.iter().any(|p| t.contains(p))
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

fn shell_command_text(call: &ToolCall) -> Option<String> {
    if call.name != "shell" {
        return None;
    }
    let obj = call.arguments.as_object()?;
    if let Some(command) = obj.get("command").and_then(|v| v.as_str()) {
        return Some(command.to_string());
    }
    let cmd = obj.get("cmd").and_then(|v| v.as_str())?;
    let mut parts = vec![cmd.to_string()];
    if let Some(args) = obj.get("args").and_then(|v| v.as_array()) {
        for arg in args {
            if let Some(arg) = arg.as_str() {
                parts.push(arg.to_string());
            }
        }
    }
    Some(parts.join(" "))
}

pub(crate) fn required_validation_command_satisfied(
    prompt: &str,
    observed_tool_calls: &[ToolCall],
    tool_executions: &[ToolExecutionRecord],
) -> bool {
    let Some(required) = prompt_required_validation_command(prompt) else {
        return true;
    };
    let mut successful_shell_executions = tool_executions
        .iter()
        .filter(|execution| execution.name == "shell" && execution.ok);
    observed_tool_calls
        .iter()
        .filter(|call| call.name == "shell")
        .any(|call| {
            shell_command_text(call).is_some_and(|cmd| {
                cmd.to_ascii_lowercase().contains(required)
                    && successful_shell_executions.next().is_some()
            })
        })
}

pub(crate) fn prompt_required_exact_final_answer(prompt: &str) -> Option<String> {
    let lower = prompt.to_ascii_lowercase();
    let marker = "your final answer must be exactly:";
    let idx = lower.find(marker)?;
    let rest = &prompt[idx + marker.len()..];
    let normalized = rest.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

pub(crate) fn final_output_matches_required_exact_answer(prompt: &str, final_output: &str) -> bool {
    let Some(required) = prompt_required_exact_final_answer(prompt) else {
        return true;
    };
    final_output.trim() == required.trim()
}

#[cfg(test)]
mod tests {
    use super::{
        final_output_matches_required_exact_answer,
        implementation_integrity_violation_with_tool_executions,
        pending_post_write_verification_paths, prompt_required_exact_final_answer,
        required_validation_command_satisfied, ToolExecutionRecord,
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

        let pending = pending_post_write_verification_paths(&execs);
        assert!(pending.contains("main.rs"));
    }
}
