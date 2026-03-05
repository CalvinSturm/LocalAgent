use std::collections::BTreeSet;

use crate::events::EventKind;
use crate::providers::ModelProvider;
use crate::types::{GenerateRequest, Message, Role, ToolCall, ToolDef};

use super::{Agent, PlanToolEnforcementMode};

impl<P: ModelProvider> Agent<P> {
    pub(super) fn emit_run_start_events(&mut self, run_id: &str) {
        self.emit_event(
            run_id,
            0,
            EventKind::RunStart,
            serde_json::json!({"model": self.model}),
        );
        if let Some(policy) = &self.policy_loaded {
            self.emit_event(
                run_id,
                0,
                EventKind::PolicyLoaded,
                serde_json::json!({
                    "version": policy.version,
                    "rules_count": policy.rules_count,
                    "includes_count": policy.includes_count,
                    "mcp_allowlist": policy.mcp_allowlist
                }),
            );
        }
    }

    pub(super) fn build_initial_messages(
        &self,
        user_prompt: &str,
        session_messages: Vec<Message>,
        injected_messages: Vec<Message>,
    ) -> Vec<Message> {
        let mut messages = vec![Message {
            role: Role::System,
            content: Some(
                "You are an agent that may call tools to gather information.\n\
\n\
TOOL_CONTRACT_VERSION: v1\n\
\n\
Tool use contract:\n\
- Use only tools explicitly provided in this run.\n\
- Emit at most one tool call per assistant step.\n\
- Tool arguments must be a valid JSON object matching the tool schema.\n\
- If a tool returns an error, read the tool error and retry with corrected arguments only when applicable.\n\
- If no tool is needed, return a direct final answer.\n\
\n\
Fallback when native tool calls are unavailable:\n\
- Emit exactly one wrapper block:\n\
  [TOOL_CALL]\n\
  {\"name\":\"<tool>\",\"arguments\":{...}}\n\
  [END_TOOL_CALL]\n\
- Emit no extra prose inside the wrapper."
                    .to_string(),
            ),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        }];
        messages.extend(session_messages);
        for msg in injected_messages {
            messages.push(msg);
        }
        messages.push(Message {
            role: Role::User,
            content: Some(user_prompt.to_string()),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        });
        messages
    }

    pub(super) fn build_generate_request(
        &self,
        messages: &[Message],
        tools_sorted: Vec<ToolDef>,
    ) -> GenerateRequest {
        GenerateRequest {
            model: self.model.clone(),
            messages: messages.to_vec(),
            tools: if self.omit_tools_field_when_empty && tools_sorted.is_empty() {
                None
            } else {
                Some(tools_sorted)
            },
            temperature: self.temperature,
            top_p: self.top_p,
            max_tokens: self.max_tokens,
            seed: self.seed,
        }
    }

    pub(super) fn compute_run_preflight_caches(&self) -> (Option<String>, Option<String>, BTreeSet<String>) {
        let expected_mcp_catalog_hash_hex = self
            .mcp_registry
            .as_ref()
            .and_then(|m| m.configured_tool_catalog_hash_hex().ok());
        let expected_mcp_docs_hash_hex = self
            .mcp_registry
            .as_ref()
            .and_then(|m| m.configured_tool_docs_hash_hex().ok());
        let allowed_tool_names: BTreeSet<String> =
            self.tools.iter().map(|t| t.name.clone()).collect();
        (
            expected_mcp_catalog_hash_hex,
            expected_mcp_docs_hash_hex,
            allowed_tool_names,
        )
    }

    pub(super) fn emit_plan_step_started_if_needed(
        &mut self,
        run_id: &str,
        step: u32,
        active_plan_step_idx: usize,
        announced_plan_step_id: &mut Option<String>,
    ) {
        if matches!(self.plan_tool_enforcement, PlanToolEnforcementMode::Off)
            || self.plan_step_constraints.is_empty()
            || active_plan_step_idx >= self.plan_step_constraints.len()
        {
            return;
        }
        let step_constraint = self.plan_step_constraints[active_plan_step_idx].clone();
        if announced_plan_step_id.as_deref() == Some(step_constraint.step_id.as_str()) {
            return;
        }
        self.emit_event(
            run_id,
            step,
            EventKind::StepStarted,
            serde_json::json!({
                "step_id": step_constraint.step_id,
                "step_index": active_plan_step_idx,
                "allowed_tools": step_constraint.intended_tools,
                "enforcement_mode": format!("{:?}", self.plan_tool_enforcement).to_lowercase()
            }),
        );
        *announced_plan_step_id = Some(step_constraint.step_id.clone());
    }

    pub(super) fn record_detected_tool_call(
        &mut self,
        run_id: &str,
        step: u32,
        tc: &ToolCall,
        observed_tool_calls: &mut Vec<ToolCall>,
    ) {
        observed_tool_calls.push(tc.clone());
        self.emit_event(
            run_id,
            step,
            EventKind::ToolCallDetected,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "arguments": tc.arguments,
                "side_effects": crate::tools::tool_side_effects(&tc.name),
                "tool_args_strict": if self.tool_rt.tool_args_strict.is_enabled() { "on" } else { "off" }
            }),
        );
    }
}
