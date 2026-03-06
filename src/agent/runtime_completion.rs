use super::PlanToolEnforcementMode;
use crate::agent_impl_guard::ToolExecutionRecord;
use crate::providers::ModelProvider;
use crate::taint::TaintState;
use crate::types::{Message, TokenUsage, ToolCall};

use super::{Agent, ToolDecisionRecord};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCompletionDecision {
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

pub(super) enum RuntimeCompletionAction {
    ContinueStep {
        blocked_runtime_completion_count: u32,
    },
    ContinueAgentStep {
        blocked_runtime_completion_count: u32,
        operator_delivery_count: u32,
    },
    ProceedToTools {
        blocked_runtime_completion_count: u32,
    },
    Finalize(Box<super::agent_types::AgentOutcome>),
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
                reason:
                    "model repeatedly attempted to halt before completing required planner steps",
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
    pub(super) async fn handle_runtime_completion_action(
        &mut self,
        decision: RuntimeCompletionDecision,
        run_id: String,
        step: u32,
        started_at: String,
        user_prompt: &str,
        last_user_output_raw: Option<&String>,
        assistant_content: Option<&str>,
        active_plan_step_idx: usize,
        enforce_implementation_integrity_guard: bool,
        blocked_runtime_completion_count: u32,
        operator_delivery_count: u32,
        messages: &mut Vec<Message>,
        observed_tool_calls: Vec<ToolCall>,
        observed_tool_executions: &mut Vec<ToolExecutionRecord>,
        observed_tool_decisions: Vec<ToolDecisionRecord>,
        request_context_chars: usize,
        last_compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> RuntimeCompletionAction {
        match decision {
            RuntimeCompletionDecision::Continue {
                reason_code,
                corrective_instruction,
            } => {
                let mut error_text = corrective_instruction.to_string();
                let mut source = "runtime_completion_policy";
                if reason_code == "pending_plan_step" && self.plan_enforcement_active() {
                    if let Some(text) = self.pending_plan_step_text(active_plan_step_idx) {
                        error_text = text;
                        source = "plan_halt_guard";
                    }
                }
                self.emit_event(
                    &run_id,
                    step,
                    crate::events::EventKind::Error,
                    serde_json::json!({
                        "error": error_text,
                        "source": source,
                        "reason_code": reason_code,
                        "blocked_count": blocked_runtime_completion_count
                    }),
                );
                self.emit_event(
                    &run_id,
                    step,
                    crate::events::EventKind::StepBlocked,
                    serde_json::json!({
                        "reason": reason_code,
                        "blocked_count": blocked_runtime_completion_count
                    }),
                );
                let corrective_message =
                    if reason_code == "pending_plan_step" && self.plan_enforcement_active() {
                        self.pending_plan_step_corrective_message(active_plan_step_idx)
                            .unwrap_or_else(|| corrective_instruction.to_string())
                    } else {
                        corrective_instruction.to_string()
                    };
                messages.push(Message {
                    role: crate::types::Role::Developer,
                    content: Some(corrective_message),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                });
                RuntimeCompletionAction::ContinueStep {
                    blocked_runtime_completion_count,
                }
            }
            RuntimeCompletionDecision::FinalizeError {
                reason,
                source,
                failure_class,
            } => {
                self.emit_event(
                    &run_id,
                    step,
                    crate::events::EventKind::Error,
                    serde_json::json!({
                        "error": reason,
                        "source": source,
                        "failure_class": failure_class
                    }),
                );
                RuntimeCompletionAction::Finalize(Box::new(self.finalize_planner_error_with_end(
                    step,
                    run_id,
                    started_at,
                    reason.to_string(),
                    messages.clone(),
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
                )))
            }
            RuntimeCompletionDecision::FinalizeOk => {
                const MAX_OPERATOR_DELIVERIES_PER_STEP: u32 = 3;
                if operator_delivery_count < MAX_OPERATOR_DELIVERIES_PER_STEP {
                    let (queue_delivered, queue_interrupted) =
                        self.inject_turn_idle_operator_messages(&run_id, step, messages);
                    if queue_interrupted || queue_delivered {
                        return RuntimeCompletionAction::ContinueAgentStep {
                            blocked_runtime_completion_count: 0,
                            operator_delivery_count: operator_delivery_count + 1,
                        };
                    }
                }
                let final_output =
                    self.final_output_for_completion(last_user_output_raw, assistant_content);
                if enforce_implementation_integrity_guard {
                    const MAX_POST_WRITE_VERIFY_PATHS: usize = 10;
                    let post_write_verify_timeout_ms =
                        self.effective_post_write_verify_timeout_ms();
                    let pending_post_write_paths =
                        crate::agent_impl_guard::pending_post_write_verification_paths(
                            observed_tool_executions,
                        );
                    for path in pending_post_write_paths
                        .into_iter()
                        .take(MAX_POST_WRITE_VERIFY_PATHS)
                    {
                        match self
                            .verify_post_write_path(
                                &run_id,
                                step,
                                &path,
                                post_write_verify_timeout_ms,
                            )
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
                                return RuntimeCompletionAction::Finalize(Box::new(
                                    self.finalize_planner_error_with_end(
                                        step,
                                        run_id,
                                        started_at,
                                        reason,
                                        messages.clone(),
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
                                    ),
                                ));
                            }
                        }
                    }
                }
                if let Some(reason) =
                    crate::agent_impl_guard::implementation_integrity_violation_with_tool_executions(
                        user_prompt,
                        &final_output,
                        &observed_tool_calls,
                        observed_tool_executions,
                        enforce_implementation_integrity_guard,
                    )
                {
                    let saw_successful_write = observed_tool_executions
                        .iter()
                        .any(|e| e.ok && matches!(e.name.as_str(), "apply_patch" | "write_file"));
                    if !saw_successful_write
                        && reason.contains("without an effective write")
                        && blocked_runtime_completion_count < 2
                    {
                        let blocked_runtime_completion_count =
                            blocked_runtime_completion_count.saturating_add(1);
                        let corrective_instruction = "Implementation task requires at least one effective write tool call. Use read_file + apply_patch (or write_file when creating a new file), then verify with read_file before finalizing.";
                        self.emit_event(
                            &run_id,
                            step,
                            crate::events::EventKind::Error,
                            serde_json::json!({
                                "error": corrective_instruction,
                                "source": "implementation_integrity_guard",
                                "reason_code": "implementation_requires_effective_write",
                                "blocked_count": blocked_runtime_completion_count
                            }),
                        );
                        self.emit_event(
                            &run_id,
                            step,
                            crate::events::EventKind::StepBlocked,
                            serde_json::json!({
                                "reason": "implementation_requires_effective_write",
                                "blocked_count": blocked_runtime_completion_count
                            }),
                        );
                        messages.push(Message {
                            role: crate::types::Role::Developer,
                            content: Some(corrective_instruction.to_string()),
                            tool_call_id: None,
                            tool_name: None,
                            tool_calls: None,
                        });
                        return RuntimeCompletionAction::ContinueAgentStep {
                            blocked_runtime_completion_count,
                            operator_delivery_count,
                        };
                    }
                    self.emit_event(
                        &run_id,
                        step,
                        crate::events::EventKind::Error,
                        serde_json::json!({
                            "error": reason,
                            "source": "implementation_integrity_guard"
                        }),
                    );
                    return RuntimeCompletionAction::Finalize(Box::new(
                        self.finalize_planner_error_with_end(
                            step,
                            run_id,
                            started_at,
                            reason,
                            messages.clone(),
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
                        ),
                    ));
                }
                RuntimeCompletionAction::Finalize(Box::new(self.finalize_ok_with_end(
                    step,
                    run_id,
                    started_at,
                    final_output,
                    messages.clone(),
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
                )))
            }
            RuntimeCompletionDecision::ExecuteTools => RuntimeCompletionAction::ProceedToTools {
                blocked_runtime_completion_count: 0,
            },
        }
    }

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
        const MAX_POST_WRITE_VERIFY_PATHS: usize = 10;
        let post_write_verify_timeout_ms = self.effective_post_write_verify_timeout_ms();
        let pending_post_write_paths =
            crate::agent_impl_guard::pending_post_write_verification_paths(
                observed_tool_executions,
            );
        let verified_paths = pending_post_write_paths
            .iter()
            .take(MAX_POST_WRITE_VERIFY_PATHS)
            .cloned()
            .collect::<Vec<_>>();
        for path in pending_post_write_paths
            .into_iter()
            .take(MAX_POST_WRITE_VERIFY_PATHS)
        {
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
