use crate::gate::GateEvent;
use crate::providers::ModelProvider;
use crate::taint::TaintState;
use crate::tools::tool_side_effects;
use crate::types::{Message, TokenUsage, ToolCall};

use super::agent_types::ToolDecisionRecord;
use super::tool_helpers::{
    normalized_tool_path_from_args, AllowedToolResultDecision, ToolRetryLoopOutcome,
};
use super::Agent;

pub(super) enum AllowToolCallDecision {
    Continue,
    RestartAgentStep,
    Finalize(Box<super::agent_types::AgentOutcome>),
}

pub(super) enum GateNonAllowDecision {
    ContinueToolLoop,
    RestartAgentStep,
    Finalize(Box<super::agent_types::AgentOutcome>),
}

pub(super) enum PlanConstraintDecision {
    Continue,
    ContinueToolLoop,
    RestartAgentStep,
    Finalize(Box<super::agent_types::AgentOutcome>),
}

impl<P: ModelProvider> Agent<P> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_existing_write_file_guard_with_end(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        started_at: String,
        messages: Vec<Message>,
        observed_tool_calls: Vec<ToolCall>,
        observed_tool_decisions: Vec<ToolDecisionRecord>,
        request_context_chars: usize,
        last_compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> super::agent_types::AgentOutcome {
        let blocked_path =
            normalized_tool_path_from_args(tc).unwrap_or_else(|| "<unknown>".to_string());
        let reason = format!(
            "implementation guard: write_file on '{blocked_path}' requires prior read_file on the same path"
        );
        self.emit_event(
            &run_id,
            step,
            crate::events::EventKind::Error,
            serde_json::json!({
                "error": reason,
                "source": "implementation_integrity_guard",
                "failure_class": "E_RUNTIME_WRITEFILE_EXISTING_BLOCKED",
                "path": blocked_path
            }),
        );
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
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn record_allowed_tool_result(
        &mut self,
        run_id: &str,
        step: u32,
        tc: &ToolCall,
        approval_id: Option<String>,
        approval_key: Option<String>,
        reason: Option<String>,
        source: Option<String>,
        taint_enforced: bool,
        escalated: bool,
        escalation_reason: Option<String>,
        approval_mode_meta: Option<String>,
        auto_scope_meta: Option<String>,
        approval_key_version_meta: Option<String>,
        tool_schema_hash_hex: Option<String>,
        hooks_config_hash_hex: Option<String>,
        planner_hash_hex: Option<String>,
        decision_exec_target: Option<String>,
        final_ok: bool,
        content: String,
        input_digest: Option<String>,
        output_digest: Option<String>,
        input_len: Option<usize>,
        output_len: Option<usize>,
        tool_retry_count: u32,
        final_truncated: bool,
        final_failure_class: Option<crate::agent_tool_exec::ToolFailureClass>,
        final_error_code: Option<crate::tools::ToolErrorCode>,
        taint_state: &TaintState,
        repeat_key: &str,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        invalid_patch_format_attempts: &mut std::collections::BTreeMap<String, u32>,
        successful_write_tool_ok_this_step: &mut bool,
        tool_msg: Message,
        messages: &mut Vec<Message>,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
    ) {
        self.gate.record(GateEvent {
            run_id: run_id.to_string(),
            step,
            tool_call_id: tc.id.clone(),
            tool: tc.name.clone(),
            arguments: tc.arguments.clone(),
            decision: "allow".to_string(),
            decision_reason: reason.clone(),
            decision_source: source.clone(),
            approval_id,
            approval_key,
            approval_mode: approval_mode_meta,
            auto_approve_scope: auto_scope_meta,
            approval_key_version: approval_key_version_meta,
            tool_schema_hash_hex,
            hooks_config_hash_hex,
            planner_hash_hex,
            exec_target: decision_exec_target,
            taint_overall: Some(taint_state.overall_str().to_string()),
            taint_enforced,
            escalated,
            escalation_reason: escalation_reason.clone(),
            result_ok: final_ok,
            result_content: content.clone(),
            result_input_digest: input_digest,
            result_output_digest: output_digest,
            result_input_len: input_len,
            result_output_len: output_len,
        });
        observed_tool_decisions.push(ToolDecisionRecord {
            step,
            tool_call_id: tc.id.clone(),
            tool: tc.name.clone(),
            decision: "allow".to_string(),
            reason,
            source,
            taint_overall: Some(taint_state.overall_str().to_string()),
            taint_enforced,
            escalated,
            escalation_reason,
        });
        if final_ok {
            failed_repeat_counts.remove(repeat_key);
            if tc.name == "apply_patch" {
                invalid_patch_format_attempts.remove(repeat_key);
            }
            if tc.name == "apply_patch" || tc.name == "write_file" {
                *successful_write_tool_ok_this_step = true;
            }
        } else {
            let n = failed_repeat_counts
                .entry(repeat_key.to_string())
                .or_insert(0);
            *n = n.saturating_add(1);
        }
        self.emit_event(
            run_id,
            step,
            crate::events::EventKind::ToolExecEnd,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "ok": final_ok,
                "truncated": final_truncated,
                "retry_count": tool_retry_count,
                "failure_class": final_failure_class.map(|c| c.as_str()),
                "error_code": final_error_code.map(|c| c.as_str())
            }),
        );
        messages.push(tool_msg);
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_plan_constraint_deny(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        plan_step_id: String,
        active_plan_step_idx: usize,
        plan_allowed_tools: Vec<String>,
        approval_mode_meta: Option<String>,
        auto_scope_meta: Option<String>,
        approval_key_version_meta: Option<String>,
        tool_schema_hash_hex: Option<String>,
        hooks_config_hash_hex: Option<String>,
        planner_hash_hex: Option<String>,
        decision_exec_target: Option<String>,
        started_at: String,
        messages: &mut Vec<Message>,
        observed_tool_calls: Vec<ToolCall>,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
        request_context_chars: usize,
        last_compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> PlanConstraintDecision {
        let reason = format!(
            "tool '{}' is not allowed for plan step {} (allowed: {})",
            tc.name,
            plan_step_id,
            if plan_allowed_tools.is_empty() {
                "none".to_string()
            } else {
                plan_allowed_tools.join(", ")
            }
        );
        self.emit_event(
            &run_id,
            step,
            crate::events::EventKind::StepBlocked,
            serde_json::json!({
                "step_id": plan_step_id.clone(),
                "tool": tc.name,
                "reason": "tool_not_allowed_by_plan",
                "allowed_tools": plan_allowed_tools.clone()
            }),
        );
        self.emit_event(
            &run_id,
            step,
            crate::events::EventKind::ToolDecision,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "decision": "deny",
                "reason": reason,
                "source": "plan_step_constraint",
                "planner_hash_hex": planner_hash_hex.clone(),
                "plan_step_id": plan_step_id,
                "plan_step_index": active_plan_step_idx,
                "plan_allowed_tools": plan_allowed_tools,
                "enforcement_mode": format!("{:?}", self.plan_tool_enforcement).to_lowercase()
            }),
        );
        self.gate.record(GateEvent {
            run_id: run_id.clone(),
            step,
            tool_call_id: tc.id.clone(),
            tool: tc.name.clone(),
            arguments: tc.arguments.clone(),
            decision: "deny".to_string(),
            decision_reason: Some(reason.clone()),
            decision_source: Some("plan_step_constraint".to_string()),
            approval_id: None,
            approval_key: None,
            approval_mode: approval_mode_meta,
            auto_approve_scope: auto_scope_meta,
            approval_key_version: approval_key_version_meta,
            tool_schema_hash_hex,
            hooks_config_hash_hex,
            planner_hash_hex,
            exec_target: decision_exec_target,
            taint_overall: Some(taint_state.overall_str().to_string()),
            taint_enforced: false,
            escalated: false,
            escalation_reason: None,
            result_ok: false,
            result_content: reason.clone(),
            result_input_digest: None,
            result_output_digest: None,
            result_input_len: None,
            result_output_len: None,
        });
        observed_tool_decisions.push(ToolDecisionRecord {
            step,
            tool_call_id: tc.id.clone(),
            tool: tc.name.clone(),
            decision: "deny".to_string(),
            reason: Some(reason.clone()),
            source: Some("plan_step_constraint".to_string()),
            taint_overall: Some(taint_state.overall_str().to_string()),
            taint_enforced: false,
            escalated: false,
            escalation_reason: None,
        });

        match self.plan_tool_enforcement {
            super::PlanToolEnforcementMode::Off => PlanConstraintDecision::Continue,
            super::PlanToolEnforcementMode::Soft => {
                self.emit_event(
                    &run_id,
                    step,
                    crate::events::EventKind::ToolExecEnd,
                    serde_json::json!({
                        "tool_call_id": tc.id,
                        "name": tc.name,
                        "ok": false,
                        "truncated": false,
                        "source": "plan_step_constraint"
                    }),
                );
                messages.push(crate::tools::envelope_to_message(
                    crate::tools::to_tool_result_envelope(
                        tc,
                        "runtime",
                        false,
                        reason,
                        false,
                        crate::tools::ToolResultMeta {
                            side_effects: tool_side_effects(&tc.name),
                            bytes: None,
                            exit_code: None,
                            stderr_truncated: None,
                            stdout_truncated: None,
                            source: "runtime".to_string(),
                            execution_target: "host".to_string(),
                            warnings: None,
                            warnings_max: None,
                            warnings_truncated: None,
                            docker: None,
                        },
                    ),
                ));
                if self.inject_post_tool_operator_messages(&run_id, step, messages) {
                    return PlanConstraintDecision::RestartAgentStep;
                }
                PlanConstraintDecision::ContinueToolLoop
            }
            super::PlanToolEnforcementMode::Hard => {
                PlanConstraintDecision::Finalize(Box::new(self.finalize_denied_with_end(
                    step,
                    run_id,
                    started_at,
                    reason,
                    None,
                    messages.clone(),
                    observed_tool_calls,
                    observed_tool_decisions.clone(),
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
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_gate_allow_tool_call(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        approval_id: Option<String>,
        approval_key: Option<String>,
        reason: Option<String>,
        source: Option<String>,
        taint_enforced: bool,
        escalated: bool,
        escalation_reason: Option<String>,
        invalid_args_error: Option<String>,
        plan_tool_allowed: bool,
        repeat_key: &str,
        approval_mode_meta: Option<String>,
        auto_scope_meta: Option<String>,
        approval_key_version_meta: Option<String>,
        tool_schema_hash_hex: Option<String>,
        hooks_config_hash_hex: Option<String>,
        planner_hash_hex: Option<String>,
        decision_exec_target: Option<String>,
        started_at: String,
        messages: &mut Vec<Message>,
        observed_tool_calls: Vec<ToolCall>,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
        observed_tool_executions: &mut Vec<crate::agent_impl_guard::ToolExecutionRecord>,
        request_context_chars: usize,
        last_compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: &mut Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &mut TaintState,
        tool_budget_usage: &mut crate::agent_budget::ToolCallBudgetUsage,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        invalid_patch_format_attempts: &mut std::collections::BTreeMap<String, u32>,
        schema_repair_attempts: &mut std::collections::BTreeMap<String, u32>,
        successful_write_tool_ok_this_step: &mut bool,
    ) -> AllowToolCallDecision {
        let side_effects = tool_side_effects(&tc.name);
        if let Some(reason) = crate::agent_budget::check_and_consume_tool_budget(
            &self.tool_call_budget,
            tool_budget_usage,
            side_effects,
        ) {
            self.emit_event(
                &run_id,
                step,
                crate::events::EventKind::ToolDecision,
                serde_json::json!({
                    "tool_call_id": tc.id,
                    "name": tc.name,
                    "decision": "deny",
                    "reason": reason.clone(),
                    "source": "runtime_budget",
                    "side_effects": side_effects,
                    "budget": {
                        "max_total_tool_calls": self.tool_call_budget.max_total_tool_calls,
                        "max_mcp_calls": self.tool_call_budget.max_mcp_calls,
                        "max_filesystem_read_calls": self.tool_call_budget.max_filesystem_read_calls,
                        "max_filesystem_write_calls": self.tool_call_budget.max_filesystem_write_calls,
                        "max_shell_calls": self.tool_call_budget.max_shell_calls,
                        "max_network_calls": self.tool_call_budget.max_network_calls,
                        "max_browser_calls": self.tool_call_budget.max_browser_calls
                    }
                }),
            );
            return AllowToolCallDecision::Finalize(Box::new(
                self.finalize_runtime_budget_deny_with_end(
                    run_id,
                    step,
                    tc,
                    reason,
                    approval_mode_meta,
                    auto_scope_meta,
                    approval_key_version_meta,
                    tool_schema_hash_hex,
                    hooks_config_hash_hex,
                    planner_hash_hex,
                    decision_exec_target,
                    started_at,
                    messages.clone(),
                    observed_tool_calls,
                    observed_tool_decisions.clone(),
                    request_context_chars,
                    last_compaction_report,
                    hook_invocations.clone(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                ),
            ));
        }
        if let Some(reason) = crate::agent_budget::check_and_consume_mcp_budget(
            &self.tool_call_budget,
            tool_budget_usage,
            tc.name.starts_with("mcp."),
        ) {
            return AllowToolCallDecision::Finalize(Box::new(
                self.finalize_runtime_mcp_budget_exceeded_with_tool_decision(
                    run_id,
                    step,
                    tc,
                    reason,
                    side_effects,
                    started_at,
                    messages.clone(),
                    observed_tool_calls,
                    observed_tool_decisions.clone(),
                    request_context_chars,
                    last_compaction_report,
                    hook_invocations.clone(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                ),
            ));
        }
        self.emit_event(
            &run_id,
            step,
            crate::events::EventKind::ToolDecision,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "decision": "allow",
                "approval_id": approval_id.clone(),
                "approval_key": approval_key.clone(),
                "reason": reason.clone(),
                "source": source.clone(),
                "approval_key_version": approval_key_version_meta.clone(),
                "tool_schema_hash_hex": tool_schema_hash_hex.clone(),
                "hooks_config_hash_hex": hooks_config_hash_hex.clone(),
                "planner_hash_hex": planner_hash_hex.clone(),
                "exec_target": decision_exec_target.clone(),
                "taint_overall": taint_state.overall_str(),
                "taint_enforced": taint_enforced,
                "escalated": escalated,
                "escalation_reason": escalation_reason.clone(),
                "side_effects": tool_side_effects(&tc.name),
                "tool_args_strict": if self.tool_rt.tool_args_strict.is_enabled() { "on" } else { "off" }
            }),
        );
        self.emit_tool_exec_start_events(&run_id, step, tc);
        let mut tool_msg = if let Some(err) = &invalid_args_error {
            crate::agent_tool_exec::make_invalid_args_tool_message(
                tc,
                err,
                self.tool_rt.exec_target_kind,
            )
        } else {
            self.run_tool_with_timeout_and_emit_mcp_events(&run_id, step, tc, "await_result")
                .await
        };
        let mut tool_retry_count = 0u32;
        if invalid_args_error.is_none() {
            match self
                .handle_tool_retry_loop(
                    run_id.clone(),
                    step,
                    tc,
                    tool_msg,
                    side_effects,
                    plan_tool_allowed,
                    repeat_key,
                    tool_budget_usage,
                    invalid_patch_format_attempts,
                    failed_repeat_counts,
                    schema_repair_attempts,
                    messages,
                    approval_mode_meta.clone(),
                    auto_scope_meta.clone(),
                    approval_key_version_meta.clone(),
                    tool_schema_hash_hex.clone(),
                    hooks_config_hash_hex.clone(),
                    planner_hash_hex.clone(),
                    decision_exec_target.clone(),
                    started_at.clone(),
                    observed_tool_calls.clone(),
                    observed_tool_decisions.clone(),
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.clone(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                )
                .await
            {
                ToolRetryLoopOutcome::Completed {
                    tool_msg: next_msg,
                    tool_retry_count: next_count,
                } => {
                    tool_msg = next_msg;
                    tool_retry_count = next_count;
                }
                ToolRetryLoopOutcome::RestartAgentStep => {
                    return AllowToolCallDecision::RestartAgentStep;
                }
                ToolRetryLoopOutcome::Finalize(outcome) => {
                    return AllowToolCallDecision::Finalize(outcome);
                }
            }
        }
        match self
            .finalize_allowed_tool_result(
                run_id,
                step,
                tc,
                tool_msg,
                tool_retry_count,
                invalid_args_error.is_some(),
                approval_id,
                approval_key,
                reason,
                source,
                taint_enforced,
                escalated,
                escalation_reason,
                approval_mode_meta,
                auto_scope_meta,
                approval_key_version_meta,
                tool_schema_hash_hex,
                hooks_config_hash_hex,
                planner_hash_hex,
                decision_exec_target,
                repeat_key,
                started_at,
                request_context_chars,
                last_compaction_report,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                hook_invocations,
                taint_state,
                failed_repeat_counts,
                invalid_patch_format_attempts,
                successful_write_tool_ok_this_step,
                messages,
                observed_tool_decisions,
                observed_tool_executions,
                observed_tool_calls,
            )
            .await
        {
            AllowedToolResultDecision::Continue => AllowToolCallDecision::Continue,
            AllowedToolResultDecision::RestartAgentStep => AllowToolCallDecision::RestartAgentStep,
            AllowedToolResultDecision::Finalize(outcome) => {
                AllowToolCallDecision::Finalize(outcome)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_non_allow_gate_decision(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        gate_decision: crate::gate::GateDecision,
        invalid_args_error: Option<&String>,
        approval_mode_meta: Option<String>,
        auto_scope_meta: Option<String>,
        approval_key_version_meta: Option<String>,
        tool_schema_hash_hex: Option<String>,
        hooks_config_hash_hex: Option<String>,
        planner_hash_hex: Option<String>,
        decision_exec_target: Option<String>,
        started_at: String,
        messages: &mut Vec<Message>,
        observed_tool_calls: Vec<ToolCall>,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
        request_context_chars: usize,
        last_compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> GateNonAllowDecision {
        match gate_decision {
            crate::gate::GateDecision::Deny {
                reason,
                approval_key,
                source,
                taint_enforced,
                escalated,
                escalation_reason,
            } => GateNonAllowDecision::Finalize(Box::new(self.finalize_gate_deny_with_end(
                run_id,
                step,
                tc,
                reason,
                approval_key,
                source,
                taint_enforced,
                escalated,
                escalation_reason,
                approval_mode_meta,
                auto_scope_meta,
                approval_key_version_meta,
                tool_schema_hash_hex,
                hooks_config_hash_hex,
                planner_hash_hex,
                decision_exec_target,
                started_at,
                messages.clone(),
                observed_tool_calls,
                observed_tool_decisions.clone(),
                request_context_chars,
                last_compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
            ))),
            crate::gate::GateDecision::RequireApproval {
                reason,
                approval_id,
                approval_key,
                source,
                taint_enforced,
                escalated,
                escalation_reason,
            } => {
                if let Some(err) = invalid_args_error {
                    if self.handle_require_approval_invalid_args(
                        &run_id,
                        step,
                        tc,
                        err,
                        source.clone(),
                        taint_enforced,
                        escalated,
                        escalation_reason.clone(),
                        approval_mode_meta.clone(),
                        auto_scope_meta.clone(),
                        approval_key_version_meta.clone(),
                        tool_schema_hash_hex.clone(),
                        hooks_config_hash_hex.clone(),
                        planner_hash_hex.clone(),
                        decision_exec_target.clone(),
                        taint_state,
                        messages,
                        observed_tool_decisions,
                    ) {
                        return GateNonAllowDecision::RestartAgentStep;
                    }
                    return GateNonAllowDecision::ContinueToolLoop;
                }
                GateNonAllowDecision::Finalize(Box::new(
                    self.finalize_gate_require_approval_with_end(
                        run_id,
                        step,
                        tc,
                        reason,
                        approval_id,
                        approval_key,
                        source,
                        taint_enforced,
                        escalated,
                        escalation_reason,
                        approval_mode_meta,
                        auto_scope_meta,
                        approval_key_version_meta,
                        tool_schema_hash_hex,
                        hooks_config_hash_hex,
                        planner_hash_hex,
                        decision_exec_target,
                        started_at,
                        messages.clone(),
                        observed_tool_calls,
                        observed_tool_decisions.clone(),
                        request_context_chars,
                        last_compaction_report,
                        hook_invocations,
                        provider_retry_count,
                        provider_error_count,
                        saw_token_usage,
                        total_token_usage,
                        taint_state,
                    ),
                ))
            }
            crate::gate::GateDecision::Allow { .. } => GateNonAllowDecision::ContinueToolLoop,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_gate_require_approval_with_end(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        reason: String,
        approval_id: String,
        approval_key: Option<String>,
        source: Option<String>,
        taint_enforced: bool,
        escalated: bool,
        escalation_reason: Option<String>,
        approval_mode_meta: Option<String>,
        auto_scope_meta: Option<String>,
        approval_key_version_meta: Option<String>,
        tool_schema_hash_hex: Option<String>,
        hooks_config_hash_hex: Option<String>,
        planner_hash_hex: Option<String>,
        decision_exec_target: Option<String>,
        started_at: String,
        messages: Vec<Message>,
        observed_tool_calls: Vec<ToolCall>,
        mut observed_tool_decisions: Vec<ToolDecisionRecord>,
        request_context_chars: usize,
        last_compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> super::agent_types::AgentOutcome {
        self.emit_event(
            &run_id,
            step,
            crate::events::EventKind::ToolDecision,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "decision": "require_approval",
                "reason": reason.clone(),
                "approval_id": approval_id.clone(),
                "approval_key": approval_key.clone(),
                "source": source.clone(),
                "approval_key_version": approval_key_version_meta.clone(),
                "tool_schema_hash_hex": tool_schema_hash_hex.clone(),
                "hooks_config_hash_hex": hooks_config_hash_hex.clone(),
                "planner_hash_hex": planner_hash_hex.clone(),
                "exec_target": decision_exec_target.clone(),
                "taint_overall": taint_state.overall_str(),
                "taint_enforced": taint_enforced,
                "escalated": escalated,
                "escalation_reason": escalation_reason.clone(),
                "side_effects": tool_side_effects(&tc.name),
                "tool_args_strict": if self.tool_rt.tool_args_strict.is_enabled() { "on" } else { "off" }
            }),
        );
        self.gate.record(GateEvent {
            run_id: run_id.clone(),
            step,
            tool_call_id: tc.id.clone(),
            tool: tc.name.clone(),
            arguments: tc.arguments.clone(),
            decision: "require_approval".to_string(),
            decision_reason: Some(reason.clone()),
            decision_source: source.clone(),
            approval_id: Some(approval_id.clone()),
            approval_key,
            approval_mode: approval_mode_meta,
            auto_approve_scope: auto_scope_meta,
            approval_key_version: approval_key_version_meta,
            tool_schema_hash_hex,
            hooks_config_hash_hex,
            planner_hash_hex,
            exec_target: decision_exec_target,
            taint_overall: Some(taint_state.overall_str().to_string()),
            taint_enforced,
            escalated,
            escalation_reason: escalation_reason.clone(),
            result_ok: false,
            result_content: reason.clone(),
            result_input_digest: None,
            result_output_digest: None,
            result_input_len: None,
            result_output_len: None,
        });
        observed_tool_decisions.push(ToolDecisionRecord {
            step,
            tool_call_id: tc.id.clone(),
            tool: tc.name.clone(),
            decision: "require_approval".to_string(),
            reason: Some(reason.clone()),
            source: source.clone(),
            taint_overall: Some(taint_state.overall_str().to_string()),
            taint_enforced,
            escalated,
            escalation_reason,
        });
        self.finalize_approval_required_with_end(
            step,
            run_id,
            started_at,
            self.approval_required_output_message(
                &approval_id,
                &reason,
                source.as_deref(),
                escalated,
                taint_state,
            ),
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
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_require_approval_invalid_args(
        &mut self,
        run_id: &str,
        step: u32,
        tc: &ToolCall,
        err: &str,
        source: Option<String>,
        taint_enforced: bool,
        escalated: bool,
        escalation_reason: Option<String>,
        approval_mode_meta: Option<String>,
        auto_scope_meta: Option<String>,
        approval_key_version_meta: Option<String>,
        tool_schema_hash_hex: Option<String>,
        hooks_config_hash_hex: Option<String>,
        planner_hash_hex: Option<String>,
        decision_exec_target: Option<String>,
        taint_state: &TaintState,
        messages: &mut Vec<Message>,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
    ) -> bool {
        let invalid_bypass_reason = format!("invalid args bypassed approval: {err}");
        self.emit_event(
            run_id,
            step,
            crate::events::EventKind::ToolDecision,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "decision": "allow",
                "reason": invalid_bypass_reason,
                "approval_key_version": approval_key_version_meta.clone(),
                "tool_schema_hash_hex": tool_schema_hash_hex.clone(),
                "hooks_config_hash_hex": hooks_config_hash_hex.clone(),
                "planner_hash_hex": planner_hash_hex.clone(),
                "exec_target": decision_exec_target.clone(),
                "taint_overall": taint_state.overall_str(),
                "taint_enforced": taint_enforced,
                "escalated": escalated,
                "escalation_reason": escalation_reason.clone(),
                "side_effects": tool_side_effects(&tc.name),
                "tool_args_strict": if self.tool_rt.tool_args_strict.is_enabled() { "on" } else { "off" }
            }),
        );
        self.emit_tool_exec_start_events(run_id, step, tc);
        let tool_msg = crate::agent_tool_exec::make_invalid_args_tool_message(
            tc,
            err,
            self.tool_rt.exec_target_kind,
        );
        let content = tool_msg.content.clone().unwrap_or_default();
        self.gate.record(GateEvent {
            run_id: run_id.to_string(),
            step,
            tool_call_id: tc.id.clone(),
            tool: tc.name.clone(),
            arguments: tc.arguments.clone(),
            decision: "allow".to_string(),
            decision_reason: Some(format!("invalid args bypassed approval: {err}")),
            decision_source: source.clone(),
            approval_id: None,
            approval_key: None,
            approval_mode: approval_mode_meta,
            auto_approve_scope: auto_scope_meta,
            approval_key_version: approval_key_version_meta,
            tool_schema_hash_hex,
            hooks_config_hash_hex,
            planner_hash_hex,
            exec_target: decision_exec_target,
            taint_overall: Some(taint_state.overall_str().to_string()),
            taint_enforced,
            escalated,
            escalation_reason: escalation_reason.clone(),
            result_ok: false,
            result_content: content,
            result_input_digest: None,
            result_output_digest: None,
            result_input_len: None,
            result_output_len: None,
        });
        observed_tool_decisions.push(ToolDecisionRecord {
            step,
            tool_call_id: tc.id.clone(),
            tool: tc.name.clone(),
            decision: "allow".to_string(),
            reason: Some(format!("invalid args bypassed approval: {err}")),
            source,
            taint_overall: Some(taint_state.overall_str().to_string()),
            taint_enforced,
            escalated,
            escalation_reason,
        });
        self.emit_event(
            run_id,
            step,
            crate::events::EventKind::ToolExecEnd,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "ok": false,
                "truncated": false
            }),
        );
        messages.push(tool_msg);
        self.inject_post_tool_operator_messages(run_id, step, messages)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_gate_deny_with_end(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        reason: String,
        approval_key: Option<String>,
        source: Option<String>,
        taint_enforced: bool,
        escalated: bool,
        escalation_reason: Option<String>,
        approval_mode_meta: Option<String>,
        auto_scope_meta: Option<String>,
        approval_key_version_meta: Option<String>,
        tool_schema_hash_hex: Option<String>,
        hooks_config_hash_hex: Option<String>,
        planner_hash_hex: Option<String>,
        decision_exec_target: Option<String>,
        started_at: String,
        messages: Vec<Message>,
        observed_tool_calls: Vec<ToolCall>,
        mut observed_tool_decisions: Vec<ToolDecisionRecord>,
        request_context_chars: usize,
        last_compaction_report: Option<crate::compaction::CompactionReport>,
        hook_invocations: Vec<crate::hooks::protocol::HookInvocationReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> super::agent_types::AgentOutcome {
        self.emit_event(
            &run_id,
            step,
            crate::events::EventKind::ToolDecision,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "decision": "deny",
                "reason": reason.clone(),
                "approval_key": approval_key.clone(),
                "source": source.clone(),
                "approval_key_version": approval_key_version_meta.clone(),
                "tool_schema_hash_hex": tool_schema_hash_hex.clone(),
                "hooks_config_hash_hex": hooks_config_hash_hex.clone(),
                "planner_hash_hex": planner_hash_hex.clone(),
                "exec_target": decision_exec_target.clone(),
                "taint_overall": taint_state.overall_str(),
                "taint_enforced": taint_enforced,
                "escalated": escalated,
                "escalation_reason": escalation_reason.clone(),
                "side_effects": tool_side_effects(&tc.name),
                "tool_args_strict": if self.tool_rt.tool_args_strict.is_enabled() { "on" } else { "off" }
            }),
        );
        self.gate.record(GateEvent {
            run_id: run_id.clone(),
            step,
            tool_call_id: tc.id.clone(),
            tool: tc.name.clone(),
            arguments: tc.arguments.clone(),
            decision: "deny".to_string(),
            decision_reason: Some(reason.clone()),
            decision_source: source.clone(),
            approval_id: None,
            approval_key,
            approval_mode: approval_mode_meta,
            auto_approve_scope: auto_scope_meta,
            approval_key_version: approval_key_version_meta,
            tool_schema_hash_hex,
            hooks_config_hash_hex,
            planner_hash_hex,
            exec_target: decision_exec_target,
            taint_overall: Some(taint_state.overall_str().to_string()),
            taint_enforced,
            escalated,
            escalation_reason: escalation_reason.clone(),
            result_ok: false,
            result_content: reason.clone(),
            result_input_digest: None,
            result_output_digest: None,
            result_input_len: None,
            result_output_len: None,
        });
        observed_tool_decisions.push(ToolDecisionRecord {
            step,
            tool_call_id: tc.id.clone(),
            tool: tc.name.clone(),
            decision: "deny".to_string(),
            reason: Some(reason.clone()),
            source: source.clone(),
            taint_overall: Some(taint_state.overall_str().to_string()),
            taint_enforced,
            escalated,
            escalation_reason,
        });
        self.finalize_denied_with_end(
            step,
            run_id,
            started_at,
            format!(
                "Tool call '{}' denied: {}",
                tc.name,
                if let Some(src) = &source {
                    format!("{} (source: {})", reason, src)
                } else {
                    reason.clone()
                }
            ),
            None,
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
        )
    }
}
