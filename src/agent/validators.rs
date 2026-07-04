use serde::{Deserialize, Serialize};

use crate::agent_impl_guard::ToolExecutionRecord;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ValidatorKind {
    Rust,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ValidatorStepName {
    Fmt,
    Test,
    Clippy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ValidatorStep {
    pub(crate) name: ValidatorStepName,
    pub(crate) command: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ValidatorStepStatus {
    Pass,
    Fail,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ValidatorStepOutcome {
    pub(crate) step: ValidatorStep,
    pub(crate) status: ValidatorStepStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ValidatorRunOutcome {
    pub(crate) kind: ValidatorKind,
    pub(crate) label: String,
    pub(crate) steps: Vec<ValidatorStepOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ValidatorCommand {
    pub(crate) kind: ValidatorKind,
    pub(crate) label: String,
    pub(crate) command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) steps: Vec<ValidatorStep>,
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
    let steps = rust_validator_steps();
    ValidatorCommand {
        kind: ValidatorKind::Rust,
        label: "rust_default".to_string(),
        command: "cargo fmt --check && cargo test && cargo clippy -- -D warnings".to_string(),
        steps,
    }
}

fn path_triggers_rust_validator(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    normalized.ends_with(".rs") || normalized == "cargo.toml" || normalized.ends_with("/cargo.toml")
}

fn rust_validator_steps() -> Vec<ValidatorStep> {
    vec![
        ValidatorStep {
            name: ValidatorStepName::Fmt,
            command: "cargo fmt --check".to_string(),
        },
        ValidatorStep {
            name: ValidatorStepName::Test,
            command: "cargo test".to_string(),
        },
        ValidatorStep {
            name: ValidatorStepName::Clippy,
            command: "cargo clippy -- -D warnings".to_string(),
        },
    ]
}

impl ValidatorCommand {
    pub(crate) fn steps_or_legacy_command(&self) -> Vec<ValidatorStep> {
        if self.steps.is_empty() {
            return vec![ValidatorStep {
                name: ValidatorStepName::Test,
                command: self.command.clone(),
            }];
        }
        self.steps.clone()
    }

    pub(crate) fn first_step(&self) -> Option<ValidatorStep> {
        self.steps_or_legacy_command().into_iter().next()
    }

    pub(crate) fn run_outcome(&self, outcomes: &[ValidatorStepOutcome]) -> ValidatorRunOutcome {
        ValidatorRunOutcome {
            kind: self.kind,
            label: self.label.clone(),
            steps: outcomes.to_vec(),
        }
    }
}

pub(crate) fn record_step_outcome_for_command(
    validator: &ValidatorCommand,
    outcomes: &mut Vec<ValidatorStepOutcome>,
    command: &str,
    passed: bool,
) -> bool {
    let steps = validator.steps_or_legacy_command();
    let Some(step_index) = steps
        .iter()
        .position(|step| commands_match(&step.command, command))
    else {
        return false;
    };
    let step = steps[step_index].clone();
    if outcomes
        .iter()
        .any(|outcome| outcome.step.name == step.name)
    {
        return false;
    }
    outcomes.push(ValidatorStepOutcome {
        step,
        status: if passed {
            ValidatorStepStatus::Pass
        } else {
            ValidatorStepStatus::Fail
        },
    });
    if !passed {
        for skipped in steps.into_iter().skip(step_index + 1) {
            outcomes.push(ValidatorStepOutcome {
                step: skipped,
                status: ValidatorStepStatus::Skipped,
            });
        }
    }
    true
}

pub(crate) fn next_pending_step(
    validator: &ValidatorCommand,
    outcomes: &[ValidatorStepOutcome],
) -> Option<ValidatorStep> {
    let steps = validator.steps_or_legacy_command();
    steps.into_iter().find(|step| {
        !outcomes
            .iter()
            .any(|outcome| outcome.step.name == step.name)
    })
}

pub(crate) fn all_steps_passed(
    validator: &ValidatorCommand,
    outcomes: &[ValidatorStepOutcome],
) -> bool {
    let steps = validator.steps_or_legacy_command();
    !steps.is_empty()
        && steps.iter().all(|step| {
            outcomes.iter().any(|outcome| {
                outcome.step.name == step.name && outcome.status == ValidatorStepStatus::Pass
            })
        })
}

pub(crate) fn failed_step(outcomes: &[ValidatorStepOutcome]) -> Option<&ValidatorStepOutcome> {
    outcomes
        .iter()
        .find(|outcome| outcome.status == ValidatorStepStatus::Fail)
}

pub(crate) fn compact_outcome_evidence(
    validator: &ValidatorCommand,
    outcomes: &[ValidatorStepOutcome],
    repair_budget_remaining: u32,
) -> String {
    let failed = failed_step(outcomes);
    let mut lines = Vec::new();
    if let Some(failed) = failed {
        lines.push(format!("validator_failed: {}", failed.step.command));
    } else if all_steps_passed(validator, outcomes) {
        lines.push("validator_passed: rust_default".to_string());
    } else {
        lines.push("validator_progress: rust_default".to_string());
    }
    let skipped = outcomes
        .iter()
        .filter(|outcome| outcome.status == ValidatorStepStatus::Skipped)
        .map(|outcome| outcome.step.command.clone())
        .collect::<Vec<_>>();
    if !skipped.is_empty() {
        lines.push("skipped:".to_string());
        lines.extend(skipped.into_iter().map(|command| format!("- {command}")));
    }
    lines.push(format!(
        "repair_budget_remaining: {repair_budget_remaining}"
    ));
    lines.join("\n")
}

fn commands_match(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

#[cfg(test)]
mod tests {
    use super::{
        all_steps_passed, compact_outcome_evidence, infer_validator_command,
        infer_validator_command_from_executions, next_pending_step,
        record_step_outcome_for_command, RepairBudget, ValidatorKind, ValidatorStepStatus,
        ValidatorTrigger,
    };
    use crate::agent_impl_guard::ToolExecutionRecord;

    #[test]
    fn rust_source_write_infers_rust_validator_suite() {
        let command = infer_validator_command(&ValidatorTrigger {
            changed_paths: vec!["src/lib.rs".into()],
        })
        .expect("rust validator");
        assert_eq!(command.kind, ValidatorKind::Rust);
        let step_commands = command
            .steps
            .iter()
            .map(|step| step.command.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            step_commands,
            vec![
                "cargo fmt --check",
                "cargo test",
                "cargo clippy -- -D warnings"
            ]
        );
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

    #[test]
    fn records_all_rust_validator_steps_as_passed() {
        let validator = infer_validator_command(&ValidatorTrigger {
            changed_paths: vec!["src/lib.rs".into()],
        })
        .expect("rust validator");
        let mut outcomes = Vec::new();
        assert!(record_step_outcome_for_command(
            &validator,
            &mut outcomes,
            "cargo fmt --check",
            true
        ));
        assert_eq!(
            next_pending_step(&validator, &outcomes)
                .expect("next")
                .command,
            "cargo test"
        );
        assert!(record_step_outcome_for_command(
            &validator,
            &mut outcomes,
            "cargo test",
            true
        ));
        assert!(record_step_outcome_for_command(
            &validator,
            &mut outcomes,
            "cargo clippy -- -D warnings",
            true
        ));
        assert!(all_steps_passed(&validator, &outcomes));
        assert!(outcomes
            .iter()
            .all(|outcome| outcome.status == ValidatorStepStatus::Pass));
    }

    #[test]
    fn fmt_failure_skips_test_and_clippy() {
        let validator = infer_validator_command(&ValidatorTrigger {
            changed_paths: vec!["src/lib.rs".into()],
        })
        .expect("rust validator");
        let mut outcomes = Vec::new();
        assert!(record_step_outcome_for_command(
            &validator,
            &mut outcomes,
            "cargo fmt --check",
            false
        ));
        let statuses = outcomes
            .iter()
            .map(|outcome| (&outcome.step.command, outcome.status))
            .collect::<Vec<_>>();
        assert_eq!(
            statuses,
            vec![
                (&"cargo fmt --check".to_string(), ValidatorStepStatus::Fail),
                (&"cargo test".to_string(), ValidatorStepStatus::Skipped),
                (
                    &"cargo clippy -- -D warnings".to_string(),
                    ValidatorStepStatus::Skipped
                ),
            ]
        );
        let evidence = compact_outcome_evidence(&validator, &outcomes, 1);
        assert!(evidence.contains("validator_failed: cargo fmt --check"));
        assert!(evidence.contains("- cargo test"));
        assert!(evidence.contains("- cargo clippy -- -D warnings"));
        assert!(evidence.contains("repair_budget_remaining: 1"));
    }
}
