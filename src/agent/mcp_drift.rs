use crate::events::EventKind;
use crate::providers::ModelProvider;
use crate::taint::TaintState;
use crate::tools::tool_side_effects;
use crate::types::{Message, TokenUsage, ToolCall};

use super::agent_types::ToolDecisionRecord;
use super::Agent;

pub(super) enum McpDriftDecision {
    Continue,
    Finalize(Box<super::agent_types::AgentOutcome>),
}

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
            approval_id: None,
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
            approval_id: None,
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

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn check_mcp_drift_for_tool_call(
        &mut self,
        run_id: String,
        step: u32,
        tc: &ToolCall,
        expected_mcp_catalog_hash_hex: Option<&String>,
        expected_mcp_docs_hash_hex: Option<&String>,
        started_at: String,
        messages: Vec<Message>,
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
    ) -> McpDriftDecision {
        if !tc.name.starts_with("mcp.")
            || matches!(self.mcp_pin_enforcement, super::McpPinEnforcementMode::Off)
        {
            return McpDriftDecision::Continue;
        }
        let (Some(registry), Some(expected_hash)) =
            (self.mcp_registry.as_ref(), expected_mcp_catalog_hash_hex)
        else {
            return McpDriftDecision::Continue;
        };

        let live_catalog = registry.live_tool_catalog_hash_hex().await;
        let live_docs = if expected_mcp_docs_hash_hex.is_some() {
            Some(registry.live_tool_docs_hash_hex().await)
        } else {
            None
        };
        match (live_catalog, live_docs) {
            (Ok(actual_hash), Some(Ok(actual_docs_hash))) => {
                let expected_docs_hash = expected_mcp_docs_hash_hex
                    .map(|h| h.as_str())
                    .unwrap_or_default();
                let catalog_drift = actual_hash != *expected_hash;
                let docs_drift = actual_docs_hash != expected_docs_hash;
                if !catalog_drift && !docs_drift {
                    return McpDriftDecision::Continue;
                }
                let mut codes = Vec::new();
                if catalog_drift {
                    codes.push("MCP_CATALOG_DRIFT");
                }
                if docs_drift {
                    codes.push("MCP_DOCS_DRIFT");
                }
                let primary_code = codes.first().copied().unwrap_or("MCP_CATALOG_DRIFT");
                let reason = if catalog_drift && docs_drift {
                    format!(
                        "MCP drift detected: catalog hash changed (expected {}, got {}) and docs hash changed (expected {}, got {})",
                        expected_hash, actual_hash, expected_docs_hash, actual_docs_hash
                    )
                } else if catalog_drift {
                    format!(
                        "MCP_CATALOG_DRIFT detected: tool catalog hash changed during run (expected {}, got {})",
                        expected_hash, actual_hash
                    )
                } else {
                    format!(
                        "MCP_DOCS_DRIFT detected: tool docs hash changed during run (expected {}, got {})",
                        expected_docs_hash, actual_docs_hash
                    )
                };
                self.emit_event(
                    &run_id,
                    step,
                    EventKind::McpDrift,
                    serde_json::json!({
                        "tool_call_id": tc.id,
                        "name": tc.name,
                        "expected_hash_hex": expected_hash,
                        "actual_hash_hex": actual_hash,
                        "catalog_hash_expected": expected_hash,
                        "catalog_hash_live": actual_hash,
                        "catalog_drift": catalog_drift,
                        "docs_hash_expected": expected_docs_hash,
                        "docs_hash_live": actual_docs_hash,
                        "docs_drift": docs_drift,
                        "enforcement": format!("{:?}", self.mcp_pin_enforcement).to_lowercase(),
                        "codes": codes,
                        "primary_code": primary_code
                    }),
                );
                if matches!(self.mcp_pin_enforcement, super::McpPinEnforcementMode::Hard) {
                    return McpDriftDecision::Finalize(Box::new(
                        self.finalize_mcp_drift_hard_deny_with_end(
                            run_id,
                            step,
                            tc,
                            reason,
                            "mcp_drift",
                            started_at,
                            messages,
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
                    ));
                }
                self.record_mcp_drift_warn_decision(
                    &run_id,
                    step,
                    tc,
                    reason,
                    taint_state,
                    observed_tool_decisions,
                );
            }
            (Ok(actual_hash), Some(Err(e))) => {
                let reason =
                    format!("MCP_DRIFT verification failed: unable to probe live docs hash ({e})");
                self.emit_event(
                    &run_id,
                    step,
                    EventKind::McpDrift,
                    serde_json::json!({
                        "tool_call_id": tc.id,
                        "name": tc.name,
                        "expected_hash_hex": expected_hash,
                        "actual_hash_hex": actual_hash,
                        "catalog_hash_expected": expected_hash,
                        "catalog_hash_live": actual_hash,
                        "catalog_drift": false,
                        "docs_hash_expected": expected_mcp_docs_hash_hex,
                        "docs_probe_error": e.to_string(),
                        "docs_drift": false,
                        "enforcement": format!("{:?}", self.mcp_pin_enforcement).to_lowercase(),
                        "codes": ["MCP_DOCS_DRIFT_PROBE_FAILED"],
                        "primary_code": "MCP_DOCS_DRIFT_PROBE_FAILED"
                    }),
                );
                if matches!(self.mcp_pin_enforcement, super::McpPinEnforcementMode::Hard) {
                    return McpDriftDecision::Finalize(Box::new(
                        self.finalize_mcp_drift_hard_deny_with_end(
                            run_id,
                            step,
                            tc,
                            reason,
                            "mcp_drift",
                            started_at,
                            messages,
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
                    ));
                }
                self.record_mcp_drift_warn_decision(
                    &run_id,
                    step,
                    tc,
                    reason,
                    taint_state,
                    observed_tool_decisions,
                );
            }
            (Ok(actual_hash), None) => {
                if actual_hash != *expected_hash {
                    let reason = format!(
                        "MCP_CATALOG_DRIFT detected: tool catalog hash changed during run (expected {}, got {})",
                        expected_hash, actual_hash
                    );
                    self.emit_event(
                        &run_id,
                        step,
                        EventKind::McpDrift,
                        serde_json::json!({
                            "tool_call_id": tc.id,
                            "name": tc.name,
                            "expected_hash_hex": expected_hash,
                            "actual_hash_hex": actual_hash,
                            "catalog_hash_expected": expected_hash,
                            "catalog_hash_live": actual_hash,
                            "catalog_drift": true,
                            "docs_drift": false,
                            "enforcement": format!("{:?}", self.mcp_pin_enforcement).to_lowercase(),
                            "codes": ["MCP_CATALOG_DRIFT"],
                            "primary_code": "MCP_CATALOG_DRIFT"
                        }),
                    );
                    if matches!(self.mcp_pin_enforcement, super::McpPinEnforcementMode::Hard) {
                        return McpDriftDecision::Finalize(Box::new(
                            self.finalize_mcp_drift_hard_deny_with_end(
                                run_id,
                                step,
                                tc,
                                reason,
                                "mcp_drift",
                                started_at,
                                messages,
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
                        ));
                    }
                    self.record_mcp_drift_warn_decision(
                        &run_id,
                        step,
                        tc,
                        reason,
                        taint_state,
                        observed_tool_decisions,
                    );
                }
            }
            (Err(e), _) => {
                let reason = format!(
                    "MCP_DRIFT verification failed: unable to probe live tool catalog ({e})"
                );
                self.emit_event(
                    &run_id,
                    step,
                    EventKind::McpDrift,
                    serde_json::json!({
                        "tool_call_id": tc.id,
                        "name": tc.name,
                        "expected_hash_hex": expected_hash,
                        "catalog_hash_expected": expected_hash,
                        "catalog_probe_error": e.to_string(),
                        "enforcement": format!("{:?}", self.mcp_pin_enforcement).to_lowercase(),
                        "codes": ["MCP_CATALOG_DRIFT_PROBE_FAILED"],
                        "primary_code": "MCP_CATALOG_DRIFT_PROBE_FAILED",
                        "error": e.to_string()
                    }),
                );
                if matches!(self.mcp_pin_enforcement, super::McpPinEnforcementMode::Hard) {
                    return McpDriftDecision::Finalize(Box::new(
                        self.finalize_mcp_drift_hard_deny_with_end(
                            run_id,
                            step,
                            tc,
                            reason,
                            "mcp_drift_probe_failed",
                            started_at,
                            messages,
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
                    ));
                }
                self.record_mcp_drift_warn_decision(
                    &run_id,
                    step,
                    tc,
                    reason,
                    taint_state,
                    observed_tool_decisions,
                );
            }
        }
        McpDriftDecision::Continue
    }
}
