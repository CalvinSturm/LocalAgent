use crate::agent_budget::ToolCallBudgetUsage;
use uuid::Uuid;

use crate::agent_impl_guard::{
    prompt_requires_tool_only, ToolExecutionRecord,
};
use crate::agent_output_sanitize::sanitize_user_visible_output as sanitize_user_visible_output_impl;
#[cfg(test)]
use crate::agent_tool_exec::{classify_tool_failure, tool_result_has_error};
use crate::agent_utils::provider_name;
use crate::compaction::{
    context_size_chars, maybe_compact, CompactionReport, CompactionSettings,
};
use crate::events::{EventKind, EventSink};
use crate::gate::{GateContext, GateDecision, GateEvent, ToolGate};
use crate::hooks::protocol::{HookInvocationReport, PreModelCompactionPayload, PreModelPayload};
use crate::hooks::runner::{make_pre_model_input, HookManager};
use crate::mcp::registry::McpRegistry;
use crate::operator_queue::{
    PendingMessageQueue, QueueLimits, QueueSubmitRequest,
};
use crate::providers::ModelProvider;
use crate::taint::{TaintMode, TaintState, TaintToggle};
use crate::tools::{
    envelope_to_message, to_tool_result_envelope, tool_side_effects, ToolResultMeta, ToolRuntime,
};
use crate::trust::policy::Policy;
use crate::types::{Message, Role, TokenUsage, ToolDef};

mod agent_types;
mod budget_guard;
mod gate_paths;
mod mcp_drift;
mod model_io;
mod operator_queue;
mod response_normalization;
mod runtime_completion;
mod run_control;
mod run_events;
mod run_finalize;
mod run_setup;
mod timeouts;
mod tool_helpers;

pub use agent_types::{
    AgentExitReason, AgentOutcome, AgentTaintRecord, McpPinEnforcementMode,
    McpRuntimeTraceEntry, PlanStepConstraint, PlanToolEnforcementMode, PolicyLoadedInfo,
    ToolCallBudget, ToolDecisionRecord,
};

pub(crate) use agent_types::WorkerStepStatus;
use runtime_completion::{
    runtime_completion_decision, RuntimeCompletionAction, RuntimeCompletionInputs,
};
#[cfg(test)]
pub(crate) use runtime_completion::RuntimeCompletionDecision;
use run_events::apply_usage_totals;
use response_normalization::{normalize_tool_calls_from_assistant, ToolWrapperParseState};
use gate_paths::{AllowToolCallDecision, GateNonAllowDecision};
use mcp_drift::McpDriftDecision;
use tool_helpers::{failed_repeat_key, injected_messages_enforce_implementation_integrity_guard};
use tool_helpers::MalformedToolCallDecision;

pub fn sanitize_user_visible_output(raw: &str) -> String {
    sanitize_user_visible_output_impl(raw)
}

#[cfg(test)]
fn contains_tool_wrapper_markers(text: &str) -> bool {
    crate::agent_tool_exec::contains_tool_wrapper_markers(text)
}

const MAX_SCHEMA_REPAIR_ATTEMPTS: u32 = 2;
const MAX_FAILED_REPEAT_PER_KEY: u32 = 3;
const DEFAULT_POST_WRITE_VERIFY_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_TOOL_EXEC_TIMEOUT_MS: u64 = 30_000;
pub(crate) const INTERNAL_ENFORCE_IMPLEMENTATION_GUARD_FLAG: &str =
    "INTERNAL_FLAG:enforce_implementation_integrity_guard";

pub struct Agent<P: ModelProvider> {
    pub provider: P,
    pub model: String,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: Option<u32>,
    pub seed: Option<u64>,
    pub tools: Vec<ToolDef>,
    pub max_steps: usize,
    pub tool_rt: ToolRuntime,
    pub gate: Box<dyn ToolGate>,
    pub gate_ctx: GateContext,
    pub mcp_registry: Option<std::sync::Arc<McpRegistry>>,
    pub stream: bool,
    pub event_sink: Option<Box<dyn EventSink>>,
    pub compaction_settings: CompactionSettings,
    pub hooks: HookManager,
    pub policy_loaded: Option<PolicyLoadedInfo>,
    pub policy_for_taint: Option<Policy>,
    pub taint_toggle: TaintToggle,
    pub taint_mode: TaintMode,
    pub taint_digest_bytes: usize,
    pub run_id_override: Option<String>,
    pub omit_tools_field_when_empty: bool,
    pub plan_tool_enforcement: PlanToolEnforcementMode,
    pub mcp_pin_enforcement: McpPinEnforcementMode,
    pub plan_step_constraints: Vec<PlanStepConstraint>,
    pub tool_call_budget: ToolCallBudget,
    pub mcp_runtime_trace: Vec<McpRuntimeTraceEntry>,
    pub operator_queue: PendingMessageQueue,
    #[allow(dead_code)]
    pub operator_queue_limits: QueueLimits,
    pub operator_queue_rx: Option<std::sync::mpsc::Receiver<QueueSubmitRequest>>,
}

impl<P: ModelProvider> Agent<P> {
    pub async fn run(
        &mut self,
        user_prompt: &str,
        session_messages: Vec<Message>,
        injected_messages: Vec<Message>,
    ) -> AgentOutcome {
        let enforce_implementation_integrity_guard =
            injected_messages_enforce_implementation_integrity_guard(&injected_messages);
        let run_id = self
            .run_id_override
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        self.gate_ctx.run_id = Some(run_id.clone());
        let started_at = crate::trust::now_rfc3339();
        self.emit_run_start_events(&run_id);
        let mut messages =
            self.build_initial_messages(user_prompt, session_messages, injected_messages);

        let mut observed_tool_calls = Vec::new();
        let mut observed_tool_executions: Vec<ToolExecutionRecord> = Vec::new();
        let mut observed_tool_decisions: Vec<ToolDecisionRecord> = Vec::new();
        let mut last_compaction_report: Option<CompactionReport> = None;
        let mut hook_invocations: Vec<HookInvocationReport> = Vec::new();
        let mut provider_retry_count: u32 = 0;
        let mut provider_error_count: u32 = 0;
        let mut total_token_usage = TokenUsage::default();
        let mut saw_token_usage = false;
        let mut taint_state = TaintState::new();
        let mut active_plan_step_idx: usize = 0;
        let mut blocked_runtime_completion_count: u32 = 0;
        let mut blocked_control_envelope_count: u32 = 0;
        let mut blocked_tool_only_count: u32 = 0;
        let mut tool_only_phase_active = prompt_requires_tool_only(user_prompt);
        let mut last_user_output: Option<String> = None;
        let mut step_retry_counts: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        let mut schema_repair_attempts: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        let mut failed_repeat_counts: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        let mut malformed_tool_call_attempts: u32 = 0;
        let mut invalid_patch_format_attempts: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        let mut tool_budget_usage = ToolCallBudgetUsage::default();
        let run_started = std::time::Instant::now();
        let mut announced_plan_step_id: Option<String> = None;
        let (expected_mcp_catalog_hash_hex, expected_mcp_docs_hash_hex, allowed_tool_names) =
            self.compute_run_preflight_caches();
        'agent_steps: for step in 0..self.max_steps {
            self.drain_external_operator_queue(&run_id, step as u32);
            if let Some(reason) =
                self.check_wall_time_budget_exceeded(&run_id, step as u32, &run_started)
            {
                let final_prompt_size_chars = context_size_chars(&messages);
                return self.finalize_budget_exceeded(
                    run_id,
                    started_at,
                    reason,
                    messages,
                    observed_tool_calls,
                    observed_tool_decisions,
                    final_prompt_size_chars,
                    last_compaction_report,
                    hook_invocations,
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    &total_token_usage,
                    &taint_state,
                );
            }
            self.emit_plan_step_started_if_needed(
                &run_id,
                step as u32,
                active_plan_step_idx,
                &mut announced_plan_step_id,
            );
            let compacted = match self.compact_messages_for_step(
                &run_id,
                step as u32,
                &messages,
                &mut provider_retry_count,
                &mut provider_error_count,
            ) {
                Ok(c) => c,
                Err(err_text) => {
                    return self.finalize_provider_error_with_end(
                        step as u32,
                        run_id,
                        started_at,
                        err_text,
                        messages,
                        observed_tool_calls,
                        observed_tool_decisions,
                        0,
                        last_compaction_report,
                        hook_invocations,
                        provider_retry_count,
                        provider_error_count,
                        saw_token_usage,
                        &total_token_usage,
                        &taint_state,
                    );
                }
            };
            if let Some(report) = compacted.report.clone() {
                self.emit_event(
                    &run_id,
                    step as u32,
                    EventKind::CompactionPerformed,
                    serde_json::json!({
                        "before_chars": report.before_chars,
                        "after_chars": report.after_chars,
                        "before_messages": report.before_messages,
                        "after_messages": report.after_messages,
                        "compacted_messages": report.compacted_messages,
                        "summary_digest_sha256": report.summary_digest_sha256
                    }),
                );
                last_compaction_report = Some(report);
            }
            messages = compacted.messages;

            let mut tools_sorted = self.tools.clone();
            tools_sorted.sort_by(|a, b| a.name.cmp(&b.name));

            if self.hooks.enabled() {
                let pre_payload = PreModelPayload {
                    messages: messages.clone(),
                    tools: tools_sorted.clone(),
                    stream: self.stream,
                    compaction: PreModelCompactionPayload::from(&self.compaction_settings),
                };
                let hook_input = make_pre_model_input(
                    &run_id,
                    step as u32,
                    provider_name(self.gate_ctx.provider),
                    &self.model,
                    &self.gate_ctx.workdir,
                    match serde_json::to_value(pre_payload) {
                        Ok(v) => v,
                        Err(e) => {
                            return self.finalize_provider_error_with_end(
                                step as u32,
                                run_id,
                                started_at,
                                format!("failed to encode pre_model hook payload: {e}"),
                                messages,
                                observed_tool_calls,
                                observed_tool_decisions,
                                0,
                                last_compaction_report,
                                hook_invocations,
                                provider_retry_count,
                                provider_error_count,
                                saw_token_usage,
                                &total_token_usage,
                                &taint_state,
                            );
                        }
                    },
                );
                match self.hooks.run_pre_model_hooks(hook_input).await {
                    Ok(result) => {
                        for inv in &result.invocations {
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::HookStart,
                                serde_json::json!({
                                    "hook_name": inv.hook_name,
                                    "stage": inv.stage
                                }),
                            );
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::HookEnd,
                                serde_json::json!({
                                    "hook_name": inv.hook_name,
                                    "stage": inv.stage,
                                    "action": inv.action,
                                    "modified": inv.modified,
                                    "duration_ms": inv.duration_ms
                                }),
                            );
                        }
                        hook_invocations.extend(result.invocations);
                        if let Some(reason) = result.abort_reason {
                            let prompt_chars = context_size_chars(&messages);
                            return self.finalize_hook_aborted_with_end(
                                step as u32,
                                run_id,
                                started_at,
                                reason.clone(),
                                reason,
                                messages,
                                observed_tool_calls,
                                observed_tool_decisions,
                                prompt_chars,
                                last_compaction_report,
                                hook_invocations,
                                provider_retry_count,
                                provider_error_count,
                                saw_token_usage,
                                &total_token_usage,
                                &taint_state,
                            );
                        }
                        if !result.append_messages.is_empty() {
                            messages.extend(result.append_messages);
                            if self.compaction_settings.max_context_chars > 0 {
                                let compacted_again =
                                    maybe_compact(&messages, &self.compaction_settings)
                                        .map_err(|e| format!("compaction failed after hooks: {e}"));
                                match compacted_again {
                                    Ok(out) => {
                                        if let Some(report) = out.report.clone() {
                                            self.emit_event(
                                                &run_id,
                                                step as u32,
                                                EventKind::CompactionPerformed,
                                                serde_json::json!({
                                                    "before_chars": report.before_chars,
                                                    "after_chars": report.after_chars,
                                                    "before_messages": report.before_messages,
                                                    "after_messages": report.after_messages,
                                                    "compacted_messages": report.compacted_messages,
                                                    "summary_digest_sha256": report.summary_digest_sha256,
                                                    "phase": "post_pre_model_hooks"
                                                }),
                                            );
                                            last_compaction_report = Some(report);
                                        }
                                        messages = out.messages;
                                        if self.compaction_settings.max_context_chars > 0
                                            && context_size_chars(&messages)
                                                > self.compaction_settings.max_context_chars
                                        {
                                            let prompt_chars = context_size_chars(&messages);
                                            return self.finalize_provider_error_with_end(
                                                step as u32,
                                                run_id,
                                                started_at,
                                                "hooks caused prompt to exceed budget".to_string(),
                                                messages,
                                                observed_tool_calls,
                                                observed_tool_decisions,
                                                prompt_chars,
                                                last_compaction_report,
                                                hook_invocations,
                                                provider_retry_count,
                                                provider_error_count,
                                                saw_token_usage,
                                                &total_token_usage,
                                                &taint_state,
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        let prompt_chars = context_size_chars(&messages);
                                        return self.finalize_provider_error_with_end(
                                            step as u32,
                                            run_id,
                                            started_at,
                                            e,
                                            messages,
                                            observed_tool_calls,
                                            observed_tool_decisions,
                                            prompt_chars,
                                            last_compaction_report,
                                            hook_invocations,
                                            provider_retry_count,
                                            provider_error_count,
                                            saw_token_usage,
                                            &total_token_usage,
                                            &taint_state,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::HookError,
                            serde_json::json!({"stage":"pre_model","error": e.message}),
                        );
                        let prompt_chars = context_size_chars(&messages);
                        return self.finalize_hook_aborted_with_end(
                            step as u32,
                            run_id,
                            started_at,
                            String::new(),
                            e.message,
                            messages,
                            observed_tool_calls,
                            observed_tool_decisions,
                            prompt_chars,
                            last_compaction_report,
                            hook_invocations,
                            provider_retry_count,
                            provider_error_count,
                            saw_token_usage,
                            &total_token_usage,
                            &taint_state,
                        );
                    }
                }
            }

            let req = self.build_generate_request(&messages, tools_sorted);
            let request_context_chars = context_size_chars(&req.messages);
            let resp_result = self.execute_model_request(&run_id, step as u32, req).await;

            let mut resp = match resp_result {
                Ok(r) => r,
                Err(e) => {
                    self.record_provider_error_events(
                        &run_id,
                        step as u32,
                        &e,
                        &mut provider_retry_count,
                        &mut provider_error_count,
                    );
                    self.emit_event(
                        &run_id,
                        step as u32,
                        EventKind::Error,
                        serde_json::json!({"error": e.to_string()}),
                    );
                    return self.finalize_provider_error_with_end(
                        step as u32,
                        run_id,
                        started_at,
                        e.to_string(),
                        messages,
                        observed_tool_calls,
                        observed_tool_decisions,
                        request_context_chars,
                        last_compaction_report,
                        hook_invocations,
                        provider_retry_count,
                        provider_error_count,
                        saw_token_usage,
                        &total_token_usage,
                        &taint_state,
                    );
                }
            };
            match normalize_tool_calls_from_assistant(&resp, step as u32, &allowed_tool_names) {
                ToolWrapperParseState::Normalized(normalized_calls) => {
                    resp.tool_calls = normalized_calls;
                    resp.assistant.content = None;
                }
                ToolWrapperParseState::MalformedWrapper => {
                    malformed_tool_call_attempts = malformed_tool_call_attempts.saturating_add(1);
                    if malformed_tool_call_attempts >= 2 {
                        let reason =
                            "MODEL_TOOL_PROTOCOL_VIOLATION: empty or malformed [TOOL_CALL] envelope"
                                .to_string();
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::Error,
                            serde_json::json!({
                                "error": reason,
                                "source": "tool_protocol_guard",
                                "failure_class": "E_PROTOCOL_TOOL_WRAPPER",
                                "attempt": malformed_tool_call_attempts
                            }),
                        );
                        return self.finalize_planner_error_with_output_with_end(
                            step as u32,
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
                            &total_token_usage,
                            &taint_state,
                        );
                    }
                }
                ToolWrapperParseState::Unchanged => {}
            }
            if let Some(usage) = &resp.usage {
                apply_usage_totals(usage, &mut saw_token_usage, &mut total_token_usage);
            }
            self.emit_event(
                &run_id,
                step as u32,
                EventKind::ModelResponseEnd,
                serde_json::json!({"tool_calls": resp.tool_calls.len()}),
            );
            if resp.tool_calls.len() > 1 {
                let reason = format!(
                    "MODEL_TOOL_PROTOCOL_VIOLATION: multiple tool calls in a single assistant step (max 1, got {})",
                    resp.tool_calls.len()
                );
                self.emit_event(
                    &run_id,
                    step as u32,
                    EventKind::Error,
                    serde_json::json!({
                        "error": reason,
                        "source": "tool_protocol_guard",
                        "failure_class": "E_PROTOCOL_MULTI_TOOL",
                        "tool_calls": resp.tool_calls.len()
                    }),
                );
                return self.finalize_planner_error_with_output_with_end(
                    step as u32,
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
                    &total_token_usage,
                    &taint_state,
                );
            }
            let has_actionable_tool_calls = !resp.tool_calls.is_empty();
            let model_signaled_finalize = !has_actionable_tool_calls;
            if tool_only_phase_active
                && model_signaled_finalize
                && !resp
                    .assistant
                    .content
                    .as_deref()
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
            {
                blocked_tool_only_count = blocked_tool_only_count.saturating_add(1);
                self.emit_event(
                    &run_id,
                    step as u32,
                    EventKind::StepBlocked,
                    serde_json::json!({
                        "reason": "tool_only_violation",
                        "blocked_count": blocked_tool_only_count
                    }),
                );
                if blocked_tool_only_count >= 2 {
                    let reason = "MODEL_TOOL_PROTOCOL_VIOLATION: repeated prose output during tool-only phase".to_string();
                    self.emit_event(
                        &run_id,
                        step as u32,
                        EventKind::Error,
                        serde_json::json!({
                            "error": reason,
                            "source": "tool_protocol_guard",
                            "failure_class": "E_PROTOCOL_TOOL_ONLY"
                        }),
                    );
                    return self.finalize_planner_error_with_output_with_end(
                        step as u32,
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
                        &total_token_usage,
                        &taint_state,
                    );
                }
                messages.push(resp.assistant.clone());
                messages.push(Message {
                    role: Role::Developer,
                    content: Some(self.tool_only_reminder_message()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                });
                continue;
            }
            if has_actionable_tool_calls {
                tool_only_phase_active = false;
            }
            let mut assistant = resp.assistant.clone();
            if let Some(c) = assistant.content.as_deref() {
                assistant.content = Some(sanitize_user_visible_output(c));
            }
            messages.push(assistant.clone());
            let worker_step_status = self.parse_worker_step_status_if_enforced(&assistant);
            if self.plan_enforcement_active()
                && worker_step_status.is_none()
                && model_signaled_finalize
            {
                blocked_control_envelope_count = blocked_control_envelope_count.saturating_add(1);
                self.emit_event(
                    &run_id,
                    step as u32,
                    EventKind::StepBlocked,
                    serde_json::json!({
                        "reason": "invalid_control_envelope",
                        "required_schema_version": crate::planner::STEP_RESULT_SCHEMA_VERSION,
                        "blocked_count": blocked_control_envelope_count
                    }),
                );
                if blocked_control_envelope_count >= 2 {
                    return self.finalize_planner_error_with_end(
                        step as u32,
                        run_id,
                        started_at,
                        "worker response missing control envelope for planner-enforced mode"
                            .to_string(),
                        messages,
                        observed_tool_calls,
                        observed_tool_decisions,
                        request_context_chars,
                        last_compaction_report,
                        hook_invocations,
                        provider_retry_count,
                        provider_error_count,
                        saw_token_usage,
                        &total_token_usage,
                        &taint_state,
                    );
                }
                messages.push(Message {
                    role: Role::Developer,
                    content: Some(self.control_envelope_reminder_message()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                });
                continue;
            }
            if let Some(step_status) = worker_step_status.as_ref() {
                blocked_control_envelope_count = 0;
                if let Some(user_output) = step_status.user_output.as_ref() {
                    if !user_output.trim().is_empty() {
                        last_user_output = Some(user_output.trim().to_string());
                    }
                }
                let current_step_id = self.current_plan_step_id_or_unknown(active_plan_step_idx);
                match step_status.status.as_str() {
                    "done" => {
                        if step_status.step_id != current_step_id {
                            let transition_error = format!(
                                "invalid step completion transition: got done for {}, expected {}",
                                step_status.step_id, current_step_id
                            );
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::StepBlocked,
                                serde_json::json!({
                                    "step_id": step_status.step_id,
                                    "expected_step_id": current_step_id,
                                    "reason": "invalid_done_transition"
                                }),
                            );
                            return self.finalize_planner_error_with_end(
                                step as u32,
                                run_id,
                                started_at,
                                transition_error,
                                messages,
                                observed_tool_calls,
                                observed_tool_decisions,
                                request_context_chars,
                                last_compaction_report,
                                hook_invocations,
                                provider_retry_count,
                                provider_error_count,
                                saw_token_usage,
                                &total_token_usage,
                                &taint_state,
                            );
                        }
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::StepVerified,
                            serde_json::json!({
                                "step_id": step_status.step_id,
                                "next_step_id": step_status.next_step_id,
                                "status": step_status.status
                            }),
                        );
                        blocked_runtime_completion_count = 0;
                        step_retry_counts.remove(&current_step_id);
                        if let Some(next) = &step_status.next_step_id {
                            if next == "final" {
                                active_plan_step_idx = self.plan_step_constraints.len();
                            } else if let Some(next_idx) = self
                                .plan_step_constraints
                                .iter()
                                .position(|s| s.step_id == *next)
                            {
                                active_plan_step_idx = next_idx;
                            } else {
                                self.emit_event(
                                    &run_id,
                                    step as u32,
                                    EventKind::StepBlocked,
                                    serde_json::json!({
                                        "step_id": step_status.step_id,
                                        "next_step_id": next,
                                        "reason": "invalid_next_step_id"
                                    }),
                                );
                                return self.finalize_planner_error_with_end(
                                    step as u32,
                                    run_id,
                                    started_at,
                                    format!("invalid next_step_id in worker status: {}", next),
                                    messages,
                                    observed_tool_calls,
                                    observed_tool_decisions,
                                    request_context_chars,
                                    last_compaction_report,
                                    hook_invocations,
                                    provider_retry_count,
                                    provider_error_count,
                                    saw_token_usage,
                                    &total_token_usage,
                                    &taint_state,
                                );
                            }
                        } else if active_plan_step_idx < self.plan_step_constraints.len() {
                            active_plan_step_idx = active_plan_step_idx.saturating_add(1);
                        }
                    }
                    "retry" => {
                        if step_status.step_id != current_step_id {
                            let transition_error = format!(
                                "invalid retry transition: got retry for {}, expected {}",
                                step_status.step_id, current_step_id
                            );
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::StepBlocked,
                                serde_json::json!({
                                    "step_id": step_status.step_id,
                                    "expected_step_id": current_step_id,
                                    "reason": "invalid_retry_transition"
                                }),
                            );
                            return self.finalize_planner_error_with_end(
                                step as u32,
                                run_id,
                                started_at,
                                transition_error,
                                messages,
                                observed_tool_calls,
                                observed_tool_decisions,
                                request_context_chars,
                                last_compaction_report,
                                hook_invocations,
                                provider_retry_count,
                                provider_error_count,
                                saw_token_usage,
                                &total_token_usage,
                                &taint_state,
                            );
                        }
                        let entry = step_retry_counts
                            .entry(step_status.step_id.clone())
                            .or_insert(0);
                        *entry = entry.saturating_add(1);
                        if *entry > 2 {
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::StepBlocked,
                                serde_json::json!({
                                    "step_id": step_status.step_id,
                                    "reason": "retry_limit_exceeded",
                                    "retry_count": *entry
                                }),
                            );
                            return self.finalize_planner_error_with_end(
                                step as u32,
                                run_id,
                                started_at,
                                format!(
                                    "step {} exceeded retry transition limit",
                                    step_status.step_id
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
                                &total_token_usage,
                                &taint_state,
                            );
                        }
                    }
                    "replan" => {
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::StepReplanned,
                            serde_json::json!({
                                "step_id": step_status.step_id,
                                "status": step_status.status
                            }),
                        );
                        return self.finalize_planner_error_with_end(
                            step as u32,
                            run_id,
                            started_at,
                            format!(
                                "worker requested {} transition for step {}",
                                step_status.status, step_status.step_id
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
                            &total_token_usage,
                            &taint_state,
                        );
                    }
                    "fail" => {
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::StepBlocked,
                            serde_json::json!({
                                "step_id": step_status.step_id,
                                "reason": "worker_fail_transition"
                            }),
                        );
                        return self.finalize_planner_error_with_end(
                            step as u32,
                            run_id,
                            started_at,
                            format!(
                                "worker requested {} transition for step {}",
                                step_status.status, step_status.step_id
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
                            &total_token_usage,
                            &taint_state,
                        );
                    }
                    _ => {}
                }
            }
            if matches!(self.taint_toggle, TaintToggle::On) {
                let idx = messages.len().saturating_sub(1);
                taint_state.mark_assistant_context_tainted(idx);
            }

            let completion_inputs = RuntimeCompletionInputs {
                has_tool_calls: has_actionable_tool_calls,
                plan_tool_enforcement: self.plan_tool_enforcement,
                active_plan_step_idx,
                plan_step_constraints_len: self.plan_step_constraints.len(),
                tool_only_phase_active,
                enforce_implementation_integrity_guard,
                observed_tool_calls_len: observed_tool_calls.len(),
                blocked_attempt_count_next: blocked_runtime_completion_count.saturating_add(1),
            };
            let completion_decision = runtime_completion_decision(&completion_inputs);
            match self
                .handle_runtime_completion_action(
                    completion_decision,
                    run_id.clone(),
                    step as u32,
                    started_at.clone(),
                    user_prompt,
                    last_user_output.as_ref(),
                    assistant.content.as_deref(),
                    active_plan_step_idx,
                    enforce_implementation_integrity_guard,
                    blocked_runtime_completion_count.saturating_add(1),
                    &mut messages,
                    observed_tool_calls.clone(),
                    &mut observed_tool_executions,
                    observed_tool_decisions.clone(),
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.clone(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    &total_token_usage,
                    &taint_state,
                )
                .await
            {
                RuntimeCompletionAction::ContinueStep {
                    blocked_runtime_completion_count: next_count,
                } => {
                    blocked_runtime_completion_count = next_count;
                    continue;
                }
                RuntimeCompletionAction::ContinueAgentStep {
                    blocked_runtime_completion_count: next_count,
                } => {
                    blocked_runtime_completion_count = next_count;
                    continue 'agent_steps;
                }
                RuntimeCompletionAction::ProceedToTools {
                    blocked_runtime_completion_count: next_count,
                } => {
                    blocked_runtime_completion_count = next_count;
                }
                RuntimeCompletionAction::Finalize(outcome) => return outcome,
            }

            let mut successful_write_tool_ok_this_step = false;
            for tc in &resp.tool_calls {
                self.record_detected_tool_call(&run_id, step as u32, tc, &mut observed_tool_calls);
                match self
                    .check_mcp_drift_for_tool_call(
                        run_id.clone(),
                        step as u32,
                        tc,
                        expected_mcp_catalog_hash_hex.as_ref(),
                        expected_mcp_docs_hash_hex.as_ref(),
                        started_at.clone(),
                        messages.clone(),
                        observed_tool_calls.clone(),
                        &mut observed_tool_decisions,
                        request_context_chars,
                        last_compaction_report.clone(),
                        hook_invocations.clone(),
                        provider_retry_count,
                        provider_error_count,
                        saw_token_usage,
                        &total_token_usage,
                        &taint_state,
                    )
                    .await
                {
                    McpDriftDecision::Continue => {}
                    McpDriftDecision::Finalize(outcome) => return outcome,
                }
                let (plan_allowed_tools, plan_tool_allowed) = self
                    .plan_allowed_tools_and_decision(active_plan_step_idx, &tc.name);
                let plan_step_id = self
                    .current_plan_constraint(active_plan_step_idx)
                    .map(|c| c.step_id)
                    .unwrap_or_else(|| "unknown".to_string());
                let repeat_key = failed_repeat_key(tc);
                let failed_repeat_count =
                    failed_repeat_counts.get(&repeat_key).copied().unwrap_or(0);
                if failed_repeat_count >= MAX_FAILED_REPEAT_PER_KEY {
                    let reason = format!(
                        "TOOL_REPEAT_BLOCKED: repeated failed tool call for '{}' exceeded repeat limit",
                        tc.name
                    );
                    self.emit_event(
                        &run_id,
                        step as u32,
                        EventKind::StepBlocked,
                        serde_json::json!({
                            "source": "tool_repeat_guard",
                            "code": "TOOL_REPEAT_BLOCKED",
                            "tool_call_id": tc.id,
                            "name": tc.name,
                            "repeat_count": failed_repeat_count,
                            "repeat_limit": MAX_FAILED_REPEAT_PER_KEY,
                            "repeat_key_sha256": repeat_key
                        }),
                    );
                    self.emit_event(
                        &run_id,
                        step as u32,
                        EventKind::Error,
                        serde_json::json!({
                            "error": reason.clone(),
                            "source": "tool_repeat_guard",
                            "tool_call_id": tc.id,
                            "name": tc.name
                        }),
                    );
                    return self.finalize_planner_error_with_output_with_end(
                        step as u32,
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
                        &total_token_usage,
                        &taint_state,
                    );
                }
                let invalid_args_error = match self.handle_malformed_tool_call(
                    run_id.clone(),
                    step as u32,
                    tc,
                    plan_tool_allowed,
                    &mut malformed_tool_call_attempts,
                    &mut schema_repair_attempts,
                    &mut messages,
                    started_at.clone(),
                    observed_tool_calls.clone(),
                    observed_tool_decisions.clone(),
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.clone(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    &total_token_usage,
                    &taint_state,
                ) {
                    MalformedToolCallDecision::ContinueToolLoop { invalid_args_error } => {
                        invalid_args_error
                    }
                    MalformedToolCallDecision::RestartAgentStep => continue 'agent_steps,
                    MalformedToolCallDecision::Finalize(outcome) => return outcome,
                };
                let (
                    approval_mode_meta,
                    auto_scope_meta,
                    approval_key_version_meta,
                    tool_schema_hash_hex,
                    hooks_config_hash_hex,
                    planner_hash_hex,
                    decision_exec_target,
                ) = self.gate_decision_metadata_for_tool(tc, &taint_state);
                if !plan_tool_allowed {
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
                        step as u32,
                        EventKind::StepBlocked,
                        serde_json::json!({
                            "step_id": plan_step_id.clone(),
                            "tool": tc.name,
                            "reason": "tool_not_allowed_by_plan",
                            "allowed_tools": plan_allowed_tools.clone()
                        }),
                    );
                    self.emit_event(
                        &run_id,
                        step as u32,
                        EventKind::ToolDecision,
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
                        step: step as u32,
                        tool_call_id: tc.id.clone(),
                        tool: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                        decision: "deny".to_string(),
                        decision_reason: Some(reason.clone()),
                        decision_source: Some("plan_step_constraint".to_string()),
                        approval_id: None,
                        approval_key: None,
                        approval_mode: approval_mode_meta.clone(),
                        auto_approve_scope: auto_scope_meta.clone(),
                        approval_key_version: approval_key_version_meta.clone(),
                        tool_schema_hash_hex: tool_schema_hash_hex.clone(),
                        hooks_config_hash_hex: hooks_config_hash_hex.clone(),
                        planner_hash_hex: planner_hash_hex.clone(),
                        exec_target: decision_exec_target.clone(),
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
                        step: step as u32,
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
                        PlanToolEnforcementMode::Off => {}
                        PlanToolEnforcementMode::Soft => {
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::ToolExecEnd,
                                serde_json::json!({
                                    "tool_call_id": tc.id,
                                    "name": tc.name,
                                    "ok": false,
                                    "truncated": false,
                                    "source": "plan_step_constraint"
                                }),
                            );
                            messages.push(envelope_to_message(to_tool_result_envelope(
                                tc,
                                "runtime",
                                false,
                                reason,
                                false,
                                ToolResultMeta {
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
                            )));
                            if self.inject_post_tool_operator_messages(
                                &run_id,
                                step as u32,
                                &mut messages,
                            ) {
                                continue 'agent_steps;
                            }
                            continue;
                        }
                        PlanToolEnforcementMode::Hard => {
                            return self.finalize_denied_with_end(
                                step as u32,
                                run_id,
                                started_at,
                                reason,
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
                                &total_token_usage,
                                &taint_state,
                            );
                        }
                    }
                }
                match self.gate.decide(&self.gate_ctx, tc) {
                    GateDecision::Allow {
                        approval_id,
                        approval_key,
                        reason,
                        source,
                        taint_enforced,
                        escalated,
                        escalation_reason,
                    } => {
                        match self
                            .handle_gate_allow_tool_call(
                                run_id.clone(),
                                step as u32,
                                tc,
                                approval_id,
                                approval_key,
                                reason,
                                source,
                                taint_enforced,
                                escalated,
                                escalation_reason,
                                invalid_args_error.clone(),
                                plan_tool_allowed,
                                &repeat_key,
                                approval_mode_meta.clone(),
                                auto_scope_meta.clone(),
                                approval_key_version_meta.clone(),
                                tool_schema_hash_hex.clone(),
                                hooks_config_hash_hex.clone(),
                                planner_hash_hex.clone(),
                                decision_exec_target.clone(),
                                started_at.clone(),
                                &mut messages,
                                observed_tool_calls.clone(),
                                &mut observed_tool_decisions,
                                &mut observed_tool_executions,
                                request_context_chars,
                                last_compaction_report.clone(),
                                &mut hook_invocations,
                                provider_retry_count,
                                provider_error_count,
                                saw_token_usage,
                                &total_token_usage,
                                &mut taint_state,
                                &mut tool_budget_usage,
                                &mut failed_repeat_counts,
                                &mut invalid_patch_format_attempts,
                                &mut schema_repair_attempts,
                                &mut successful_write_tool_ok_this_step,
                            )
                            .await
                        {
                            AllowToolCallDecision::Continue => {}
                            AllowToolCallDecision::RestartAgentStep => continue 'agent_steps,
                            AllowToolCallDecision::Finalize(outcome) => return outcome,
                        }
                    }
                    gate_decision => match self.handle_non_allow_gate_decision(
                        run_id.clone(),
                        step as u32,
                        tc,
                        gate_decision,
                        invalid_args_error.as_ref(),
                        approval_mode_meta.clone(),
                        auto_scope_meta.clone(),
                        approval_key_version_meta.clone(),
                        tool_schema_hash_hex.clone(),
                        hooks_config_hash_hex.clone(),
                        planner_hash_hex.clone(),
                        decision_exec_target.clone(),
                        started_at.clone(),
                        &mut messages,
                        observed_tool_calls.clone(),
                        &mut observed_tool_decisions,
                        request_context_chars,
                        last_compaction_report.clone(),
                        hook_invocations.clone(),
                        provider_retry_count,
                        provider_error_count,
                        saw_token_usage,
                        &total_token_usage,
                        &taint_state,
                    ) {
                        GateNonAllowDecision::ContinueToolLoop => continue,
                        GateNonAllowDecision::RestartAgentStep => continue 'agent_steps,
                        GateNonAllowDecision::Finalize(outcome) => return outcome,
                    },
                }
            }

            if enforce_implementation_integrity_guard && successful_write_tool_ok_this_step {
                return self.finalize_verified_write_step_or_error(
                    run_id,
                    step as u32,
                    started_at,
                    user_prompt,
                    observed_tool_calls,
                    &mut observed_tool_executions,
                    observed_tool_decisions,
                    messages,
                    request_context_chars,
                    last_compaction_report,
                    hook_invocations,
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    &total_token_usage,
                    &taint_state,
                    enforce_implementation_integrity_guard,
                )
                .await;
            }
        }

        let final_prompt_size_chars = context_size_chars(&messages);
        self.finalize_max_steps_with_end(
            self.max_steps as u32,
            run_id,
            started_at,
            "Max steps reached before the model produced a final answer.".to_string(),
            messages,
            observed_tool_calls,
            observed_tool_decisions,
            final_prompt_size_chars,
            last_compaction_report,
            hook_invocations,
            provider_retry_count,
            provider_error_count,
            saw_token_usage,
            &total_token_usage,
            &taint_state,
        )
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod agent_tests;
