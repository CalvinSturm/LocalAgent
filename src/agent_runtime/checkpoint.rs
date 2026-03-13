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
        _ => None,
    }
}

pub(super) fn initial_runtime_state_checkpoint(
    execution_tier: ExecutionTier,
    prompt: &str,
) -> RuntimeStateCheckpointV1 {
    RuntimeStateCheckpointV1 {
        schema_version: "openagent.runtime_state_checkpoint.v1".to_string(),
        phase: RunPhase::Executing,
        step_index: 0,
        execution_tier,
        terminal_boundary: false,
        retry_state: RetryState::default(),
        tool_protocol_state: crate::agent_runtime::state::ToolProtocolState {
            tool_only_phase_active: crate::agent_impl_guard::prompt_requires_tool_only(prompt),
            ..Default::default()
        },
        validation_state: ValidationState {
            required_command: crate::agent_impl_guard::prompt_required_validation_command(prompt)
                .map(ToOwned::to_owned),
            ..Default::default()
        },
        approval_state: ApprovalState::default(),
        active_plan_step_id: None,
        last_tool_fact_envelopes: Vec::new(),
    }
}

pub(super) fn is_terminal_phase(phase: &RunPhase) -> bool {
    matches!(phase, RunPhase::Done | RunPhase::Failed | RunPhase::Cancelled)
}

pub(super) fn validate_terminal_runtime_state_checkpoint(
    outcome: &crate::agent::AgentOutcome,
    checkpoint: &RuntimeStateCheckpointV1,
) -> anyhow::Result<()> {
    use anyhow::ensure;

    if !is_terminal_phase(&checkpoint.phase) {
        return Ok(());
    }

    ensure!(
        checkpoint.terminal_boundary,
        "terminal runtime checkpoint must set terminal_boundary=true"
    );
    ensure!(
        !checkpoint.approval_state.awaiting_approval,
        "terminal runtime checkpoint cannot leave approval_state.awaiting_approval active"
    );

    match checkpoint.phase {
        RunPhase::Done => {
            ensure!(
                matches!(outcome.exit_reason, AgentExitReason::Ok),
                "done runtime checkpoint must map to AgentExitReason::Ok"
            );
            ensure!(
                checkpoint.validation_state.required_command.is_none()
                    || checkpoint.validation_state.satisfied,
                "done runtime checkpoint must satisfy required validation before finalization"
            );
        }
        RunPhase::Failed => {
            ensure!(
                !matches!(
                    outcome.exit_reason,
                    AgentExitReason::Ok
                        | AgentExitReason::Cancelled
                        | AgentExitReason::ApprovalRequired
                ),
                "failed runtime checkpoint cannot map to ok/cancelled/approval_required outcomes"
            );
        }
        RunPhase::Cancelled => {
            ensure!(
                matches!(outcome.exit_reason, AgentExitReason::Cancelled),
                "cancelled runtime checkpoint must map to AgentExitReason::Cancelled"
            );
        }
        _ => {}
    }

    Ok(())
}

pub(super) fn validate_final_run_artifact_consistency(
    outcome: &crate::agent::AgentOutcome,
    run_checkpoint: Option<&crate::store::RunCheckpointV1>,
    final_checkpoint: &RuntimeStateCheckpointV1,
    interrupt_history: &[crate::agent_runtime::state::InterruptHistoryEntryV1],
    phase_summary: &[PhaseSummaryEntryV1],
    completion_decisions: &[CompletionDecisionRecordV1],
) -> anyhow::Result<()> {
    use anyhow::ensure;

    let expected_phase = terminal_phase_for_outcome(outcome);
    ensure!(
        final_checkpoint.phase == expected_phase,
        "final run artifact phase {:?} does not match outcome phase {:?}",
        final_checkpoint.phase,
        expected_phase
    );

    let final_decision = completion_decisions
        .last()
        .ok_or_else(|| anyhow::anyhow!("final run artifact is missing completion decisions"))?;
    ensure!(
        final_decision.next_phase.as_ref() == Some(&expected_phase),
        "final completion decision next_phase {:?} does not match outcome phase {:?}",
        final_decision.next_phase,
        expected_phase
    );

    let final_phase_entry = phase_summary
        .last()
        .ok_or_else(|| anyhow::anyhow!("final run artifact is missing phase summary"))?;
    ensure!(
        final_phase_entry.phase == expected_phase,
        "final phase summary entry {:?} does not match outcome phase {:?}",
        final_phase_entry.phase,
        expected_phase
    );
    ensure!(
        final_phase_entry.exited_at.is_none(),
        "final phase summary entry must remain open for the terminal/boundary phase"
    );

    let unresolved_interrupts = interrupt_history
        .iter()
        .filter(|entry| entry.resolved_at.is_none())
        .collect::<Vec<_>>();

    match outcome.exit_reason {
        AgentExitReason::ApprovalRequired => {
            let checkpoint = run_checkpoint
                .ok_or_else(|| anyhow::anyhow!("approval-required run artifact must keep a run checkpoint"))?;
            ensure!(
                checkpoint.phase == crate::store::RunCheckpointPhase::WaitingForApproval,
                "approval-required run checkpoint must stay in waiting_for_approval"
            );
            ensure!(
                matches!(
                    checkpoint.pending_interrupt.as_ref().map(|it| &it.kind),
                    Some(crate::store::RunCheckpointInterruptKind::ApprovalRequired)
                ),
                "approval-required run checkpoint must carry an approval-required interrupt"
            );
            ensure!(
                final_checkpoint.approval_state.awaiting_approval,
                "approval-required final checkpoint must leave approval_state.awaiting_approval active"
            );
            ensure!(
                unresolved_interrupts.len() == 1
                    && unresolved_interrupts[0].kind
                        == crate::agent_runtime::state::InterruptKindV1::ApprovalRequired,
                "approval-required run artifact must have exactly one unresolved approval interrupt"
            );
        }
        AgentExitReason::Cancelled => {
            ensure!(
                run_checkpoint.is_none(),
                "cancelled run artifact cannot keep a resumable run checkpoint"
            );
            ensure!(
                unresolved_interrupts.is_empty(),
                "cancelled run artifact cannot keep unresolved interrupts"
            );
            ensure!(
                interrupt_history.iter().any(|entry| {
                    entry.kind == crate::agent_runtime::state::InterruptKindV1::OperatorInterrupt
                        && entry.resolved_at.is_some()
                }),
                "cancelled run artifact must record a resolved operator interrupt"
            );
        }
        AgentExitReason::Ok
        | AgentExitReason::ProviderError
        | AgentExitReason::PlannerError
        | AgentExitReason::Denied
        | AgentExitReason::HookAborted
        | AgentExitReason::MaxSteps
        | AgentExitReason::BudgetExceeded => {
            ensure!(
                run_checkpoint.is_none(),
                "terminal run artifact cannot keep a resumable run checkpoint"
            );
            ensure!(
                unresolved_interrupts.is_empty(),
                "terminal run artifact cannot keep unresolved interrupts"
            );
        }
    }

    Ok(())
}

pub(super) fn runtime_state_checkpoint_for_outcome(
    outcome: &crate::agent::AgentOutcome,
    prompt: &str,
    execution_tier: ExecutionTier,
    tool_fact_envelopes: &[crate::agent::tool_facts::ToolFactEnvelopeV1],
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
        tool_protocol_state: crate::agent_runtime::state::ToolProtocolState::default(),
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

pub(super) fn terminal_phase_for_outcome(outcome: &crate::agent::AgentOutcome) -> RunPhase {
    match outcome.exit_reason {
        AgentExitReason::ApprovalRequired => RunPhase::WaitingForApproval,
        AgentExitReason::Cancelled => RunPhase::Cancelled,
        AgentExitReason::Ok => RunPhase::Done,
        _ => RunPhase::Failed,
    }
}

pub(super) fn phase_summary_for_outcome(
    outcome: &crate::agent::AgentOutcome,
) -> Vec<PhaseSummaryEntryV1> {
    let final_phase = terminal_phase_for_outcome(outcome);
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

pub(super) fn phase_summary_for_outcome_with_prior(
    outcome: &crate::agent::AgentOutcome,
    prior: Option<&RuntimeRunCheckpointRecordV1>,
) -> Vec<PhaseSummaryEntryV1> {
    let Some(prior) = prior.filter(|record| !record.phase_summary.is_empty()) else {
        return phase_summary_for_outcome(outcome);
    };

    let final_phase = terminal_phase_for_outcome(outcome);
    let mut summary = prior.phase_summary.clone();
    if let Some(last_open_phase) = summary.iter_mut().rev().find(|entry| entry.exited_at.is_none()) {
        if last_open_phase.phase == final_phase {
            return summary;
        }
        last_open_phase.exited_at = Some(outcome.finished_at.clone());
    }
    summary.push(PhaseSummaryEntryV1 {
        phase: final_phase,
        entered_at: outcome.finished_at.clone(),
        exited_at: None,
    });
    summary
}

pub(super) fn completion_decisions_for_outcome(
    outcome: &crate::agent::AgentOutcome,
    runtime_checkpoint: &RuntimeStateCheckpointV1,
) -> Vec<CompletionDecisionRecordV1> {
    completion_decisions_for_outcome_with_prior(outcome, runtime_checkpoint, None)
}

pub(super) fn completion_decisions_for_outcome_with_prior(
    outcome: &crate::agent::AgentOutcome,
    runtime_checkpoint: &RuntimeStateCheckpointV1,
    prior: Option<&RuntimeRunCheckpointRecordV1>,
) -> Vec<CompletionDecisionRecordV1> {
    let validation_facts = crate::agent::completion_policy::collect_validation_facts_from_checkpoint(
        runtime_checkpoint,
        &runtime_checkpoint.last_tool_fact_envelopes,
    );
    let mut decisions = prior
        .map(|record| record.completion_decisions.clone())
        .unwrap_or_default();
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
            false,
            "run cancelled before completion".to_string(),
            Some(RunPhase::Cancelled),
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
    decisions.push(CompletionDecisionRecordV1 {
        kind: "finalize".to_string(),
        allowed,
        retryable,
        next_phase,
        reason,
        unmet_requirements,
    });
    decisions
}

pub(super) fn interrupt_history_for_outcome_with_prior(
    outcome: &crate::agent::AgentOutcome,
    prior: Option<&RuntimeRunCheckpointRecordV1>,
) -> Vec<crate::agent_runtime::state::InterruptHistoryEntryV1> {
    let mut history = prior
        .map(|record| record.interrupt_history.clone())
        .unwrap_or_default();
    history.extend(crate::agent::interrupts::interrupt_history_for_outcome(outcome));
    history
}

pub(super) fn runtime_checkpoint_record_for_outcome(
    outcome: &crate::agent::AgentOutcome,
    prompt: &str,
    args: &RunArgs,
    execution_tier: ExecutionTier,
    tool_facts: &[crate::agent::tool_facts::ToolFactV1],
    tool_fact_envelopes: &[crate::agent::tool_facts::ToolFactEnvelopeV1],
    prior: Option<&RuntimeRunCheckpointRecordV1>,
) -> Option<RuntimeRunCheckpointRecordV1> {
    let checkpoint = checkpoint_for_outcome(outcome)?;
    let checkpoint_phase_name = match checkpoint.phase {
        RunCheckpointPhase::WaitingForApproval => "waiting_for_approval",
        RunCheckpointPhase::Interrupted => "interrupted",
    };
    let interrupt_history = interrupt_history_for_outcome_with_prior(outcome, prior);
    let phase_summary = phase_summary_for_outcome_with_prior(outcome, prior);
    let runtime_state_checkpoint =
        runtime_state_checkpoint_for_outcome(outcome, prompt, execution_tier.clone(), tool_fact_envelopes);
    let completion_decisions =
        completion_decisions_for_outcome_with_prior(outcome, &runtime_state_checkpoint, prior);
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
                crate::agent::tool_facts::ToolFactSourceV1::Transcript,
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
        checkpoint_for_outcome, completion_decisions_for_outcome,
        validate_final_run_artifact_consistency, parse_resume_args,
        phase_summary_for_outcome, phase_summary_for_outcome_with_prior,
        runtime_checkpoint_record_for_outcome, runtime_state_checkpoint_for_outcome,
        validate_terminal_runtime_state_checkpoint,
    };
    use crate::agent::{AgentExitReason, AgentOutcome, ToolDecisionRecord};
    use crate::agent_runtime::state::{
        ApprovalState, CompletionDecisionRecordV1, ExecutionTier, InterruptHistoryEntryV1,
        InterruptKindV1, PhaseSummaryEntryV1, RetryState,
        RunCheckpointV1 as RuntimeStateCheckpointV1, RunPhase, ToolProtocolState, ValidationState,
    };
    use crate::compaction::{CompactionMode, CompactionSettings, ToolResultPersist};
    use crate::store::RuntimeRunCheckpointRecordV1;
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
                .is_none();
        assert!(checkpoint);
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
            None,
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

    #[test]
    fn terminal_checkpoint_validation_accepts_valid_done_state() {
        let outcome = outcome(AgentExitReason::Ok, None);
        let checkpoint = RuntimeStateCheckpointV1 {
            schema_version: "openagent.runtime_state_checkpoint.v1".to_string(),
            phase: RunPhase::Done,
            step_index: 1,
            execution_tier: ExecutionTier::ScopedHostShell,
            terminal_boundary: true,
            retry_state: RetryState::default(),
            tool_protocol_state: ToolProtocolState::default(),
            validation_state: ValidationState {
                required_command: Some("cargo test".to_string()),
                satisfied: true,
                repair_mode: false,
                collecting_final_answer: false,
            },
            approval_state: ApprovalState::default(),
            active_plan_step_id: None,
            last_tool_fact_envelopes: Vec::new(),
        };

        validate_terminal_runtime_state_checkpoint(&outcome, &checkpoint).expect("valid done");
    }

    #[test]
    fn terminal_checkpoint_validation_rejects_done_without_required_validation() {
        let outcome = outcome(AgentExitReason::Ok, None);
        let checkpoint = RuntimeStateCheckpointV1 {
            schema_version: "openagent.runtime_state_checkpoint.v1".to_string(),
            phase: RunPhase::Done,
            step_index: 1,
            execution_tier: ExecutionTier::ScopedHostShell,
            terminal_boundary: true,
            retry_state: RetryState::default(),
            tool_protocol_state: ToolProtocolState::default(),
            validation_state: ValidationState {
                required_command: Some("cargo test".to_string()),
                satisfied: false,
                repair_mode: false,
                collecting_final_answer: false,
            },
            approval_state: ApprovalState::default(),
            active_plan_step_id: None,
            last_tool_fact_envelopes: Vec::new(),
        };

        let err = validate_terminal_runtime_state_checkpoint(&outcome, &checkpoint)
            .expect_err("missing validation must fail");
        assert!(err
            .to_string()
            .contains("must satisfy required validation"));
    }

    #[test]
    fn cancelled_completion_decision_points_to_cancelled_terminal_phase() {
        let outcome = outcome(AgentExitReason::Cancelled, Some("cancelled"));
        let checkpoint = RuntimeStateCheckpointV1 {
            schema_version: "openagent.runtime_state_checkpoint.v1".to_string(),
            phase: RunPhase::Cancelled,
            step_index: 1,
            execution_tier: ExecutionTier::ScopedHostShell,
            terminal_boundary: true,
            retry_state: RetryState::default(),
            tool_protocol_state: ToolProtocolState::default(),
            validation_state: ValidationState::default(),
            approval_state: ApprovalState::default(),
            active_plan_step_id: None,
            last_tool_fact_envelopes: Vec::new(),
        };

        let decisions = completion_decisions_for_outcome(&outcome, &checkpoint);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].next_phase, Some(RunPhase::Cancelled));
        assert!(!decisions[0].retryable);
    }

    #[test]
    fn cancelled_outcome_does_not_emit_resumable_runtime_checkpoint_record() {
        let args = crate::RunArgs::parse_from([
            "localagent",
            "--provider",
            "mock",
            "--prompt",
            "placeholder",
        ]);
        let record = runtime_checkpoint_record_for_outcome(
            &outcome(AgentExitReason::Cancelled, Some("cancelled")),
            "real prompt",
            &args,
            ExecutionTier::ScopedHostShell,
            &[],
            &[],
            None,
        );
        assert!(record.is_none());
    }

    #[test]
    fn cancelled_interrupt_history_is_marked_resolved() {
        let history = crate::agent::interrupts::interrupt_history_for_outcome(&outcome(
            AgentExitReason::Cancelled,
            Some("cancelled"),
        ));
        assert_eq!(history.len(), 1);
        assert!(history[0].resolved_at.is_some());
    }

    #[test]
    fn prior_phase_summary_is_preserved_when_appending_terminal_phase() {
        let outcome = outcome(AgentExitReason::Ok, None);
        let prior = RuntimeRunCheckpointRecordV1 {
            schema_version: "openagent.runtime_checkpoint.v1".to_string(),
            runtime_run_id: "r1".to_string(),
            prompt: "fix it".to_string(),
            resume_argv: Vec::new(),
            checkpoint: None,
            runtime_state_checkpoint: RuntimeStateCheckpointV1 {
                schema_version: "openagent.runtime_state_checkpoint.v1".to_string(),
                phase: RunPhase::Executing,
                step_index: 1,
                execution_tier: ExecutionTier::ScopedHostShell,
                terminal_boundary: false,
                retry_state: RetryState::default(),
                tool_protocol_state: ToolProtocolState::default(),
                validation_state: ValidationState::default(),
                approval_state: ApprovalState::default(),
                active_plan_step_id: None,
                last_tool_fact_envelopes: Vec::new(),
            },
            execution_tier: ExecutionTier::ScopedHostShell,
            resume_session_messages: Vec::new(),
            interrupt_history: vec![InterruptHistoryEntryV1 {
                kind: InterruptKindV1::ApprovalRequired,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                resolved_at: Some("2026-01-01T00:00:01Z".to_string()),
                approval_id: Some("approval-1".to_string()),
                tool_call_id: Some("tc1".to_string()),
                reason: Some("approval".to_string()),
            }],
            phase_summary: vec![
                PhaseSummaryEntryV1 {
                    phase: RunPhase::WaitingForApproval,
                    entered_at: "2026-01-01T00:00:00Z".to_string(),
                    exited_at: Some("2026-01-01T00:00:01Z".to_string()),
                },
                PhaseSummaryEntryV1 {
                    phase: RunPhase::Executing,
                    entered_at: "2026-01-01T00:00:01Z".to_string(),
                    exited_at: None,
                },
            ],
            completion_decisions: vec![CompletionDecisionRecordV1 {
                kind: "resume".to_string(),
                allowed: true,
                retryable: false,
                next_phase: Some(RunPhase::Executing),
                reason: "resume".to_string(),
                unmet_requirements: Vec::new(),
            }],
            tool_facts: Vec::new(),
            tool_fact_envelopes: Vec::new(),
            pending_tool_call: None,
            boundary_output: None,
        };

        let summary = phase_summary_for_outcome_with_prior(&outcome, Some(&prior));
        assert_eq!(summary.len(), 3);
        assert_eq!(summary[0].phase, RunPhase::WaitingForApproval);
        assert_eq!(summary[1].phase, RunPhase::Executing);
        assert_eq!(summary[1].exited_at.as_deref(), Some("2026-01-01T00:00:01Z"));
        assert_eq!(summary[2].phase, RunPhase::Done);
    }

    #[test]
    fn approval_required_artifact_requires_unresolved_approval_interrupt() {
        let outcome = outcome(AgentExitReason::ApprovalRequired, None);
        let final_checkpoint = runtime_state_checkpoint_for_outcome(
            &outcome,
            "fix it",
            ExecutionTier::ScopedHostShell,
            &[],
        );
        let run_checkpoint = checkpoint_for_outcome(&outcome);

        let err = validate_final_run_artifact_consistency(
            &outcome,
            run_checkpoint.as_ref(),
            &final_checkpoint,
            &[],
            &phase_summary_for_outcome(&outcome),
            &completion_decisions_for_outcome(&outcome, &final_checkpoint),
        )
        .expect_err("approval artifact should require unresolved interrupt");
        assert!(err.to_string().contains("unresolved approval interrupt"));
    }

    #[test]
    fn done_artifact_rejects_unresolved_interrupts() {
        let outcome = outcome(AgentExitReason::Ok, None);
        let final_checkpoint = runtime_state_checkpoint_for_outcome(
            &outcome,
            "fix it",
            ExecutionTier::ScopedHostShell,
            &[],
        );
        let err = validate_final_run_artifact_consistency(
            &outcome,
            None,
            &final_checkpoint,
            &[InterruptHistoryEntryV1 {
                kind: InterruptKindV1::OperatorInterrupt,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                resolved_at: None,
                approval_id: None,
                tool_call_id: None,
                reason: Some("paused".to_string()),
            }],
            &phase_summary_for_outcome(&outcome),
            &completion_decisions_for_outcome(&outcome, &final_checkpoint),
        )
        .expect_err("done artifact should reject unresolved interrupts");
        assert!(err.to_string().contains("cannot keep unresolved interrupts"));
    }
}
