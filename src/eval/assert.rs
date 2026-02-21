use std::path::Path;

use crate::agent::AgentOutcome;
use crate::types::Role;
use globset::Glob;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Assertion {
    FileExists { path: String },
    FileContains { path: String, substring: String },
    ToolUsed { name: String },
    ToolUsedGlob { pattern: String },
    ToolUsedPrefix { prefix: String },
    ToolArgContains { tool: String, substring: String },
    ToolNotUsed { pattern: String },
    ToolNotUsedGlob { pattern: String },
    OutputContains { substring: String },
    McpResultContains { substring: String },
}

pub fn evaluate_assertions(
    assertions: &[Assertion],
    workdir: &Path,
    outcome: &AgentOutcome,
) -> Vec<String> {
    let mut failures = Vec::new();
    for assertion in assertions {
        match assertion {
            Assertion::FileExists { path } => {
                let full = workdir.join(path);
                if !full.exists() {
                    failures.push(format!("assertion failed: file_exists({path})"));
                }
            }
            Assertion::FileContains { path, substring } => {
                let full = workdir.join(path);
                match std::fs::read_to_string(&full) {
                    Ok(content) => {
                        if !content.contains(substring) {
                            failures.push(format!(
                                "assertion failed: file_contains({path}, {:?})",
                                substring
                            ));
                        }
                    }
                    Err(_) => {
                        failures.push(format!("assertion failed: file_contains({path}, ..)"));
                    }
                }
            }
            Assertion::ToolUsed { name } => {
                let used = outcome.tool_calls.iter().any(|tc| tc.name == *name);
                if !used {
                    failures.push(format!("assertion failed: tool_used({name})"));
                }
            }
            Assertion::ToolUsedGlob { pattern } => {
                let used = outcome
                    .tool_calls
                    .iter()
                    .any(|tc| matches_pattern(&tc.name, pattern));
                if !used {
                    failures.push(format!("assertion failed: tool_used_glob({pattern})"));
                }
            }
            Assertion::ToolUsedPrefix { prefix } => {
                let used = outcome
                    .tool_calls
                    .iter()
                    .any(|tc| tc.name.starts_with(prefix));
                if !used {
                    failures.push(format!("assertion failed: tool_used_prefix({prefix})"));
                }
            }
            Assertion::ToolArgContains { tool, substring } => {
                let used = outcome
                    .tool_calls
                    .iter()
                    .filter(|tc| tc.name == *tool)
                    .any(|tc| tc.arguments.to_string().contains(substring));
                if !used {
                    failures.push(format!(
                        "assertion failed: tool_arg_contains({}, {:?})",
                        tool, substring
                    ));
                }
            }
            Assertion::ToolNotUsed { pattern } | Assertion::ToolNotUsedGlob { pattern } => {
                let used = outcome
                    .tool_calls
                    .iter()
                    .any(|tc| matches_pattern(&tc.name, pattern));
                if used {
                    failures.push(format!("assertion failed: tool_not_used({pattern})"));
                }
            }
            Assertion::OutputContains { substring } => {
                if !outcome.final_output.contains(substring) {
                    failures.push(format!(
                        "assertion failed: output_contains({:?})",
                        substring
                    ));
                }
            }
            Assertion::McpResultContains { substring } => {
                let found = outcome.messages.iter().any(|m| {
                    matches!(m.role, Role::Tool)
                        && m.tool_name
                            .as_deref()
                            .is_some_and(|name| name.starts_with("mcp."))
                        && m.content
                            .as_deref()
                            .is_some_and(|content| content.contains(substring))
                });
                if !found {
                    failures.push(format!(
                        "assertion failed: mcp_result_contains({:?})",
                        substring
                    ));
                }
            }
        }
    }
    failures
}

fn matches_pattern(name: &str, pattern: &str) -> bool {
    if pattern.contains('*') || pattern.contains('?') || pattern.contains('[') {
        Glob::new(pattern)
            .map(|g| g.compile_matcher().is_match(name))
            .unwrap_or(false)
    } else {
        name == pattern
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{evaluate_assertions, Assertion};
    use crate::agent::{AgentExitReason, AgentOutcome};
    use crate::compaction::{CompactionMode, CompactionSettings, ToolResultPersist};
    use crate::types::{Message, ToolCall};

    #[test]
    fn file_assertions_work() {
        let tmp = tempdir().expect("tempdir");
        let file = tmp.path().join("a.txt");
        std::fs::write(&file, "hello world").expect("write");

        let outcome = AgentOutcome {
            run_id: "r".to_string(),
            started_at: "2026-01-01T00:00:00Z".to_string(),
            finished_at: "2026-01-01T00:00:01Z".to_string(),
            exit_reason: AgentExitReason::Ok,
            final_output: String::new(),
            error: None,
            messages: Vec::<Message>::new(),
            tool_calls: Vec::new(),
            tool_decisions: Vec::new(),
            compaction_settings: CompactionSettings {
                max_context_chars: 0,
                mode: CompactionMode::Off,
                keep_last: 20,
                tool_result_persist: ToolResultPersist::Digest,
            },
            final_prompt_size_chars: 0,
            compaction_report: None,
            hook_invocations: Vec::new(),
            provider_retry_count: 0,
            provider_error_count: 0,
            token_usage: None,
        };
        let failures = evaluate_assertions(
            &[
                Assertion::FileExists {
                    path: "a.txt".to_string(),
                },
                Assertion::FileContains {
                    path: "a.txt".to_string(),
                    substring: "hello".to_string(),
                },
            ],
            tmp.path(),
            &outcome,
        );
        assert!(failures.is_empty());
    }

    #[test]
    fn tool_not_used_passes_and_fails() {
        let outcome = AgentOutcome {
            run_id: "r".to_string(),
            started_at: "2026-01-01T00:00:00Z".to_string(),
            finished_at: "2026-01-01T00:00:01Z".to_string(),
            exit_reason: AgentExitReason::Ok,
            final_output: String::new(),
            error: None,
            messages: Vec::<Message>::new(),
            tool_calls: vec![ToolCall {
                id: "1".to_string(),
                name: "shell".to_string(),
                arguments: serde_json::json!({"cmd":"echo"}),
            }],
            tool_decisions: Vec::new(),
            compaction_settings: CompactionSettings {
                max_context_chars: 0,
                mode: CompactionMode::Off,
                keep_last: 20,
                tool_result_persist: ToolResultPersist::Digest,
            },
            final_prompt_size_chars: 0,
            compaction_report: None,
            hook_invocations: Vec::new(),
            provider_retry_count: 0,
            provider_error_count: 0,
            token_usage: None,
        };
        let ok = evaluate_assertions(
            &[Assertion::ToolNotUsedGlob {
                pattern: "write_file".to_string(),
            }],
            std::path::Path::new("."),
            &outcome,
        );
        assert!(ok.is_empty());
        let bad = evaluate_assertions(
            &[Assertion::ToolNotUsedGlob {
                pattern: "shell".to_string(),
            }],
            std::path::Path::new("."),
            &outcome,
        );
        assert_eq!(bad.len(), 1);
    }
}
