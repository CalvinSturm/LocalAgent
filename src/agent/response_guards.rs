use crate::agent_runtime::state::{RunCheckpointV1, RunPhase};
use crate::types::{Message, ToolCall};

pub(crate) enum RequiredValidationPhaseDecision {
    Proceed,
    TransitionToExecuting,
    ContinueStep {
        developer_message: String,
        blocked_count: u32,
    },
    PlannerError {
        reason: String,
        blocked_count: u32,
    },
}

pub(crate) enum PostResponseGuardDecision {
    Proceed,
    ContinueAgentStep {
        developer_message: String,
        blocked_count: u32,
        step_block_reason: &'static str,
    },
    PlannerError {
        reason: String,
        blocked_count: u32,
        step_block_reason: &'static str,
        failure_class: &'static str,
        error_source: &'static str,
    },
}

pub(crate) fn decide_required_validation_phase_response(
    user_prompt: &str,
    validation_shell_available: bool,
    runtime_checkpoint: &mut RunCheckpointV1,
    assistant: &Message,
    tool_calls: &[ToolCall],
    required_validation_phase_message: String,
) -> RequiredValidationPhaseDecision {
    if runtime_checkpoint.phase != RunPhase::Validating {
        return RequiredValidationPhaseDecision::Proceed;
    }
    if !validation_shell_available {
        runtime_checkpoint.phase = RunPhase::Executing;
        runtime_checkpoint.retry_state.blocked_required_validation_phase_count = 0;
        return RequiredValidationPhaseDecision::Proceed;
    }

    let assistant_has_prose = !assistant.content.as_deref().unwrap_or_default().trim().is_empty();
    let valid_required_validation_turn =
        tool_calls.len() == 1 && tool_calls[0].name == "shell" && !assistant_has_prose;
    if valid_required_validation_turn {
        runtime_checkpoint.phase = RunPhase::Executing;
        runtime_checkpoint.retry_state.blocked_required_validation_phase_count = 0;
        return RequiredValidationPhaseDecision::TransitionToExecuting;
    }

    runtime_checkpoint.retry_state.blocked_required_validation_phase_count = runtime_checkpoint
        .retry_state
        .blocked_required_validation_phase_count
        .saturating_add(1);
    let blocked_count = runtime_checkpoint.retry_state.blocked_required_validation_phase_count;
    if blocked_count >= 2 {
        return RequiredValidationPhaseDecision::PlannerError {
            reason: "MODEL_TOOL_PROTOCOL_VIOLATION: required validation phase requires exactly one shell tool call and no prose".to_string(),
            blocked_count,
        };
    }

    let _ = user_prompt;
    RequiredValidationPhaseDecision::ContinueStep {
        developer_message: required_validation_phase_message,
        blocked_count,
    }
}

pub(crate) fn decide_post_response_phase_guard(
    runtime_checkpoint: &mut RunCheckpointV1,
    assistant: &Message,
    has_actionable_tool_calls: bool,
    model_signaled_finalize: bool,
    tool_calls: &[ToolCall],
    post_validation_final_answer_only_message: String,
    tool_only_reminder_message: String,
) -> PostResponseGuardDecision {
    if runtime_checkpoint.validation_state.repair_mode
        && has_actionable_tool_calls
        && tool_calls.iter().all(|tc| tc.name == "shell")
    {
        runtime_checkpoint.retry_state.blocked_validation_failure_repair_count = runtime_checkpoint
            .retry_state
            .blocked_validation_failure_repair_count
            .saturating_add(1);
        let blocked_count = runtime_checkpoint.retry_state.blocked_validation_failure_repair_count;
        if blocked_count >= 2 {
            return PostResponseGuardDecision::PlannerError {
                reason: "MODEL_TOOL_PROTOCOL_VIOLATION: validation is still failing; inspect and change code before retrying shell".to_string(),
                blocked_count,
                step_block_reason: "validation_failure_requires_code_fix",
                failure_class: "E_PROTOCOL_VALIDATION_RETRY_WITHOUT_FIX",
                error_source: "runtime_required_validation_guard",
            };
        }
        return PostResponseGuardDecision::ContinueAgentStep {
            developer_message: "Validation is still failing. Do not rerun shell yet. Use read_file or grep to inspect the code, make a real code change with edit/apply_patch, then rerun the validation command.".to_string(),
            blocked_count,
            step_block_reason: "validation_failure_requires_code_fix",
        };
    }

    if runtime_checkpoint.phase == RunPhase::CollectingFinalAnswer
        && runtime_checkpoint.validation_state.satisfied
        && has_actionable_tool_calls
    {
        runtime_checkpoint.retry_state.blocked_post_validation_final_answer_count = runtime_checkpoint
            .retry_state
            .blocked_post_validation_final_answer_count
            .saturating_add(1);
        let blocked_count = runtime_checkpoint
            .retry_state
            .blocked_post_validation_final_answer_count;
        if blocked_count >= 2 {
            return PostResponseGuardDecision::PlannerError {
                reason: "MODEL_TOOL_PROTOCOL_VIOLATION: validation already succeeded; reply with final answer only".to_string(),
                blocked_count,
                step_block_reason: "post_validation_final_answer_only",
                failure_class: "E_PROTOCOL_POST_VALIDATION_TOOL_CALL",
                error_source: "runtime_required_validation_guard",
            };
        }
        return PostResponseGuardDecision::ContinueAgentStep {
            developer_message: post_validation_final_answer_only_message,
            blocked_count,
            step_block_reason: "post_validation_final_answer_only",
        };
    }

    if runtime_checkpoint.tool_protocol_state.tool_only_phase_active
        && model_signaled_finalize
        && !assistant.content.as_deref().unwrap_or_default().trim().is_empty()
    {
        runtime_checkpoint.tool_protocol_state.blocked_tool_only_count = runtime_checkpoint
            .tool_protocol_state
            .blocked_tool_only_count
            .saturating_add(1);
        let blocked_count = runtime_checkpoint.tool_protocol_state.blocked_tool_only_count;
        if blocked_count >= 2 {
            return PostResponseGuardDecision::PlannerError {
                reason: "MODEL_TOOL_PROTOCOL_VIOLATION: repeated prose output during tool-only phase"
                    .to_string(),
                blocked_count,
                step_block_reason: "tool_only_violation",
                failure_class: "E_PROTOCOL_TOOL_ONLY",
                error_source: "tool_protocol_guard",
            };
        }
        return PostResponseGuardDecision::ContinueAgentStep {
            developer_message: tool_only_reminder_message,
            blocked_count,
            step_block_reason: "tool_only_violation",
        };
    }

    PostResponseGuardDecision::Proceed
}
