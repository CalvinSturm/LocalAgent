use crate::agent_budget::{
    check_and_consume_mcp_budget, check_and_consume_tool_budget, ToolCallBudgetUsage,
};
use uuid::Uuid;

use crate::agent_impl_guard::{
    implementation_integrity_violation_with_tool_executions, pending_post_write_verification_paths,
    prompt_requires_tool_only, ToolExecutionRecord,
};
use crate::agent_output_sanitize::sanitize_user_visible_output as sanitize_user_visible_output_impl;
use crate::agent_taint_helpers::compute_taint_spans_for_tool;
use crate::agent_tool_exec::{
    classify_tool_failure, infer_truncated_flag, is_apply_patch_invalid_format_error,
    make_invalid_args_tool_message, schema_repair_instruction_message, tool_result_error_code,
    tool_result_has_error,
};
use crate::agent_utils::{provider_name, sha256_hex};
use crate::compaction::{
    context_size_chars, maybe_compact, CompactionReport, CompactionSettings,
};
use crate::events::{EventKind, EventSink};
use crate::gate::{GateContext, GateDecision, GateEvent, ToolGate};
use crate::hooks::protocol::{
    HookInvocationReport, PreModelCompactionPayload, PreModelPayload, ToolResultPayload,
};
use crate::hooks::runner::{make_pre_model_input, make_tool_result_input, HookManager};
use crate::mcp::registry::McpRegistry;
use crate::operator_queue::{
    PendingMessageQueue, QueueLimits, QueueSubmitRequest,
};
use crate::providers::ModelProvider;
use crate::taint::{TaintMode, TaintState, TaintToggle};
use crate::tools::{
    envelope_to_message, to_tool_result_envelope, tool_side_effects, validate_builtin_tool_args,
    ToolErrorCode, ToolResultMeta, ToolRuntime,
};
use crate::trust::policy::Policy;
use crate::types::{Message, Role, TokenUsage, ToolDef};
use std::time::{Duration, Instant};

mod agent_types;
mod budget_guard;
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
    runtime_completion_decision, RuntimeCompletionDecision, RuntimeCompletionInputs,
};
use run_events::apply_usage_totals;
use response_normalization::{normalize_tool_calls_from_assistant, ToolWrapperParseState};
use tool_helpers::{
    failed_repeat_key, injected_messages_enforce_implementation_integrity_guard,
    is_repairable_error_code, normalized_tool_path_from_args,
};

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
            match completion_decision {
                RuntimeCompletionDecision::Continue {
                    reason_code,
                    corrective_instruction,
                } => {
                    blocked_runtime_completion_count =
                        blocked_runtime_completion_count.saturating_add(1);
                    let mut source = "runtime_completion_policy";
                    let mut error_text = corrective_instruction.to_string();
                    if reason_code == "pending_plan_step"
                        && self.plan_enforcement_active()
                    {
                        if let Some(text) = self.pending_plan_step_text(active_plan_step_idx) {
                            error_text = text;
                            source = "plan_halt_guard";
                        }
                    }
                    self.emit_event(
                        &run_id,
                        step as u32,
                        EventKind::Error,
                        serde_json::json!({
                            "error": error_text,
                            "source": source,
                            "reason_code": reason_code,
                            "blocked_count": blocked_runtime_completion_count
                        }),
                    );
                    self.emit_event(
                        &run_id,
                        step as u32,
                        EventKind::StepBlocked,
                        serde_json::json!({
                            "reason": reason_code,
                            "blocked_count": blocked_runtime_completion_count
                        }),
                    );
                    let corrective_message = if reason_code == "pending_plan_step"
                        && self.plan_enforcement_active()
                    {
                        self.pending_plan_step_corrective_message(active_plan_step_idx)
                            .unwrap_or_else(|| corrective_instruction.to_string())
                    } else {
                        corrective_instruction.to_string()
                    };
                    messages.push(Message {
                        role: Role::Developer,
                        content: Some(corrective_message),
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                    });
                    continue;
                }
                RuntimeCompletionDecision::FinalizeError {
                    reason,
                    source,
                    failure_class,
                } => {
                    self.emit_event(
                        &run_id,
                        step as u32,
                        EventKind::Error,
                        serde_json::json!({
                            "error": reason,
                            "source": source,
                            "failure_class": failure_class
                        }),
                    );
                    return self.finalize_planner_error_with_end(
                        step as u32,
                        run_id,
                        started_at,
                        reason.to_string(),
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
                RuntimeCompletionDecision::FinalizeOk => {
                    blocked_runtime_completion_count = 0;
                    let (queue_delivered, queue_interrupted) = self
                        .inject_turn_idle_operator_messages(&run_id, step as u32, &mut messages);
                    if queue_interrupted || queue_delivered {
                        continue 'agent_steps;
                    }
                    let final_output = self.final_output_for_completion(
                        last_user_output.as_ref(),
                        assistant.content.as_deref(),
                    );
                    if enforce_implementation_integrity_guard {
                        let post_write_verify_timeout_ms =
                            self.effective_post_write_verify_timeout_ms();
                        let pending_post_write_paths =
                            pending_post_write_verification_paths(&observed_tool_executions);
                        for path in pending_post_write_paths {
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::PostWriteVerifyStart,
                                serde_json::json!({
                                    "name": "read_file",
                                    "path": path.clone(),
                                    "source": "runtime_post_write_verify",
                                    "timeout_ms": post_write_verify_timeout_ms
                                }),
                            );
                            let verify_started = Instant::now();
                            let verify = match tokio::time::timeout(
                                Duration::from_millis(post_write_verify_timeout_ms),
                                self.tool_rt.exec_target.read_file(crate::target::ReadReq {
                                    workdir: self.tool_rt.workdir.clone(),
                                    path: path.clone(),
                                    max_read_bytes: self.tool_rt.max_read_bytes,
                                }),
                            )
                            .await
                            {
                                Ok(result) => result,
                                Err(_) => {
                                    self.emit_event(
                                        &run_id,
                                        step as u32,
                                        EventKind::PostWriteVerifyEnd,
                                        serde_json::json!({
                                            "name": "read_file",
                                            "path": path.clone(),
                                            "ok": false,
                                            "status": "timeout",
                                            "source": "runtime_post_write_verify",
                                            "failure_class": "E_RUNTIME_POST_WRITE_VERIFY_TIMEOUT",
                                            "elapsed_ms": verify_started.elapsed().as_millis() as u64,
                                            "timeout_ms": post_write_verify_timeout_ms
                                        }),
                                    );
                                    let reason = format!(
                                        "implementation guard: runtime post-write verification timed out on read_file for '{path}' after {}ms",
                                        post_write_verify_timeout_ms
                                    );
                                    self.emit_event(
                                        &run_id,
                                        step as u32,
                                        EventKind::Error,
                                        serde_json::json!({
                                            "error": reason,
                                            "source": "implementation_integrity_guard",
                                            "failure_class": "E_RUNTIME_POST_WRITE_VERIFY_TIMEOUT",
                                            "path": path.clone(),
                                            "timeout_ms": post_write_verify_timeout_ms
                                        }),
                                    );
                                    return self.finalize_planner_error_with_end(
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
                            };
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::PostWriteVerifyEnd,
                                serde_json::json!({
                                    "name": "read_file",
                                    "path": path.clone(),
                                    "ok": verify.ok,
                                    "status": if verify.ok { "ok" } else { "failed" },
                                    "source": "runtime_post_write_verify",
                                    "failure_class": if verify.ok { serde_json::Value::Null } else { serde_json::Value::String("E_RUNTIME_POST_WRITE_VERIFY_FAILED".to_string()) },
                                    "elapsed_ms": verify_started.elapsed().as_millis() as u64
                                }),
                            );
                            observed_tool_executions.push(ToolExecutionRecord {
                                name: "read_file".to_string(),
                                path: Some(path.clone()),
                                ok: verify.ok,
                            });
                            if !verify.ok {
                                let reason = format!(
                                    "implementation guard: runtime post-write verification failed read_file on '{path}': {}",
                                    verify.content
                                );
                                self.emit_event(
                                    &run_id,
                                    step as u32,
                                    EventKind::Error,
                                    serde_json::json!({
                                        "error": reason,
                                        "source": "implementation_integrity_guard"
                                    }),
                                );
                                return self.finalize_planner_error_with_end(
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
                    }
                    if let Some(reason) = implementation_integrity_violation_with_tool_executions(
                        user_prompt,
                        &final_output,
                        &observed_tool_calls,
                        &observed_tool_executions,
                        enforce_implementation_integrity_guard,
                    ) {
                        let saw_write_attempt = observed_tool_executions
                            .iter()
                            .any(|e| matches!(e.name.as_str(), "apply_patch" | "write_file"));
                        if !saw_write_attempt
                            && reason.contains("without an effective write")
                            && blocked_runtime_completion_count < 2
                        {
                            blocked_runtime_completion_count =
                                blocked_runtime_completion_count.saturating_add(1);
                            let corrective_instruction = "Implementation task requires at least one effective write tool call. Use read_file + apply_patch (or write_file when creating a new file), then verify with read_file before finalizing.";
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::Error,
                                serde_json::json!({
                                    "error": corrective_instruction,
                                    "source": "implementation_integrity_guard",
                                    "reason_code": "implementation_requires_effective_write",
                                    "blocked_count": blocked_runtime_completion_count
                                }),
                            );
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::StepBlocked,
                                serde_json::json!({
                                    "reason": "implementation_requires_effective_write",
                                    "blocked_count": blocked_runtime_completion_count
                                }),
                            );
                            messages.push(Message {
                                role: Role::Developer,
                                content: Some(corrective_instruction.to_string()),
                                tool_call_id: None,
                                tool_name: None,
                                tool_calls: None,
                            });
                            continue 'agent_steps;
                        }
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::Error,
                            serde_json::json!({
                                "error": reason,
                                "source": "implementation_integrity_guard"
                            }),
                        );
                        return self.finalize_planner_error_with_end(
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
                    return self.finalize_ok_with_end(
                        step as u32,
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
                        &total_token_usage,
                        &taint_state,
                    );
                }
                RuntimeCompletionDecision::ExecuteTools => {
                    blocked_runtime_completion_count = 0;
                }
            }

            let mut successful_write_tool_ok_this_step = false;
            for tc in &resp.tool_calls {
                self.record_detected_tool_call(&run_id, step as u32, tc, &mut observed_tool_calls);
                if tc.name.starts_with("mcp.") {
                    if matches!(self.mcp_pin_enforcement, McpPinEnforcementMode::Off) {
                        // Drift probing disabled by configuration.
                    } else if let (Some(registry), Some(expected_hash)) = (
                        self.mcp_registry.as_ref(),
                        expected_mcp_catalog_hash_hex.as_ref(),
                    ) {
                        let live_catalog = registry.live_tool_catalog_hash_hex().await;
                        let live_docs = if expected_mcp_docs_hash_hex.is_some() {
                            Some(registry.live_tool_docs_hash_hex().await)
                        } else {
                            None
                        };
                        match (live_catalog, live_docs) {
                            (Ok(actual_hash), Some(Ok(actual_docs_hash))) => {
                                let expected_docs_hash =
                                    expected_mcp_docs_hash_hex.as_deref().unwrap_or_default();
                                let catalog_drift = actual_hash != *expected_hash;
                                let docs_drift = actual_docs_hash != expected_docs_hash;
                                if !catalog_drift && !docs_drift {
                                    // No drift.
                                } else {
                                    let mut codes = Vec::new();
                                    if catalog_drift {
                                        codes.push("MCP_CATALOG_DRIFT");
                                    }
                                    if docs_drift {
                                        codes.push("MCP_DOCS_DRIFT");
                                    }
                                    let primary_code =
                                        codes.first().copied().unwrap_or("MCP_CATALOG_DRIFT");
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
                                        step as u32,
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
                                    if matches!(
                                        self.mcp_pin_enforcement,
                                        McpPinEnforcementMode::Hard
                                    ) {
                                        return self.finalize_mcp_drift_hard_deny_with_end(
                                            run_id,
                                            step as u32,
                                            tc,
                                            reason,
                                            "mcp_drift",
                                            started_at,
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
                                    self.record_mcp_drift_warn_decision(
                                        &run_id,
                                        step as u32,
                                        tc,
                                        reason,
                                        &taint_state,
                                        &mut observed_tool_decisions,
                                    );
                                }
                            }
                            (Ok(actual_hash), Some(Err(e))) => {
                                let reason = format!(
                                    "MCP_DRIFT verification failed: unable to probe live docs hash ({e})"
                                );
                                self.emit_event(
                                    &run_id,
                                    step as u32,
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
                                if matches!(self.mcp_pin_enforcement, McpPinEnforcementMode::Hard) {
                                    return self.finalize_mcp_drift_hard_deny_with_end(
                                        run_id,
                                        step as u32,
                                        tc,
                                        reason,
                                        "mcp_drift",
                                        started_at,
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
                                self.record_mcp_drift_warn_decision(
                                    &run_id,
                                    step as u32,
                                    tc,
                                    reason,
                                    &taint_state,
                                    &mut observed_tool_decisions,
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
                                        step as u32,
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
                                    if matches!(
                                        self.mcp_pin_enforcement,
                                        McpPinEnforcementMode::Hard
                                    ) {
                                        return self.finalize_mcp_drift_hard_deny_with_end(
                                            run_id,
                                            step as u32,
                                            tc,
                                            reason,
                                            "mcp_drift",
                                            started_at,
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
                                    self.record_mcp_drift_warn_decision(
                                        &run_id,
                                        step as u32,
                                        tc,
                                        reason,
                                        &taint_state,
                                        &mut observed_tool_decisions,
                                    );
                                }
                            }
                            (Err(e), _) => {
                                let reason = format!(
                                    "MCP_DRIFT verification failed: unable to probe live tool catalog ({e})"
                                );
                                self.emit_event(
                                    &run_id,
                                    step as u32,
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
                                if matches!(self.mcp_pin_enforcement, McpPinEnforcementMode::Hard) {
                                    return self.finalize_mcp_drift_hard_deny_with_end(
                                        run_id,
                                        step as u32,
                                        tc,
                                        reason,
                                        "mcp_drift_probe_failed",
                                        started_at,
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
                                self.record_mcp_drift_warn_decision(
                                    &run_id,
                                    step as u32,
                                    tc,
                                    reason,
                                    &taint_state,
                                    &mut observed_tool_decisions,
                                );
                            }
                        }
                    }
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
                let invalid_args_error = if tc.name.starts_with("mcp.") {
                    self.mcp_registry.as_ref().and_then(|reg| {
                        reg.validate_namespaced_tool_args(tc, self.tool_rt.tool_args_strict)
                            .err()
                    })
                } else {
                    validate_builtin_tool_args(
                        &tc.name,
                        &tc.arguments,
                        self.tool_rt.tool_args_strict,
                    )
                    .err()
                };
                if let Some(err) = &invalid_args_error {
                    malformed_tool_call_attempts = malformed_tool_call_attempts.saturating_add(1);
                    if malformed_tool_call_attempts >= 2 {
                        let reason = format!(
                            "MODEL_TOOL_PROTOCOL_VIOLATION: repeated malformed tool calls (tool='{}', error='{}')",
                            tc.name, err
                        );
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::Error,
                            serde_json::json!({
                                "error": reason,
                                "source": "tool_protocol_guard",
                                "tool_call_id": tc.id,
                                "name": tc.name,
                                "failure_class": "E_SCHEMA",
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
                    let repair_key = format!("{}|{}", tc.name, err);
                    let attempts = schema_repair_attempts
                        .entry(repair_key)
                        .and_modify(|n| *n = n.saturating_add(1))
                        .or_insert(1);
                    if *attempts <= MAX_SCHEMA_REPAIR_ATTEMPTS && plan_tool_allowed {
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::ToolRetry,
                            serde_json::json!({
                                "tool_call_id": tc.id,
                                "name": tc.name,
                                "attempt": *attempts,
                                "max_retries": MAX_SCHEMA_REPAIR_ATTEMPTS,
                                "max_attempts": MAX_SCHEMA_REPAIR_ATTEMPTS,
                                "failure_class": "E_SCHEMA",
                                "action": "repair",
                                "error_code": ToolErrorCode::ToolArgsInvalid.as_str()
                            }),
                        );
                        let tool_msg =
                            make_invalid_args_tool_message(tc, err, self.tool_rt.exec_target_kind);
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::ToolExecEnd,
                            serde_json::json!({
                                "tool_call_id": tc.id,
                                "name": tc.name,
                                "ok": false,
                                "truncated": false,
                                "retry_count": 0,
                                "failure_class": "E_SCHEMA",
                                "source": "schema_repair",
                                "repair_attempted": true,
                                "repair_succeeded": false,
                                "error_code": ToolErrorCode::ToolArgsInvalid.as_str()
                            }),
                        );
                        messages.push(tool_msg);
                        if self.inject_post_tool_operator_messages(
                            &run_id,
                            step as u32,
                            &mut messages,
                        ) {
                            continue 'agent_steps;
                        }
                        messages.push(schema_repair_instruction_message(tc, err));
                        continue 'agent_steps;
                    } else {
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::ToolRetry,
                            serde_json::json!({
                                "tool_call_id": tc.id,
                                "name": tc.name,
                                "attempt": *attempts,
                                "max_retries": MAX_SCHEMA_REPAIR_ATTEMPTS,
                                "max_attempts": MAX_SCHEMA_REPAIR_ATTEMPTS,
                                "failure_class": "E_SCHEMA",
                                "action": "stop",
                                "error_code": ToolErrorCode::ToolArgsInvalid.as_str()
                            }),
                        );
                        if *attempts > MAX_SCHEMA_REPAIR_ATTEMPTS {
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::Error,
                                serde_json::json!({
                                    "error": "schema repair attempts exhausted",
                                    "source": "schema_repair",
                                    "code": "TOOL_SCHEMA_REPAIR_EXHAUSTED",
                                    "tool_call_id": tc.id,
                                    "name": tc.name,
                                    "attempt": *attempts,
                                    "max_attempts": MAX_SCHEMA_REPAIR_ATTEMPTS
                                }),
                            );
                        }
                    }
                }
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
                        let side_effects = tool_side_effects(&tc.name);
                        if let Some(reason) = check_and_consume_tool_budget(
                            &self.tool_call_budget,
                            &mut tool_budget_usage,
                            side_effects,
                        ) {
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::ToolDecision,
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
                            return self.finalize_runtime_budget_deny_with_end(
                                run_id,
                                step as u32,
                                tc,
                                reason,
                                approval_mode_meta.clone(),
                                auto_scope_meta.clone(),
                                approval_key_version_meta.clone(),
                                tool_schema_hash_hex.clone(),
                                hooks_config_hash_hex.clone(),
                                planner_hash_hex.clone(),
                                decision_exec_target.clone(),
                                started_at,
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
                        if let Some(reason) = check_and_consume_mcp_budget(
                            &self.tool_call_budget,
                            &mut tool_budget_usage,
                            tc.name.starts_with("mcp."),
                        ) {
                            return self.finalize_runtime_mcp_budget_exceeded_with_tool_decision(
                                run_id,
                                step as u32,
                                tc,
                                reason,
                                side_effects,
                                started_at,
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
                            EventKind::ToolDecision,
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
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::ToolExecTarget,
                            serde_json::json!({
                                "tool_call_id": tc.id,
                                "name": tc.name,
                                "exec_target": if tc.name.starts_with("mcp.") { "host" } else {
                                    match self.tool_rt.exec_target_kind {
                                        crate::target::ExecTargetKind::Host => "host",
                                        crate::target::ExecTargetKind::Docker => "docker",
                                    }
                                }
                            }),
                        );
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::ToolExecStart,
                            serde_json::json!({"tool_call_id": tc.id, "name": tc.name, "side_effects": tool_side_effects(&tc.name)}),
                        );
                        let mut tool_msg = if let Some(err) = &invalid_args_error {
                            make_invalid_args_tool_message(tc, err, self.tool_rt.exec_target_kind)
                        } else {
                            self.run_tool_with_timeout_and_emit_mcp_events(
                                &run_id,
                                step as u32,
                                tc,
                                "await_result",
                            )
                            .await
                        };
                        let mut tool_retry_count = 0u32;
                        if invalid_args_error.is_none() {
                            loop {
                                let current_content = tool_msg.content.clone().unwrap_or_default();
                                if !tool_result_has_error(&current_content) {
                                    break;
                                }
                                if is_apply_patch_invalid_format_error(tc, &current_content) {
                                    let attempts = invalid_patch_format_attempts
                                        .entry(repeat_key.clone())
                                        .and_modify(|n| *n = n.saturating_add(1))
                                        .or_insert(1);
                                    let invalid_patch_attempt = *attempts;
                                    if invalid_patch_attempt < 2 && plan_tool_allowed {
                                        self.emit_event(
                                            &run_id,
                                            step as u32,
                                            EventKind::ToolExecEnd,
                                            serde_json::json!({
                                                "tool_call_id": tc.id,
                                                "name": tc.name,
                                                "ok": false,
                                                "truncated": infer_truncated_flag(&current_content),
                                                "retry_count": tool_retry_count,
                                                "failure_class": "E_SCHEMA",
                                                "error_code": "tool_args_invalid",
                                                "attempt": invalid_patch_attempt
                                            }),
                                        );
                                        messages.push(tool_msg);
                                        if self.inject_post_tool_operator_messages(
                                            &run_id,
                                            step as u32,
                                            &mut messages,
                                        ) {
                                            continue 'agent_steps;
                                        }
                                        let n = failed_repeat_counts
                                            .entry(repeat_key.clone())
                                            .or_insert(0);
                                        *n = n.saturating_add(1);
                                        messages.push(schema_repair_instruction_message(
                                            tc,
                                            "invalid patch format",
                                        ));
                                        continue 'agent_steps;
                                    }
                                    if invalid_patch_attempt >= 2 {
                                        let reason = "MODEL_TOOL_PROTOCOL_VIOLATION: repeated invalid patch format for apply_patch".to_string();
                                        self.emit_event(
                                            &run_id,
                                            step as u32,
                                            EventKind::ToolExecEnd,
                                            serde_json::json!({
                                                "tool_call_id": tc.id,
                                                "name": tc.name,
                                                "ok": false,
                                                "truncated": infer_truncated_flag(&current_content),
                                                "retry_count": tool_retry_count,
                                                "failure_class": "E_PROTOCOL_PATCH_FORMAT",
                                                "attempt": invalid_patch_attempt
                                            }),
                                        );
                                        self.emit_event(
                                            &run_id,
                                            step as u32,
                                            EventKind::Error,
                                            serde_json::json!({
                                                "error": reason,
                                                "source": "tool_protocol_guard",
                                                "tool_call_id": tc.id,
                                                "name": tc.name,
                                                "failure_class": "E_PROTOCOL_PATCH_FORMAT",
                                                "attempt": invalid_patch_attempt
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
                                if let Some(error_code) = tool_result_error_code(&current_content) {
                                    if is_repairable_error_code(error_code) && plan_tool_allowed {
                                        let repair_key =
                                            format!("{}|{}", tc.name, error_code.as_str());
                                        let attempts = schema_repair_attempts
                                            .entry(repair_key)
                                            .and_modify(|n| *n = n.saturating_add(1))
                                            .or_insert(1);
                                        if *attempts <= MAX_SCHEMA_REPAIR_ATTEMPTS {
                                            self.emit_event(
                                                &run_id,
                                                step as u32,
                                                EventKind::ToolRetry,
                                                serde_json::json!({
                                                    "tool_call_id": tc.id,
                                                    "name": tc.name,
                                                    "attempt": *attempts,
                                                    "max_retries": MAX_SCHEMA_REPAIR_ATTEMPTS,
                                                    "max_attempts": MAX_SCHEMA_REPAIR_ATTEMPTS,
                                                    "failure_class": "E_SCHEMA",
                                                    "action": "repair",
                                                    "error_code": error_code.as_str()
                                                }),
                                            );
                                            self.emit_event(
                                                &run_id,
                                                step as u32,
                                                EventKind::ToolExecEnd,
                                                serde_json::json!({
                                                    "tool_call_id": tc.id,
                                                    "name": tc.name,
                                                    "ok": false,
                                                    "truncated": infer_truncated_flag(&current_content),
                                                    "retry_count": tool_retry_count,
                                                    "failure_class": "E_SCHEMA",
                                                    "error_code": error_code.as_str()
                                                }),
                                            );
                                            messages.push(tool_msg);
                                            if self.inject_post_tool_operator_messages(
                                                &run_id,
                                                step as u32,
                                                &mut messages,
                                            ) {
                                                continue 'agent_steps;
                                            }
                                            let n = failed_repeat_counts
                                                .entry(repeat_key.clone())
                                                .or_insert(0);
                                            *n = n.saturating_add(1);
                                            messages.push(schema_repair_instruction_message(
                                                tc,
                                                error_code.as_str(),
                                            ));
                                            continue 'agent_steps;
                                        }
                                        self.emit_event(
                                            &run_id,
                                            step as u32,
                                            EventKind::ToolRetry,
                                            serde_json::json!({
                                                "tool_call_id": tc.id,
                                                "name": tc.name,
                                                "attempt": *attempts,
                                                "max_retries": MAX_SCHEMA_REPAIR_ATTEMPTS,
                                                "max_attempts": MAX_SCHEMA_REPAIR_ATTEMPTS,
                                                "failure_class": "E_SCHEMA",
                                                "action": "stop",
                                                "error_code": error_code.as_str()
                                            }),
                                        );
                                        self.emit_event(
                                            &run_id,
                                            step as u32,
                                            EventKind::Error,
                                            serde_json::json!({
                                                "error": "schema repair attempts exhausted",
                                                "source": "schema_repair",
                                                "code": "TOOL_SCHEMA_REPAIR_EXHAUSTED",
                                                "tool_call_id": tc.id,
                                                "name": tc.name,
                                                "attempt": *attempts,
                                                "max_attempts": MAX_SCHEMA_REPAIR_ATTEMPTS
                                            }),
                                        );
                                    }
                                }
                                let class = classify_tool_failure(tc, &current_content, false);
                                let retry_error_code =
                                    tool_result_error_code(&current_content).map(|c| c.as_str());
                                let max_retries = class.retry_limit_for(side_effects);
                                if tool_retry_count >= max_retries {
                                    self.emit_event(
                                        &run_id,
                                        step as u32,
                                        EventKind::ToolRetry,
                                        serde_json::json!({
                                            "tool_call_id": tc.id,
                                            "name": tc.name,
                                            "attempt": tool_retry_count,
                                            "max_retries": max_retries,
                                            "failure_class": class.as_str(),
                                            "action": "stop",
                                            "error_code": retry_error_code
                                        }),
                                    );
                                    break;
                                }
                                self.emit_event(
                                    &run_id,
                                    step as u32,
                                    EventKind::ToolRetry,
                                    serde_json::json!({
                                        "tool_call_id": tc.id,
                                        "name": tc.name,
                                        "attempt": tool_retry_count + 1,
                                        "max_retries": max_retries,
                                        "failure_class": class.as_str(),
                                        "action": "retry",
                                        "error_code": retry_error_code
                                    }),
                                );
                                tool_retry_count = tool_retry_count.saturating_add(1);
                                if let Some(reason) = check_and_consume_tool_budget(
                                    &self.tool_call_budget,
                                    &mut tool_budget_usage,
                                    side_effects,
                                ) {
                                    self.emit_event(
                                        &run_id,
                                        step as u32,
                                        EventKind::ToolDecision,
                                        serde_json::json!({
                                            "tool_call_id": tc.id,
                                            "name": tc.name,
                                            "decision": "deny",
                                            "reason": reason.clone(),
                                            "source": "runtime_budget",
                                            "side_effects": side_effects
                                        }),
                                    );
                                    return self.finalize_runtime_budget_deny_with_end(
                                        run_id,
                                        step as u32,
                                        tc,
                                        reason,
                                        approval_mode_meta.clone(),
                                        auto_scope_meta.clone(),
                                        approval_key_version_meta.clone(),
                                        tool_schema_hash_hex.clone(),
                                        hooks_config_hash_hex.clone(),
                                        planner_hash_hex.clone(),
                                        decision_exec_target.clone(),
                                        started_at,
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
                                if let Some(reason) = check_and_consume_mcp_budget(
                                    &self.tool_call_budget,
                                    &mut tool_budget_usage,
                                    tc.name.starts_with("mcp."),
                                ) {
                                    return self.finalize_runtime_mcp_budget_exceeded_with_error(
                                        run_id,
                                        step as u32,
                                        reason,
                                        started_at,
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
                                tool_msg = self
                                    .run_tool_with_timeout_and_emit_mcp_events(
                                        &run_id,
                                        step as u32,
                                        tc,
                                        "retry_await_result",
                                    )
                                    .await;
                            }
                        }
                        let original_content = tool_msg.content.clone().unwrap_or_default();
                        let mut input_digest = sha256_hex(original_content.as_bytes());
                        let mut output_digest = input_digest.clone();
                        let mut input_len = original_content.chars().count();
                        let mut output_len = input_len;
                        let mut final_truncated = infer_truncated_flag(&original_content);

                        if self.hooks.enabled() {
                            let payload = ToolResultPayload {
                                tool_call_id: tc.id.clone(),
                                tool_name: tc.name.clone(),
                                ok: !tool_result_has_error(&original_content),
                                content: original_content.clone(),
                                truncated: final_truncated,
                            };
                            let hook_input = make_tool_result_input(
                                &run_id,
                                step as u32,
                                provider_name(self.gate_ctx.provider),
                                &self.model,
                                &self.gate_ctx.workdir,
                                match serde_json::to_value(payload) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        self.emit_event(
                                            &run_id,
                                            step as u32,
                                            EventKind::HookError,
                                            serde_json::json!({"stage":"tool_result","error": e.to_string()}),
                                        );
                                        return self.finalize_hook_aborted_with_end(
                                            step as u32,
                                            run_id,
                                            started_at,
                                            String::new(),
                                            format!(
                                                "failed to encode tool_result hook payload: {e}"
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
                                },
                            );
                            match self
                                .hooks
                                .run_tool_result_hooks(
                                    hook_input,
                                    &tc.name,
                                    &original_content,
                                    final_truncated,
                                )
                                .await
                            {
                                Ok(hook_out) => {
                                    for inv in &hook_out.invocations {
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
                                                "duration_ms": inv.duration_ms,
                                                "input_digest": inv.input_digest,
                                                "output_digest": inv.output_digest
                                            }),
                                        );
                                    }
                                    hook_invocations.extend(hook_out.invocations);
                                    if let Some(reason) = hook_out.abort_reason {
                                        return self.finalize_hook_aborted_with_end(
                                            step as u32,
                                            run_id,
                                            started_at,
                                            String::new(),
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
                                    tool_msg.content = Some(hook_out.content.clone());
                                    final_truncated = hook_out.truncated;
                                    input_digest = hook_out.input_digest;
                                    output_digest = hook_out.output_digest;
                                    input_len = hook_out.input_len;
                                    output_len = hook_out.output_len;
                                }
                                Err(e) => {
                                    self.emit_event(
                                        &run_id,
                                        step as u32,
                                        EventKind::HookError,
                                        serde_json::json!({"stage":"tool_result","error": e.message}),
                                    );
                                    return self.finalize_hook_aborted_with_end(
                                        step as u32,
                                        run_id,
                                        started_at,
                                        String::new(),
                                        e.message,
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

                        let content = tool_msg.content.clone().unwrap_or_default();
                        let final_ok = !tool_result_has_error(&content);
                        let final_error_code = tool_result_error_code(&content);
                        observed_tool_executions.push(ToolExecutionRecord {
                            name: tc.name.clone(),
                            path: normalized_tool_path_from_args(tc),
                            ok: final_ok,
                        });
                        let final_failure_class = if tool_result_has_error(&content) {
                            Some(classify_tool_failure(
                                tc,
                                &content,
                                invalid_args_error.is_some(),
                            ))
                        } else {
                            None
                        };
                        if matches!(self.taint_toggle, TaintToggle::On) {
                            let spans = compute_taint_spans_for_tool(
                                tc,
                                &content,
                                self.policy_for_taint.as_ref(),
                                self.taint_digest_bytes,
                            );
                            if !spans.is_empty() {
                                let tool_message_index = messages.len();
                                taint_state.add_tool_spans(
                                    &tc.id,
                                    tool_message_index,
                                    spans.clone(),
                                );
                                self.emit_event(
                                    &run_id,
                                    step as u32,
                                    EventKind::TaintUpdated,
                                    serde_json::json!({
                                        "overall": taint_state.overall_str(),
                                        "new_spans": spans.len(),
                                        "sources": taint_state.sources_count_for_last_update()
                                    }),
                                );
                            }
                        }
                        self.gate.record(GateEvent {
                            run_id: run_id.clone(),
                            step: step as u32,
                            tool_call_id: tc.id.clone(),
                            tool: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                            decision: "allow".to_string(),
                            decision_reason: reason.clone(),
                            decision_source: source.clone(),
                            approval_id,
                            approval_key,
                            approval_mode: approval_mode_meta.clone(),
                            auto_approve_scope: auto_scope_meta.clone(),
                            approval_key_version: approval_key_version_meta.clone(),
                            tool_schema_hash_hex: tool_schema_hash_hex.clone(),
                            hooks_config_hash_hex: hooks_config_hash_hex.clone(),
                            planner_hash_hex: planner_hash_hex.clone(),
                            exec_target: decision_exec_target.clone(),
                            taint_overall: Some(taint_state.overall_str().to_string()),
                            taint_enforced,
                            escalated,
                            escalation_reason: escalation_reason.clone(),
                            result_ok: final_ok,
                            result_content: content.clone(),
                            result_input_digest: Some(input_digest),
                            result_output_digest: Some(output_digest),
                            result_input_len: Some(input_len),
                            result_output_len: Some(output_len),
                        });
                        observed_tool_decisions.push(ToolDecisionRecord {
                            step: step as u32,
                            tool_call_id: tc.id.clone(),
                            tool: tc.name.clone(),
                            decision: "allow".to_string(),
                            reason: reason.clone(),
                            source: source.clone(),
                            taint_overall: Some(taint_state.overall_str().to_string()),
                            taint_enforced,
                            escalated,
                            escalation_reason: escalation_reason.clone(),
                        });
                        if final_ok {
                            failed_repeat_counts.remove(&repeat_key);
                            if tc.name == "apply_patch" {
                                invalid_patch_format_attempts.remove(&repeat_key);
                            }
                            if tc.name == "apply_patch" || tc.name == "write_file" {
                                successful_write_tool_ok_this_step = true;
                            }
                        } else {
                            let n = failed_repeat_counts.entry(repeat_key).or_insert(0);
                            *n = n.saturating_add(1);
                        }
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::ToolExecEnd,
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
                        if !final_ok
                            && tc.name == "write_file"
                            && content.contains("write_file blocked for existing file")
                        {
                            let blocked_path = normalized_tool_path_from_args(tc)
                                .unwrap_or_else(|| "<unknown>".to_string());
                            let reason = format!(
                                "implementation guard: write_file on '{blocked_path}' requires prior read_file on the same path"
                            );
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::Error,
                                serde_json::json!({
                                    "error": reason,
                                    "source": "implementation_integrity_guard",
                                    "failure_class": "E_RUNTIME_WRITEFILE_EXISTING_BLOCKED",
                                    "path": blocked_path
                                }),
                            );
                            return self.finalize_planner_error_with_end(
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
                        if self.inject_post_tool_operator_messages(
                            &run_id,
                            step as u32,
                            &mut messages,
                        ) {
                            continue 'agent_steps;
                        }
                    }
                    GateDecision::Deny {
                        reason,
                        approval_key,
                        source,
                        taint_enforced,
                        escalated,
                        escalation_reason,
                    } => {
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::ToolDecision,
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
                            step: step as u32,
                            tool_call_id: tc.id.clone(),
                            tool: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                            decision: "deny".to_string(),
                            decision_reason: Some(reason.clone()),
                            decision_source: source.clone(),
                            approval_id: None,
                            approval_key,
                            approval_mode: approval_mode_meta.clone(),
                            auto_approve_scope: auto_scope_meta.clone(),
                            approval_key_version: approval_key_version_meta.clone(),
                            tool_schema_hash_hex: tool_schema_hash_hex.clone(),
                            hooks_config_hash_hex: hooks_config_hash_hex.clone(),
                            planner_hash_hex: planner_hash_hex.clone(),
                            exec_target: decision_exec_target.clone(),
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
                            step: step as u32,
                            tool_call_id: tc.id.clone(),
                            tool: tc.name.clone(),
                            decision: "deny".to_string(),
                            reason: Some(reason.clone()),
                            source: source.clone(),
                            taint_overall: Some(taint_state.overall_str().to_string()),
                            taint_enforced,
                            escalated,
                            escalation_reason: escalation_reason.clone(),
                        });
                        return self.finalize_denied_with_end(
                            step as u32,
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
                            &total_token_usage,
                            &taint_state,
                        );
                    }
                    GateDecision::RequireApproval {
                        reason,
                        approval_id,
                        approval_key,
                        source,
                        taint_enforced,
                        escalated,
                        escalation_reason,
                    } => {
                        if let Some(err) = &invalid_args_error {
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::ToolDecision,
                                serde_json::json!({
                                    "tool_call_id": tc.id,
                                    "name": tc.name,
                                    "decision": "allow",
                                "reason": format!("invalid args bypassed approval: {err}"),
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
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::ToolExecTarget,
                                serde_json::json!({
                                    "tool_call_id": tc.id,
                                    "name": tc.name,
                                    "exec_target": if tc.name.starts_with("mcp.") { "host" } else {
                                        match self.tool_rt.exec_target_kind {
                                            crate::target::ExecTargetKind::Host => "host",
                                            crate::target::ExecTargetKind::Docker => "docker",
                                        }
                                    }
                                }),
                            );
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::ToolExecStart,
                                serde_json::json!({"tool_call_id": tc.id, "name": tc.name, "side_effects": tool_side_effects(&tc.name)}),
                            );
                            let tool_msg = make_invalid_args_tool_message(
                                tc,
                                err,
                                self.tool_rt.exec_target_kind,
                            );
                            let content = tool_msg.content.clone().unwrap_or_default();
                            self.gate.record(GateEvent {
                                run_id: run_id.clone(),
                                step: step as u32,
                                tool_call_id: tc.id.clone(),
                                tool: tc.name.clone(),
                                arguments: tc.arguments.clone(),
                                decision: "allow".to_string(),
                                decision_reason: Some(format!(
                                    "invalid args bypassed approval: {err}"
                                )),
                                decision_source: source.clone(),
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
                                taint_enforced,
                                escalated,
                                escalation_reason: escalation_reason.clone(),
                                result_ok: false,
                                result_content: content.clone(),
                                result_input_digest: None,
                                result_output_digest: None,
                                result_input_len: None,
                                result_output_len: None,
                            });
                            observed_tool_decisions.push(ToolDecisionRecord {
                                step: step as u32,
                                tool_call_id: tc.id.clone(),
                                tool: tc.name.clone(),
                                decision: "allow".to_string(),
                                reason: Some(format!("invalid args bypassed approval: {err}")),
                                source: source.clone(),
                                taint_overall: Some(taint_state.overall_str().to_string()),
                                taint_enforced,
                                escalated,
                                escalation_reason: escalation_reason.clone(),
                            });
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::ToolExecEnd,
                                serde_json::json!({
                                    "tool_call_id": tc.id,
                                    "name": tc.name,
                                    "ok": false,
                                    "truncated": false
                                }),
                            );
                            messages.push(tool_msg);
                            if self.inject_post_tool_operator_messages(
                                &run_id,
                                step as u32,
                                &mut messages,
                            ) {
                                continue 'agent_steps;
                            }
                            continue;
                        }
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::ToolDecision,
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
                            step: step as u32,
                            tool_call_id: tc.id.clone(),
                            tool: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                            decision: "require_approval".to_string(),
                            decision_reason: Some(reason.clone()),
                            decision_source: source.clone(),
                            approval_id: Some(approval_id.clone()),
                            approval_key,
                            approval_mode: approval_mode_meta.clone(),
                            auto_approve_scope: auto_scope_meta.clone(),
                            approval_key_version: approval_key_version_meta.clone(),
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
                            step: step as u32,
                            tool_call_id: tc.id.clone(),
                            tool: tc.name.clone(),
                            decision: "require_approval".to_string(),
                            reason: Some(reason.clone()),
                            source: source.clone(),
                            taint_overall: Some(taint_state.overall_str().to_string()),
                            taint_enforced,
                            escalated,
                            escalation_reason: escalation_reason.clone(),
                        });
                        return self.finalize_approval_required_with_end(
                            step as u32,
                            run_id,
                            started_at,
                            self.approval_required_output_message(
                                &approval_id,
                                &reason,
                                source.as_deref(),
                                escalated,
                                &taint_state,
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
            }

            if enforce_implementation_integrity_guard && successful_write_tool_ok_this_step {
                let post_write_verify_timeout_ms = self.effective_post_write_verify_timeout_ms();
                let pending_post_write_paths =
                    pending_post_write_verification_paths(&observed_tool_executions);
                let verified_paths = pending_post_write_paths.iter().cloned().collect::<Vec<_>>();
                for path in pending_post_write_paths {
                    self.emit_event(
                        &run_id,
                        step as u32,
                        EventKind::PostWriteVerifyStart,
                        serde_json::json!({
                            "name": "read_file",
                            "path": path.clone(),
                            "source": "runtime_post_write_verify",
                            "timeout_ms": post_write_verify_timeout_ms
                        }),
                    );
                    let verify_started = Instant::now();
                    let verify = match tokio::time::timeout(
                        Duration::from_millis(post_write_verify_timeout_ms),
                        self.tool_rt.exec_target.read_file(crate::target::ReadReq {
                            workdir: self.tool_rt.workdir.clone(),
                            path: path.clone(),
                            max_read_bytes: self.tool_rt.max_read_bytes,
                        }),
                    )
                    .await
                    {
                        Ok(result) => result,
                        Err(_) => {
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::PostWriteVerifyEnd,
                                serde_json::json!({
                                    "name": "read_file",
                                    "path": path.clone(),
                                    "ok": false,
                                    "status": "timeout",
                                    "source": "runtime_post_write_verify",
                                    "failure_class": "E_RUNTIME_POST_WRITE_VERIFY_TIMEOUT",
                                    "elapsed_ms": verify_started.elapsed().as_millis() as u64,
                                    "timeout_ms": post_write_verify_timeout_ms
                                }),
                            );
                            let reason = format!(
                                "implementation guard: runtime post-write verification timed out on read_file for '{path}' after {}ms",
                                post_write_verify_timeout_ms
                            );
                            self.emit_event(
                                &run_id,
                                step as u32,
                                EventKind::Error,
                                serde_json::json!({
                                    "error": reason,
                                    "source": "implementation_integrity_guard",
                                    "failure_class": "E_RUNTIME_POST_WRITE_VERIFY_TIMEOUT",
                                    "path": path.clone(),
                                    "timeout_ms": post_write_verify_timeout_ms
                                }),
                            );
                            return self.finalize_planner_error_with_end(
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
                    };
                    self.emit_event(
                        &run_id,
                        step as u32,
                        EventKind::PostWriteVerifyEnd,
                        serde_json::json!({
                            "name": "read_file",
                            "path": path.clone(),
                            "ok": verify.ok,
                            "status": if verify.ok { "ok" } else { "failed" },
                            "source": "runtime_post_write_verify",
                            "failure_class": if verify.ok { serde_json::Value::Null } else { serde_json::Value::String("E_RUNTIME_POST_WRITE_VERIFY_FAILED".to_string()) },
                            "elapsed_ms": verify_started.elapsed().as_millis() as u64
                        }),
                    );
                    observed_tool_executions.push(ToolExecutionRecord {
                        name: "read_file".to_string(),
                        path: Some(path.clone()),
                        ok: verify.ok,
                    });
                    if !verify.ok {
                        let reason = format!(
                            "implementation guard: runtime post-write verification failed read_file on '{path}': {}",
                            verify.content
                        );
                        self.emit_event(
                            &run_id,
                            step as u32,
                            EventKind::Error,
                            serde_json::json!({
                                "error": reason,
                                "source": "implementation_integrity_guard"
                            }),
                        );
                        return self.finalize_planner_error_with_end(
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
                let final_output = if verified_paths.is_empty() {
                    "Applied requested file changes and verified.".to_string()
                } else {
                    format!(
                        "Applied requested file changes and verified: {}.",
                        verified_paths.join(", ")
                    )
                };
                if let Some(reason) = implementation_integrity_violation_with_tool_executions(
                    user_prompt,
                    &final_output,
                    &observed_tool_calls,
                    &observed_tool_executions,
                    enforce_implementation_integrity_guard,
                ) {
                    self.emit_event(
                        &run_id,
                        step as u32,
                        EventKind::Error,
                        serde_json::json!({
                            "error": reason,
                            "source": "implementation_integrity_guard"
                        }),
                    );
                    return self.finalize_planner_error_with_end(
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
                return self.finalize_ok_with_end(
                    step as u32,
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
                    &total_token_usage,
                    &taint_state,
                );
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
