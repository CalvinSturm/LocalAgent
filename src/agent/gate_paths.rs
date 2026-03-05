use crate::gate::GateEvent;
use crate::providers::ModelProvider;
use crate::taint::TaintState;
use crate::tools::tool_side_effects;
use crate::types::{Message, ToolCall, TokenUsage};

use super::agent_types::ToolDecisionRecord;
use super::Agent;

impl<P: ModelProvider> Agent<P> {
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
