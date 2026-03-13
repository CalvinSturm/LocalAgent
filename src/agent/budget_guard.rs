use crate::gate::GateEvent;
use crate::providers::ModelProvider;
use crate::taint::TaintState;
use crate::types::{Message, TokenUsage, ToolCall};

use super::agent_types::ToolDecisionRecord;
use super::Agent;

impl<P: ModelProvider> Agent<P> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_runtime_mcp_budget_exceeded_with_tool_decision(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        reason: String,
        side_effects: crate::types::SideEffects,
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
                    "max_mcp_calls": self.tool_call_budget.max_mcp_calls
                }
            }),
        );
        self.finalize_budget_exceeded_with_end(
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
    pub(super) fn finalize_runtime_mcp_budget_exceeded_with_error(
        &mut self,
        run_id: String,
        step: u32,
        reason: String,
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
        self.emit_event(
            &run_id,
            step,
            crate::events::EventKind::Error,
            serde_json::json!({
                "error": reason.clone(),
                "source": "runtime_budget",
                "budget": {
                    "max_mcp_calls": self.tool_call_budget.max_mcp_calls
                }
            }),
        );
        self.finalize_budget_exceeded_with_end(
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
    pub(super) fn finalize_runtime_budget_deny_with_end(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        reason: String,
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
        self.gate.record(GateEvent {
            run_id: run_id.clone(),
            step,
            tool_call_id: tc.id.clone(),
            tool: tc.name.clone(),
            arguments: tc.arguments.clone(),
            decision: "deny".to_string(),
            decision_reason: Some(reason.clone()),
            decision_source: Some("runtime_budget".to_string()),
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
            source: Some("runtime_budget".to_string()),
            approval_id: None,
            taint_overall: Some(taint_state.overall_str().to_string()),
            taint_enforced: false,
            escalated: false,
            escalation_reason: None,
        });
        self.emit_event(
            &run_id,
            step,
            crate::events::EventKind::Error,
            serde_json::json!({"error": reason.clone()}),
        );
        self.finalize_budget_exceeded_with_end(
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
}
