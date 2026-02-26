use crate::types::ToolCall;

pub(crate) fn implementation_integrity_violation(
    user_prompt: &str,
    final_output: &str,
    observed_tool_calls: &[ToolCall],
) -> Option<String> {
    if !is_implementation_task_prompt(user_prompt) {
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
    let mut read_paths = std::collections::BTreeSet::<String>::new();
    let mut pending_post_write_verification = std::collections::BTreeSet::<String>::new();
    let allow_new_file_without_read = prompt_allows_new_file_without_read(user_prompt);
    for call in observed_tool_calls {
        match call.name.as_str() {
            "read_file" => {
                if let Some(path) = call
                    .arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(normalize_tool_path)
                {
                    read_paths.insert(path.clone());
                    pending_post_write_verification.remove(&path);
                }
            }
            "apply_patch" => {
                if let Some(path) = call
                    .arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(normalize_tool_path)
                {
                    if !read_paths.contains(&path) {
                        return Some(format!(
                            "implementation guard: apply_patch on '{path}' requires prior read_file on the same path"
                        ));
                    }
                    pending_post_write_verification.insert(path);
                }
            }
            "write_file" => {
                if let Some(path) = call
                    .arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(normalize_tool_path)
                {
                    if !allow_new_file_without_read && !read_paths.contains(&path) {
                        return Some(format!(
                            "implementation guard: write_file on '{path}' requires prior read_file on the same path"
                        ));
                    }
                    pending_post_write_verification.insert(path);
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

fn normalize_tool_path(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

fn is_implementation_task_prompt(prompt: &str) -> bool {
    let p = prompt.to_ascii_lowercase();
    let action = [
        "improve",
        "fix",
        "implement",
        "update",
        "rewrite",
        "refactor",
        "patch",
        "edit",
        "modify",
        "build",
    ]
    .iter()
    .any(|kw| p.contains(kw));
    let artifact = [
        "file",
        "files",
        "directory",
        "dir",
        ".rs",
        ".py",
        ".js",
        ".ts",
        ".tsx",
        ".jsx",
        ".html",
        ".css",
        ".md",
        ".json",
        ".yaml",
        ".yml",
    ]
    .iter()
    .any(|kw| p.contains(kw));
    action && artifact
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
