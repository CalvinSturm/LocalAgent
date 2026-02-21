use clap::ValueEnum;

use crate::eval::assert::Assertion;
use crate::eval::fixtures_repo::{cli_bugfix_fixtures, workspace_refactor_fixtures};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EvalPack {
    Coding,
    Browser,
    All,
}

#[derive(Debug, Clone)]
pub enum Fixture {
    WriteFile { path: String, content: String },
    CreateDir { path: String },
}

#[derive(Debug, Clone)]
pub struct RequiredCapabilities {
    pub needs_write_tools: bool,
    pub needs_shell: bool,
}

#[derive(Debug, Clone)]
pub struct VerifierSpec {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub summary_success_contains: String,
}

#[derive(Debug, Clone)]
pub struct EvalTask {
    pub id: String,
    pub prompt: String,
    pub required_tools: Vec<String>,
    pub assertions: Vec<Assertion>,
    pub fixtures: Vec<Fixture>,
    pub needs_write: bool,
    pub needs_playwright: bool,
    pub optional: bool,
    pub required_capabilities: RequiredCapabilities,
    pub verifier: Option<VerifierSpec>,
}

impl EvalTask {
    pub fn required_flags(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.required_capabilities.needs_write_tools {
            out.push("--enable-write-tools".to_string());
            out.push("--allow-write".to_string());
        }
        if self.required_capabilities.needs_shell {
            out.push("--allow-shell".to_string());
        }
        out
    }
}

pub fn tasks_for_pack(pack: EvalPack) -> Vec<EvalTask> {
    let mut all = Vec::new();
    all.extend(coding_tasks());
    all.extend(browser_tasks());
    all.into_iter()
        .filter(|t| match pack {
            EvalPack::Coding => t.id.starts_with('C'),
            EvalPack::Browser => t.id.starts_with('B'),
            EvalPack::All => true,
        })
        .collect()
}

fn coding_tasks() -> Vec<EvalTask> {
    vec![
        EvalTask {
            id: "C1".to_string(),
            prompt: "Create a new file at src/hello.txt containing exactly hello followed by a newline. Use the write_file tool. Then respond with a brief confirmation.".to_string(),
            required_tools: vec!["write_file".to_string()],
            assertions: vec![
                Assertion::FileExists {
                    path: "src/hello.txt".to_string(),
                },
                Assertion::FileContains {
                    path: "src/hello.txt".to_string(),
                    substring: "hello\n".to_string(),
                },
                Assertion::ToolUsed {
                    name: "write_file".to_string(),
                },
            ],
            fixtures: vec![Fixture::CreateDir {
                path: "src".to_string(),
            }],
            needs_write: true,
            needs_playwright: false,
            optional: false,
            required_capabilities: RequiredCapabilities {
                needs_write_tools: true,
                needs_shell: false,
            },
            verifier: None,
        },
        EvalTask {
            id: "C2".to_string(),
            prompt: "Edit main.rs by using apply_patch so that fn answer() returns 2 instead of 1. Do not rewrite the whole file with write_file. Then confirm done.".to_string(),
            required_tools: vec!["apply_patch".to_string()],
            assertions: vec![
                Assertion::FileContains {
                    path: "main.rs".to_string(),
                    substring: "return 2;".to_string(),
                },
                Assertion::ToolUsed {
                    name: "apply_patch".to_string(),
                },
            ],
            fixtures: vec![Fixture::WriteFile {
                path: "main.rs".to_string(),
                content: "fn answer() -> i32 {\n    return 1;\n}\n".to_string(),
            }],
            needs_write: true,
            needs_playwright: false,
            optional: false,
            required_capabilities: RequiredCapabilities {
                needs_write_tools: true,
                needs_shell: false,
            },
            verifier: None,
        },
        EvalTask {
            id: "C3".to_string(),
            prompt: "In this crate, fix the parsing bug so all tests pass, then run cargo test and summarize the result.".to_string(),
            required_tools: vec!["write_file".to_string(), "shell".to_string()],
            assertions: vec![Assertion::OutputContains {
                substring: "test".to_string(),
            }],
            fixtures: cli_bugfix_fixtures(),
            needs_write: true,
            needs_playwright: false,
            optional: false,
            required_capabilities: RequiredCapabilities {
                needs_write_tools: true,
                needs_shell: true,
            },
            verifier: Some(VerifierSpec {
                command: "cargo".to_string(),
                args: vec!["test".to_string()],
                cwd: ".".to_string(),
                summary_success_contains: "test result: ok".to_string(),
            }),
        },
        EvalTask {
            id: "C4".to_string(),
            prompt: "You are in a Rust workspace fixture. Fix the failing test, refactor libcore::combine signature from two i32 args to one tuple argument across both crates, and update README with a short line starting with 'Refactor note:'. Prefer apply_patch for edits. After edits, run cargo test and report success."
                .to_string(),
            required_tools: vec![
                "apply_patch".to_string(),
                "write_file".to_string(),
                "shell".to_string(),
            ],
            assertions: vec![
                Assertion::FileContains {
                    path: "crates/libcore/src/lib.rs".to_string(),
                    substring: "combine(pair: (i32, i32))".to_string(),
                },
                Assertion::FileContains {
                    path: "crates/app/src/main.rs".to_string(),
                    substring: "combine((10, 5))".to_string(),
                },
                Assertion::FileContains {
                    path: "README.md".to_string(),
                    substring: "Refactor note:".to_string(),
                },
            ],
            fixtures: workspace_refactor_fixtures(),
            needs_write: true,
            needs_playwright: false,
            optional: false,
            required_capabilities: RequiredCapabilities {
                needs_write_tools: true,
                needs_shell: true,
            },
            verifier: Some(VerifierSpec {
                command: "cargo".to_string(),
                args: vec!["test".to_string()],
                cwd: ".".to_string(),
                summary_success_contains: "test result: ok".to_string(),
            }),
        },
        EvalTask {
            id: "C5".to_string(),
            prompt: "Fix the parsing bug in this CLI fixture and add one additional regression test named parses_spaced_count_extra in tests/regression.rs. Keep the behavior deterministic and then summarize what changed."
                .to_string(),
            required_tools: vec!["write_file".to_string(), "apply_patch".to_string()],
            assertions: vec![
                Assertion::FileContains {
                    path: "tests/regression.rs".to_string(),
                    substring: "parses_spaced_count_extra".to_string(),
                },
                Assertion::FileContains {
                    path: "src/lib.rs".to_string(),
                    substring: "trim()".to_string(),
                },
            ],
            fixtures: cli_bugfix_fixtures(),
            needs_write: true,
            needs_playwright: false,
            optional: false,
            required_capabilities: RequiredCapabilities {
                needs_write_tools: true,
                needs_shell: true,
            },
            verifier: Some(VerifierSpec {
                command: "cargo".to_string(),
                args: vec!["test".to_string()],
                cwd: ".".to_string(),
                summary_success_contains: "test result: ok".to_string(),
            }),
        },
    ]
}

fn browser_tasks() -> Vec<EvalTask> {
    vec![
        EvalTask {
            id: "B1".to_string(),
            prompt: "Using Playwright MCP tools, navigate to https://example.com and return the exact page title.".to_string(),
            required_tools: vec!["mcp.playwright.*".to_string()],
            assertions: vec![
                Assertion::OutputContains {
                    substring: "Example Domain".to_string(),
                },
                Assertion::McpResultContains {
                    substring: "Example Domain".to_string(),
                },
            ],
            fixtures: vec![],
            needs_write: false,
            needs_playwright: true,
            optional: false,
            required_capabilities: RequiredCapabilities {
                needs_write_tools: false,
                needs_shell: false,
            },
            verifier: None,
        },
        EvalTask {
            id: "B2".to_string(),
            prompt: "Using Playwright MCP tools on https://example.com, report the first heading text.".to_string(),
            required_tools: vec!["mcp.playwright.*".to_string()],
            assertions: vec![
                Assertion::OutputContains {
                    substring: "Example Domain".to_string(),
                },
                Assertion::McpResultContains {
                    substring: "Example Domain".to_string(),
                },
            ],
            fixtures: vec![],
            needs_write: false,
            needs_playwright: true,
            optional: true,
            required_capabilities: RequiredCapabilities {
                needs_write_tools: false,
                needs_shell: false,
            },
            verifier: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{tasks_for_pack, EvalPack};

    #[test]
    fn coding_pack_has_c1_to_c5() {
        let tasks = tasks_for_pack(EvalPack::Coding);
        let ids = tasks.into_iter().map(|t| t.id).collect::<Vec<_>>();
        assert!(ids.contains(&"C1".to_string()));
        assert!(ids.contains(&"C2".to_string()));
        assert!(ids.contains(&"C3".to_string()));
        assert!(ids.contains(&"C4".to_string()));
        assert!(ids.contains(&"C5".to_string()));
    }

    #[test]
    fn c4_has_required_flags_metadata() {
        let c4 = tasks_for_pack(EvalPack::Coding)
            .into_iter()
            .find(|t| t.id == "C4")
            .expect("c4");
        let flags = c4.required_flags();
        assert!(flags.contains(&"--enable-write-tools".to_string()));
        assert!(flags.contains(&"--allow-write".to_string()));
        assert!(flags.contains(&"--allow-shell".to_string()));
    }
}
