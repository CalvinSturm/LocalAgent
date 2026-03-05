use crate::types::ToolCall;

#[derive(Debug, Clone)]
pub(crate) struct ToolExecutionRecord {
    pub name: String,
    pub path: Option<String>,
    pub ok: bool,
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
    let allow_new_file_without_read = prompt_allows_new_file_without_read(user_prompt);
    for execution in tool_executions {
        if !execution.ok {
            continue;
        }
        match execution.name.as_str() {
            "read_file" => {
                if let Some(path) = &execution.path {
                    successful_read_paths.insert(path.clone());
                    pending_post_write_verification.remove(path);
                }
            }
            "apply_patch" => {
                if let Some(path) = &execution.path {
                    if !successful_read_paths.contains(path) {
                        return Some(format!(
                            "implementation guard: apply_patch on '{path}' requires prior read_file on the same path"
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
            "apply_patch" | "write_file" => {
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
