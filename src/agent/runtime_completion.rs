use super::PlanToolEnforcementMode;
use crate::agent_impl_guard::ToolExecutionRecord;
use crate::providers::ModelProvider;
use crate::taint::TaintState;
use crate::types::{Message, ToolCall, TokenUsage};

use super::{Agent, ToolDecisionRecord};

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

impl<P: ModelProvider> Agent<P> {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn finalize_verified_write_step_or_error(
        &mut self,
        run_id: String,
        step: u32,
        started_at: String,
        user_prompt: &str,
        observed_tool_calls: Vec<ToolCall>,
        observed_tool_executions: &mut Vec<ToolExecutionRecord>,
        observed_tool_decisions: Vec<ToolDecisionRecord>,
        messages: Vec<Message>,
        request_context_chars: usize,
        last_compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
        enforce_implementation_integrity_guard: bool,
    ) -> super::agent_types::AgentOutcome {
        let post_write_verify_timeout_ms = self.effective_post_write_verify_timeout_ms();
        let pending_post_write_paths =
            crate::agent_impl_guard::pending_post_write_verification_paths(observed_tool_executions);
        let verified_paths = pending_post_write_paths.iter().cloned().collect::<Vec<_>>();
        for path in pending_post_write_paths {
            match self
                .verify_post_write_path(&run_id, step, &path, post_write_verify_timeout_ms)
                .await
            {
                Ok(record) => observed_tool_executions.push(record),
                Err(reason) => {
                    self.emit_event(
                        &run_id,
                        step,
                        crate::events::EventKind::Error,
                        serde_json::json!({
                            "error": reason,
                            "source": "implementation_integrity_guard"
                        }),
                    );
                    return self.finalize_planner_error_with_end(
                        step,
                        run_id,
                        started_at,
                        reason,
                        messages,
                        observed_tool_calls,
                        observed_tool_decisions,
                        request_context_chars,
                        last_compaction_report,
                        hook_invocations,
                        provider_retry_count,
                        provider_error_count,
                        saw_token_usage,
                        total_token_usage,
                        taint_state,
                    );
                }
            }
        }
        self.finalize_verified_write_completion(
            step,
            run_id,
            started_at,
            user_prompt,
            verified_paths,
            observed_tool_calls,
            observed_tool_executions,
            observed_tool_decisions,
            messages,
            request_context_chars,
            last_compaction_report,
            hook_invocations,
            provider_retry_count,
            provider_error_count,
            saw_token_usage,
            total_token_usage,
            taint_state,
            enforce_implementation_integrity_guard,
        )
    }
}
