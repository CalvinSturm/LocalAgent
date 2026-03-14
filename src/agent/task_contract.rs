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

fn normalize_task_kind_phrase(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut last_was_space = true;
    for ch in value.trim().chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            ' '
        };
        if mapped == ' ' {
            if !last_was_space {
                normalized.push(' ');
            }
            last_was_space = true;
        } else {
            normalized.push(mapped);
            last_was_space = false;
        }
    }
    normalized.trim().to_string()
}

pub(crate) fn canonicalize_task_kind(value: &str) -> String {
    let normalized = normalize_task_kind_phrase(value);
    match normalized.as_str() {
        "coding" | "code" | "code fix" | "code modification" | "bugfix" | "edit" | "fix"
        | "implement" | "implementation" | "patch" | "refactor" => "coding".to_string(),
        "analysis" | "read only" | "read only analysis" | "readonly" | "readonly analysis"
        | "review" => "analysis".to_string(),
        "plan" | "planning" | "planning only" => "planning".to_string(),
        "test" | "testing" | "validate" | "validation" | "validation only" => {
            "validation".to_string()
        }
        "" | "general" => "general".to_string(),
        _ => normalized,
    }
}

pub(crate) fn task_kind_enables_implementation_guard(value: &str) -> bool {
    canonicalize_task_kind(value) == "coding"
}

fn completion_policy_for_task_kind(task_kind: &str) -> CompletionPolicyV1 {
    let requires_write_guards = task_kind == "coding";
    CompletionPolicyV1 {
        require_pre_write_read: requires_write_guards,
        require_post_write_readback: requires_write_guards,
        require_effective_write: requires_write_guards,
    }
}

fn write_requirement_for_task_kind(task_kind: &str) -> WriteRequirement {
    match task_kind {
        "coding" => WriteRequirement::Required,
        "analysis" | "planning" | "validation" => WriteRequirement::None,
        _ => WriteRequirement::Optional,
    }
}

fn normalize_task_kind(value: &str) -> String {
    let canonical = canonicalize_task_kind(value);
    if canonical == "coding" {
        "coding".to_string()
    } else {
        canonical
    }
}

fn prompt_suggests_coding_task(prompt: &str) -> bool {
    let p = prompt.to_ascii_lowercase();
    crate::agent::tool_facts::prompt_requires_effective_write(prompt)
        || crate::agent_impl_guard::prompt_required_validation_command(prompt).is_some()
        || p.contains("landing page")
        || p.contains("index.html")
        || p.contains("html file")
        || p.contains("current directory")
        || p.contains("src/")
        || p.contains("cargo.toml")
        || p.contains("package.json")
}

pub(crate) fn resolve_task_contract(
    args: &RunArgs,
    prompt: &str,
    selected_task_kind: Option<&str>,
    implementation_guard_enabled: bool,
    exposed_tools: &[crate::types::ToolDef],
) -> TaskContractResolution {
    let (task_kind, task_kind_source) = if let Some(value) = args.task_kind.as_deref() {
        (normalize_task_kind(value), ContractValueSource::Explicit)
    } else if let Some(value) = selected_task_kind {
        (normalize_task_kind(value), ContractValueSource::Explicit)
    } else if implementation_guard_enabled && prompt_suggests_coding_task(prompt) {
        ("coding".to_string(), ContractValueSource::Inferred)
    } else {
        ("general".to_string(), ContractValueSource::Defaulted)
    };

    let write_requirement = write_requirement_for_task_kind(&task_kind);
    let completion_policy = completion_policy_for_task_kind(&task_kind);
    let defaulted_task_kind =
        task_kind == "general" && matches!(task_kind_source, ContractValueSource::Defaulted);
    let write_requirement_source = if defaulted_task_kind {
        ContractValueSource::Defaulted
    } else {
        task_kind_source.clone()
    };
    let completion_policy_source = if defaulted_task_kind {
        ContractValueSource::Defaulted
    } else {
        task_kind_source.clone()
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
            completion_policy,
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
            completion_policy: completion_policy_source,
            retry_policy: ContractValueSource::Defaulted,
            final_answer_mode: final_answer_mode_source,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        canonicalize_task_kind, resolve_task_contract, task_kind_enables_implementation_guard,
        AllowedToolsSemantics, ContractValueSource, FinalAnswerMode, ValidationRequirement,
        WriteRequirement,
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
    fn task_profile_is_an_explicit_task_kind_source() {
        let args = RunArgs::parse_from(["localagent"]);
        let resolution =
            resolve_task_contract(&args, "do work", Some("planning"), false, &tool_defs(&[]));
        assert_eq!(resolution.contract.task_kind, "planning");
        assert_eq!(
            resolution.provenance.task_kind,
            ContractValueSource::Explicit
        );
    }

    #[test]
    fn task_kind_aliases_are_exact_phrase_based_not_substring_based() {
        assert_eq!(canonicalize_task_kind("Code Fix"), "coding");
        assert_eq!(canonicalize_task_kind("read-only-analysis"), "analysis");
        assert_eq!(canonicalize_task_kind("decoder"), "decoder");
        assert!(!task_kind_enables_implementation_guard("decoder"));
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
            ContractValueSource::Explicit
        );
        assert_eq!(
            resolution.provenance.completion_policy,
            ContractValueSource::Explicit
        );
    }

    #[test]
    fn build_mode_does_not_force_coding_for_general_chat_prompt() {
        let args = RunArgs::parse_from(["localagent"]);
        let resolution = resolve_task_contract(
            &args,
            "Explain what this project does at a high level.",
            None,
            true,
            &tool_defs(&["read_file"]),
        );
        assert_eq!(resolution.contract.task_kind, "general");
        assert_eq!(
            resolution.provenance.task_kind,
            ContractValueSource::Defaulted
        );
        assert_eq!(
            resolution.contract.write_requirement,
            WriteRequirement::Optional
        );
        assert!(
            !resolution
                .contract
                .completion_policy
                .require_effective_write
        );
    }

    #[test]
    fn build_mode_still_infers_coding_for_landing_page_prompt() {
        let args = RunArgs::parse_from(["localagent"]);
        let resolution = resolve_task_contract(
            &args,
            "Create a landing page in the current directory.",
            None,
            true,
            &tool_defs(&["write_file"]),
        );
        assert_eq!(resolution.contract.task_kind, "coding");
        assert_eq!(
            resolution.provenance.task_kind,
            ContractValueSource::Inferred
        );
    }

    #[test]
    fn explicit_analysis_task_kind_controls_contract_defaults() {
        let args = RunArgs::parse_from(["localagent", "--task-kind", "analysis"]);
        let resolution = resolve_task_contract(
            &args,
            "summarize the repository",
            None,
            true,
            &tool_defs(&[]),
        );
        assert_eq!(resolution.contract.task_kind, "analysis");
        assert_eq!(
            resolution.contract.write_requirement,
            WriteRequirement::None
        );
        assert!(!resolution.contract.completion_policy.require_pre_write_read);
        assert!(
            !resolution
                .contract
                .completion_policy
                .require_post_write_readback
        );
        assert!(
            !resolution
                .contract
                .completion_policy
                .require_effective_write
        );
        assert_eq!(
            resolution.provenance.write_requirement,
            ContractValueSource::Explicit
        );
        assert_eq!(
            resolution.provenance.completion_policy,
            ContractValueSource::Explicit
        );
    }

    #[test]
    fn explicit_planning_task_kind_controls_contract_defaults() {
        let args = RunArgs::parse_from(["localagent", "--task-kind", "planning"]);
        let resolution =
            resolve_task_contract(&args, "plan the migration", None, false, &tool_defs(&[]));
        assert_eq!(
            resolution.contract.write_requirement,
            WriteRequirement::None
        );
        assert!(
            !resolution
                .contract
                .completion_policy
                .require_effective_write
        );
        assert_eq!(
            resolution.provenance.write_requirement,
            ContractValueSource::Explicit
        );
    }

    #[test]
    fn explicit_validation_task_kind_controls_contract_defaults() {
        let args = RunArgs::parse_from(["localagent", "--task-kind", "validation"]);
        let resolution = resolve_task_contract(&args, "run checks", None, false, &tool_defs(&[]));
        assert_eq!(
            resolution.contract.write_requirement,
            WriteRequirement::None
        );
        assert!(
            !resolution
                .contract
                .completion_policy
                .require_effective_write
        );
        assert_eq!(
            resolution.provenance.write_requirement,
            ContractValueSource::Explicit
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
