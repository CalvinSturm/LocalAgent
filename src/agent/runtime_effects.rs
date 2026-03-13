use crate::types::Message;

use super::completion_policy::{
    validation_resume_execution_transition_decision, RuntimePhaseTransitionDecision,
};
use super::phase_transitions::{
    PostToolPhaseRefreshEffect, VerifiedWriteFollowOnUpdate,
};
use super::response_guards::{
    PostResponseGuardDecision, RequiredValidationPhaseDecision,
};
use super::{PhaseLoopControl, PhaseStepDispatch, Role};

pub(crate) struct StepBlockedEffect {
    pub(crate) reason: &'static str,
    pub(crate) blocked_count: u32,
}

pub(crate) struct PlannerErrorEffect {
    pub(crate) reason: String,
    pub(crate) step_block: StepBlockedEffect,
    pub(crate) failure_class: &'static str,
    pub(crate) error_source: &'static str,
}

pub(crate) enum GuardEffect {
    Proceed,
    ContinueStep(StepBlockedEffect),
    ContinueAgentStep(StepBlockedEffect),
    PlannerError(PlannerErrorEffect),
    EmitPhaseTransition(RuntimePhaseTransitionDecision),
}

pub(crate) struct CompletionBlockedEffect {
    pub(crate) transition: RuntimePhaseTransitionDecision,
    pub(crate) reason: String,
}

pub(crate) fn apply_required_validation_guard_decision(
    decision: RequiredValidationPhaseDecision,
    assistant: &Message,
    messages: &mut Vec<Message>,
) -> GuardEffect {
    match decision {
        RequiredValidationPhaseDecision::Proceed => GuardEffect::Proceed,
        RequiredValidationPhaseDecision::TransitionToExecuting => {
            GuardEffect::EmitPhaseTransition(validation_resume_execution_transition_decision())
        }
        RequiredValidationPhaseDecision::ContinueStep {
            developer_message,
            blocked_count,
        } => {
            append_assistant_and_developer(messages, assistant, developer_message);
            GuardEffect::ContinueStep(StepBlockedEffect {
                reason: "required_validation_phase_requires_shell",
                blocked_count,
            })
        }
        RequiredValidationPhaseDecision::PlannerError {
            reason,
            blocked_count,
        } => GuardEffect::PlannerError(PlannerErrorEffect {
            reason,
            step_block: StepBlockedEffect {
                reason: "required_validation_phase_requires_shell",
                blocked_count,
            },
            failure_class: "E_PROTOCOL_REQUIRED_VALIDATION_PHASE",
            error_source: "runtime_required_validation_guard",
        }),
    }
}

pub(crate) fn apply_post_response_guard_decision(
    decision: PostResponseGuardDecision,
    assistant: &Message,
    messages: &mut Vec<Message>,
) -> GuardEffect {
    match decision {
        PostResponseGuardDecision::Proceed => GuardEffect::Proceed,
        PostResponseGuardDecision::ContinueAgentStep {
            developer_message,
            blocked_count,
            step_block_reason,
        } => {
            append_assistant_and_developer(messages, assistant, developer_message);
            GuardEffect::ContinueAgentStep(StepBlockedEffect {
                reason: step_block_reason,
                blocked_count,
            })
        }
        PostResponseGuardDecision::PlannerError {
            reason,
            blocked_count,
            step_block_reason,
            failure_class,
            error_source,
        } => GuardEffect::PlannerError(PlannerErrorEffect {
            reason,
            step_block: StepBlockedEffect {
                reason: step_block_reason,
                blocked_count,
            },
            failure_class,
            error_source,
        }),
    }
}

pub(crate) fn completion_blocked_effect_from_post_tool_refresh(
    effect: PostToolPhaseRefreshEffect,
) -> Option<CompletionBlockedEffect> {
    match effect {
        PostToolPhaseRefreshEffect::None => None,
        PostToolPhaseRefreshEffect::EmitCompletionBlocked { transition, reason } => {
            Some(CompletionBlockedEffect { transition, reason })
        }
    }
}

pub(crate) fn apply_verified_write_follow_on_update(
    update: VerifiedWriteFollowOnUpdate,
    messages: &mut Vec<Message>,
) -> PhaseStepDispatch {
    push_developer_message(messages, update.developer_message);
    phase_step_dispatch_from_control(update.control)
}

pub(crate) fn phase_step_dispatch_from_control(control: PhaseLoopControl) -> PhaseStepDispatch {
    match control {
        PhaseLoopControl::Proceed => PhaseStepDispatch::StepComplete,
        PhaseLoopControl::ContinueStep => PhaseStepDispatch::ContinueStep,
        PhaseLoopControl::ContinueAgentStep => PhaseStepDispatch::ContinueAgentStep,
    }
}

fn append_assistant_and_developer(
    messages: &mut Vec<Message>,
    assistant: &Message,
    developer_message: String,
) {
    messages.push(assistant.clone());
    push_developer_message(messages, developer_message);
}

fn push_developer_message(messages: &mut Vec<Message>, developer_message: String) {
    messages.push(Message {
        role: Role::Developer,
        content: Some(developer_message),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
    });
}
