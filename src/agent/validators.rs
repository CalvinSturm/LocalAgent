use serde::{Deserialize, Serialize};

use crate::agent_impl_guard::ToolExecutionRecord;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ValidatorKind {
    Rust,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ValidatorCommand {
    pub(crate) kind: ValidatorKind,
    pub(crate) label: String,
    pub(crate) command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ValidatorTrigger {
    pub(crate) changed_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ValidatorOutcome {
    Passed,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RepairBudget {
    pub(crate) used: u32,
    pub(crate) max: u32,
}

impl RepairBudget {
    pub(crate) fn one_repair(used: u32) -> Self {
        Self { used, max: 1 }
    }

    pub(crate) fn can_authorize_repair(&self) -> bool {
        self.used < self.max
    }
}

pub(crate) fn infer_validator_command_from_executions(
    executions: &[ToolExecutionRecord],
) -> Option<ValidatorCommand> {
    let changed_paths = executions
        .iter()
        .filter(|execution| {
            execution.ok
                && matches!(
                    execution.name.as_str(),
                    "apply_patch" | "edit" | "write_file" | "str_replace"
                )
                && execution.changed.unwrap_or(true)
        })
        .filter_map(|execution| execution.path.clone())
        .collect::<Vec<_>>();
    infer_validator_command(&ValidatorTrigger { changed_paths })
}

pub(crate) fn infer_validator_command(trigger: &ValidatorTrigger) -> Option<ValidatorCommand> {
    if trigger
        .changed_paths
        .iter()
        .any(|path| path_triggers_rust_validator(path))
    {
        return Some(rust_validator_command());
    }
    None
}

fn rust_validator_command() -> ValidatorCommand {
    ValidatorCommand {
        kind: ValidatorKind::Rust,
        label: "rust_default".to_string(),
        command: "cargo fmt --check && cargo test && cargo clippy -- -D warnings".to_string(),
    }
}

fn path_triggers_rust_validator(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    normalized.ends_with(".rs") || normalized == "cargo.toml" || normalized.ends_with("/cargo.toml")
}

#[cfg(test)]
mod tests {
    use super::{
        infer_validator_command, infer_validator_command_from_executions, RepairBudget,
        ValidatorKind, ValidatorTrigger,
    };
    use crate::agent_impl_guard::ToolExecutionRecord;

    #[test]
    fn rust_source_write_infers_rust_validator_suite() {
        let command = infer_validator_command(&ValidatorTrigger {
            changed_paths: vec!["src/lib.rs".into()],
        })
        .expect("rust validator");
        assert_eq!(command.kind, ValidatorKind::Rust);
        assert!(command.command.contains("cargo fmt --check"));
        assert!(command.command.contains("cargo test"));
        assert!(command.command.contains("cargo clippy -- -D warnings"));
    }

    #[test]
    fn cargo_toml_write_infers_rust_validator_suite() {
        assert!(infer_validator_command(&ValidatorTrigger {
            changed_paths: vec!["Cargo.toml".into()]
        })
        .is_some());
    }

    #[test]
    fn read_only_execution_does_not_infer_validator() {
        let executions = vec![ToolExecutionRecord {
            name: "read_file".to_string(),
            path: Some("src/lib.rs".to_string()),
            ok: true,
            changed: None,
        }];
        assert!(infer_validator_command_from_executions(&executions).is_none());
    }

    #[test]
    fn unchanged_write_does_not_infer_validator() {
        let executions = vec![ToolExecutionRecord {
            name: "edit".to_string(),
            path: Some("src/lib.rs".to_string()),
            ok: true,
            changed: Some(false),
        }];
        assert!(infer_validator_command_from_executions(&executions).is_none());
    }

    #[test]
    fn repair_budget_allows_one_repair_only() {
        assert!(RepairBudget::one_repair(0).can_authorize_repair());
        assert!(!RepairBudget::one_repair(1).can_authorize_repair());
    }
}
