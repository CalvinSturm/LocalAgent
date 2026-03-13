use serde::{Deserialize, Serialize};

use crate::cli_args::RunArgs;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriteRequirement {
    None,
    Optional,
    Required,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ValidationRequirement {
    None,
    Command { command: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinalAnswerMode {
    Freeform,
    Exact { required_text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AllowedToolsSemantics {
    ExposedSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContractValueSource {
    Explicit,
    Inferred,
    Defaulted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletionPolicyV1 {
    pub require_pre_write_read: bool,
    pub require_post_write_readback: bool,
    pub require_effective_write: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetryPolicyV1 {
    pub max_schema_repairs: u32,
    pub max_repeat_failures_per_key: u32,
    pub max_runtime_blocked_completions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskContractV1 {
    pub task_kind: String,
    pub write_requirement: WriteRequirement,
    pub validation_requirement: ValidationRequirement,
    pub allowed_tools: Option<Vec<String>>,
    pub allowed_tools_semantics: AllowedToolsSemantics,
    pub completion_policy: CompletionPolicyV1,
    pub retry_policy: RetryPolicyV1,
    pub final_answer_mode: FinalAnswerMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskContractProvenanceV1 {
    pub task_kind: ContractValueSource,
    pub write_requirement: ContractValueSource,
    pub validation_requirement: ContractValueSource,
    pub allowed_tools: ContractValueSource,
    pub allowed_tools_semantics: ContractValueSource,
    pub completion_policy: ContractValueSource,
    pub retry_policy: ContractValueSource,
    pub final_answer_mode: ContractValueSource,
}

#[derive(Debug, Clone)]
pub(crate) struct TaskContractResolution {
    pub(crate) contract: TaskContractV1,
    pub(crate) provenance: TaskContractProvenanceV1,
}

fn task_kind_is_coding_like(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    lowered.contains("coding")
        || lowered.contains("code")
        || lowered.contains("implement")
        || lowered.contains("fix")
        || lowered.contains("refactor")
        || lowered.contains("patch")
        || lowered.contains("edit")
        || lowered.contains("bugfix")
}

fn normalize_task_kind(value: &str) -> String {
    let lowered = value.trim().to_ascii_lowercase();
    if task_kind_is_coding_like(&lowered) {
        "coding".to_string()
    } else if lowered.contains("analysis") || lowered.contains("read") || lowered.contains("review")
    {
        "analysis".to_string()
    } else if lowered.contains("plan") {
        "planning".to_string()
    } else if lowered.contains("validation") || lowered.contains("test") {
        "validation".to_string()
    } else if lowered.is_empty() {
        "general".to_string()
    } else {
        lowered
    }
}

pub(crate) fn resolve_task_contract(
    args: &RunArgs,
    prompt: &str,
    selected_task_profile: Option<&str>,
    implementation_guard_enabled: bool,
    exposed_tools: &[crate::types::ToolDef],
) -> TaskContractResolution {
    let (task_kind, task_kind_source) = if let Some(value) = args.task_kind.as_deref() {
        (normalize_task_kind(value), ContractValueSource::Explicit)
    } else if let Some(value) = selected_task_profile {
        (normalize_task_kind(value), ContractValueSource::Inferred)
    } else if implementation_guard_enabled {
        ("coding".to_string(), ContractValueSource::Inferred)
    } else {
        ("general".to_string(), ContractValueSource::Defaulted)
    };

    let (write_requirement, write_requirement_source) = if implementation_guard_enabled {
        (WriteRequirement::Required, ContractValueSource::Inferred)
    } else if task_kind == "analysis" || task_kind == "planning" || task_kind == "validation" {
        (WriteRequirement::None, ContractValueSource::Inferred)
    } else {
        (WriteRequirement::Optional, ContractValueSource::Defaulted)
    };

    let (validation_requirement, validation_requirement_source) =
        if let Some(command) = args.validation_command_override.as_deref() {
            (
                ValidationRequirement::Command {
                    command: command.to_string(),
                },
                ContractValueSource::Explicit,
            )
        } else if let Some(command) =
            crate::agent_impl_guard::prompt_required_validation_command(prompt)
        {
            (
                ValidationRequirement::Command {
                    command: command.to_string(),
                },
                ContractValueSource::Inferred,
            )
        } else {
            (ValidationRequirement::None, ContractValueSource::Defaulted)
        };

    let allowed_tools = {
        let mut names = exposed_tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        Some(names)
    };

    let (final_answer_mode, final_answer_mode_source) =
        if let Some(required_text) = args.exact_final_answer_override.as_deref() {
            (
                FinalAnswerMode::Exact {
                    required_text: required_text.to_string(),
                },
                ContractValueSource::Explicit,
            )
        } else if let Some(required_text) =
            crate::agent_impl_guard::prompt_required_exact_final_answer(prompt)
        {
            (
                FinalAnswerMode::Exact { required_text },
                ContractValueSource::Inferred,
            )
        } else {
            (FinalAnswerMode::Freeform, ContractValueSource::Defaulted)
        };

    TaskContractResolution {
        contract: TaskContractV1 {
            task_kind,
            write_requirement,
            validation_requirement,
            allowed_tools,
            allowed_tools_semantics: AllowedToolsSemantics::ExposedSnapshot,
            completion_policy: CompletionPolicyV1 {
                require_pre_write_read: implementation_guard_enabled,
                require_post_write_readback: implementation_guard_enabled,
                require_effective_write: implementation_guard_enabled,
            },
            retry_policy: RetryPolicyV1 {
                max_schema_repairs: 2,
                max_repeat_failures_per_key: 3,
                max_runtime_blocked_completions: 2,
            },
            final_answer_mode,
        },
        provenance: TaskContractProvenanceV1 {
            task_kind: task_kind_source,
            write_requirement: write_requirement_source,
            validation_requirement: validation_requirement_source,
            allowed_tools: ContractValueSource::Inferred,
            allowed_tools_semantics: ContractValueSource::Defaulted,
            completion_policy: if implementation_guard_enabled {
                ContractValueSource::Inferred
            } else {
                ContractValueSource::Defaulted
            },
            retry_policy: ContractValueSource::Defaulted,
            final_answer_mode: final_answer_mode_source,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_task_contract, AllowedToolsSemantics, ContractValueSource, FinalAnswerMode,
        ValidationRequirement, WriteRequirement,
    };
    use crate::cli_args::RunArgs;
    use clap::Parser;

    fn tool_defs(names: &[&str]) -> Vec<crate::types::ToolDef> {
        names
            .iter()
            .map(|name| crate::types::ToolDef {
                name: (*name).to_string(),
                description: String::new(),
                parameters: serde_json::json!({}),
                side_effects: crate::types::SideEffects::None,
            })
            .collect()
    }

    #[test]
    fn explicit_task_kind_wins_and_normalizes() {
        let args = RunArgs::parse_from(["localagent", "--task-kind", "Code Fix"]);
        let resolution =
            resolve_task_contract(&args, "do work", None, false, &tool_defs(&["read_file"]));
        assert_eq!(resolution.contract.task_kind, "coding");
        assert_eq!(
            resolution.provenance.task_kind,
            ContractValueSource::Explicit
        );
        assert_eq!(
            resolution.contract.allowed_tools_semantics,
            AllowedToolsSemantics::ExposedSnapshot
        );
    }

    #[test]
    fn implementation_guard_implies_required_write_contract() {
        let args = RunArgs::parse_from(["localagent"]);
        let resolution = resolve_task_contract(
            &args,
            "Fix main.rs",
            Some("coding"),
            true,
            &tool_defs(&["read_file", "edit"]),
        );
        assert_eq!(
            resolution.contract.write_requirement,
            WriteRequirement::Required
        );
        assert!(
            resolution
                .contract
                .completion_policy
                .require_effective_write
        );
        assert_eq!(
            resolution.provenance.write_requirement,
            ContractValueSource::Inferred
        );
    }

    #[test]
    fn prompt_validation_and_exact_answer_are_inferred() {
        let args = RunArgs::parse_from(["localagent"]);
        let resolution = resolve_task_contract(
            &args,
            "Before finishing, run node --test successfully.\n\nReply with exactly:\n\nverified fix\n",
            None,
            false,
            &tool_defs(&["read_file", "shell"]),
        );
        assert_eq!(
            resolution.contract.validation_requirement,
            ValidationRequirement::Command {
                command: "node --test".to_string()
            }
        );
        assert_eq!(
            resolution.contract.final_answer_mode,
            FinalAnswerMode::Exact {
                required_text: "verified fix".to_string()
            }
        );
    }

    #[test]
    fn explicit_validation_override_beats_prompt_inference() {
        let mut args = RunArgs::parse_from(["localagent"]);
        args.validation_command_override = Some("cargo test --workspace".to_string());
        let resolution = resolve_task_contract(
            &args,
            "Before finishing, run node --test successfully.",
            None,
            false,
            &tool_defs(&["read_file", "shell"]),
        );
        assert_eq!(
            resolution.contract.validation_requirement,
            ValidationRequirement::Command {
                command: "cargo test --workspace".to_string()
            }
        );
        assert_eq!(
            resolution.provenance.validation_requirement,
            ContractValueSource::Explicit
        );
    }

    #[test]
    fn explicit_exact_final_answer_override_beats_prompt_inference() {
        let mut args = RunArgs::parse_from(["localagent"]);
        args.exact_final_answer_override = Some("tests passed".to_string());
        let resolution = resolve_task_contract(
            &args,
            "Reply with exactly:\n\nverified fix\n",
            None,
            false,
            &tool_defs(&["read_file", "shell"]),
        );
        assert_eq!(
            resolution.contract.final_answer_mode,
            FinalAnswerMode::Exact {
                required_text: "tests passed".to_string()
            }
        );
        assert_eq!(
            resolution.provenance.final_answer_mode,
            ContractValueSource::Explicit
        );
    }
}
