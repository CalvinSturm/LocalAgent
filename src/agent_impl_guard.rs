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
            "apply_patch" | "write_file" | "str_replace"
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
            "apply_patch" | "str_replace" => {
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
            "implementation guard: file-edit task finalized without an effective write (writes failed or apply_patch changed:false)".to_string(),
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
            "apply_patch" | "write_file" | "str_replace" => {
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
