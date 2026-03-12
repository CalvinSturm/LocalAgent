use std::collections::BTreeSet;

use crate::agent_worker_protocol::parse_worker_step_status;
use crate::events::EventKind;
use crate::providers::ModelProvider;
use crate::taint::{TaintState, TaintToggle};
use crate::types::{GenerateRequest, Message, Role, ToolCall, ToolDef};

use super::{Agent, PlanStepConstraint, PlanToolEnforcementMode, WorkerStepStatus};

pub(super) struct ToolCallPlanningContext {
    pub(super) plan_allowed_tools: Vec<String>,
    pub(super) plan_tool_allowed: bool,
    pub(super) plan_step_id: String,
    pub(super) repeat_key: String,
    pub(super) failed_repeat_count: u32,
    pub(super) failed_repeat_name_count: u32,
}

impl<P: ModelProvider> Agent<P> {
    #[allow(clippy::type_complexity)]
    pub(super) fn gate_decision_metadata_for_tool(
        &mut self,
        tc: &ToolCall,
        taint_state: &TaintState,
    ) -> (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) {
        let approval_mode_meta =
            if matches!(self.gate_ctx.approval_mode, crate::gate::ApprovalMode::Auto) {
                Some("auto".to_string())
            } else {
                None
            };
        let auto_scope_meta =
            if matches!(self.gate_ctx.approval_mode, crate::gate::ApprovalMode::Auto) {
                Some(
                    match self.gate_ctx.auto_approve_scope {
                        crate::gate::AutoApproveScope::Run => "run",
                        crate::gate::AutoApproveScope::Session => "session",
                    }
                    .to_string(),
                )
            } else {
                None
            };
        let approval_key_version_meta =
            Some(self.gate_ctx.approval_key_version.as_str().to_string());
        let tool_schema_hash_hex = self.gate_ctx.tool_schema_hashes.get(&tc.name).cloned();
        let hooks_config_hash_hex = self.gate_ctx.hooks_config_hash_hex.clone();
        let planner_hash_hex = self.gate_ctx.planner_hash_hex.clone();
        self.gate_ctx.taint_enabled = matches!(self.taint_toggle, TaintToggle::On);
        self.gate_ctx.taint_mode = self.taint_mode;
        self.gate_ctx.taint_overall = taint_state.overall;
        self.gate_ctx.taint_sources = taint_state.last_sources.clone();
        let decision_exec_target = Some(
            match self.gate_ctx.exec_target {
                crate::target::ExecTargetKind::Host => "host",
                crate::target::ExecTargetKind::Docker => "docker",
            }
            .to_string(),
        );
        (
            approval_mode_meta,
            auto_scope_meta,
            approval_key_version_meta,
            tool_schema_hash_hex,
            hooks_config_hash_hex,
            planner_hash_hex,
            decision_exec_target,
        )
    }

    pub(super) fn plan_enforcement_active(&self) -> bool {
        !matches!(self.plan_tool_enforcement, PlanToolEnforcementMode::Off)
            && !self.plan_step_constraints.is_empty()
    }

    pub(super) fn parse_worker_step_status_if_enforced(
        &self,
        assistant: &Message,
    ) -> Option<WorkerStepStatus> {
        if self.plan_enforcement_active() {
            parse_worker_step_status(
                assistant.content.as_deref().unwrap_or_default(),
                &self.plan_step_constraints,
            )
        } else {
            None
        }
    }

    pub(super) fn current_plan_constraint(
        &self,
        active_plan_step_idx: usize,
    ) -> Option<PlanStepConstraint> {
        self.plan_step_constraints
            .get(active_plan_step_idx)
            .cloned()
    }

    pub(super) fn current_plan_step_id_or_unknown(&self, active_plan_step_idx: usize) -> String {
        self.current_plan_constraint(active_plan_step_idx)
            .map(|c| c.step_id)
            .unwrap_or_else(|| "unknown".to_string())
    }

    pub(super) fn is_plan_tool_allowed(
        &self,
        active_plan_step_idx: usize,
        tool_name: &str,
    ) -> bool {
        if !self.plan_enforcement_active() {
            return true;
        }
        let Some(constraint) = self.current_plan_constraint(active_plan_step_idx) else {
            return true;
        };
        constraint.intended_tools.is_empty()
            || constraint.intended_tools.iter().any(|t| t == tool_name)
    }

    pub(super) fn plan_allowed_tools_and_decision(
        &self,
        active_plan_step_idx: usize,
        tool_name: &str,
    ) -> (Vec<String>, bool) {
        let plan_allowed_tools = self
            .current_plan_constraint(active_plan_step_idx)
            .map(|c| c.intended_tools)
            .unwrap_or_default();
        let allowed = self.is_plan_tool_allowed(active_plan_step_idx, tool_name);
        (plan_allowed_tools, allowed)
    }

    pub(super) fn build_tool_call_planning_context(
        &self,
        active_plan_step_idx: usize,
        tc: &ToolCall,
        failed_repeat_counts: &std::collections::BTreeMap<String, u32>,
    ) -> ToolCallPlanningContext {
        let (plan_allowed_tools, plan_tool_allowed) =
            self.plan_allowed_tools_and_decision(active_plan_step_idx, &tc.name);
        let plan_step_id = self.current_plan_step_id_or_unknown(active_plan_step_idx);
        let repeat_key = crate::agent::tool_helpers::failed_repeat_key(tc);
        let failed_repeat_count = failed_repeat_counts.get(&repeat_key).copied().unwrap_or(0);
        let name_repeat_key = format!("name::{}", tc.name);
        let failed_repeat_name_count = failed_repeat_counts
            .get(&name_repeat_key)
            .copied()
            .unwrap_or(0);
        ToolCallPlanningContext {
            plan_allowed_tools,
            plan_tool_allowed,
            plan_step_id,
            repeat_key,
            failed_repeat_count,
            failed_repeat_name_count,
        }
    }

    pub(super) fn pending_plan_step_text(&self, active_plan_step_idx: usize) -> Option<String> {
        let step_constraint = self.current_plan_constraint(active_plan_step_idx)?;
        Some(format!(
            "premature finalization blocked: plan step {} still pending (allowed tools: {})",
            step_constraint.step_id,
            if step_constraint.intended_tools.is_empty() {
                "none".to_string()
            } else {
                step_constraint.intended_tools.join(", ")
            }
        ))
    }

    pub(super) fn pending_plan_step_corrective_message(
        &self,
        active_plan_step_idx: usize,
    ) -> Option<String> {
        let step_constraint = self.current_plan_constraint(active_plan_step_idx)?;
        Some(format!(
            "Continue execution. Do not finalize yet. Complete pending step {} using only intended tools ({}), then return the next tool call.",
            step_constraint.step_id,
            if step_constraint.intended_tools.is_empty() {
                "none".to_string()
            } else {
                step_constraint.intended_tools.join(", ")
            }
        ))
    }

    pub(super) fn final_output_for_completion(
        &self,
        last_user_output: Option<&String>,
        assistant_content: Option<&str>,
    ) -> String {
        if self.plan_enforcement_active() {
            last_user_output.cloned().unwrap_or_default()
        } else {
            assistant_content.unwrap_or_default().to_string()
        }
    }

    pub(super) fn assistant_content_has_protocol_artifacts(
        &self,
        assistant_content: Option<&str>,
    ) -> bool {
        let text = assistant_content.unwrap_or_default();
        let upper = text.to_ascii_uppercase();
        crate::agent_tool_exec::contains_tool_wrapper_markers(text)
            || upper.contains("[TOOL_RESULT]")
            || upper.contains("[END_TOOL_RESULT]")
    }

    pub(super) fn control_envelope_reminder_message(&self) -> String {
        format!(
            "Return control JSON only using schema_version '{}'. Include step_id, status, and optional user_output for final response.",
            crate::planner::STEP_RESULT_SCHEMA_VERSION
        )
    }

    pub(super) fn tool_only_reminder_message(&self) -> String {
        "Tool-only phase active. Return exactly one valid tool call and no prose.".to_string()
    }

    pub(super) fn required_validation_phase_message(&self, user_prompt: &str) -> String {
        let required_command =
            crate::agent_impl_guard::prompt_required_validation_command(user_prompt)
                .unwrap_or("the required validation command");
        format!(
            "Required validation phase active. Return exactly one shell tool call and no prose. Run `{required_command}`. Example arguments: {{\"command\":\"{required_command}\"}}."
        )
    }

    pub(super) fn post_validation_final_answer_only_message(&self, user_prompt: &str) -> String {
        if crate::agent_impl_guard::prompt_required_exact_final_answer(user_prompt).is_some() {
            "Validation already succeeded. Do not call more tools. Reply with the required final answer only.".to_string()
        } else {
            "Validation already succeeded. Do not call more tools. Reply with the final answer only.".to_string()
        }
    }

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
Edit workflow (required):\n\
- Always use read_file on a path BEFORE editing it.\n\
- When creating a brand-new file with write_file, a prior read_file is not required if the path does not already exist.\n\
- If the path already exists, read_file it first and prefer edit for a single in-place replacement. Use apply_patch when edit would need multiple coordinated changes or larger context. Use overwrite_existing=true only for an explicit full rewrite.\n\
- Use workdir-relative paths like `src/main.rs`, not absolute paths.\n\
- After editing, use read_file again to verify your changes.\n\
- If the task requires a validation command (for example `node --test`), do not run it first. Read the target file, make the edit, read the file back to verify the change, and only then run the validation command.\n\
- Default edit path: read_file -> edit -> read_file -> shell -> final answer.\n\
- Prefer edit over str_replace for ordinary existing-file fixes. Use str_replace only when you have a trivial exact unique match and do not need broader context. Use apply_patch for larger multi-hunk changes or whenever exact-match repair is failing. Only use write_file for new files or explicit full rewrites.\n\
- If str_replace fails because old_string was not found or was not unique, immediately read_file again and switch to edit or apply_patch instead of repeating the same replacement.\n\
- If the prompt requires validation or an exact final answer, treat the final answer as reporting only after the edit is verified and the validation command succeeds.\n\
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

    pub(super) fn compute_run_preflight_caches(
        &self,
    ) -> (Option<String>, Option<String>, BTreeSet<String>) {
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
