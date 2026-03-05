use crate::events::EventKind;
use crate::providers::ModelProvider;
use crate::taint::TaintState;
use crate::types::TokenUsage;

use super::agent_types::{AgentOutcome, AgentOutcomeBuilderInput};
use super::Agent;

impl<P: ModelProvider> Agent<P> {
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
        self.finalize_run_outcome_with_end(
            step,
            AgentOutcomeBuilderInput {
                run_id,
                started_at,
                exit_reason: super::AgentExitReason::PlannerError,
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
