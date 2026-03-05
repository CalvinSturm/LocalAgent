use crate::events::EventKind;
use crate::providers::ModelProvider;
use crate::taint::TaintState;
use crate::tools::tool_side_effects;
use crate::types::{Message, ToolCall, TokenUsage};

use super::agent_types::ToolDecisionRecord;
use super::Agent;

impl<P: ModelProvider> Agent<P> {
    pub(super) fn record_mcp_drift_warn_decision(
        &mut self,
        run_id: &str,
        step: u32,
        tc: &ToolCall,
        reason: String,
        taint_state: &TaintState,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
    ) {
        observed_tool_decisions.push(ToolDecisionRecord {
            step,
            tool_call_id: tc.id.clone(),
            tool: tc.name.clone(),
            decision: "allow".to_string(),
            reason: Some(reason.clone()),
            source: Some("mcp_drift_warn".to_string()),
            taint_overall: Some(taint_state.overall_str().to_string()),
            taint_enforced: false,
            escalated: false,
            escalation_reason: None,
        });
        self.emit_event(
            run_id,
            step,
            EventKind::ToolDecision,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "decision": "allow",
                "reason": reason,
                "source": "mcp_drift_warn",
                "side_effects": tool_side_effects(&tc.name)
            }),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn finalize_mcp_drift_hard_deny_with_end(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        reason: String,
        step_block_reason: &str,
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
            EventKind::StepBlocked,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "reason": step_block_reason
            }),
        );
        observed_tool_decisions.push(ToolDecisionRecord {
            step,
            tool_call_id: tc.id.clone(),
            tool: tc.name.clone(),
            decision: "deny".to_string(),
            reason: Some(reason.clone()),
            source: Some("mcp_drift".to_string()),
            taint_overall: Some(taint_state.overall_str().to_string()),
            taint_enforced: false,
            escalated: false,
            escalation_reason: None,
        });
        self.emit_event(
            &run_id,
            step,
            EventKind::ToolDecision,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "decision": "deny",
                "reason": reason,
                "source": "mcp_drift",
                "side_effects": tool_side_effects(&tc.name)
            }),
        );
        self.finalize_denied_with_end(
            step,
            run_id,
            started_at,
            reason.clone(),
            Some(reason),
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
