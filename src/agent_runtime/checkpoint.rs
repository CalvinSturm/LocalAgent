use clap::ValueEnum;

use crate::agent::AgentExitReason;
use crate::agent_runtime::state::{
    ApprovalState, CompletionDecisionRecordV1, ExecutionTier, PhaseSummaryEntryV1, RetryState,
    RunCheckpointV1 as RuntimeStateCheckpointV1, RunPhase, ValidationState,
};
use crate::store::{
    extract_session_messages, PendingApprovalToolCallV1, RunCheckpointInterruptKind,
    RunCheckpointInterruptV1, RunCheckpointPhase, RunCheckpointV1, RuntimeRunCheckpointRecordV1,
};
use crate::{Cli, RunArgs};

pub(super) fn checkpoint_for_outcome(
    outcome: &crate::agent::AgentOutcome,
) -> Option<RunCheckpointV1> {
    match outcome.exit_reason {
        AgentExitReason::ApprovalRequired => Some(RunCheckpointV1 {
            schema_version: "openagent.run_checkpoint.v1".to_string(),
            phase: RunCheckpointPhase::WaitingForApproval,
            terminal_boundary: true,
            pending_interrupt: Some(RunCheckpointInterruptV1 {
                kind: RunCheckpointInterruptKind::ApprovalRequired,
                reason: Some(outcome.final_output.clone()),
            }),
        }),
        AgentExitReason::Cancelled => Some(RunCheckpointV1 {
            schema_version: "openagent.run_checkpoint.v1".to_string(),
            phase: RunCheckpointPhase::Interrupted,
            terminal_boundary: true,
            pending_interrupt: Some(RunCheckpointInterruptV1 {
                kind: RunCheckpointInterruptKind::OperatorInterrupt,
                reason: outcome.error.clone(),
            }),
        }),
        _ => None,
    }
}

pub(super) fn initial_runtime_state_checkpoint(
    execution_tier: ExecutionTier,
) -> RuntimeStateCheckpointV1 {
    RuntimeStateCheckpointV1 {
        schema_version: "openagent.runtime_state_checkpoint.v1".to_string(),
        phase: RunPhase::Executing,
        step_index: 0,
        execution_tier,
        terminal_boundary: false,
        retry_state: RetryState::default(),
        validation_state: ValidationState::default(),
        approval_state: ApprovalState::default(),
        active_plan_step_id: None,
        last_tool_fact_envelopes: Vec::new(),
    }
}

pub(super) fn runtime_state_checkpoint_for_outcome(
    outcome: &crate::agent::AgentOutcome,
    prompt: &str,
    execution_tier: ExecutionTier,
    tool_fact_envelopes: &[crate::agent::ToolFactEnvelopeV1],
) -> RuntimeStateCheckpointV1 {
    let required_command = crate::agent_impl_guard::prompt_required_validation_command(prompt)
        .map(ToOwned::to_owned);
    let approval = outcome
        .tool_decisions
        .iter()
        .rev()
        .find(|decision| decision.decision == "require_approval");
    RuntimeStateCheckpointV1 {
        schema_version: "openagent.runtime_state_checkpoint.v1".to_string(),
        phase: match outcome.exit_reason {
            AgentExitReason::ApprovalRequired => RunPhase::WaitingForApproval,
            AgentExitReason::Cancelled => RunPhase::Cancelled,
            AgentExitReason::Ok => RunPhase::Done,
            _ => RunPhase::Failed,
        },
        step_index: outcome
            .tool_decisions
            .iter()
            .map(|decision| decision.step)
            .max()
            .unwrap_or(0),
        execution_tier,
        terminal_boundary: true,
        retry_state: RetryState::default(),
        validation_state: ValidationState {
            required_command: required_command.clone(),
            satisfied: crate::agent::required_validation_command_satisfied_from_facts(
                prompt,
                &tool_fact_envelopes
                    .iter()
                    .map(|envelope| envelope.fact.clone())
                    .collect::<Vec<_>>(),
            ),
            repair_mode: false,
            collecting_final_answer:
                crate::agent_impl_guard::prompt_required_exact_final_answer(prompt).is_some(),
        },
        approval_state: ApprovalState {
            approval_id: approval.and_then(|decision| decision.approval_id.clone()),
            tool_call_id: approval.map(|decision| decision.tool_call_id.clone()),
            awaiting_approval: matches!(outcome.exit_reason, AgentExitReason::ApprovalRequired),
        },
        active_plan_step_id: None,
        last_tool_fact_envelopes: tool_fact_envelopes.to_vec(),
    }
}

pub(super) fn phase_summary_for_outcome(
    outcome: &crate::agent::AgentOutcome,
) -> Vec<PhaseSummaryEntryV1> {
    let final_phase = match outcome.exit_reason {
        AgentExitReason::ApprovalRequired => RunPhase::WaitingForApproval,
        AgentExitReason::Cancelled => RunPhase::Cancelled,
        AgentExitReason::Ok => RunPhase::Done,
        _ => RunPhase::Failed,
    };
    vec![
        PhaseSummaryEntryV1 {
            phase: RunPhase::Setup,
            entered_at: outcome.started_at.clone(),
            exited_at: Some(outcome.started_at.clone()),
        },
        PhaseSummaryEntryV1 {
            phase: RunPhase::Executing,
            entered_at: outcome.started_at.clone(),
            exited_at: Some(outcome.finished_at.clone()),
        },
        PhaseSummaryEntryV1 {
            phase: final_phase,
            entered_at: outcome.finished_at.clone(),
            exited_at: None,
        },
    ]
}

pub(super) fn completion_decisions_for_outcome(
    outcome: &crate::agent::AgentOutcome,
    runtime_checkpoint: &RuntimeStateCheckpointV1,
) -> Vec<CompletionDecisionRecordV1> {
    let validation_facts = crate::agent::collect_validation_facts_from_checkpoint(
        runtime_checkpoint,
        &runtime_checkpoint.last_tool_fact_envelopes,
    );
    let (allowed, retryable, reason, next_phase, unmet_requirements) = match outcome.exit_reason {
        AgentExitReason::Ok => (
            validation_facts.satisfied,
            false,
            if validation_facts.satisfied {
                "run finalized successfully".to_string()
            } else {
                "run finalized without satisfying required validation evidence".to_string()
            },
            Some(RunPhase::Done),
            if validation_facts.satisfied {
                Vec::new()
            } else {
                vec!["required_validation".to_string()]
            },
        ),
        AgentExitReason::ApprovalRequired => (
            false,
            true,
            "run blocked pending operator approval".to_string(),
            Some(RunPhase::WaitingForApproval),
            vec!["approval_required".to_string()],
        ),
        AgentExitReason::Cancelled => (
            false,
            true,
            "run interrupted before completion".to_string(),
            Some(RunPhase::WaitingForOperatorInput),
            vec!["operator_interrupt".to_string()],
        ),
        _ => (
            false,
            false,
            outcome
                .error
                .clone()
                .unwrap_or_else(|| "run ended without satisfying completion policy".to_string()),
            Some(runtime_checkpoint.phase.clone()),
            vec!["runtime_failure".to_string()],
        ),
    };
    vec![CompletionDecisionRecordV1 {
        kind: "finalize".to_string(),
        allowed,
        retryable,
        next_phase,
        reason,
        unmet_requirements,
    }]
}

pub(super) fn runtime_checkpoint_record_for_outcome(
    outcome: &crate::agent::AgentOutcome,
    prompt: &str,
    args: &RunArgs,
    execution_tier: ExecutionTier,
    tool_facts: &[crate::agent::ToolFactV1],
    tool_fact_envelopes: &[crate::agent::ToolFactEnvelopeV1],
) -> Option<RuntimeRunCheckpointRecordV1> {
    let checkpoint = checkpoint_for_outcome(outcome)?;
    let checkpoint_phase_name = match checkpoint.phase {
        RunCheckpointPhase::WaitingForApproval => "waiting_for_approval",
        RunCheckpointPhase::Interrupted => "interrupted",
    };
    let interrupt_history = crate::agent::interrupt_history_for_outcome(outcome);
    let phase_summary = phase_summary_for_outcome(outcome);
    let runtime_state_checkpoint =
        runtime_state_checkpoint_for_outcome(outcome, prompt, execution_tier.clone(), tool_fact_envelopes);
    let completion_decisions = completion_decisions_for_outcome(outcome, &runtime_state_checkpoint);
    Some(RuntimeRunCheckpointRecordV1 {
        schema_version: "openagent.runtime_checkpoint.v1".to_string(),
        runtime_run_id: outcome.run_id.clone(),
        prompt: prompt.to_string(),
        resume_argv: build_resume_argv(args, prompt),
        checkpoint: Some(checkpoint),
        runtime_state_checkpoint,
        execution_tier,
        resume_session_messages: extract_session_messages(&outcome.messages),
        interrupt_history,
        phase_summary,
        completion_decisions,
        tool_facts: tool_facts.to_vec(),
        tool_fact_envelopes: if tool_fact_envelopes.is_empty() {
            crate::agent::tool_fact_envelopes_from_facts(
                tool_facts,
                crate::agent::ToolFactSourceV1::Transcript,
                Some("checkpoint_boundary"),
                Some(checkpoint_phase_name),
            )
        } else {
            tool_fact_envelopes.to_vec()
        },
        pending_tool_call: pending_approval_tool_call(outcome),
        boundary_output: (!outcome.final_output.is_empty()).then(|| outcome.final_output.clone()),
    })
}

fn pending_approval_tool_call(
    outcome: &crate::agent::AgentOutcome,
) -> Option<PendingApprovalToolCallV1> {
    let decision = outcome
        .tool_decisions
        .iter()
        .rev()
        .find(|decision| decision.decision == "require_approval")?;
    let tool_call = outcome
        .tool_calls
        .iter()
        .rev()
        .find(|tool_call| tool_call.id == decision.tool_call_id)?;
    Some(PendingApprovalToolCallV1 {
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.name.clone(),
        arguments: tool_call.arguments.to_string(),
        approval_id: decision.approval_id.clone(),
        reason: decision.reason.clone(),
    })
}

fn build_resume_argv(args: &RunArgs, prompt: &str) -> Vec<String> {
    let mut out = vec!["localagent".to_string()];
    push_value_enum_opt(&mut out, "--provider", args.provider);
    push_option(&mut out, "--model", args.model.as_ref());
    push_option(&mut out, "--base-url", args.base_url.as_ref());
    push_option(&mut out, "--api-key", args.api_key.as_ref());
    push_arg(&mut out, "--prompt", prompt);
    push_option_display(&mut out, "--temperature", args.temperature);
    push_option_display(&mut out, "--top-p", args.top_p);
    push_option_display(&mut out, "--max-tokens", args.max_tokens);
    push_option_display(&mut out, "--seed", args.seed);
    push_arg(&mut out, "--max-steps", &args.max_steps.to_string());
    push_arg(&mut out, "--max-wall-time-ms", &args.max_wall_time_ms.to_string());
    push_arg(
        &mut out,
        "--max-total-tool-calls",
        &args.max_total_tool_calls.to_string(),
    );
    push_arg(&mut out, "--max-mcp-calls", &args.max_mcp_calls.to_string());
    push_arg(
        &mut out,
        "--max-filesystem-read-calls",
        &args.max_filesystem_read_calls.to_string(),
    );
    push_arg(
        &mut out,
        "--max-filesystem-write-calls",
        &args.max_filesystem_write_calls.to_string(),
    );
    push_arg(&mut out, "--max-shell-calls", &args.max_shell_calls.to_string());
    push_arg(
        &mut out,
        "--max-network-calls",
        &args.max_network_calls.to_string(),
    );
    push_arg(
        &mut out,
        "--max-browser-calls",
        &args.max_browser_calls.to_string(),
    );
    push_arg(
        &mut out,
        "--tool-exec-timeout-ms",
        &args.tool_exec_timeout_ms.to_string(),
    );
    push_arg(
        &mut out,
        "--post-write-verify-timeout-ms",
        &args.post_write_verify_timeout_ms.to_string(),
    );
    push_arg(&mut out, "--workdir", &args.workdir.display().to_string());
    push_path_opt(&mut out, "--state-dir", args.state_dir.as_ref());
    push_vec(&mut out, "--mcp", &args.mcp);
    push_vec(&mut out, "--pack", &args.packs);
    push_path_opt(&mut out, "--mcp-config", args.mcp_config.as_ref());
    push_flag(&mut out, "--allow-shell", args.allow_shell);
    push_flag(
        &mut out,
        "--allow-shell-in-workdir",
        args.allow_shell_in_workdir,
    );
    push_flag(&mut out, "--allow-write", args.allow_write);
    push_flag(&mut out, "--enable-write-tools", args.enable_write_tools);
    push_value_enum(&mut out, "--agent-mode", args.agent_mode);
    push_value_enum(&mut out, "--exec-target", args.exec_target);
    push_arg(&mut out, "--docker-image", &args.docker_image);
    push_arg(&mut out, "--docker-workdir", &args.docker_workdir);
    push_value_enum(&mut out, "--docker-network", args.docker_network);
    push_option(&mut out, "--docker-user", args.docker_user.as_ref());
    push_arg(
        &mut out,
        "--max-tool-output-bytes",
        &args.max_tool_output_bytes.to_string(),
    );
    push_arg(&mut out, "--max-read-bytes", &args.max_read_bytes.to_string());
    push_value_enum(&mut out, "--trust", args.trust);
    push_value_enum(&mut out, "--approval-mode", args.approval_mode);
    push_value_enum(&mut out, "--auto-approve-scope", args.auto_approve_scope);
    push_value_enum(&mut out, "--approval-key", args.approval_key);
    push_flag(&mut out, "--unsafe", args.unsafe_mode);
    push_flag(&mut out, "--no-limits", args.no_limits);
    push_flag(
        &mut out,
        "--unsafe-bypass-allow-flags",
        args.unsafe_bypass_allow_flags,
    );
    push_path_opt(&mut out, "--policy", args.policy.as_ref());
    push_path_opt(&mut out, "--approvals", args.approvals.as_ref());
    push_path_opt(&mut out, "--audit", args.audit.as_ref());
    push_arg(&mut out, "--session", &args.session);
    push_flag(&mut out, "--no-session", args.no_session);
    push_flag(&mut out, "--reset-session", args.reset_session);
    push_arg(
        &mut out,
        "--max-session-messages",
        &args.max_session_messages.to_string(),
    );
    push_flag(
        &mut out,
        "--use-session-settings",
        args.use_session_settings,
    );
    push_arg(
        &mut out,
        "--max-context-chars",
        &args.max_context_chars.to_string(),
    );
    push_flag(&mut out, "--use-repomap", args.use_repomap);
    push_arg(
        &mut out,
        "--repomap-max-bytes",
        &args.repomap_max_bytes.to_string(),
    );
    push_value_enum_opt(&mut out, "--lsp-provider", args.lsp_provider);
    push_path_opt(&mut out, "--lsp-command", args.lsp_command.as_ref());
    push_option(&mut out, "--reliability-profile", args.reliability_profile.as_ref());
    push_value_enum(&mut out, "--compaction-mode", args.compaction_mode);
    push_arg(
        &mut out,
        "--compaction-keep-last",
        &args.compaction_keep_last.to_string(),
    );
    push_value_enum(&mut out, "--tool-result-persist", args.tool_result_persist);
    push_value_enum(&mut out, "--hooks", args.hooks);
    push_path_opt(&mut out, "--hooks-config", args.hooks_config.as_ref());
    push_flag(&mut out, "--hooks-strict", args.hooks_strict);
    push_arg(
        &mut out,
        "--hooks-timeout-ms",
        &args.hooks_timeout_ms.to_string(),
    );
    push_arg(
        &mut out,
        "--hooks-max-stdout-bytes",
        &args.hooks_max_stdout_bytes.to_string(),
    );
    push_value_enum(&mut out, "--tool-args-strict", args.tool_args_strict);
    push_path_opt(
        &mut out,
        "--instructions-config",
        args.instructions_config.as_ref(),
    );
    push_option(
        &mut out,
        "--instruction-model-profile",
        args.instruction_model_profile.as_ref(),
    );
    push_option(
        &mut out,
        "--instruction-task-profile",
        args.instruction_task_profile.as_ref(),
    );
    push_option(&mut out, "--task-kind", args.task_kind.as_ref());
    push_flag(
        &mut out,
        "--disable-implementation-guard",
        args.disable_implementation_guard,
    );
    push_value_enum(&mut out, "--taint", args.taint);
    push_value_enum(&mut out, "--taint-mode", args.taint_mode);
    push_arg(
        &mut out,
        "--taint-digest-bytes",
        &args.taint_digest_bytes.to_string(),
    );
    push_value_enum(&mut out, "--repro", args.repro);
    push_path_opt(&mut out, "--repro-out", args.repro_out.as_ref());
    push_value_enum(&mut out, "--repro-env", args.repro_env);
    push_value_enum(&mut out, "--caps", args.caps);
    push_flag(&mut out, "--stream", args.stream);
    push_value_enum(&mut out, "--output", args.output);
    push_path_opt(&mut out, "--events", args.events.as_ref());
    push_arg(
        &mut out,
        "--http-max-retries",
        &args.http_max_retries.to_string(),
    );
    push_arg(&mut out, "--http-timeout-ms", &args.http_timeout_ms.to_string());
    push_arg(
        &mut out,
        "--http-connect-timeout-ms",
        &args.http_connect_timeout_ms.to_string(),
    );
    push_arg(
        &mut out,
        "--http-stream-idle-timeout-ms",
        &args.http_stream_idle_timeout_ms.to_string(),
    );
    push_arg(
        &mut out,
        "--http-max-response-bytes",
        &args.http_max_response_bytes.to_string(),
    );
    push_arg(
        &mut out,
        "--http-max-line-bytes",
        &args.http_max_line_bytes.to_string(),
    );
    push_flag(&mut out, "--tui", args.tui);
    push_arg(
        &mut out,
        "--tui-refresh-ms",
        &args.tui_refresh_ms.to_string(),
    );
    push_arg(
        &mut out,
        "--tui-max-log-lines",
        &args.tui_max_log_lines.to_string(),
    );
    push_value_enum(&mut out, "--mode", args.mode);
    push_option(&mut out, "--planner-model", args.planner_model.as_ref());
    push_option(&mut out, "--worker-model", args.worker_model.as_ref());
    push_arg(
        &mut out,
        "--planner-max-steps",
        &args.planner_max_steps.to_string(),
    );
    push_value_enum(&mut out, "--planner-output", args.planner_output);
    push_value_enum(
        &mut out,
        "--enforce-plan-tools",
        args.enforce_plan_tools,
    );
    push_value_enum(
        &mut out,
        "--mcp-pin-enforcement",
        args.mcp_pin_enforcement,
    );
    push_bool_set(&mut out, "--planner-strict", args.planner_strict);
    push_flag(&mut out, "--no-planner-strict", args.no_planner_strict);
    out
}

fn push_arg(out: &mut Vec<String>, flag: &str, value: &str) {
    out.push(flag.to_string());
    out.push(value.to_string());
}

fn push_option(out: &mut Vec<String>, flag: &str, value: Option<&String>) {
    if let Some(value) = value {
        push_arg(out, flag, value);
    }
}

fn push_path_opt(out: &mut Vec<String>, flag: &str, value: Option<&std::path::PathBuf>) {
    if let Some(value) = value {
        push_arg(out, flag, &value.display().to_string());
    }
}

fn push_option_display<T: ToString>(out: &mut Vec<String>, flag: &str, value: Option<T>) {
    if let Some(value) = value {
        push_arg(out, flag, &value.to_string());
    }
}

fn push_flag(out: &mut Vec<String>, flag: &str, enabled: bool) {
    if enabled {
        out.push(flag.to_string());
    }
}

fn push_bool_set(out: &mut Vec<String>, flag: &str, value: bool) {
    push_arg(out, flag, if value { "true" } else { "false" });
}

fn push_vec(out: &mut Vec<String>, flag: &str, values: &[String]) {
    for value in values {
        push_arg(out, flag, value);
    }
}

fn push_value_enum<T: ValueEnum + Copy>(out: &mut Vec<String>, flag: &str, value: T) {
    if let Some(name) = value.to_possible_value().map(|value| value.get_name().to_string()) {
        push_arg(out, flag, &name);
    }
}

fn push_value_enum_opt<T: ValueEnum + Copy>(out: &mut Vec<String>, flag: &str, value: Option<T>) {
    if let Some(value) = value {
        push_value_enum(out, flag, value);
    }
}

pub(crate) fn parse_resume_args(argv: &[String]) -> anyhow::Result<RunArgs> {
    use clap::Parser;

    let cli = Cli::try_parse_from(argv)
        .map_err(|err| anyhow::anyhow!("failed parsing checkpoint resume argv: {err}"))?;
    Ok(cli.run)
}

#[cfg(test)]
mod tests {
    use super::{
        checkpoint_for_outcome, parse_resume_args, runtime_checkpoint_record_for_outcome,
    };
    use crate::agent::{AgentExitReason, AgentOutcome, ToolDecisionRecord};
    use crate::compaction::{CompactionMode, CompactionSettings, ToolResultPersist};
    use crate::store::{RunCheckpointInterruptKind, RunCheckpointPhase};
    use crate::types::{Message, Role, ToolCall};
    use clap::Parser;

    fn outcome(exit_reason: AgentExitReason, error: Option<&str>) -> AgentOutcome {
        AgentOutcome {
            run_id: "r1".to_string(),
            started_at: "2026-01-01T00:00:00Z".to_string(),
            finished_at: "2026-01-01T00:00:01Z".to_string(),
            exit_reason,
            final_output: "approval required".to_string(),
            error: error.map(str::to_string),
            messages: vec![
                Message {
                    role: Role::System,
                    content: Some(
                        "You are an agent that may call tools to gather information.".to_string(),
                    ),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::User,
                    content: Some("fix it".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
            ],
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "shell".to_string(),
                arguments: serde_json::json!({"command":"cargo test"}),
            }],
            tool_decisions: vec![ToolDecisionRecord {
                step: 1,
                tool_call_id: "call_1".to_string(),
                tool: "shell".to_string(),
                decision: "require_approval".to_string(),
                reason: Some("approval".to_string()),
                source: Some("policy".to_string()),
                approval_id: Some("appr_1".to_string()),
                taint_overall: None,
                taint_enforced: false,
                escalated: false,
                escalation_reason: None,
            }],
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
            taint: None,
        }
    }

    #[test]
    fn checkpoint_created_for_approval_required() {
        let checkpoint =
            checkpoint_for_outcome(&outcome(AgentExitReason::ApprovalRequired, None))
                .expect("checkpoint");
        assert_eq!(checkpoint.phase, RunCheckpointPhase::WaitingForApproval);
        assert_eq!(
            checkpoint.pending_interrupt.as_ref().map(|it| &it.kind),
            Some(&RunCheckpointInterruptKind::ApprovalRequired)
        );
    }

    #[test]
    fn checkpoint_created_for_cancelled_interrupt_boundary() {
        let checkpoint =
            checkpoint_for_outcome(&outcome(AgentExitReason::Cancelled, Some("cancelled")))
                .expect("checkpoint");
        assert_eq!(checkpoint.phase, RunCheckpointPhase::Interrupted);
        assert_eq!(
            checkpoint.pending_interrupt.as_ref().map(|it| &it.kind),
            Some(&RunCheckpointInterruptKind::OperatorInterrupt)
        );
    }

    #[test]
    fn runtime_checkpoint_record_includes_pending_tool_and_roundtrippable_args() {
        let args = crate::RunArgs::parse_from([
            "localagent",
            "--provider",
            "mock",
            "--prompt",
            "placeholder",
            "--task-kind",
            "coding",
            "--allow-shell",
        ]);
        let record = runtime_checkpoint_record_for_outcome(
            &outcome(AgentExitReason::ApprovalRequired, None),
            "real prompt",
            &args,
            crate::agent_runtime::state::ExecutionTier::ScopedHostShell,
            &[],
            &[],
        )
        .expect("checkpoint record");
        assert_eq!(record.prompt, "real prompt");
        assert_eq!(
            record.pending_tool_call.as_ref().map(|it| it.tool_name.as_str()),
            Some("shell")
        );
        assert_eq!(
            record
                .pending_tool_call
                .as_ref()
                .and_then(|it| it.approval_id.as_deref()),
            Some("appr_1")
        );
        let resumed = parse_resume_args(&record.resume_argv).expect("resume args");
        assert_eq!(resumed.prompt.as_deref(), Some("real prompt"));
        assert_eq!(resumed.task_kind.as_deref(), Some("coding"));
        assert!(resumed.allow_shell);
    }
}
