use crate::events::EventKind;
use crate::providers::ModelProvider;
use crate::taint::TaintState;
use crate::types::TokenUsage;

use super::agent_types::{AgentOutcome, AgentOutcomeBuilderInput};
use super::Agent;

impl<P: ModelProvider> Agent<P> {
    fn has_user_facing_assistant_closeout_after_last_tool(
        &self,
        messages: &[crate::types::Message],
    ) -> bool {
        let last_tool_idx = messages
            .iter()
            .rposition(|m| matches!(m.role, crate::types::Role::Tool));
        let Some(last_tool_idx) = last_tool_idx else {
            return messages
                .iter()
                .rev()
                .filter(|m| matches!(m.role, crate::types::Role::Assistant))
                .filter_map(|m| m.content.as_deref())
                .map(str::trim)
                .any(|content| {
                    !content.is_empty()
                        && !self.assistant_content_has_protocol_artifacts(Some(content))
                });
        };

        messages.iter().skip(last_tool_idx + 1).any(|m| {
            matches!(m.role, crate::types::Role::Assistant)
                && m.content.as_deref().is_some_and(|content| {
                    let content = content.trim();
                    !content.is_empty()
                        && !self.assistant_content_has_protocol_artifacts(Some(content))
                })
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_verified_write_completion(
        &mut self,
        step: u32,
        run_id: String,
        started_at: String,
        user_prompt: &str,
        verified_paths: Vec<String>,
        observed_tool_calls: Vec<crate::types::ToolCall>,
        observed_tool_executions: &[crate::agent_impl_guard::ToolExecutionRecord],
        observed_tool_decisions: Vec<super::ToolDecisionRecord>,
        messages: Vec<crate::types::Message>,
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
    ) -> super::runtime_completion::VerifiedWriteResult {
        use super::runtime_completion::VerifiedWriteResult;
        let post_verify_note = if verified_paths.is_empty() {
            "Runtime note: post-write verification succeeded. Prefer finishing now. If the requested change is complete, provide the final answer immediately and do not call another write tool. Only call another tool if the prompt still has an unfinished required step, such as running tests or inspecting another file.".to_string()
        } else {
            format!(
                "Runtime note: post-write verification succeeded for {}. Prefer finishing now. If the requested change is complete, provide the final answer immediately and do not call another write tool for that path. Only call another tool if the prompt still has an unfinished required step, such as running tests or inspecting another file.",
                verified_paths.join(", ")
            )
        };
        if let Some(reason) =
            crate::agent_impl_guard::implementation_integrity_violation_with_tool_executions(
                user_prompt,
                &post_verify_note,
                &observed_tool_calls,
                observed_tool_executions,
                enforce_implementation_integrity_guard,
            )
        {
            let is_retryable = reason.contains("requires prior read_file")
                || reason.contains("post-write verification missing");
            if is_retryable && post_write_guard_retry_count < 1 {
                self.emit_event(
                    &run_id,
                    step,
                    EventKind::Error,
                    serde_json::json!({
                        "error": reason,
                        "source": "implementation_integrity_guard",
                        "reason_code": "post_write_guard_retry"
                    }),
                );
                let corrective = if reason.contains("requires prior read_file") {
                    "You must read_file on a path before editing it. Use read_file to inspect the file contents first, then apply your changes, then read_file again to verify."
                } else {
                    "Post-write verification requires a read_file call after writing. Use read_file on the modified path to verify your changes."
                };
                return VerifiedWriteResult::GuardRetry(corrective.to_string());
            }
            self.emit_event(
                &run_id,
                step,
                EventKind::Error,
                serde_json::json!({
                    "error": reason,
                    "source": "implementation_integrity_guard"
                }),
            );
            return VerifiedWriteResult::Done(Box::new(self.finalize_planner_error_with_end(
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
            )));
        }
        let final_output = messages
            .iter()
            .rev()
            .filter(|m| matches!(m.role, crate::types::Role::Assistant))
            .filter_map(|m| m.content.as_deref())
            .map(str::trim)
            .find(|content| {
                !content.is_empty() && !self.assistant_content_has_protocol_artifacts(Some(content))
            })
            .unwrap_or_default()
            .to_string();
        let has_post_tool_assistant_closeout =
            self.has_user_facing_assistant_closeout_after_last_tool(&messages);
        let needs_follow_on_turn =
            crate::agent_impl_guard::prompt_requires_post_write_follow_on(user_prompt)
                && !has_post_tool_assistant_closeout;
        if needs_follow_on_turn && post_write_follow_on_turn_count < 1 {
            self.emit_event(
                &run_id,
                step,
                EventKind::StepBlocked,
                serde_json::json!({
                    "reason": "post_write_follow_on_required",
                    "source": "runtime_post_write_follow_on",
                    "follow_on_turn_count": post_write_follow_on_turn_count
                }),
            );
            let follow_on_message = if verified_paths.is_empty() {
                "Post-write verification succeeded. The user prompt still requires a follow-on step. Take exactly one more turn now: if the prompt asked for validation or tests, do that next if permitted; otherwise provide the final user-facing answer summarizing what changed. Do not call another write tool unless a still-unfinished required step makes it necessary."
                    .to_string()
            } else {
                format!(
                    "Post-write verification succeeded for {}. The user prompt still requires a follow-on step. Take exactly one more turn now: if the prompt asked for validation or tests, do that next if permitted; otherwise provide the final user-facing answer summarizing what changed. Do not call another write tool for those paths unless a still-unfinished required step makes it necessary.",
                    verified_paths.join(", ")
                )
            };
            return VerifiedWriteResult::FollowOnTurn(follow_on_message);
        }
        VerifiedWriteResult::Done(Box::new(self.finalize_ok_with_end(
            step,
            run_id,
            started_at,
            final_output,
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
        )))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_budget_exceeded(
        &self,
        run_id: String,
        started_at: String,
        reason: String,
        messages: Vec<crate::types::Message>,
        tool_calls: Vec<crate::types::ToolCall>,
        tool_decisions: Vec<super::ToolDecisionRecord>,
        final_prompt_size_chars: usize,
        compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> AgentOutcome {
        self.finalize_run_outcome(
            AgentOutcomeBuilderInput {
                run_id,
                started_at,
                exit_reason: super::AgentExitReason::BudgetExceeded,
                final_output: reason.clone(),
                error: Some(reason),
                messages,
                tool_calls,
                tool_decisions,
                final_prompt_size_chars,
                compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
            },
            saw_token_usage,
            total_token_usage,
            taint_state,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_approval_required_with_end(
        &mut self,
        step: u32,
        run_id: String,
        started_at: String,
        final_output: String,
        messages: Vec<crate::types::Message>,
        tool_calls: Vec<crate::types::ToolCall>,
        tool_decisions: Vec<super::ToolDecisionRecord>,
        final_prompt_size_chars: usize,
        compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> AgentOutcome {
        self.finalize_run_outcome_with_end(
            step,
            AgentOutcomeBuilderInput {
                run_id,
                started_at,
                exit_reason: super::AgentExitReason::ApprovalRequired,
                final_output,
                error: None,
                messages,
                tool_calls,
                tool_decisions,
                final_prompt_size_chars,
                compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
            },
            saw_token_usage,
            total_token_usage,
            taint_state,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_ok_with_end(
        &mut self,
        step: u32,
        run_id: String,
        started_at: String,
        final_output: String,
        messages: Vec<crate::types::Message>,
        tool_calls: Vec<crate::types::ToolCall>,
        tool_decisions: Vec<super::ToolDecisionRecord>,
        final_prompt_size_chars: usize,
        compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> AgentOutcome {
        self.finalize_run_outcome_with_end(
            step,
            AgentOutcomeBuilderInput {
                run_id,
                started_at,
                exit_reason: super::AgentExitReason::Ok,
                final_output,
                error: None,
                messages,
                tool_calls,
                tool_decisions,
                final_prompt_size_chars,
                compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
            },
            saw_token_usage,
            total_token_usage,
            taint_state,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_max_steps_with_end(
        &mut self,
        step: u32,
        run_id: String,
        started_at: String,
        final_output: String,
        messages: Vec<crate::types::Message>,
        tool_calls: Vec<crate::types::ToolCall>,
        tool_decisions: Vec<super::ToolDecisionRecord>,
        final_prompt_size_chars: usize,
        compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> AgentOutcome {
        self.finalize_run_outcome_with_end(
            step,
            AgentOutcomeBuilderInput {
                run_id,
                started_at,
                exit_reason: super::AgentExitReason::MaxSteps,
                final_output,
                error: None,
                messages,
                tool_calls,
                tool_decisions,
                final_prompt_size_chars,
                compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
            },
            saw_token_usage,
            total_token_usage,
            taint_state,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_hook_aborted_with_end(
        &mut self,
        step: u32,
        run_id: String,
        started_at: String,
        final_output: String,
        error: String,
        messages: Vec<crate::types::Message>,
        tool_calls: Vec<crate::types::ToolCall>,
        tool_decisions: Vec<super::ToolDecisionRecord>,
        final_prompt_size_chars: usize,
        compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> AgentOutcome {
        self.finalize_run_outcome_with_end(
            step,
            AgentOutcomeBuilderInput {
                run_id,
                started_at,
                exit_reason: super::AgentExitReason::HookAborted,
                final_output,
                error: Some(error),
                messages,
                tool_calls,
                tool_decisions,
                final_prompt_size_chars,
                compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
            },
            saw_token_usage,
            total_token_usage,
            taint_state,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_provider_error_with_end(
        &mut self,
        step: u32,
        run_id: String,
        started_at: String,
        error: String,
        messages: Vec<crate::types::Message>,
        tool_calls: Vec<crate::types::ToolCall>,
        tool_decisions: Vec<super::ToolDecisionRecord>,
        final_prompt_size_chars: usize,
        compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> AgentOutcome {
        self.finalize_run_outcome_with_end(
            step,
            AgentOutcomeBuilderInput {
                run_id,
                started_at,
                exit_reason: super::AgentExitReason::ProviderError,
                final_output: String::new(),
                error: Some(error),
                messages,
                tool_calls,
                tool_decisions,
                final_prompt_size_chars,
                compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
            },
            saw_token_usage,
            total_token_usage,
            taint_state,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_budget_exceeded_with_end(
        &mut self,
        step: u32,
        run_id: String,
        started_at: String,
        reason: String,
        messages: Vec<crate::types::Message>,
        tool_calls: Vec<crate::types::ToolCall>,
        tool_decisions: Vec<super::ToolDecisionRecord>,
        final_prompt_size_chars: usize,
        compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> AgentOutcome {
        self.finalize_run_outcome_with_end(
            step,
            AgentOutcomeBuilderInput {
                run_id,
                started_at,
                exit_reason: super::AgentExitReason::BudgetExceeded,
                final_output: reason.clone(),
                error: Some(reason),
                messages,
                tool_calls,
                tool_decisions,
                final_prompt_size_chars,
                compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
            },
            saw_token_usage,
            total_token_usage,
            taint_state,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_denied_with_end(
        &mut self,
        step: u32,
        run_id: String,
        started_at: String,
        final_output: String,
        error: Option<String>,
        messages: Vec<crate::types::Message>,
        tool_calls: Vec<crate::types::ToolCall>,
        tool_decisions: Vec<super::ToolDecisionRecord>,
        final_prompt_size_chars: usize,
        compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> AgentOutcome {
        self.finalize_run_outcome_with_end(
            step,
            AgentOutcomeBuilderInput {
                run_id,
                started_at,
                exit_reason: super::AgentExitReason::Denied,
                final_output,
                error,
                messages,
                tool_calls,
                tool_decisions,
                final_prompt_size_chars,
                compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
            },
            saw_token_usage,
            total_token_usage,
            taint_state,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_planner_error_with_end(
        &mut self,
        step: u32,
        run_id: String,
        started_at: String,
        error: String,
        messages: Vec<crate::types::Message>,
        tool_calls: Vec<crate::types::ToolCall>,
        tool_decisions: Vec<super::ToolDecisionRecord>,
        final_prompt_size_chars: usize,
        compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> AgentOutcome {
        let final_output = messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, crate::types::Role::Assistant))
            .and_then(|m| m.content.clone())
            .unwrap_or_default();
        self.finalize_run_outcome_with_end(
            step,
            AgentOutcomeBuilderInput {
                run_id,
                started_at,
                exit_reason: super::AgentExitReason::PlannerError,
                final_output,
                error: Some(error),
                messages,
                tool_calls,
                tool_decisions,
                final_prompt_size_chars,
                compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
            },
            saw_token_usage,
            total_token_usage,
            taint_state,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_planner_error_with_output_with_end(
        &mut self,
        step: u32,
        run_id: String,
        started_at: String,
        error: String,
        messages: Vec<crate::types::Message>,
        tool_calls: Vec<crate::types::ToolCall>,
        tool_decisions: Vec<super::ToolDecisionRecord>,
        final_prompt_size_chars: usize,
        compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> AgentOutcome {
        self.finalize_run_outcome_with_end(
            step,
            AgentOutcomeBuilderInput {
                run_id,
                started_at,
                exit_reason: super::AgentExitReason::PlannerError,
                final_output: error.clone(),
                error: Some(error),
                messages,
                tool_calls,
                tool_decisions,
                final_prompt_size_chars,
                compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
            },
            saw_token_usage,
            total_token_usage,
            taint_state,
        )
    }

    pub(super) fn finalize_run_outcome(
        &self,
        input: AgentOutcomeBuilderInput,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> AgentOutcome {
        AgentOutcome {
            run_id: input.run_id,
            started_at: input.started_at,
            finished_at: crate::trust::now_rfc3339(),
            exit_reason: input.exit_reason,
            final_output: input.final_output,
            error: input.error,
            messages: input.messages,
            tool_calls: input.tool_calls,
            tool_decisions: input.tool_decisions,
            compaction_settings: self.compaction_settings.clone(),
            final_prompt_size_chars: input.final_prompt_size_chars,
            compaction_report: input.compaction_report,
            hook_invocations: input.hook_invocations,
            provider_retry_count: input.provider_retry_count,
            provider_error_count: input.provider_error_count,
            token_usage: if saw_token_usage {
                Some(total_token_usage.clone())
            } else {
                None
            },
            taint: crate::agent_taint_helpers::taint_record_from_state(
                self.taint_toggle,
                self.taint_mode,
                self.taint_digest_bytes,
                taint_state,
            ),
        }
    }

    pub(super) fn finalize_run_outcome_with_end(
        &mut self,
        step: u32,
        input: AgentOutcomeBuilderInput,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> AgentOutcome {
        let run_id = input.run_id.clone();
        let exit_reason = input.exit_reason.as_str().to_string();
        self.emit_event(
            &run_id,
            step,
            EventKind::RunEnd,
            serde_json::json!({"exit_reason": exit_reason}),
        );
        self.finalize_run_outcome(input, saw_token_usage, total_token_usage, taint_state)
    }
}
