use super::PlanToolEnforcementMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RuntimeCompletionDecision {
    ExecuteTools,
    Continue {
        reason_code: &'static str,
        corrective_instruction: &'static str,
    },
    FinalizeOk,
    FinalizeError {
        reason: &'static str,
        source: &'static str,
        failure_class: &'static str,
    },
}

pub(super) struct RuntimeCompletionInputs {
    pub(super) has_tool_calls: bool,
    pub(super) plan_tool_enforcement: PlanToolEnforcementMode,
    pub(super) active_plan_step_idx: usize,
    pub(super) plan_step_constraints_len: usize,
    pub(super) tool_only_phase_active: bool,
    pub(super) enforce_implementation_integrity_guard: bool,
    pub(super) observed_tool_calls_len: usize,
    pub(super) blocked_attempt_count_next: u32,
}

pub(super) fn runtime_completion_decision(
    inputs: &RuntimeCompletionInputs,
) -> RuntimeCompletionDecision {
    if inputs.has_tool_calls {
        return RuntimeCompletionDecision::ExecuteTools;
    }
    if !matches!(inputs.plan_tool_enforcement, PlanToolEnforcementMode::Off)
        && inputs.plan_step_constraints_len > 0
        && inputs.active_plan_step_idx < inputs.plan_step_constraints_len
    {
        if inputs.blocked_attempt_count_next >= 2 {
            return RuntimeCompletionDecision::FinalizeError {
                reason: "model repeatedly attempted to halt before completing required planner steps",
                source: "runtime_completion_policy",
                failure_class: "E_RUNTIME_COMPLETION_PENDING_PLAN",
            };
        }
        return RuntimeCompletionDecision::Continue {
            reason_code: "pending_plan_step",
            corrective_instruction: "Continue execution. Do not finalize yet. Complete the pending planner step and return the next tool call.",
        };
    }
    if inputs.tool_only_phase_active {
        if inputs.blocked_attempt_count_next >= 2 {
            return RuntimeCompletionDecision::FinalizeError {
                reason: "model repeatedly attempted to finalize during tool-only phase without a tool call",
                source: "runtime_completion_policy",
                failure_class: "E_RUNTIME_COMPLETION_TOOL_ONLY",
            };
        }
        return RuntimeCompletionDecision::Continue {
            reason_code: "tool_only_requires_tool_call",
            corrective_instruction:
                "Tool-only phase active. Return exactly one valid tool call and no prose.",
        };
    }
    if inputs.enforce_implementation_integrity_guard && inputs.observed_tool_calls_len == 0 {
        if inputs.blocked_attempt_count_next >= 2 {
            return RuntimeCompletionDecision::FinalizeError {
                reason: "implementation guard: file-edit task finalized without any tool calls",
                source: "implementation_integrity_guard",
                failure_class: "E_RUNTIME_COMPLETION_IMPLEMENTATION_NO_TOOLS",
            };
        }
        return RuntimeCompletionDecision::Continue {
            reason_code: "implementation_requires_tool_calls",
            corrective_instruction:
                "Implementation task requires concrete tool-backed changes. Read/edit files with tools and then continue.",
        };
    }
    RuntimeCompletionDecision::FinalizeOk
}
