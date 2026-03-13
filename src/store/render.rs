use serde_json::Value;

use crate::types::Message;

use super::RunRecord;

fn format_allowed_tools(record: &RunRecord) -> String {
    let Some(contract) = &record.task_contract else {
        return "-".to_string();
    };
    let tools = contract
        .allowed_tools
        .as_ref()
        .map(|items| items.join(", "))
        .filter(|joined| !joined.is_empty())
        .unwrap_or_else(|| "-".to_string());
    format!("{tools} ({:?})", contract.allowed_tools_semantics)
}

fn push_task_contract_section(out: &mut String, record: &RunRecord) {
    let Some(contract) = &record.task_contract else {
        return;
    };
    out.push_str("task_contract:\n");
    out.push_str(&format!("  task_kind: {}\n", contract.task_kind));
    out.push_str(&format!(
        "  write_requirement: {:?}\n",
        contract.write_requirement
    ));
    out.push_str(&format!(
        "  validation_requirement: {:?}\n",
        contract.validation_requirement
    ));
    out.push_str(&format!("  allowed_tools: {}\n", format_allowed_tools(record)));
    out.push_str(&format!(
        "  final_answer_mode: {:?}\n",
        contract.final_answer_mode
    ));
    out.push_str(&format!(
        "  completion_policy: pre_write_read={} post_write_readback={} effective_write={}\n",
        contract.completion_policy.require_pre_write_read,
        contract.completion_policy.require_post_write_readback,
        contract.completion_policy.require_effective_write
    ));
    out.push_str(&format!(
        "  retry_policy: schema_repairs={} repeat_failures={} blocked_completions={}\n",
        contract.retry_policy.max_schema_repairs,
        contract.retry_policy.max_repeat_failures_per_key,
        contract.retry_policy.max_runtime_blocked_completions
    ));
    if let Some(provenance) = &record.task_contract_provenance {
        out.push_str("task_contract_provenance:\n");
        out.push_str(&format!("  task_kind: {:?}\n", provenance.task_kind));
        out.push_str(&format!(
            "  write_requirement: {:?}\n",
            provenance.write_requirement
        ));
        out.push_str(&format!(
            "  validation_requirement: {:?}\n",
            provenance.validation_requirement
        ));
        out.push_str(&format!("  allowed_tools: {:?}\n", provenance.allowed_tools));
        out.push_str(&format!(
            "  allowed_tools_semantics: {:?}\n",
            provenance.allowed_tools_semantics
        ));
        out.push_str(&format!(
            "  final_answer_mode: {:?}\n",
            provenance.final_answer_mode
        ));
    }
    if let Some(checkpoint) = &record.run_checkpoint {
        out.push_str("run_checkpoint:\n");
        out.push_str(&format!("  phase: {:?}\n", checkpoint.phase));
        out.push_str(&format!(
            "  terminal_boundary: {}\n",
            checkpoint.terminal_boundary
        ));
        if let Some(interrupt) = &checkpoint.pending_interrupt {
            out.push_str(&format!("  interrupt: {:?}\n", interrupt.kind));
        }
    }
}

fn push_tool_facts_section(out: &mut String, record: &RunRecord) {
    if record.tool_facts.is_empty() {
        return;
    }
    out.push_str("tool_facts:\n");
    for fact in &record.tool_facts {
        out.push_str(&format!("  - {:?}\n", fact));
    }
}

fn push_tool_fact_envelopes_section(out: &mut String, record: &RunRecord) {
    if record.tool_fact_envelopes.is_empty() {
        return;
    }
    out.push_str("tool_fact_envelopes:\n");
    for envelope in &record.tool_fact_envelopes {
        out.push_str(&format!(
            "  - source={:?} phase={} checkpoint_phase={} fact={:?}\n",
            envelope.provenance.source,
            envelope.provenance.phase.as_deref().unwrap_or("-"),
            envelope
                .provenance
                .checkpoint_phase
                .as_deref()
                .unwrap_or("-"),
            envelope.fact
        ));
    }
}

pub fn render_replay(record: &RunRecord) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "run_id: {}\nmode: {}\nagent_mode: {}\noutput_mode: {}\nprovider: {}\nmodel: {}\nexit_reason: {}\nPolicy hash: {}\nConfig hash: {}\napproval_mode: {}\nauto_approve_scope: {}\nunsafe: {}\nno_limits: {}\nunsafe_bypass_allow_flags: {}\n",
        record.metadata.run_id,
        record.mode,
        record.cli.agent_mode,
        record.cli.output_mode,
        record.cli.provider,
        record.cli.model,
        record.metadata.exit_reason,
        record.policy_hash_hex.as_deref().unwrap_or("-"),
        record.config_hash_hex,
        record.cli.approval_mode,
        record.cli.auto_approve_scope,
        record.cli.unsafe_mode,
        record.cli.no_limits,
        record.cli.unsafe_bypass_allow_flags
    ));
    out.push_str(&format!("exec_target: {}\n", record.cli.exec_target));
    if let Some(summary) = &record.cli.docker_config_summary {
        out.push_str(&format!("docker_config: {}\n", summary));
    }
    out.push_str(&format!("tui_enabled: {}\n", record.cli.tui_enabled));
    out.push_str(&format!(
        "taint: {} mode={} digest_bytes={}\n",
        record.cli.taint, record.cli.taint_mode, record.cli.taint_digest_bytes
    ));
    if let Some(planner) = &record.planner {
        let steps_count = planner
            .plan_json
            .get("steps")
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0);
        let goal = planner
            .plan_json
            .get("goal")
            .and_then(Value::as_str)
            .unwrap_or_default();
        out.push_str(&format!(
            "planner: model={} ok={} steps={} hash={}\nplanner_goal: {}\n",
            planner.model, planner.ok, steps_count, planner.plan_hash_hex, goal
        ));
    }
    push_task_contract_section(&mut out, record);
    push_tool_facts_section(&mut out, record);
    push_tool_fact_envelopes_section(&mut out, record);
    for m in &record.transcript {
        let content = m.content.clone().unwrap_or_default();
        match m.role {
            crate::types::Role::User => out.push_str(&format!("USER: {}\n", content)),
            crate::types::Role::Assistant => out.push_str(&format!("ASSISTANT: {}\n", content)),
            crate::types::Role::Tool => {
                let name = m.tool_name.clone().unwrap_or_else(|| "unknown".to_string());
                out.push_str(&format!("TOOL({}): {}\n", name, content));
            }
            crate::types::Role::System => out.push_str(&format!("SYSTEM: {}\n", content)),
            crate::types::Role::Developer => out.push_str(&format!("DEVELOPER: {}\n", content)),
        }
    }
    out
}

pub fn extract_session_messages(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .enumerate()
        .filter_map(|(idx, m)| {
            if idx == 0
                && matches!(m.role, crate::types::Role::System)
                && m.content
                    .as_deref()
                    .unwrap_or_default()
                    .contains("You are an agent that may call tools")
            {
                return None;
            }
            if matches!(m.role, crate::types::Role::Developer)
                && m.content
                    .as_deref()
                    .unwrap_or_default()
                    .starts_with(crate::session::TASK_MEMORY_HEADER)
            {
                return None;
            }
            if matches!(m.role, crate::types::Role::Developer)
                && m.content
                    .as_deref()
                    .unwrap_or_default()
                    .starts_with(crate::planner::PLANNER_HANDOFF_HEADER)
            {
                return None;
            }
            Some(m.clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{extract_session_messages, render_replay};
    use crate::agent::{
        AllowedToolsSemantics, CompletionPolicyV1, ContractValueSource, FinalAnswerMode,
        RetryPolicyV1, TaskContractProvenanceV1, TaskContractV1, ValidationRequirement,
        WriteRequirement,
    };
    use crate::types::{Message, Role};

    fn msg(role: Role, content: &str) -> Message {
        Message {
            role,
            content: Some(content.to_string()),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        }
    }

    #[test]
    fn extract_session_messages_skips_agent_prologue_task_memory_and_planner_handoff() {
        let msgs = vec![
            msg(
                Role::System,
                "You are an agent that may call tools to gather information.",
            ),
            msg(
                Role::Developer,
                "TASK MEMORY (user-authored, authoritative)\n- [1] foo: bar",
            ),
            msg(
                Role::Developer,
                "PLANNER HANDOFF (openagent.plan.v1)\n{\"schema_version\":\"openagent.plan.v1\"}",
            ),
            msg(Role::User, "hello"),
            msg(Role::Assistant, "hi"),
        ];

        let out = extract_session_messages(&msgs);
        assert_eq!(out.len(), 2);
        assert!(matches!(out[0].role, Role::User));
        assert!(matches!(out[1].role, Role::Assistant));
    }

    #[test]
    fn extract_session_messages_keeps_non_matching_system_and_developer_messages() {
        let msgs = vec![
            msg(Role::System, "custom system prompt"),
            msg(Role::Developer, "normal developer instruction"),
            msg(Role::User, "hello"),
        ];

        let out = extract_session_messages(&msgs);
        assert_eq!(out.len(), 3);
        assert!(matches!(out[0].role, Role::System));
        assert!(matches!(out[1].role, Role::Developer));
        assert!(matches!(out[2].role, Role::User));
    }

    #[test]
    fn render_replay_includes_task_contract_section() {
        let rendered = render_replay(&crate::store::RunRecord {
            metadata: crate::store::RunMetadata {
                run_id: "r1".to_string(),
                started_at: "2026-01-01T00:00:00Z".to_string(),
                finished_at: "2026-01-01T00:00:01Z".to_string(),
                exit_reason: "ok".to_string(),
            },
            mode: "single".to_string(),
            planner: None,
            worker: None,
            cli: crate::store::RunCliConfig {
                mode: "single".to_string(),
                agent_mode: "build".to_string(),
                output_mode: "human".to_string(),
                provider: "mock".to_string(),
                base_url: "mock://local".to_string(),
                model: "mock-model".to_string(),
                temperature: None,
                top_p: None,
                max_tokens: None,
                seed: None,
                planner_model: None,
                worker_model: None,
                planner_max_steps: None,
                planner_output: None,
                planner_strict: None,
                enforce_plan_tools: "off".to_string(),
                mcp_pin_enforcement: "hard".to_string(),
                trust_mode: "off".to_string(),
                allow_shell: false,
                allow_write: false,
                enable_write_tools: false,
                exec_target: "host".to_string(),
                docker_image: None,
                docker_workdir: None,
                docker_network: None,
                docker_user: None,
                docker_config_summary: None,
                max_tool_output_bytes: 0,
                max_read_bytes: 0,
                max_wall_time_ms: 0,
                max_total_tool_calls: 0,
                max_mcp_calls: 0,
                max_filesystem_read_calls: 0,
                max_filesystem_write_calls: 0,
                max_shell_calls: 0,
                max_network_calls: 0,
                max_browser_calls: 0,
                tool_exec_timeout_ms: 0,
                post_write_verify_timeout_ms: 0,
                approval_mode: "interrupt".to_string(),
                auto_approve_scope: "run".to_string(),
                approval_key: "v1".to_string(),
                unsafe_mode: false,
                no_limits: false,
                unsafe_bypass_allow_flags: false,
                stream: false,
                events_path: None,
                max_context_chars: 0,
                compaction_mode: "off".to_string(),
                compaction_keep_last: 0,
                tool_result_persist: "digest".to_string(),
                hooks_mode: "off".to_string(),
                caps_mode: "off".to_string(),
                hooks_config_path: String::new(),
                hooks_strict: false,
                hooks_timeout_ms: 0,
                hooks_max_stdout_bytes: 0,
                tool_args_strict: "on".to_string(),
                taint: "off".to_string(),
                taint_mode: "propagate".to_string(),
                taint_digest_bytes: 4096,
                repro: "off".to_string(),
                repro_env: "safe".to_string(),
                repro_out: None,
                use_session_settings: false,
                resolved_settings_source: Default::default(),
                tui_enabled: false,
                tui_refresh_ms: 0,
                tui_max_log_lines: 0,
                http_max_retries: 0,
                http_timeout_ms: 0,
                http_connect_timeout_ms: 0,
                http_stream_idle_timeout_ms: 0,
                http_max_response_bytes: 0,
                http_max_line_bytes: 0,
                tool_catalog: Vec::new(),
                mcp_tool_snapshot: Vec::new(),
                mcp_tool_catalog_hash_hex: None,
                mcp_servers: Vec::new(),
                mcp_config_path: None,
                policy_version: None,
                includes_resolved: Vec::new(),
                mcp_allowlist: None,
                instructions_config_path: None,
                instructions_config_hash_hex: None,
                instruction_model_profile: None,
                instruction_task_profile: None,
                instruction_message_count: 0,
                project_guidance_hash_hex: None,
                project_guidance_sources: Vec::new(),
                project_guidance_truncated: false,
                project_guidance_bytes_loaded: 0,
                project_guidance_bytes_kept: 0,
                repo_map_hash_hex: None,
                repo_map_format: None,
                repo_map_truncated: false,
                repo_map_truncated_reason: None,
                repo_map_bytes_scanned: 0,
                repo_map_bytes_kept: 0,
                repo_map_file_count_included: 0,
                repo_map_injected: false,
                lsp_context_provider: None,
                lsp_context_schema_version: None,
                lsp_context_truncated: false,
                lsp_context_truncation_reason: None,
                lsp_context_bytes_kept: 0,
                lsp_context_diagnostics_included: 0,
                lsp_context_symbol_query: None,
                lsp_context_symbols_included: 0,
                lsp_context_definitions_included: 0,
                lsp_context_references_included: 0,
                lsp_context_injected: false,
                active_profile: None,
                profile_source: None,
                profile_hash_hex: None,
                activated_packs: Vec::new(),
            },
            resolved_paths: crate::store::RunResolvedPaths {
                state_dir: ".".to_string(),
                policy_path: "./policy.yaml".to_string(),
                approvals_path: "./approvals.json".to_string(),
                audit_path: "./audit.jsonl".to_string(),
            },
            policy_source: "none".to_string(),
            policy_hash_hex: None,
            policy_version: None,
            includes_resolved: Vec::new(),
            mcp_allowlist: None,
            config_hash_hex: "cfg".to_string(),
            config_fingerprint: None,
            task_contract: Some(TaskContractV1 {
                task_kind: "coding".to_string(),
                write_requirement: WriteRequirement::Required,
                validation_requirement: ValidationRequirement::Command {
                    command: "cargo test".to_string(),
                },
                allowed_tools: Some(vec!["read_file".to_string(), "shell".to_string()]),
                allowed_tools_semantics: AllowedToolsSemantics::ExposedSnapshot,
                completion_policy: CompletionPolicyV1 {
                    require_pre_write_read: true,
                    require_post_write_readback: true,
                    require_effective_write: true,
                },
                retry_policy: RetryPolicyV1 {
                    max_schema_repairs: 2,
                    max_repeat_failures_per_key: 3,
                    max_runtime_blocked_completions: 2,
                },
                final_answer_mode: FinalAnswerMode::Freeform,
            }),
            task_contract_provenance: Some(TaskContractProvenanceV1 {
                task_kind: ContractValueSource::Explicit,
                write_requirement: ContractValueSource::Inferred,
                validation_requirement: ContractValueSource::Inferred,
                allowed_tools: ContractValueSource::Inferred,
                allowed_tools_semantics: ContractValueSource::Defaulted,
                completion_policy: ContractValueSource::Inferred,
                retry_policy: ContractValueSource::Defaulted,
                final_answer_mode: ContractValueSource::Defaulted,
            }),
            run_checkpoint: None,
            tool_schema_hash_hex_map: Default::default(),
            hooks_config_hash_hex: None,
            transcript: Vec::new(),
            tool_calls: Vec::new(),
            tool_decisions: Vec::new(),
            tool_facts: Vec::new(),
            tool_fact_envelopes: Vec::new(),
            compaction: None,
            hook_report: Vec::new(),
            tool_catalog: Vec::new(),
            mcp_runtime_trace: Vec::new(),
            tool_reliability: Default::default(),
            mcp_pin_snapshot: None,
            taint: None,
            repro: None,
            final_output: String::new(),
            error: None,
        });
        assert!(rendered.contains("task_contract:"));
        assert!(rendered.contains("task_kind: coding"));
        assert!(rendered.contains("allowed_tools_semantics: Defaulted"));
    }
}
