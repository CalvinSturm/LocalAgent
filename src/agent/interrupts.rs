use crate::agent::AgentOutcome;
use crate::agent_runtime::state::{InterruptHistoryEntryV1, InterruptKindV1};
use crate::store::RuntimeRunCheckpointRecordV1;

pub(crate) fn run_phase_name(phase: &crate::agent_runtime::state::RunPhase) -> &'static str {
    match phase {
        crate::agent_runtime::state::RunPhase::Setup => "setup",
        crate::agent_runtime::state::RunPhase::Planning => "planning",
        crate::agent_runtime::state::RunPhase::Executing => "executing",
        crate::agent_runtime::state::RunPhase::WaitingForApproval => "waiting_for_approval",
        crate::agent_runtime::state::RunPhase::WaitingForOperatorInput => {
            "waiting_for_operator_input"
        }
        crate::agent_runtime::state::RunPhase::VerifyingChanges => "verifying_changes",
        crate::agent_runtime::state::RunPhase::Validating => "validating",
        crate::agent_runtime::state::RunPhase::CollectingFinalAnswer => {
            "collecting_final_answer"
        }
        crate::agent_runtime::state::RunPhase::Finalizing => "finalizing",
        crate::agent_runtime::state::RunPhase::Done => "done",
        crate::agent_runtime::state::RunPhase::Failed => "failed",
        crate::agent_runtime::state::RunPhase::Cancelled => "cancelled",
    }
}

pub(crate) fn interrupt_kind_name(kind: &InterruptKindV1) -> &'static str {
    match kind {
        InterruptKindV1::ApprovalRequired => "approval_required",
        InterruptKindV1::OperatorInterrupt => "operator_interrupt",
    }
}

pub(crate) fn interrupt_history_for_outcome(outcome: &AgentOutcome) -> Vec<InterruptHistoryEntryV1> {
    match outcome.exit_reason {
        crate::agent::AgentExitReason::ApprovalRequired => {
            let approval = outcome
                .tool_decisions
                .iter()
                .rev()
                .find(|decision| decision.decision == "require_approval");
            vec![InterruptHistoryEntryV1 {
                kind: InterruptKindV1::ApprovalRequired,
                created_at: outcome.finished_at.clone(),
                resolved_at: None,
                approval_id: approval.and_then(|decision| decision.approval_id.clone()),
                tool_call_id: approval.map(|decision| decision.tool_call_id.clone()),
                reason: approval.and_then(|decision| decision.reason.clone()),
            }]
        }
        crate::agent::AgentExitReason::Cancelled => vec![InterruptHistoryEntryV1 {
            kind: InterruptKindV1::OperatorInterrupt,
            created_at: outcome.finished_at.clone(),
            resolved_at: None,
            approval_id: None,
            tool_call_id: None,
            reason: outcome.error.clone(),
        }],
        _ => Vec::new(),
    }
}

pub(crate) fn transition_runtime_checkpoint_to_executing(
    checkpoint: &RuntimeRunCheckpointRecordV1,
) -> RuntimeRunCheckpointRecordV1 {
    let mut updated = checkpoint.clone();
    let now = crate::trust::now_rfc3339();
    let prior_phase = updated.runtime_state_checkpoint.phase.clone();

    if let Some(last_phase) = updated
        .phase_summary
        .iter_mut()
        .rev()
        .find(|entry| entry.phase == prior_phase && entry.exited_at.is_none())
    {
        last_phase.exited_at = Some(now.clone());
    }
    updated
        .phase_summary
        .push(crate::agent_runtime::state::PhaseSummaryEntryV1 {
            phase: crate::agent_runtime::state::RunPhase::Executing,
            entered_at: now.clone(),
            exited_at: None,
        });

    if let Some(last_interrupt) = updated
        .interrupt_history
        .iter_mut()
        .rev()
        .find(|entry| entry.resolved_at.is_none())
    {
        last_interrupt.resolved_at = Some(now.clone());
    }

    updated.runtime_state_checkpoint.phase = crate::agent_runtime::state::RunPhase::Executing;
    updated.runtime_state_checkpoint.terminal_boundary = false;
    updated.runtime_state_checkpoint.approval_state.awaiting_approval = false;
    updated.runtime_state_checkpoint.validation_state.collecting_final_answer = false;
    updated.runtime_state_checkpoint.step_index = updated
        .runtime_state_checkpoint
        .step_index
        .saturating_add(1);

    updated.checkpoint = None;
    updated.pending_tool_call = None;
    updated.completion_decisions.push(
        crate::agent_runtime::state::CompletionDecisionRecordV1 {
            kind: "resume".to_string(),
            allowed: true,
            retryable: false,
            next_phase: Some(crate::agent_runtime::state::RunPhase::Executing),
            reason: format!(
                "checkpoint resume accepted from {:?} and transitioned back to executing",
                prior_phase
            ),
            unmet_requirements: Vec::new(),
        },
    );

    updated
}
