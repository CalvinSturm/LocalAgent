use super::agent_types::AgentOutcome;
use super::completion_policy::{
    collect_validation_facts, decide_validation_phase_transition,
    post_validation_final_answer_transition_decision,
    validation_resume_execution_transition_decision, RuntimePhaseTransitionDecision,
    ValidationPhaseTransitionDecision,
};
use super::runtime_completion::{RuntimeCompletionAction, VerifiedWriteResult};
use super::tool_facts::{tool_fact_envelopes_from_facts, ToolFactSourceV1};
use super::ToolFactEnvelopeV1;
use super::{PhaseLoopControl, ToolExecutionRecord, ToolFactV1};
use crate::types::ToolCall;

pub(crate) enum PostToolPhaseRefreshEffect {
    None,
    EmitCompletionBlocked {
        transition: RuntimePhaseTransitionDecision,
        reason: String,
    },
}

pub(crate) struct VerifiedWriteFollowOnUpdate {
    pub(crate) control: PhaseLoopControl,
    pub(crate) developer_message: String,
}

#[allow(clippy::result_large_err)]
pub(crate) fn apply_runtime_completion_action_to_checkpoint(
    _user_prompt: &str,
    action: RuntimeCompletionAction,
    runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
) -> Result<PhaseLoopControl, AgentOutcome> {
    match action {
        RuntimeCompletionAction::ContinueStep {
            blocked_runtime_completion_count: next_count,
        } => {
            runtime_checkpoint
                .retry_state
                .blocked_runtime_completion_count = next_count;
            Ok(PhaseLoopControl::ContinueStep)
        }
        RuntimeCompletionAction::ContinueAgentStep {
            blocked_runtime_completion_count: next_count,
            operator_delivery_count: next_op_count,
        } => {
            runtime_checkpoint
                .retry_state
                .blocked_runtime_completion_count = next_count;
            runtime_checkpoint
                .tool_protocol_state
                .operator_delivery_count = next_op_count;
            Ok(PhaseLoopControl::ContinueAgentStep)
        }
        RuntimeCompletionAction::ContinueExactFinalAnswer {
            blocked_runtime_completion_count: next_count,
            operator_delivery_count: next_op_count,
        } => {
            runtime_checkpoint
                .retry_state
                .blocked_runtime_completion_count = next_count;
            runtime_checkpoint
                .tool_protocol_state
                .operator_delivery_count = next_op_count;
            runtime_checkpoint
                .retry_state
                .exact_final_answer_retry_count += 1;
            runtime_checkpoint.phase = crate::agent_runtime::state::RunPhase::CollectingFinalAnswer;
            runtime_checkpoint.validation_state.collecting_final_answer = true;
            Ok(PhaseLoopControl::ContinueAgentStep)
        }
        RuntimeCompletionAction::ContinueRequiredValidation {
            blocked_runtime_completion_count: next_count,
            operator_delivery_count: next_op_count,
        } => {
            runtime_checkpoint
                .retry_state
                .blocked_runtime_completion_count = next_count;
            runtime_checkpoint
                .tool_protocol_state
                .operator_delivery_count = next_op_count;
            runtime_checkpoint
                .retry_state
                .required_validation_retry_count += 1;
            runtime_checkpoint.phase = crate::agent_runtime::state::RunPhase::Validating;
            runtime_checkpoint.validation_state.repair_mode = false;
            runtime_checkpoint.validation_state.satisfied = false;
            runtime_checkpoint
                .retry_state
                .blocked_validation_failure_repair_count = 0;
            runtime_checkpoint
                .retry_state
                .blocked_post_validation_final_answer_count = 0;
            Ok(PhaseLoopControl::ContinueAgentStep)
        }
        RuntimeCompletionAction::ProceedToTools {
            blocked_runtime_completion_count: next_count,
        } => {
            runtime_checkpoint
                .retry_state
                .blocked_runtime_completion_count = next_count;
            runtime_checkpoint.phase = crate::agent_runtime::state::RunPhase::Executing;
            runtime_checkpoint.validation_state.repair_mode = false;
            Ok(PhaseLoopControl::Proceed)
        }
        RuntimeCompletionAction::Finalize(outcome) => Err(*outcome),
    }
}

pub(crate) fn refresh_phase_state_from_tool_facts(
    _user_prompt: &str,
    runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
    observed_tool_calls: &[ToolCall],
    observed_tool_executions: &[ToolExecutionRecord],
    successful_write_tool_ok_this_step: bool,
) -> PostToolPhaseRefreshEffect {
    let required_command = runtime_checkpoint
        .validation_state
        .required_command
        .as_deref();
    let tool_facts = crate::agent::tool_facts_from_calls_and_executions(
        required_command,
        observed_tool_calls,
        observed_tool_executions,
    );
    let validation_facts = collect_validation_facts(
        required_command,
        runtime_checkpoint
            .validation_state
            .exact_final_answer_required,
        &tool_facts,
    );
    runtime_checkpoint.validation_state.satisfied = validation_facts.satisfied;
    runtime_checkpoint.last_tool_fact_envelopes =
        tool_fact_envelopes_from_tool_facts(&tool_facts, &runtime_checkpoint.phase);

    match decide_validation_phase_transition(&validation_facts, successful_write_tool_ok_this_step)
    {
        ValidationPhaseTransitionDecision::EnterRepair => {
            runtime_checkpoint.phase = crate::agent_runtime::state::RunPhase::Executing;
            runtime_checkpoint.validation_state.repair_mode = true;
            runtime_checkpoint.validation_state.collecting_final_answer = false;
            PostToolPhaseRefreshEffect::EmitCompletionBlocked {
                transition: validation_resume_execution_transition_decision(),
                reason: "validation failed and runtime requires a code-fix repair step".to_string(),
            }
        }
        ValidationPhaseTransitionDecision::EnterPostValidationFinalAnswerOnly => {
            runtime_checkpoint.phase = crate::agent_runtime::state::RunPhase::CollectingFinalAnswer;
            runtime_checkpoint.validation_state.repair_mode = false;
            runtime_checkpoint.validation_state.satisfied = true;
            runtime_checkpoint.validation_state.collecting_final_answer = true;
            runtime_checkpoint
                .retry_state
                .blocked_post_validation_final_answer_count = 0;
            PostToolPhaseRefreshEffect::EmitCompletionBlocked {
                transition: post_validation_final_answer_transition_decision(),
                reason: post_validation_final_answer_transition_decision()
                    .completion_reason
                    .to_string(),
            }
        }
        ValidationPhaseTransitionDecision::ClearRepair => {
            runtime_checkpoint.validation_state.repair_mode = false;
            runtime_checkpoint
                .retry_state
                .blocked_validation_failure_repair_count = 0;
            PostToolPhaseRefreshEffect::None
        }
        ValidationPhaseTransitionDecision::NoChange => PostToolPhaseRefreshEffect::None,
    }
}

pub(crate) fn apply_verified_write_follow_on(
    user_prompt: &str,
    runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
    result: &VerifiedWriteResult,
) -> Option<VerifiedWriteFollowOnUpdate> {
    match result {
        VerifiedWriteResult::GuardRetry(message) => {
            runtime_checkpoint.retry_state.post_write_guard_retry_count += 1;
            runtime_checkpoint
                .retry_state
                .blocked_runtime_completion_count = 0;
            runtime_checkpoint.phase = crate::agent_runtime::state::RunPhase::VerifyingChanges;
            Some(VerifiedWriteFollowOnUpdate {
                control: PhaseLoopControl::ContinueAgentStep,
                developer_message: message.clone(),
            })
        }
        VerifiedWriteResult::FollowOnTurn(message) => {
            runtime_checkpoint
                .retry_state
                .post_write_follow_on_turn_count += 1;
            runtime_checkpoint.retry_state.post_write_guard_retry_count = 0;
            runtime_checkpoint
                .retry_state
                .blocked_runtime_completion_count = 0;
            runtime_checkpoint.phase = crate::agent_runtime::state::RunPhase::VerifyingChanges;
            Some(VerifiedWriteFollowOnUpdate {
                control: PhaseLoopControl::ContinueAgentStep,
                developer_message: message.clone(),
            })
        }
        VerifiedWriteResult::StartRequiredValidationPhase(message) => {
            runtime_checkpoint.retry_state.post_write_guard_retry_count = 0;
            runtime_checkpoint
                .retry_state
                .blocked_runtime_completion_count = 0;
            runtime_checkpoint.phase = crate::agent_runtime::state::RunPhase::Validating;
            if runtime_checkpoint
                .validation_state
                .required_command
                .is_none()
            {
                runtime_checkpoint.validation_state.required_command =
                    crate::agent_impl_guard::prompt_required_validation_command(user_prompt)
                        .map(str::to_string);
            }
            Some(VerifiedWriteFollowOnUpdate {
                control: PhaseLoopControl::ContinueAgentStep,
                developer_message: message.clone(),
            })
        }
        VerifiedWriteResult::Done(_) => None,
    }
}

fn tool_fact_envelopes_from_tool_facts(
    tool_facts: &[ToolFactV1],
    phase: &crate::agent_runtime::state::RunPhase,
) -> Vec<ToolFactEnvelopeV1> {
    tool_fact_envelopes_from_facts(
        tool_facts,
        ToolFactSourceV1::ExecutionRecords,
        Some(crate::agent::interrupts::run_phase_name(phase)),
        None,
    )
}
