use super::PlanToolEnforcementMode;
use crate::agent_impl_guard::ToolExecutionRecord;
use crate::providers::ModelProvider;
use crate::taint::TaintState;
use crate::types::{Message, TokenUsage, ToolCall};

use super::{Agent, ToolDecisionRecord};

fn assistant_content_fabricates_tool_result(assistant_content: Option<&str>) -> bool {
    let text = assistant_content.unwrap_or_default().to_ascii_uppercase();
    text.contains("[TOOL_RESULT]") || text.contains("[END_TOOL_RESULT]")
}

fn last_successful_read_path(observed_tool_executions: &[ToolExecutionRecord]) -> Option<String> {
    observed_tool_executions.iter().rev().find_map(|record| {
        (record.ok && record.name == "read_file")
            .then(|| record.path.clone())
            .flatten()
    })
}

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
    ContinueExactFinalAnswer {
        blocked_runtime_completion_count: u32,
        operator_delivery_count: u32,
    },
    ContinueRequiredValidation {
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
    pub(super) exact_final_answer_only_phase_active: bool,
    pub(super) enforce_implementation_integrity_guard: bool,
    pub(super) observed_tool_calls_len: usize,
    pub(super) blocked_attempt_count_next: u32,
}

pub(super) fn runtime_completion_decision(
    inputs: &RuntimeCompletionInputs,
) -> RuntimeCompletionDecision {
    if inputs.exact_final_answer_only_phase_active && inputs.has_tool_calls {
        return RuntimeCompletionDecision::FinalizeError {
            reason: "exact final-answer retry does not allow additional tool calls",
            source: "runtime_exact_final_answer_guard",
            failure_class: "E_RUNTIME_COMPLETION_EXACT_FINAL_TOOL_CALL",
        };
    }
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
        exact_final_answer_retry_count: u32,
        required_validation_retry_count: u32,
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
                if self.assistant_content_has_protocol_artifacts(assistant_content) {
                    let blocked_runtime_completion_count =
                        blocked_runtime_completion_count.saturating_add(1);
                    let fabricated_tool_result =
                        assistant_content_fabricates_tool_result(assistant_content);
                    let recent_read_path =
                        if fabricated_tool_result && enforce_implementation_integrity_guard {
                            last_successful_read_path(observed_tool_executions)
                        } else {
                            None
                        };
                    let (corrective_instruction, reason_code) = if let Some(path) = recent_read_path
                    {
                        (
                            format!(
                                "Do not simulate tool results. Emit exactly one real tool call now and no prose. You already read `{path}`. Use edit or apply_patch on that file."
                            ),
                            "assistant_fabricated_tool_result_after_read",
                        )
                    } else if fabricated_tool_result {
                        (
                            "Do not simulate tool results. Emit exactly one real tool call now and no prose. If you already inspected the file, use edit or apply_patch on that file."
                                .to_string(),
                            "assistant_fabricated_tool_result",
                        )
                    } else {
                        (
                            "Your last message repeated tool protocol artifacts instead of a user-facing answer. Do not echo [TOOL_CALL] or [TOOL_RESULT] blocks. If the task is complete, reply with the final answer only."
                                .to_string(),
                            "assistant_protocol_artifact_echo",
                        )
                    };
                    self.emit_event(
                        &run_id,
                        step,
                        crate::events::EventKind::Error,
                        serde_json::json!({
                            "error": corrective_instruction,
                            "source": "tool_protocol_guard",
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
                    messages.push(Message {
                        role: crate::types::Role::Developer,
                        content: Some(corrective_instruction),
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                    });
                    return RuntimeCompletionAction::ContinueAgentStep {
                        blocked_runtime_completion_count,
                        operator_delivery_count,
                    };
                }
                let mut final_output =
                    self.final_output_for_completion(last_user_output_raw, assistant_content);
                if enforce_implementation_integrity_guard {
                    const MAX_POST_WRITE_VERIFY_PATHS: usize = 10;
                    let post_write_verify_timeout_ms =
                        self.effective_post_write_verify_timeout_ms();
                    let pending_post_write_paths =
                        crate::agent_impl_guard::pending_post_write_verification_paths(
                            &observed_tool_calls,
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
                    let saw_effective_write = observed_tool_executions.iter().any(|e| {
                        e.ok && matches!(
                            e.name.as_str(),
                            "apply_patch" | "edit" | "write_file" | "str_replace"
                        ) && e.changed != Some(false)
                    });
                    let is_retryable = (!saw_effective_write
                        && reason.contains("without an effective write"))
                        || reason.contains("requires prior read_file");
                    if is_retryable && blocked_runtime_completion_count < 2 {
                        let blocked_runtime_completion_count =
                            blocked_runtime_completion_count.saturating_add(1);
                        let corrective_instruction = if reason.contains("requires prior read_file")
                        {
                            "You must read_file on a path before editing it. Use read_file to inspect the file contents first, then edit or apply_patch to make changes, then read_file again to verify."
                        } else {
                            "Implementation task requires at least one effective write tool call. Use read_file + edit/apply_patch (or write_file when creating a new file), then verify with read_file before finalizing."
                        };
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
                let validation_facts = crate::agent::completion_policy::collect_validation_facts(
                    user_prompt,
                    &crate::agent::tool_facts_from_calls_and_executions(
                        user_prompt,
                        &observed_tool_calls,
                        observed_tool_executions,
                    ),
                );
                match crate::agent::completion_policy::decide_required_validation_completion(
                    &validation_facts,
                    required_validation_retry_count,
                ) {
                    crate::agent::completion_policy::RequiredValidationCompletionDecision::FinalizeNow => {}
                    crate::agent::completion_policy::RequiredValidationCompletionDecision::ContinueRequiredValidation(corrective_instruction) => {
                        let blocked_runtime_completion_count =
                            blocked_runtime_completion_count.saturating_add(1);
                        self.emit_event(
                            &run_id,
                            step,
                            crate::events::EventKind::Error,
                            serde_json::json!({
                                "error": corrective_instruction,
                                "source": "runtime_required_validation_guard",
                                "reason_code": "required_validation_before_final",
                                "blocked_count": blocked_runtime_completion_count
                            }),
                        );
                        self.emit_event(
                            &run_id,
                            step,
                            crate::events::EventKind::StepBlocked,
                            serde_json::json!({
                                "reason": "required_validation_before_final",
                                "blocked_count": blocked_runtime_completion_count
                            }),
                        );
                        messages.push(Message {
                            role: crate::types::Role::Developer,
                            content: Some(corrective_instruction),
                            tool_call_id: None,
                            tool_name: None,
                            tool_calls: None,
                        });
                        return RuntimeCompletionAction::ContinueRequiredValidation {
                            blocked_runtime_completion_count,
                            operator_delivery_count,
                        };
                    }
                    crate::agent::completion_policy::RequiredValidationCompletionDecision::FinalizeError(reason) => {
                        self.emit_event(
                            &run_id,
                            step,
                            crate::events::EventKind::Error,
                            serde_json::json!({
                                "error": reason,
                                "source": "runtime_required_validation_guard",
                                "failure_class": "E_RUNTIME_COMPLETION_REQUIRED_VALIDATION"
                            }),
                        );
                        return RuntimeCompletionAction::Finalize(Box::new(
                            self.finalize_planner_error_with_end(
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
                            ),
                        ));
                    }
                }
                if crate::agent_impl_guard::prompt_required_exact_final_answer(user_prompt)
                    .is_some()
                    && !crate::agent_impl_guard::final_output_matches_required_exact_answer(
                        user_prompt,
                        &final_output,
                    )
                {
                    if let Some(recovered_output) =
                        crate::agent_impl_guard::recover_required_exact_final_answer(
                            user_prompt,
                            &final_output,
                        )
                    {
                        final_output = recovered_output;
                    } else if exact_final_answer_retry_count < 1 {
                        let blocked_runtime_completion_count =
                            blocked_runtime_completion_count.saturating_add(1);
                        let corrective_instruction = "The task work is complete. Do not explain your steps. Reply now with the required final answer only, exactly matching the requested format. Do not call tools.";
                        self.emit_event(
                            &run_id,
                            step,
                            crate::events::EventKind::Error,
                            serde_json::json!({
                                "error": corrective_instruction,
                                "source": "runtime_exact_final_answer_guard",
                                "reason_code": "exact_final_answer_required",
                                "blocked_count": blocked_runtime_completion_count
                            }),
                        );
                        self.emit_event(
                            &run_id,
                            step,
                            crate::events::EventKind::StepBlocked,
                            serde_json::json!({
                                "reason": "exact_final_answer_required",
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
                        return RuntimeCompletionAction::ContinueExactFinalAnswer {
                            blocked_runtime_completion_count,
                            operator_delivery_count,
                        };
                    } else {
                        let reason =
                            "model failed exact final-answer compliance after bounded retry";
                        self.emit_event(
                            &run_id,
                            step,
                            crate::events::EventKind::Error,
                            serde_json::json!({
                                "error": reason,
                                "source": "runtime_exact_final_answer_guard",
                                "failure_class": "E_RUNTIME_COMPLETION_EXACT_FINAL_OUTPUT"
                            }),
                        );
                        return RuntimeCompletionAction::Finalize(Box::new(
                            self.finalize_planner_error_with_end(
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
                            ),
                        ));
                    }
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
        post_write_guard_retry_count: u32,
        post_write_follow_on_turn_count: u32,
    ) -> VerifiedWriteResult {
        const MAX_POST_WRITE_VERIFY_PATHS: usize = 10;
        let post_write_verify_timeout_ms = self.effective_post_write_verify_timeout_ms();
        let pending_post_write_paths =
            crate::agent_impl_guard::pending_post_write_verification_paths(
                &observed_tool_calls,
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
                    if reason.contains("requires prior read_file")
                        && post_write_guard_retry_count < 1
                    {
                        self.emit_event(
                            &run_id,
                            step,
                            crate::events::EventKind::Error,
                            serde_json::json!({
                                "error": reason,
                                "source": "implementation_integrity_guard",
                                "reason_code": "post_write_guard_retry"
                            }),
                        );
                        return VerifiedWriteResult::GuardRetry(
                            "You must read_file on a path before editing it. Use read_file to inspect the file contents first, then apply your changes, then read_file again to verify.".to_string(),
                        );
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
                    return VerifiedWriteResult::Done(Box::new(
                        self.finalize_planner_error_with_end(
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
                        ),
                    ));
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
            post_write_guard_retry_count,
            post_write_follow_on_turn_count,
        )
    }
}

pub(super) enum VerifiedWriteResult {
    Done(Box<super::agent_types::AgentOutcome>),
    GuardRetry(String),
    FollowOnTurn(String),
    StartRequiredValidationPhase(String),
}
