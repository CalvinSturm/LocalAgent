use crate::agent_budget::ToolCallBudgetUsage;
use crate::agent_impl_guard::{prompt_requires_tool_only, ToolExecutionRecord};
use crate::agent_output_sanitize::sanitize_user_visible_output as sanitize_user_visible_output_impl;
#[cfg(test)]
use crate::agent_tool_exec::{classify_tool_failure, tool_result_has_error};
use crate::agent_utils::provider_name;
use crate::compaction::{context_size_chars, maybe_compact, CompactionReport, CompactionSettings};
use crate::events::{EventKind, EventSink};
use crate::gate::{GateContext, GateDecision, ToolGate};
use crate::hooks::protocol::{HookInvocationReport, PreModelCompactionPayload, PreModelPayload};
use crate::hooks::runner::{make_pre_model_input, HookManager};
use crate::mcp::registry::McpRegistry;
use crate::operator_queue::{PendingMessageQueue, QueueLimits, QueueSubmitRequest};
use crate::providers::ModelProvider;
use crate::taint::{TaintMode, TaintState, TaintToggle};
use crate::tools::ToolRuntime;
use crate::trust::policy::Policy;
use crate::types::{Message, Role, TokenUsage, ToolCall, ToolDef};
use serde_json::json;
use uuid::Uuid;

mod agent_types;
mod budget_guard;
pub(crate) mod completion_policy;
mod gate_paths;
pub(crate) mod interrupts;
mod mcp_drift;
mod model_io;
mod operator_queue;
mod phase_transitions;
mod planner_phase;
mod response_guards;
mod response_normalization;
mod run_control;
mod run_events;
mod run_finalize;
mod run_setup;
mod runtime_effects;
mod runtime_completion;
pub mod task_contract;
mod timeouts;
pub mod tool_facts;
mod tool_helpers;
pub use agent_types::{
    AgentExitReason, AgentOutcome, AgentTaintRecord, McpPinEnforcementMode, McpRuntimeTraceEntry,
    PlanStepConstraint, PlanToolEnforcementMode, PolicyLoadedInfo, ToolCallBudget,
    ToolDecisionRecord,
};
pub use tool_facts::{ToolFactEnvelopeV1, ToolFactV1};
#[allow(unused_imports)]
pub use task_contract::{
    AllowedToolsSemantics, CompletionPolicyV1, ContractValueSource, FinalAnswerMode,
    RetryPolicyV1,
    TaskContractProvenanceV1, TaskContractV1, ValidationRequirement, WriteRequirement,
};
#[allow(unused_imports)]
pub(crate) use tool_facts::{
    implementation_integrity_violation_from_facts, pending_post_write_verification_paths_from_facts,
    read_before_edit_violation_from_facts, required_validation_command_satisfied_from_facts,
    required_validation_failure_needs_repair_from_facts, tool_fact_envelopes_from_facts,
    tool_facts_from_calls_and_executions, tool_facts_from_transcript,
};
pub(crate) use completion_policy::{
    approval_boundary_transition_decision, exact_final_answer_boundary_transition_decision,
    operator_boundary_transition_decision, required_validation_boundary_transition_decision,
};

pub(crate) use agent_types::WorkerStepStatus;
use gate_paths::{AllowToolCallDecision, GateNonAllowDecision, PlanConstraintDecision};
use mcp_drift::McpDriftDecision;
use phase_transitions::{
    apply_runtime_completion_action_to_checkpoint, apply_verified_write_follow_on,
    refresh_phase_state_from_tool_facts,
};
use planner_phase::{evaluate_planner_response, PlannerResponseDecision};
use response_guards::{decide_post_response_phase_guard, decide_required_validation_phase_response};
use response_normalization::{normalize_assistant_response, AssistantResponseNormalization};
use runtime_effects::{
    apply_post_response_guard_decision, apply_required_validation_guard_decision,
    apply_verified_write_follow_on_update, completion_blocked_effect_from_post_tool_refresh,
    GuardEffect,
};
use run_events::apply_usage_totals;
#[cfg(test)]
pub(crate) use runtime_completion::RuntimeCompletionDecision;
use runtime_completion::{
    runtime_completion_decision, RuntimeCompletionInputs,
};
use tool_helpers::injected_messages_enforce_implementation_integrity_guard;
use tool_helpers::{FailedRepeatGuardDecision, MalformedToolCallDecision};
pub fn sanitize_user_visible_output(raw: &str) -> String {
    sanitize_user_visible_output_impl(raw)
}
#[cfg(test)]
fn contains_tool_wrapper_markers(text: &str) -> bool {
    crate::agent_tool_exec::contains_tool_wrapper_markers(text)
}
const MAX_SCHEMA_REPAIR_ATTEMPTS: u32 = 2;
const MAX_FAILED_REPEAT_PER_KEY: u32 = 3;
const MAX_FAILED_REPEAT_PER_TOOL_NAME: u32 = 5;
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

enum PhaseLoopControl {
    Proceed,
    ContinueStep,
    ContinueAgentStep,
}

enum PlannerEnvelopeControl {
    Proceed,
    ContinueStep,
}

enum ToolLoopControl {
    Proceed,
    RestartAgentStep,
}

enum PhaseStepDispatch {
    StepComplete,
    ContinueStep,
    ContinueAgentStep,
}

struct NormalizedTurnState {
    assistant: Message,
    request_context_chars: usize,
    has_actionable_tool_calls: bool,
    tool_calls: Vec<ToolCall>,
}

struct GeneratedTurnResponse {
    request_context_chars: usize,
    resp: crate::types::GenerateResponse,
}

impl<P: ModelProvider> Agent<P> {
    fn initial_runtime_checkpoint(&self, user_prompt: &str) -> crate::agent_runtime::state::RunCheckpointV1 {
        let execution_tier = match self.tool_rt.exec_target_kind {
            crate::target::ExecTargetKind::Docker => {
                crate::agent_runtime::state::ExecutionTier::DockerIsolated
            }
            crate::target::ExecTargetKind::Host => {
                if self.tool_rt.allow_shell {
                    crate::agent_runtime::state::ExecutionTier::ScopedHostShell
                } else if self.tool_rt.allow_write {
                    crate::agent_runtime::state::ExecutionTier::ScopedHostWrite
                } else {
                    crate::agent_runtime::state::ExecutionTier::ReadOnlyHost
                }
            }
        };
        crate::agent_runtime::state::RunCheckpointV1 {
            schema_version: "openagent.runtime_state_checkpoint.v1".to_string(),
            phase: crate::agent_runtime::state::RunPhase::Executing,
            step_index: 0,
            execution_tier,
            terminal_boundary: false,
            retry_state: crate::agent_runtime::state::RetryState::default(),
            tool_protocol_state: crate::agent_runtime::state::ToolProtocolState {
                tool_only_phase_active: prompt_requires_tool_only(user_prompt),
                ..Default::default()
            },
            validation_state: crate::agent_runtime::state::ValidationState {
                required_command: crate::agent_impl_guard::prompt_required_validation_command(user_prompt)
                    .map(ToOwned::to_owned),
                satisfied: false,
                repair_mode: false,
                collecting_final_answer: false,
            },
            approval_state: crate::agent_runtime::state::ApprovalState::default(),
            active_plan_step_id: None,
            last_tool_fact_envelopes: Vec::new(),
        }
    }

    fn validation_shell_available(&self) -> bool {
        self.tool_rt.allow_shell && self.tools.iter().any(|tool| tool.name == "shell")
    }

    fn synthesize_required_validation_shell_call(
        &self,
        user_prompt: &str,
        assistant: &Message,
    ) -> Option<ToolCall> {
        let required_command =
            crate::agent_impl_guard::prompt_required_validation_command(user_prompt)?;
        let raw = assistant.content.as_deref().unwrap_or_default().trim();
        if raw.is_empty()
            || crate::agent_impl_guard::final_output_matches_required_exact_answer(user_prompt, raw)
        {
            return Some(ToolCall {
                id: format!("tc_validation_shell_{}", Uuid::new_v4().simple()),
                name: "shell".to_string(),
                arguments: json!({ "command": required_command }),
            });
        }

        if let Some(args) = synthesize_shell_args_from_validation_text(raw, required_command) {
            return Some(ToolCall {
                id: format!("tc_validation_shell_{}", Uuid::new_v4().simple()),
                name: "shell".to_string(),
                arguments: args,
            });
        }
        None
    }

    fn emit_phase_transition(
        &mut self,
        run_id: &str,
        step: u32,
        transition: &crate::agent::completion_policy::RuntimePhaseTransitionDecision,
    ) {
        self.emit_event(
            run_id,
            step,
            EventKind::PhaseExited,
            serde_json::json!({
                "phase": crate::agent::interrupts::run_phase_name(&transition.from_phase),
                "next_phase": crate::agent::interrupts::run_phase_name(&transition.to_phase)
            }),
        );
        self.emit_event(
            run_id,
            step,
            EventKind::PhaseEntered,
            serde_json::json!({
                "phase": crate::agent::interrupts::run_phase_name(&transition.to_phase)
            }),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_required_validation_phase_response(
        &mut self,
        user_prompt: &str,
        resp: &mut crate::types::GenerateResponse,
        run_id: &str,
        step: u32,
        started_at: &str,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        messages: &mut Vec<Message>,
        observed_tool_calls: &[ToolCall],
        observed_tool_decisions: &[ToolDecisionRecord],
        request_context_chars: usize,
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> Result<PhaseLoopControl, AgentOutcome> {
        if runtime_checkpoint.phase != crate::agent_runtime::state::RunPhase::Validating {
            return Ok(PhaseLoopControl::Proceed);
        }
        if !self.validation_shell_available() {
            runtime_checkpoint.phase = crate::agent_runtime::state::RunPhase::Executing;
            runtime_checkpoint.retry_state.blocked_required_validation_phase_count = 0;
            return Ok(PhaseLoopControl::Proceed);
        }
        if resp.tool_calls.is_empty() {
            if let Some(shell_call) =
                self.synthesize_required_validation_shell_call(user_prompt, &resp.assistant)
            {
                self.emit_event(
                    run_id,
                    step,
                    EventKind::StepBlocked,
                    serde_json::json!({
                        "reason": "required_validation_phase_shell_shape_repaired",
                        "tool_name": "shell"
                    }),
                );
                resp.tool_calls.push(shell_call);
                resp.assistant.content = Some(String::new());
            }
        }
        let decision = decide_required_validation_phase_response(
            user_prompt,
            self.validation_shell_available(),
            runtime_checkpoint,
            &resp.assistant,
            &resp.tool_calls,
            self.required_validation_phase_message(user_prompt),
        );
        match apply_required_validation_guard_decision(decision, &resp.assistant, messages) {
                GuardEffect::Proceed => Ok(PhaseLoopControl::Proceed),
                GuardEffect::EmitPhaseTransition(transition) => {
                    self.emit_phase_transition(run_id, step, &transition);
                    Ok(PhaseLoopControl::Proceed)
                }
                GuardEffect::ContinueStep(step_block) => {
                    self.emit_event(
                        run_id,
                        step,
                        EventKind::StepBlocked,
                        serde_json::json!({
                            "reason": step_block.reason,
                            "blocked_count": step_block.blocked_count
                        }),
                    );
                    Ok(PhaseLoopControl::ContinueStep)
                }
                GuardEffect::ContinueAgentStep(step_block) => {
                    self.emit_event(
                        run_id,
                        step,
                        EventKind::StepBlocked,
                        serde_json::json!({
                            "reason": step_block.reason,
                            "blocked_count": step_block.blocked_count
                        }),
                    );
                    Ok(PhaseLoopControl::ContinueAgentStep)
                }
                GuardEffect::PlannerError(error) => {
                    self.emit_event(
                        run_id,
                        step,
                        EventKind::StepBlocked,
                        serde_json::json!({
                            "reason": error.step_block.reason,
                            "blocked_count": error.step_block.blocked_count
                        }),
                    );
                    self.emit_event(
                        run_id,
                        step,
                        EventKind::Error,
                        serde_json::json!({
                            "error": error.reason,
                            "source": error.error_source,
                            "failure_class": error.failure_class
                        }),
                    );
                    Err(self.finalize_planner_error_with_output_with_end(
                        step,
                        run_id.to_string(),
                        started_at.to_string(),
                        error.reason,
                        messages.clone(),
                        observed_tool_calls.to_vec(),
                        observed_tool_decisions.to_vec(),
                        request_context_chars,
                        last_compaction_report.clone(),
                        hook_invocations.to_vec(),
                        provider_retry_count,
                        provider_error_count,
                        saw_token_usage,
                        total_token_usage,
                        taint_state,
                    ))
                }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_post_response_phase_guards(
        &mut self,
        user_prompt: &str,
        assistant: &Message,
        has_actionable_tool_calls: bool,
        model_signaled_finalize: bool,
        run_id: &str,
        step: u32,
        started_at: &str,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        messages: &mut Vec<Message>,
        observed_tool_calls: &[ToolCall],
        observed_tool_decisions: &[ToolDecisionRecord],
        request_context_chars: usize,
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
        tool_calls: &[ToolCall],
    ) -> Result<PhaseLoopControl, AgentOutcome> {
        let decision = decide_post_response_phase_guard(
            runtime_checkpoint,
            assistant,
            has_actionable_tool_calls,
            model_signaled_finalize,
            tool_calls,
            self.post_validation_final_answer_only_message(user_prompt),
            self.tool_only_reminder_message(),
        );
        match apply_post_response_guard_decision(decision, assistant, messages) {
                GuardEffect::Proceed => Ok(PhaseLoopControl::Proceed),
                GuardEffect::EmitPhaseTransition(transition) => {
                    self.emit_phase_transition(run_id, step, &transition);
                    Ok(PhaseLoopControl::Proceed)
                }
                GuardEffect::ContinueStep(step_block) => {
                    self.emit_event(
                        run_id,
                        step,
                        EventKind::StepBlocked,
                        serde_json::json!({
                            "reason": step_block.reason,
                            "blocked_count": step_block.blocked_count
                        }),
                    );
                    Ok(PhaseLoopControl::ContinueStep)
                }
                GuardEffect::ContinueAgentStep(step_block) => {
                    self.emit_event(
                        run_id,
                        step,
                        EventKind::StepBlocked,
                        serde_json::json!({
                            "reason": step_block.reason,
                            "blocked_count": step_block.blocked_count
                        }),
                    );
                    Ok(PhaseLoopControl::ContinueAgentStep)
                }
                GuardEffect::PlannerError(error) => {
                    self.emit_event(
                        run_id,
                        step,
                        EventKind::StepBlocked,
                        serde_json::json!({
                            "reason": error.step_block.reason,
                            "blocked_count": error.step_block.blocked_count
                        }),
                    );
                    self.emit_event(
                        run_id,
                        step,
                        EventKind::Error,
                        serde_json::json!({
                            "error": error.reason,
                            "source": error.error_source,
                            "failure_class": error.failure_class
                        }),
                    );
                    Err(self.finalize_planner_error_with_output_with_end(
                        step,
                        run_id.to_string(),
                        started_at.to_string(),
                        error.reason,
                        messages.clone(),
                        observed_tool_calls.to_vec(),
                        observed_tool_decisions.to_vec(),
                        request_context_chars,
                        last_compaction_report.clone(),
                        hook_invocations.to_vec(),
                        provider_retry_count,
                        provider_error_count,
                        saw_token_usage,
                        total_token_usage,
                        taint_state,
                    ))
                }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_planner_control_envelope(
        &mut self,
        assistant: &Message,
        run_id: &str,
        step: u32,
        started_at: &str,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        messages: &mut Vec<Message>,
        observed_tool_calls: &[ToolCall],
        observed_tool_decisions: &[ToolDecisionRecord],
        request_context_chars: usize,
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
        has_actionable_tool_calls: bool,
        model_signaled_finalize: bool,
        active_plan_step_idx: &mut usize,
        last_user_output: &mut Option<String>,
        step_retry_counts: &mut std::collections::BTreeMap<String, u32>,
    ) -> Result<PlannerEnvelopeControl, AgentOutcome> {
        let worker_step_status = self.parse_worker_step_status_if_enforced(assistant);
        match evaluate_planner_response(crate::agent::planner_phase::PlannerResponseContext {
            plan_enforcement_active: self.plan_enforcement_active(),
            has_actionable_tool_calls,
            model_signaled_finalize,
            worker_step_status: worker_step_status.as_ref(),
            blocked_control_envelope_count: runtime_checkpoint
                .tool_protocol_state
                .blocked_control_envelope_count,
            active_plan_step_idx: *active_plan_step_idx,
            plan_step_constraints: &self.plan_step_constraints,
            step_retry_counts,
        }) {
            PlannerResponseDecision::Proceed => {}
            PlannerResponseDecision::RemindControlEnvelope { blocked_count } => {
                runtime_checkpoint.tool_protocol_state.blocked_control_envelope_count =
                    blocked_count;
                self.emit_event(
                    run_id,
                    step,
                    EventKind::StepBlocked,
                    serde_json::json!({
                        "reason": "invalid_control_envelope",
                        "required_schema_version": crate::planner::STEP_RESULT_SCHEMA_VERSION,
                        "blocked_count": blocked_count
                    }),
                );
                messages.push(Message {
                    role: Role::Developer,
                    content: Some(self.control_envelope_reminder_message()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                });
                return Ok(PlannerEnvelopeControl::ContinueStep);
            }
            PlannerResponseDecision::MissingControlEnvelopeFatal { blocked_count } => {
                runtime_checkpoint.tool_protocol_state.blocked_control_envelope_count =
                    blocked_count;
                self.emit_event(
                    run_id,
                    step,
                    EventKind::StepBlocked,
                    serde_json::json!({
                        "reason": "invalid_control_envelope",
                        "required_schema_version": crate::planner::STEP_RESULT_SCHEMA_VERSION,
                        "blocked_count": blocked_count
                    }),
                );
                return Err(self.finalize_planner_error_with_end(
                    step,
                    run_id.to_string(),
                    started_at.to_string(),
                    "worker response missing control envelope for planner-enforced mode"
                        .to_string(),
                    messages.clone(),
                    observed_tool_calls.to_vec(),
                    observed_tool_decisions.to_vec(),
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.to_vec(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                ));
            }
            PlannerResponseDecision::StepDone {
                completed_step_id,
                next_step_id,
                next_active_plan_step_idx,
                user_output,
            } => {
                runtime_checkpoint.tool_protocol_state.blocked_control_envelope_count = 0;
                runtime_checkpoint.retry_state.blocked_runtime_completion_count = 0;
                if let Some(user_output) = user_output {
                    *last_user_output = Some(user_output);
                }
                self.emit_event(
                    run_id,
                    step,
                    EventKind::StepVerified,
                    serde_json::json!({
                        "step_id": completed_step_id,
                        "next_step_id": next_step_id,
                        "status": "done"
                    }),
                );
                step_retry_counts.remove(&completed_step_id);
                *active_plan_step_idx = next_active_plan_step_idx;
            }
            PlannerResponseDecision::InvalidDoneTransition {
                step_id,
                expected_step_id,
            } => {
                runtime_checkpoint.tool_protocol_state.blocked_control_envelope_count = 0;
                self.emit_event(
                    run_id,
                    step,
                    EventKind::StepBlocked,
                    serde_json::json!({
                        "step_id": step_id,
                        "expected_step_id": expected_step_id,
                        "reason": "invalid_done_transition"
                    }),
                );
                return Err(self.finalize_planner_error_with_end(
                    step,
                    run_id.to_string(),
                    started_at.to_string(),
                    format!(
                        "invalid step completion transition: got done for {}, expected {}",
                        step_id, expected_step_id
                    ),
                    messages.clone(),
                    observed_tool_calls.to_vec(),
                    observed_tool_decisions.to_vec(),
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.to_vec(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                ));
            }
            PlannerResponseDecision::InvalidNextStepId {
                step_id,
                next_step_id,
            } => {
                runtime_checkpoint.tool_protocol_state.blocked_control_envelope_count = 0;
                self.emit_event(
                    run_id,
                    step,
                    EventKind::StepBlocked,
                    serde_json::json!({
                        "step_id": step_id,
                        "next_step_id": next_step_id,
                        "reason": "invalid_next_step_id"
                    }),
                );
                return Err(self.finalize_planner_error_with_end(
                    step,
                    run_id.to_string(),
                    started_at.to_string(),
                    format!("invalid next_step_id in worker status: {}", next_step_id),
                    messages.clone(),
                    observed_tool_calls.to_vec(),
                    observed_tool_decisions.to_vec(),
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.to_vec(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                ));
            }
            PlannerResponseDecision::StepRetry {
                step_id,
                retry_count,
                user_output,
            } => {
                runtime_checkpoint.tool_protocol_state.blocked_control_envelope_count = 0;
                if let Some(user_output) = user_output {
                    *last_user_output = Some(user_output);
                }
                step_retry_counts.insert(step_id, retry_count);
            }
            PlannerResponseDecision::RetryLimitExceeded {
                step_id,
                retry_count,
            } => {
                runtime_checkpoint.tool_protocol_state.blocked_control_envelope_count = 0;
                self.emit_event(
                    run_id,
                    step,
                    EventKind::StepBlocked,
                    serde_json::json!({
                        "step_id": step_id,
                        "reason": "retry_limit_exceeded",
                        "retry_count": retry_count
                    }),
                );
                return Err(self.finalize_planner_error_with_end(
                    step,
                    run_id.to_string(),
                    started_at.to_string(),
                    format!("step {} exceeded retry transition limit", step_id),
                    messages.clone(),
                    observed_tool_calls.to_vec(),
                    observed_tool_decisions.to_vec(),
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.to_vec(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                ));
            }
            PlannerResponseDecision::InvalidRetryTransition {
                step_id,
                expected_step_id,
            } => {
                runtime_checkpoint.tool_protocol_state.blocked_control_envelope_count = 0;
                self.emit_event(
                    run_id,
                    step,
                    EventKind::StepBlocked,
                    serde_json::json!({
                        "step_id": step_id,
                        "expected_step_id": expected_step_id,
                        "reason": "invalid_retry_transition"
                    }),
                );
                return Err(self.finalize_planner_error_with_end(
                    step,
                    run_id.to_string(),
                    started_at.to_string(),
                    format!(
                        "invalid retry transition: got retry for {}, expected {}",
                        step_id, expected_step_id
                    ),
                    messages.clone(),
                    observed_tool_calls.to_vec(),
                    observed_tool_decisions.to_vec(),
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.to_vec(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                ));
            }
            PlannerResponseDecision::ReplanRequested { step_id, status } => {
                runtime_checkpoint.tool_protocol_state.blocked_control_envelope_count = 0;
                self.emit_event(
                    run_id,
                    step,
                    EventKind::StepReplanned,
                    serde_json::json!({
                        "step_id": step_id,
                        "status": status
                    }),
                );
                return Err(self.finalize_planner_error_with_end(
                    step,
                    run_id.to_string(),
                    started_at.to_string(),
                    format!("worker requested {} transition for step {}", status, step_id),
                    messages.clone(),
                    observed_tool_calls.to_vec(),
                    observed_tool_decisions.to_vec(),
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.to_vec(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                ));
            }
            PlannerResponseDecision::FailRequested { step_id, status } => {
                runtime_checkpoint.tool_protocol_state.blocked_control_envelope_count = 0;
                self.emit_event(
                    run_id,
                    step,
                    EventKind::StepBlocked,
                    serde_json::json!({
                        "step_id": step_id,
                        "reason": "worker_fail_transition"
                    }),
                );
                return Err(self.finalize_planner_error_with_end(
                    step,
                    run_id.to_string(),
                    started_at.to_string(),
                    format!("worker requested {} transition for step {}", status, step_id),
                    messages.clone(),
                    observed_tool_calls.to_vec(),
                    observed_tool_decisions.to_vec(),
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.to_vec(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                ));
            }
        }
        Ok(PlannerEnvelopeControl::Proceed)
    }

    #[allow(clippy::too_many_arguments)]
    async fn process_tool_calls_for_response(
        &mut self,
        tool_calls: &[ToolCall],
        run_id: &str,
        step: u32,
        started_at: &str,
        active_plan_step_idx: usize,
        request_context_chars: usize,
        expected_mcp_catalog_hash_hex: Option<&String>,
        expected_mcp_docs_hash_hex: Option<&String>,
        messages: &mut Vec<Message>,
        observed_tool_calls: &mut Vec<ToolCall>,
        observed_tool_executions: &mut Vec<ToolExecutionRecord>,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
        hook_invocations: &mut Vec<HookInvocationReport>,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        malformed_tool_call_attempts: &mut u32,
        invalid_patch_format_attempts: &mut std::collections::BTreeMap<String, u32>,
        schema_repair_attempts: &mut std::collections::BTreeMap<String, u32>,
        tool_budget_usage: &mut ToolCallBudgetUsage,
        last_compaction_report: &Option<CompactionReport>,
        provider_retry_count: u32,
        provider_error_count: u32,
        saw_token_usage: bool,
        total_token_usage: &TokenUsage,
        taint_state: &mut TaintState,
        successful_write_tool_ok_this_step: &mut bool,
    ) -> Result<ToolLoopControl, AgentOutcome> {
        for tc in tool_calls {
            self.record_detected_tool_call(run_id, step, tc, observed_tool_calls);
            match self
                .check_mcp_drift_for_tool_call(
                    run_id.to_string(),
                    step,
                    tc,
                    expected_mcp_catalog_hash_hex,
                    expected_mcp_docs_hash_hex,
                    started_at.to_string(),
                    messages.clone(),
                    observed_tool_calls.clone(),
                    observed_tool_decisions,
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
                McpDriftDecision::Continue => {}
                McpDriftDecision::Finalize(outcome) => return Err(*outcome),
            }
            let planning_ctx =
                self.build_tool_call_planning_context(active_plan_step_idx, tc, failed_repeat_counts);
            match self.handle_failed_repeat_guard(
                run_id.to_string(),
                step,
                tc,
                planning_ctx.failed_repeat_count,
                planning_ctx.failed_repeat_name_count,
                &planning_ctx.repeat_key,
                started_at.to_string(),
                messages,
                failed_repeat_counts,
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
            ) {
                FailedRepeatGuardDecision::Continue => {}
                FailedRepeatGuardDecision::RestartAgentStep => {
                    return Ok(ToolLoopControl::RestartAgentStep);
                }
                FailedRepeatGuardDecision::Finalize(outcome) => return Err(*outcome),
            }
            let invalid_args_error = match self.handle_malformed_tool_call(
                run_id.to_string(),
                step,
                tc,
                planning_ctx.plan_tool_allowed,
                malformed_tool_call_attempts,
                schema_repair_attempts,
                messages,
                started_at.to_string(),
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
            ) {
                MalformedToolCallDecision::ContinueToolLoop { invalid_args_error } => {
                    invalid_args_error
                }
                MalformedToolCallDecision::RestartAgentStep => {
                    return Ok(ToolLoopControl::RestartAgentStep);
                }
                MalformedToolCallDecision::Finalize(outcome) => return Err(*outcome),
            };
            let (
                approval_mode_meta,
                auto_scope_meta,
                approval_key_version_meta,
                tool_schema_hash_hex,
                hooks_config_hash_hex,
                planner_hash_hex,
                decision_exec_target,
            ) = self.gate_decision_metadata_for_tool(tc, taint_state);
            if !planning_ctx.plan_tool_allowed {
                match self.handle_plan_constraint_deny(
                    run_id.to_string(),
                    step,
                    tc,
                    planning_ctx.plan_step_id,
                    active_plan_step_idx,
                    planning_ctx.plan_allowed_tools,
                    approval_mode_meta.clone(),
                    auto_scope_meta.clone(),
                    approval_key_version_meta.clone(),
                    tool_schema_hash_hex.clone(),
                    hooks_config_hash_hex.clone(),
                    planner_hash_hex.clone(),
                    decision_exec_target.clone(),
                    started_at.to_string(),
                    messages,
                    observed_tool_calls.clone(),
                    observed_tool_decisions,
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.clone(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                ) {
                    PlanConstraintDecision::Continue => {}
                    PlanConstraintDecision::ContinueToolLoop => continue,
                    PlanConstraintDecision::RestartAgentStep => {
                        return Ok(ToolLoopControl::RestartAgentStep);
                    }
                    PlanConstraintDecision::Finalize(outcome) => return Err(*outcome),
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
                            run_id.to_string(),
                            step,
                            tc,
                            approval_id,
                            approval_key,
                            reason,
                            source,
                            taint_enforced,
                            escalated,
                            escalation_reason,
                            invalid_args_error.clone(),
                            planning_ctx.plan_tool_allowed,
                            &planning_ctx.repeat_key,
                            approval_mode_meta.clone(),
                            auto_scope_meta.clone(),
                            approval_key_version_meta.clone(),
                            tool_schema_hash_hex.clone(),
                            hooks_config_hash_hex.clone(),
                            planner_hash_hex.clone(),
                            decision_exec_target.clone(),
                            started_at.to_string(),
                            messages,
                            observed_tool_calls.clone(),
                            observed_tool_decisions,
                            observed_tool_executions,
                            request_context_chars,
                            last_compaction_report.clone(),
                            hook_invocations,
                            provider_retry_count,
                            provider_error_count,
                            saw_token_usage,
                            total_token_usage,
                            taint_state,
                            tool_budget_usage,
                            failed_repeat_counts,
                            invalid_patch_format_attempts,
                            schema_repair_attempts,
                            successful_write_tool_ok_this_step,
                        )
                        .await
                    {
                        AllowToolCallDecision::Continue => {}
                        AllowToolCallDecision::RestartAgentStep => {
                            return Ok(ToolLoopControl::RestartAgentStep);
                        }
                        AllowToolCallDecision::Finalize(outcome) => return Err(*outcome),
                    }
                }
                gate_decision => match self.handle_non_allow_gate_decision(
                    run_id.to_string(),
                    step,
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
                    started_at.to_string(),
                    messages,
                    observed_tool_calls.clone(),
                    observed_tool_decisions,
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.clone(),
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                ) {
                    GateNonAllowDecision::ContinueToolLoop => continue,
                    GateNonAllowDecision::RestartAgentStep => {
                        return Ok(ToolLoopControl::RestartAgentStep);
                    }
                    GateNonAllowDecision::Finalize(outcome) => return Err(*outcome),
                },
            }
        }
        Ok(ToolLoopControl::Proceed)
    }

    #[allow(clippy::too_many_arguments)]
    async fn dispatch_runtime_phase_step(
        &mut self,
        user_prompt: &str,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &mut Vec<Message>,
        observed_tool_calls: &mut Vec<ToolCall>,
        observed_tool_executions: &mut Vec<ToolExecutionRecord>,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &mut Vec<HookInvocationReport>,
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
        saw_token_usage: &mut bool,
        total_token_usage: &mut TokenUsage,
        taint_state: &mut TaintState,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        active_plan_step_idx: &mut usize,
        last_user_output: &mut Option<String>,
        step_retry_counts: &mut std::collections::BTreeMap<String, u32>,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        malformed_tool_call_attempts: &mut u32,
        invalid_patch_format_attempts: &mut std::collections::BTreeMap<String, u32>,
        schema_repair_attempts: &mut std::collections::BTreeMap<String, u32>,
        tool_budget_usage: &mut ToolCallBudgetUsage,
        enforce_implementation_integrity_guard: bool,
        expected_mcp_catalog_hash_hex: Option<&String>,
        expected_mcp_docs_hash_hex: Option<&String>,
        allowed_tool_names: &std::collections::BTreeSet<String>,
        tools_sorted: Vec<ToolDef>,
    ) -> Result<PhaseStepDispatch, AgentOutcome> {
        use crate::agent_runtime::state::RunPhase;

        match runtime_checkpoint.phase {
            RunPhase::Executing => {
                self.run_executing_phase_step(
                    user_prompt,
                    run_id,
                    step,
                    started_at,
                    messages,
                    observed_tool_calls,
                    observed_tool_executions,
                    observed_tool_decisions,
                    last_compaction_report,
                    hook_invocations,
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                    runtime_checkpoint,
                    active_plan_step_idx,
                    last_user_output,
                    step_retry_counts,
                    failed_repeat_counts,
                    malformed_tool_call_attempts,
                    invalid_patch_format_attempts,
                    schema_repair_attempts,
                    tool_budget_usage,
                    enforce_implementation_integrity_guard,
                    expected_mcp_catalog_hash_hex,
                    expected_mcp_docs_hash_hex,
                    allowed_tool_names,
                    tools_sorted,
                )
                .await
            }
            RunPhase::Validating => {
                self.run_validating_phase_step(
                    user_prompt,
                    run_id,
                    step,
                    started_at,
                    messages,
                    observed_tool_calls,
                    observed_tool_executions,
                    observed_tool_decisions,
                    last_compaction_report,
                    hook_invocations,
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                    runtime_checkpoint,
                    active_plan_step_idx,
                    last_user_output,
                    step_retry_counts,
                    failed_repeat_counts,
                    malformed_tool_call_attempts,
                    invalid_patch_format_attempts,
                    schema_repair_attempts,
                    tool_budget_usage,
                    enforce_implementation_integrity_guard,
                    expected_mcp_catalog_hash_hex,
                    expected_mcp_docs_hash_hex,
                    allowed_tool_names,
                    tools_sorted,
                )
                .await
            }
            RunPhase::VerifyingChanges => {
                self.run_verifying_changes_phase_step(
                    user_prompt,
                    run_id,
                    step,
                    started_at,
                    messages,
                    observed_tool_calls,
                    observed_tool_executions,
                    observed_tool_decisions,
                    last_compaction_report,
                    hook_invocations,
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                    runtime_checkpoint,
                    active_plan_step_idx,
                    last_user_output,
                    step_retry_counts,
                    failed_repeat_counts,
                    malformed_tool_call_attempts,
                    invalid_patch_format_attempts,
                    schema_repair_attempts,
                    tool_budget_usage,
                    enforce_implementation_integrity_guard,
                    expected_mcp_catalog_hash_hex,
                    expected_mcp_docs_hash_hex,
                    allowed_tool_names,
                    tools_sorted,
                )
                .await
            }
            RunPhase::CollectingFinalAnswer => {
                self.run_collecting_final_answer_phase_step(
                    user_prompt,
                    run_id,
                    step,
                    started_at,
                    messages,
                    observed_tool_calls,
                    observed_tool_executions,
                    observed_tool_decisions,
                    last_compaction_report,
                    hook_invocations,
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                    runtime_checkpoint,
                    active_plan_step_idx,
                    last_user_output,
                    step_retry_counts,
                    failed_repeat_counts,
                    malformed_tool_call_attempts,
                    invalid_patch_format_attempts,
                    schema_repair_attempts,
                    tool_budget_usage,
                    enforce_implementation_integrity_guard,
                    expected_mcp_catalog_hash_hex,
                    expected_mcp_docs_hash_hex,
                    allowed_tool_names,
                    tools_sorted,
                )
                .await
            }
            RunPhase::WaitingForApproval | RunPhase::WaitingForOperatorInput => {
                self.run_interrupt_phase_step(
                    run_id,
                    step,
                    started_at,
                    messages,
                    observed_tool_calls,
                    observed_tool_decisions,
                    last_compaction_report,
                    hook_invocations,
                    provider_retry_count,
                    provider_error_count,
                    saw_token_usage,
                    total_token_usage,
                    taint_state,
                    &runtime_checkpoint.phase,
                )
            }
            RunPhase::Setup => self.run_setup_phase_step(
                run_id,
                step,
                started_at,
                messages,
                observed_tool_calls,
                observed_tool_decisions,
                last_compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
            ),
            RunPhase::Planning => self.run_planning_phase_step(
                run_id,
                step,
                started_at,
                messages,
                observed_tool_calls,
                observed_tool_decisions,
                last_compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
            ),
            RunPhase::Finalizing => self.run_finalizing_phase_step(
                run_id,
                step,
                started_at,
                messages,
                observed_tool_calls,
                observed_tool_decisions,
                last_compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
            ),
            RunPhase::Done | RunPhase::Failed | RunPhase::Cancelled => self.run_terminal_phase_step(
                run_id,
                step,
                started_at,
                messages,
                observed_tool_calls,
                observed_tool_decisions,
                last_compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
                &runtime_checkpoint.phase,
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_setup_phase_step(
        &mut self,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &[Message],
        observed_tool_calls: &[ToolCall],
        observed_tool_decisions: &[ToolDecisionRecord],
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: &u32,
        provider_error_count: &u32,
        saw_token_usage: &bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> Result<PhaseStepDispatch, AgentOutcome> {
        self.run_non_active_phase_step(
            run_id,
            step,
            started_at,
            messages,
            observed_tool_calls,
            observed_tool_decisions,
            last_compaction_report,
            hook_invocations,
            provider_retry_count,
            provider_error_count,
            saw_token_usage,
            total_token_usage,
            taint_state,
            crate::agent_runtime::state::RunPhase::Setup,
            "setup",
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn run_planning_phase_step(
        &mut self,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &[Message],
        observed_tool_calls: &[ToolCall],
        observed_tool_decisions: &[ToolDecisionRecord],
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: &u32,
        provider_error_count: &u32,
        saw_token_usage: &bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> Result<PhaseStepDispatch, AgentOutcome> {
        self.run_non_active_phase_step(
            run_id,
            step,
            started_at,
            messages,
            observed_tool_calls,
            observed_tool_decisions,
            last_compaction_report,
            hook_invocations,
            provider_retry_count,
            provider_error_count,
            saw_token_usage,
            total_token_usage,
            taint_state,
            crate::agent_runtime::state::RunPhase::Planning,
            "planning",
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn run_finalizing_phase_step(
        &mut self,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &[Message],
        observed_tool_calls: &[ToolCall],
        observed_tool_decisions: &[ToolDecisionRecord],
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: &u32,
        provider_error_count: &u32,
        saw_token_usage: &bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
    ) -> Result<PhaseStepDispatch, AgentOutcome> {
        self.run_non_active_phase_step(
            run_id,
            step,
            started_at,
            messages,
            observed_tool_calls,
            observed_tool_decisions,
            last_compaction_report,
            hook_invocations,
            provider_retry_count,
            provider_error_count,
            saw_token_usage,
            total_token_usage,
            taint_state,
            crate::agent_runtime::state::RunPhase::Finalizing,
            "finalizing",
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn run_terminal_phase_step(
        &mut self,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &[Message],
        observed_tool_calls: &[ToolCall],
        observed_tool_decisions: &[ToolDecisionRecord],
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: &u32,
        provider_error_count: &u32,
        saw_token_usage: &bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
        phase: &crate::agent_runtime::state::RunPhase,
    ) -> Result<PhaseStepDispatch, AgentOutcome> {
        self.run_non_active_phase_step(
            run_id,
            step,
            started_at,
            messages,
            observed_tool_calls,
            observed_tool_decisions,
            last_compaction_report,
            hook_invocations,
            provider_retry_count,
            provider_error_count,
            saw_token_usage,
            total_token_usage,
            taint_state,
            phase.clone(),
            "terminal",
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn run_interrupt_phase_step(
        &mut self,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &[Message],
        observed_tool_calls: &[ToolCall],
        observed_tool_decisions: &[ToolDecisionRecord],
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: &u32,
        provider_error_count: &u32,
        saw_token_usage: &bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
        phase: &crate::agent_runtime::state::RunPhase,
    ) -> Result<PhaseStepDispatch, AgentOutcome> {
        self.run_non_active_phase_step(
            run_id,
            step,
            started_at,
            messages,
            observed_tool_calls,
            observed_tool_decisions,
            last_compaction_report,
            hook_invocations,
            provider_retry_count,
            provider_error_count,
            saw_token_usage,
            total_token_usage,
            taint_state,
            phase.clone(),
            "interrupt",
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn run_non_active_phase_step(
        &mut self,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &[Message],
        observed_tool_calls: &[ToolCall],
        observed_tool_decisions: &[ToolDecisionRecord],
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: &u32,
        provider_error_count: &u32,
        saw_token_usage: &bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
        phase: crate::agent_runtime::state::RunPhase,
        class_name: &str,
    ) -> Result<PhaseStepDispatch, AgentOutcome> {
        Err(self.finalize_planner_error_with_output_with_end(
            step,
            run_id.to_string(),
            started_at.to_string(),
            format!(
                "runtime entered {class_name} phase {:?} inside active loop dispatcher",
                phase
            ),
            messages.to_vec(),
            observed_tool_calls.to_vec(),
            observed_tool_decisions.to_vec(),
            context_size_chars(messages),
            last_compaction_report.clone(),
            hook_invocations.to_vec(),
            *provider_retry_count,
            *provider_error_count,
            *saw_token_usage,
            total_token_usage,
            taint_state,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_executing_phase_step(
        &mut self,
        user_prompt: &str,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &mut Vec<Message>,
        observed_tool_calls: &mut Vec<ToolCall>,
        observed_tool_executions: &mut Vec<ToolExecutionRecord>,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &mut Vec<HookInvocationReport>,
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
        saw_token_usage: &mut bool,
        total_token_usage: &mut TokenUsage,
        taint_state: &mut TaintState,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        active_plan_step_idx: &mut usize,
        last_user_output: &mut Option<String>,
        step_retry_counts: &mut std::collections::BTreeMap<String, u32>,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        malformed_tool_call_attempts: &mut u32,
        invalid_patch_format_attempts: &mut std::collections::BTreeMap<String, u32>,
        schema_repair_attempts: &mut std::collections::BTreeMap<String, u32>,
        tool_budget_usage: &mut ToolCallBudgetUsage,
        enforce_implementation_integrity_guard: bool,
        expected_mcp_catalog_hash_hex: Option<&String>,
        expected_mcp_docs_hash_hex: Option<&String>,
        allowed_tool_names: &std::collections::BTreeSet<String>,
        tools_sorted: Vec<ToolDef>,
    ) -> Result<PhaseStepDispatch, AgentOutcome> {
        let normalized = match self
            .prepare_active_phase_turn(
                user_prompt,
                run_id,
                step,
                started_at,
                messages,
                observed_tool_calls,
                observed_tool_decisions,
                last_compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
                runtime_checkpoint,
                active_plan_step_idx,
                last_user_output,
                step_retry_counts,
                malformed_tool_call_attempts,
                allowed_tool_names,
                tools_sorted,
            )
            .await?
        {
            Ok(state) => state,
            Err(control) => return Ok(control),
        };
        self.run_completion_and_tool_phase(
            user_prompt,
            run_id,
            step,
            started_at,
            messages,
            observed_tool_calls,
            observed_tool_executions,
            observed_tool_decisions,
            last_compaction_report,
            hook_invocations,
            provider_retry_count,
            provider_error_count,
            saw_token_usage,
            total_token_usage,
            taint_state,
            runtime_checkpoint,
            active_plan_step_idx,
            last_user_output,
            failed_repeat_counts,
            malformed_tool_call_attempts,
            invalid_patch_format_attempts,
            schema_repair_attempts,
            tool_budget_usage,
            enforce_implementation_integrity_guard,
            expected_mcp_catalog_hash_hex,
            expected_mcp_docs_hash_hex,
            normalized,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_validating_phase_step(
        &mut self,
        user_prompt: &str,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &mut Vec<Message>,
        observed_tool_calls: &mut Vec<ToolCall>,
        observed_tool_executions: &mut Vec<ToolExecutionRecord>,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &mut Vec<HookInvocationReport>,
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
        saw_token_usage: &mut bool,
        total_token_usage: &mut TokenUsage,
        taint_state: &mut TaintState,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        active_plan_step_idx: &mut usize,
        last_user_output: &mut Option<String>,
        step_retry_counts: &mut std::collections::BTreeMap<String, u32>,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        malformed_tool_call_attempts: &mut u32,
        invalid_patch_format_attempts: &mut std::collections::BTreeMap<String, u32>,
        schema_repair_attempts: &mut std::collections::BTreeMap<String, u32>,
        tool_budget_usage: &mut ToolCallBudgetUsage,
        enforce_implementation_integrity_guard: bool,
        expected_mcp_catalog_hash_hex: Option<&String>,
        expected_mcp_docs_hash_hex: Option<&String>,
        allowed_tool_names: &std::collections::BTreeSet<String>,
        tools_sorted: Vec<ToolDef>,
    ) -> Result<PhaseStepDispatch, AgentOutcome> {
        let normalized = match self
            .prepare_active_phase_turn(
                user_prompt,
                run_id,
                step,
                started_at,
                messages,
                observed_tool_calls,
                observed_tool_decisions,
                last_compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
                runtime_checkpoint,
                active_plan_step_idx,
                last_user_output,
                step_retry_counts,
                malformed_tool_call_attempts,
                allowed_tool_names,
                tools_sorted,
            )
            .await?
        {
            Ok(state) => state,
            Err(control) => return Ok(control),
        };
        self.run_completion_and_tool_phase(
            user_prompt,
            run_id,
            step,
            started_at,
            messages,
            observed_tool_calls,
            observed_tool_executions,
            observed_tool_decisions,
            last_compaction_report,
            hook_invocations,
            provider_retry_count,
            provider_error_count,
            saw_token_usage,
            total_token_usage,
            taint_state,
            runtime_checkpoint,
            active_plan_step_idx,
            last_user_output,
            failed_repeat_counts,
            malformed_tool_call_attempts,
            invalid_patch_format_attempts,
            schema_repair_attempts,
            tool_budget_usage,
            enforce_implementation_integrity_guard,
            expected_mcp_catalog_hash_hex,
            expected_mcp_docs_hash_hex,
            normalized,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_verifying_changes_phase_step(
        &mut self,
        user_prompt: &str,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &mut Vec<Message>,
        observed_tool_calls: &mut Vec<ToolCall>,
        observed_tool_executions: &mut Vec<ToolExecutionRecord>,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &mut Vec<HookInvocationReport>,
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
        saw_token_usage: &mut bool,
        total_token_usage: &mut TokenUsage,
        taint_state: &mut TaintState,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        active_plan_step_idx: &mut usize,
        last_user_output: &mut Option<String>,
        step_retry_counts: &mut std::collections::BTreeMap<String, u32>,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        malformed_tool_call_attempts: &mut u32,
        invalid_patch_format_attempts: &mut std::collections::BTreeMap<String, u32>,
        schema_repair_attempts: &mut std::collections::BTreeMap<String, u32>,
        tool_budget_usage: &mut ToolCallBudgetUsage,
        enforce_implementation_integrity_guard: bool,
        expected_mcp_catalog_hash_hex: Option<&String>,
        expected_mcp_docs_hash_hex: Option<&String>,
        allowed_tool_names: &std::collections::BTreeSet<String>,
        tools_sorted: Vec<ToolDef>,
    ) -> Result<PhaseStepDispatch, AgentOutcome> {
        let normalized = match self
            .prepare_active_phase_turn(
                user_prompt,
                run_id,
                step,
                started_at,
                messages,
                observed_tool_calls,
                observed_tool_decisions,
                last_compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
                runtime_checkpoint,
                active_plan_step_idx,
                last_user_output,
                step_retry_counts,
                malformed_tool_call_attempts,
                allowed_tool_names,
                tools_sorted,
            )
            .await?
        {
            Ok(state) => state,
            Err(control) => return Ok(control),
        };
        self.run_completion_and_tool_phase(
            user_prompt,
            run_id,
            step,
            started_at,
            messages,
            observed_tool_calls,
            observed_tool_executions,
            observed_tool_decisions,
            last_compaction_report,
            hook_invocations,
            provider_retry_count,
            provider_error_count,
            saw_token_usage,
            total_token_usage,
            taint_state,
            runtime_checkpoint,
            active_plan_step_idx,
            last_user_output,
            failed_repeat_counts,
            malformed_tool_call_attempts,
            invalid_patch_format_attempts,
            schema_repair_attempts,
            tool_budget_usage,
            enforce_implementation_integrity_guard,
            expected_mcp_catalog_hash_hex,
            expected_mcp_docs_hash_hex,
            normalized,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_collecting_final_answer_phase_step(
        &mut self,
        user_prompt: &str,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &mut Vec<Message>,
        observed_tool_calls: &mut Vec<ToolCall>,
        observed_tool_executions: &mut Vec<ToolExecutionRecord>,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &mut Vec<HookInvocationReport>,
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
        saw_token_usage: &mut bool,
        total_token_usage: &mut TokenUsage,
        taint_state: &mut TaintState,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        active_plan_step_idx: &mut usize,
        last_user_output: &mut Option<String>,
        step_retry_counts: &mut std::collections::BTreeMap<String, u32>,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        malformed_tool_call_attempts: &mut u32,
        invalid_patch_format_attempts: &mut std::collections::BTreeMap<String, u32>,
        schema_repair_attempts: &mut std::collections::BTreeMap<String, u32>,
        tool_budget_usage: &mut ToolCallBudgetUsage,
        enforce_implementation_integrity_guard: bool,
        expected_mcp_catalog_hash_hex: Option<&String>,
        expected_mcp_docs_hash_hex: Option<&String>,
        allowed_tool_names: &std::collections::BTreeSet<String>,
        tools_sorted: Vec<ToolDef>,
    ) -> Result<PhaseStepDispatch, AgentOutcome> {
        let normalized = match self
            .prepare_active_phase_turn(
                user_prompt,
                run_id,
                step,
                started_at,
                messages,
                observed_tool_calls,
                observed_tool_decisions,
                last_compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
                runtime_checkpoint,
                active_plan_step_idx,
                last_user_output,
                step_retry_counts,
                malformed_tool_call_attempts,
                allowed_tool_names,
                tools_sorted,
            )
            .await?
        {
            Ok(state) => state,
            Err(control) => return Ok(control),
        };
        self.run_completion_and_tool_phase(
            user_prompt,
            run_id,
            step,
            started_at,
            messages,
            observed_tool_calls,
            observed_tool_executions,
            observed_tool_decisions,
            last_compaction_report,
            hook_invocations,
            provider_retry_count,
            provider_error_count,
            saw_token_usage,
            total_token_usage,
            taint_state,
            runtime_checkpoint,
            active_plan_step_idx,
            last_user_output,
            failed_repeat_counts,
            malformed_tool_call_attempts,
            invalid_patch_format_attempts,
            schema_repair_attempts,
            tool_budget_usage,
            enforce_implementation_integrity_guard,
            expected_mcp_catalog_hash_hex,
            expected_mcp_docs_hash_hex,
            normalized,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn prepare_active_phase_turn(
        &mut self,
        user_prompt: &str,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &mut Vec<Message>,
        observed_tool_calls: &[ToolCall],
        observed_tool_decisions: &[ToolDecisionRecord],
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
        saw_token_usage: &mut bool,
        total_token_usage: &mut TokenUsage,
        taint_state: &mut TaintState,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        active_plan_step_idx: &mut usize,
        last_user_output: &mut Option<String>,
        step_retry_counts: &mut std::collections::BTreeMap<String, u32>,
        malformed_tool_call_attempts: &mut u32,
        allowed_tool_names: &std::collections::BTreeSet<String>,
        tools_sorted: Vec<ToolDef>,
    ) -> Result<Result<NormalizedTurnState, PhaseStepDispatch>, AgentOutcome> {
        let GeneratedTurnResponse {
            request_context_chars,
            mut resp,
        } = self
            .generate_and_normalize_turn_response(
                run_id,
                step,
                started_at,
                messages,
                observed_tool_calls,
                observed_tool_decisions,
                last_compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
                malformed_tool_call_attempts,
                allowed_tool_names,
                tools_sorted,
            )
            .await?;
        self.process_normalized_model_response(
            user_prompt,
            run_id,
            step,
            started_at,
            &mut resp,
            messages,
            observed_tool_calls,
            observed_tool_decisions,
            last_compaction_report,
            hook_invocations,
            provider_retry_count,
            provider_error_count,
            saw_token_usage,
            total_token_usage,
            taint_state,
            runtime_checkpoint,
            active_plan_step_idx,
            last_user_output,
            step_retry_counts,
            request_context_chars,
        )
    }

    #[allow(clippy::too_many_arguments)]
    async fn generate_and_normalize_turn_response(
        &mut self,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &[Message],
        observed_tool_calls: &[ToolCall],
        observed_tool_decisions: &[ToolDecisionRecord],
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
        saw_token_usage: &mut bool,
        total_token_usage: &TokenUsage,
        taint_state: &TaintState,
        malformed_tool_call_attempts: &mut u32,
        allowed_tool_names: &std::collections::BTreeSet<String>,
        tools_sorted: Vec<ToolDef>,
    ) -> Result<GeneratedTurnResponse, AgentOutcome> {
        let req = self.build_generate_request(messages, tools_sorted);
        let request_context_chars = context_size_chars(&req.messages);
        let resp_result = self.execute_model_request(run_id, step, req).await;

        let mut resp = match resp_result {
            Ok(r) => r,
            Err(e) => {
                self.record_provider_error_events(
                    run_id,
                    step,
                    &e,
                    provider_retry_count,
                    provider_error_count,
                );
                self.emit_event(
                    run_id,
                    step,
                    EventKind::Error,
                    serde_json::json!({"error": e.to_string()}),
                );
                return Err(self.finalize_provider_error_with_end(
                    step,
                    run_id.to_string(),
                    started_at.to_string(),
                    e.to_string(),
                    messages.to_vec(),
                    observed_tool_calls.to_vec(),
                    observed_tool_decisions.to_vec(),
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.to_vec(),
                    *provider_retry_count,
                    *provider_error_count,
                    *saw_token_usage,
                    total_token_usage,
                    taint_state,
                ));
            }
        };
        match normalize_assistant_response(&mut resp, step, allowed_tool_names) {
            AssistantResponseNormalization::Ready => {}
            AssistantResponseNormalization::MalformedWrapper => {
                *malformed_tool_call_attempts = malformed_tool_call_attempts.saturating_add(1);
                if *malformed_tool_call_attempts >= 2 {
                    let reason =
                        "MODEL_TOOL_PROTOCOL_VIOLATION: empty or malformed [TOOL_CALL] envelope"
                            .to_string();
                    self.emit_event(
                        run_id,
                        step,
                        EventKind::Error,
                        serde_json::json!({
                            "error": reason,
                            "source": "tool_protocol_guard",
                            "failure_class": "E_PROTOCOL_TOOL_WRAPPER",
                            "attempt": malformed_tool_call_attempts
                        }),
                    );
                    return Err(self.finalize_planner_error_with_output_with_end(
                        step,
                        run_id.to_string(),
                        started_at.to_string(),
                        reason,
                        messages.to_vec(),
                        observed_tool_calls.to_vec(),
                        observed_tool_decisions.to_vec(),
                        request_context_chars,
                        last_compaction_report.clone(),
                        hook_invocations.to_vec(),
                        *provider_retry_count,
                        *provider_error_count,
                        *saw_token_usage,
                        total_token_usage,
                        taint_state,
                    ));
                }
            }
            AssistantResponseNormalization::MultipleToolCalls { count } => {
                let reason = format!(
                    "MODEL_TOOL_PROTOCOL_VIOLATION: multiple tool calls in a single assistant step (max 1, got {})",
                    count
                );
                self.emit_event(
                    run_id,
                    step,
                    EventKind::Error,
                    serde_json::json!({
                        "error": reason,
                        "source": "tool_protocol_guard",
                        "failure_class": "E_PROTOCOL_MULTI_TOOL",
                        "tool_calls": count
                    }),
                );
                return Err(self.finalize_planner_error_with_output_with_end(
                    step,
                    run_id.to_string(),
                    started_at.to_string(),
                    reason,
                    messages.to_vec(),
                    observed_tool_calls.to_vec(),
                    observed_tool_decisions.to_vec(),
                    request_context_chars,
                    last_compaction_report.clone(),
                    hook_invocations.to_vec(),
                    *provider_retry_count,
                    *provider_error_count,
                    *saw_token_usage,
                    total_token_usage,
                    taint_state,
                ));
            }
        };
        Ok(GeneratedTurnResponse {
            request_context_chars,
            resp,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn process_normalized_model_response(
        &mut self,
        user_prompt: &str,
        run_id: &str,
        step: u32,
        started_at: &str,
        resp: &mut crate::types::GenerateResponse,
        messages: &mut Vec<Message>,
        observed_tool_calls: &[ToolCall],
        observed_tool_decisions: &[ToolDecisionRecord],
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
        saw_token_usage: &mut bool,
        total_token_usage: &mut TokenUsage,
        taint_state: &mut TaintState,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        active_plan_step_idx: &mut usize,
        last_user_output: &mut Option<String>,
        step_retry_counts: &mut std::collections::BTreeMap<String, u32>,
        request_context_chars: usize,
    ) -> Result<Result<NormalizedTurnState, PhaseStepDispatch>, AgentOutcome> {
        if let Some(usage) = &resp.usage {
            apply_usage_totals(usage, saw_token_usage, total_token_usage);
        }
        self.emit_event(
            run_id,
            step,
            EventKind::ModelResponseEnd,
            serde_json::json!({"tool_calls": resp.tool_calls.len()}),
        );
        match self.handle_required_validation_phase_response(
            user_prompt,
            resp,
            run_id,
            step,
            started_at,
            runtime_checkpoint,
            messages,
            observed_tool_calls,
            observed_tool_decisions,
            request_context_chars,
            last_compaction_report,
            hook_invocations,
            *provider_retry_count,
            *provider_error_count,
            *saw_token_usage,
            total_token_usage,
            taint_state,
        ) {
            Ok(PhaseLoopControl::Proceed) => {}
            Ok(PhaseLoopControl::ContinueStep) => {
                return Ok(Err(PhaseStepDispatch::ContinueStep));
            }
            Ok(PhaseLoopControl::ContinueAgentStep) => {
                return Ok(Err(PhaseStepDispatch::ContinueAgentStep));
            }
            Err(outcome) => return Err(outcome),
        }
        let has_actionable_tool_calls = !resp.tool_calls.is_empty();
        let model_signaled_finalize = !has_actionable_tool_calls;
        match self.handle_post_response_phase_guards(
            user_prompt,
            &resp.assistant,
            has_actionable_tool_calls,
            model_signaled_finalize,
            run_id,
            step,
            started_at,
            runtime_checkpoint,
            messages,
            observed_tool_calls,
            observed_tool_decisions,
            request_context_chars,
            last_compaction_report,
            hook_invocations,
            *provider_retry_count,
            *provider_error_count,
            *saw_token_usage,
            total_token_usage,
            taint_state,
            &resp.tool_calls,
        ) {
            Ok(PhaseLoopControl::Proceed) => {}
            Ok(PhaseLoopControl::ContinueStep) => {
                return Ok(Err(PhaseStepDispatch::ContinueStep));
            }
            Ok(PhaseLoopControl::ContinueAgentStep) => {
                return Ok(Err(PhaseStepDispatch::ContinueAgentStep));
            }
            Err(outcome) => return Err(outcome),
        }
        if has_actionable_tool_calls {
            runtime_checkpoint.tool_protocol_state.tool_only_phase_active = false;
            runtime_checkpoint.tool_protocol_state.blocked_tool_only_count = 0;
        }
        let mut assistant = resp.assistant.clone();
        if let Some(c) = assistant.content.as_deref() {
            assistant.content = Some(sanitize_user_visible_output(c));
        }
        messages.push(assistant.clone());
        match self.handle_planner_control_envelope(
            &assistant,
            run_id,
            step,
            started_at,
            runtime_checkpoint,
            messages,
            observed_tool_calls,
            observed_tool_decisions,
            request_context_chars,
            last_compaction_report,
            hook_invocations,
            *provider_retry_count,
            *provider_error_count,
            *saw_token_usage,
            total_token_usage,
            taint_state,
            has_actionable_tool_calls,
            model_signaled_finalize,
            active_plan_step_idx,
            last_user_output,
            step_retry_counts,
        ) {
            Ok(PlannerEnvelopeControl::Proceed) => {}
            Ok(PlannerEnvelopeControl::ContinueStep) => {
                return Ok(Err(PhaseStepDispatch::ContinueStep));
            }
            Err(outcome) => return Err(outcome),
        }
        if matches!(self.taint_toggle, TaintToggle::On) {
            let idx = messages.len().saturating_sub(1);
            taint_state.mark_assistant_context_tainted(idx);
        }
        Ok(Ok(NormalizedTurnState {
            assistant,
            request_context_chars,
            has_actionable_tool_calls,
            tool_calls: resp.tool_calls.clone(),
        }))
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_verified_write_follow_on_phase(
        &mut self,
        user_prompt: &str,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &mut Vec<Message>,
        observed_tool_calls: &[ToolCall],
        observed_tool_executions: &mut Vec<ToolExecutionRecord>,
        observed_tool_decisions: &[ToolDecisionRecord],
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
        saw_token_usage: &mut bool,
        total_token_usage: &mut TokenUsage,
        taint_state: &mut TaintState,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        enforce_implementation_integrity_guard: bool,
        successful_write_tool_ok_this_step: bool,
        request_context_chars: usize,
    ) -> Result<Option<PhaseStepDispatch>, AgentOutcome> {
        if !(enforce_implementation_integrity_guard && successful_write_tool_ok_this_step) {
            return Ok(None);
        }
        let verified_write_result = self
            .finalize_verified_write_step_or_error(
                run_id.to_string(),
                step,
                started_at.to_string(),
                user_prompt,
                observed_tool_calls.to_vec(),
                observed_tool_executions,
                observed_tool_decisions.to_vec(),
                messages.clone(),
                request_context_chars,
                last_compaction_report.clone(),
                hook_invocations.to_vec(),
                *provider_retry_count,
                *provider_error_count,
                *saw_token_usage,
                total_token_usage,
                taint_state,
                enforce_implementation_integrity_guard,
                runtime_checkpoint.retry_state.post_write_guard_retry_count,
                runtime_checkpoint.retry_state.post_write_follow_on_turn_count,
            )
            .await;
        match verified_write_result {
            runtime_completion::VerifiedWriteResult::Done(outcome) => Err(*outcome),
            other => {
                let control = apply_verified_write_follow_on(user_prompt, runtime_checkpoint, &other)
                    .map(|update| apply_verified_write_follow_on_update(update, messages));
                Ok(control)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_completion_and_tool_phase(
        &mut self,
        user_prompt: &str,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &mut Vec<Message>,
        observed_tool_calls: &mut Vec<ToolCall>,
        observed_tool_executions: &mut Vec<ToolExecutionRecord>,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &mut Vec<HookInvocationReport>,
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
        saw_token_usage: &mut bool,
        total_token_usage: &mut TokenUsage,
        taint_state: &mut TaintState,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        active_plan_step_idx: &mut usize,
        last_user_output: &mut Option<String>,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        malformed_tool_call_attempts: &mut u32,
        invalid_patch_format_attempts: &mut std::collections::BTreeMap<String, u32>,
        schema_repair_attempts: &mut std::collections::BTreeMap<String, u32>,
        tool_budget_usage: &mut ToolCallBudgetUsage,
        enforce_implementation_integrity_guard: bool,
        expected_mcp_catalog_hash_hex: Option<&String>,
        expected_mcp_docs_hash_hex: Option<&String>,
        normalized: NormalizedTurnState,
    ) -> Result<PhaseStepDispatch, AgentOutcome> {
        if let Some(control) = self
            .run_runtime_completion_phase(
                user_prompt,
                run_id,
                step,
                started_at,
                messages,
                observed_tool_calls,
                observed_tool_executions,
                observed_tool_decisions,
                last_compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
                runtime_checkpoint,
                active_plan_step_idx,
                last_user_output,
                enforce_implementation_integrity_guard,
                &normalized,
            )
            .await?
        {
            return Ok(control);
        }

        let successful_write_tool_ok_this_step = match self
            .run_tool_execution_phase(
                user_prompt,
                run_id,
                step,
                started_at,
                expected_mcp_catalog_hash_hex,
                expected_mcp_docs_hash_hex,
                messages,
                observed_tool_calls,
                observed_tool_executions,
                observed_tool_decisions,
                hook_invocations,
                failed_repeat_counts,
                malformed_tool_call_attempts,
                invalid_patch_format_attempts,
                schema_repair_attempts,
                tool_budget_usage,
                last_compaction_report,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
                runtime_checkpoint,
                active_plan_step_idx,
                &normalized,
            )
            .await
        {
            Ok(Ok(successful_write_tool_ok_this_step)) => successful_write_tool_ok_this_step,
            Ok(Err(control)) => return Ok(control),
            Err(outcome) => return Err(outcome),
        };

        self.run_post_tool_phase(
            user_prompt,
            run_id,
            step,
            started_at,
            messages,
            observed_tool_calls,
            observed_tool_executions,
            observed_tool_decisions,
            last_compaction_report,
            hook_invocations,
            provider_retry_count,
            provider_error_count,
            saw_token_usage,
            total_token_usage,
            taint_state,
            runtime_checkpoint,
            enforce_implementation_integrity_guard,
            successful_write_tool_ok_this_step,
            normalized.request_context_chars,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_runtime_completion_phase(
        &mut self,
        user_prompt: &str,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &mut Vec<Message>,
        observed_tool_calls: &[ToolCall],
        observed_tool_executions: &mut Vec<ToolExecutionRecord>,
        observed_tool_decisions: &[ToolDecisionRecord],
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
        saw_token_usage: &mut bool,
        total_token_usage: &mut TokenUsage,
        taint_state: &mut TaintState,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        active_plan_step_idx: &mut usize,
        last_user_output: &mut Option<String>,
        enforce_implementation_integrity_guard: bool,
        normalized: &NormalizedTurnState,
    ) -> Result<Option<PhaseStepDispatch>, AgentOutcome> {
        let blocked_attempt_count_next = runtime_checkpoint
            .retry_state
            .blocked_runtime_completion_count
            .saturating_add(1);
        let completion_inputs = RuntimeCompletionInputs {
            has_tool_calls: normalized.has_actionable_tool_calls,
            plan_tool_enforcement: self.plan_tool_enforcement,
            active_plan_step_idx: *active_plan_step_idx,
            plan_step_constraints_len: self.plan_step_constraints.len(),
            tool_only_phase_active: runtime_checkpoint.tool_protocol_state.tool_only_phase_active,
            exact_final_answer_only_phase_active: runtime_checkpoint.phase
                == crate::agent_runtime::state::RunPhase::CollectingFinalAnswer,
            enforce_implementation_integrity_guard,
            observed_tool_calls_len: observed_tool_calls.len(),
            blocked_attempt_count_next,
        };
        let completion_decision = runtime_completion_decision(&completion_inputs);
        let completion_action = self
            .handle_runtime_completion_action(
                completion_decision,
                run_id.to_string(),
                step,
                started_at.to_string(),
                user_prompt,
                last_user_output.as_ref(),
                normalized.assistant.content.as_deref(),
                *active_plan_step_idx,
                enforce_implementation_integrity_guard,
                blocked_attempt_count_next,
                runtime_checkpoint.tool_protocol_state.operator_delivery_count,
                messages,
                observed_tool_calls.to_vec(),
                observed_tool_executions,
                observed_tool_decisions.to_vec(),
                normalized.request_context_chars,
                last_compaction_report.clone(),
                hook_invocations.to_vec(),
                *provider_retry_count,
                *provider_error_count,
                *saw_token_usage,
                total_token_usage,
                taint_state,
                runtime_checkpoint.retry_state.exact_final_answer_retry_count,
                runtime_checkpoint.retry_state.required_validation_retry_count,
            )
            .await;
        match apply_runtime_completion_action_to_checkpoint(
            user_prompt,
            completion_action,
            runtime_checkpoint,
        ) {
            Ok(PhaseLoopControl::Proceed) => Ok(None),
            Ok(PhaseLoopControl::ContinueStep) => Ok(Some(PhaseStepDispatch::ContinueStep)),
            Ok(PhaseLoopControl::ContinueAgentStep) => {
                Ok(Some(PhaseStepDispatch::ContinueAgentStep))
            }
            Err(outcome) => Err(outcome),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_tool_execution_phase(
        &mut self,
        user_prompt: &str,
        run_id: &str,
        step: u32,
        started_at: &str,
        expected_mcp_catalog_hash_hex: Option<&String>,
        expected_mcp_docs_hash_hex: Option<&String>,
        messages: &mut Vec<Message>,
        observed_tool_calls: &mut Vec<ToolCall>,
        observed_tool_executions: &mut Vec<ToolExecutionRecord>,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
        hook_invocations: &mut Vec<HookInvocationReport>,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        malformed_tool_call_attempts: &mut u32,
        invalid_patch_format_attempts: &mut std::collections::BTreeMap<String, u32>,
        schema_repair_attempts: &mut std::collections::BTreeMap<String, u32>,
        tool_budget_usage: &mut ToolCallBudgetUsage,
        last_compaction_report: &Option<CompactionReport>,
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
        saw_token_usage: &mut bool,
        total_token_usage: &mut TokenUsage,
        taint_state: &mut TaintState,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        active_plan_step_idx: &mut usize,
        normalized: &NormalizedTurnState,
    ) -> Result<Result<bool, PhaseStepDispatch>, AgentOutcome> {
        let mut successful_write_tool_ok_this_step = false;
        match self
            .process_tool_calls_for_response(
                &normalized.tool_calls,
                run_id,
                step,
                started_at,
                *active_plan_step_idx,
                normalized.request_context_chars,
                expected_mcp_catalog_hash_hex,
                expected_mcp_docs_hash_hex,
                messages,
                observed_tool_calls,
                observed_tool_executions,
                observed_tool_decisions,
                hook_invocations,
                failed_repeat_counts,
                malformed_tool_call_attempts,
                invalid_patch_format_attempts,
                schema_repair_attempts,
                tool_budget_usage,
                last_compaction_report,
                *provider_retry_count,
                *provider_error_count,
                *saw_token_usage,
                total_token_usage,
                taint_state,
                &mut successful_write_tool_ok_this_step,
            )
            .await
        {
            Ok(ToolLoopControl::Proceed) => {}
            Ok(ToolLoopControl::RestartAgentStep) => {
                return Ok(Err(PhaseStepDispatch::ContinueAgentStep));
            }
            Err(outcome) => return Err(outcome),
        }

        if let Some(effect) = completion_blocked_effect_from_post_tool_refresh(
            refresh_phase_state_from_tool_facts(
                user_prompt,
                runtime_checkpoint,
                observed_tool_calls,
                observed_tool_executions,
                successful_write_tool_ok_this_step,
            ),
        ) {
            self.emit_phase_transition(run_id, step, &effect.transition);
            self.emit_event(
                run_id,
                step,
                EventKind::CompletionBlocked,
                serde_json::json!({
                    "reason": effect.reason,
                    "next_phase": crate::agent::interrupts::run_phase_name(&effect.transition.to_phase)
                }),
            );
        }

        Ok(Ok(successful_write_tool_ok_this_step))
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_post_tool_phase(
        &mut self,
        user_prompt: &str,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &mut Vec<Message>,
        observed_tool_calls: &[ToolCall],
        observed_tool_executions: &mut Vec<ToolExecutionRecord>,
        observed_tool_decisions: &[ToolDecisionRecord],
        last_compaction_report: &Option<CompactionReport>,
        hook_invocations: &[HookInvocationReport],
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
        saw_token_usage: &mut bool,
        total_token_usage: &mut TokenUsage,
        taint_state: &mut TaintState,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        enforce_implementation_integrity_guard: bool,
        successful_write_tool_ok_this_step: bool,
        request_context_chars: usize,
    ) -> Result<PhaseStepDispatch, AgentOutcome> {
        if let Some(control) = self
            .handle_verified_write_follow_on_phase(
                user_prompt,
                run_id,
                step,
                started_at,
                messages,
                observed_tool_calls,
                observed_tool_executions,
                observed_tool_decisions,
                last_compaction_report,
                hook_invocations,
                provider_retry_count,
                provider_error_count,
                saw_token_usage,
                total_token_usage,
                taint_state,
                runtime_checkpoint,
                enforce_implementation_integrity_guard,
                successful_write_tool_ok_this_step,
                request_context_chars,
            )
            .await?
        {
            return Ok(control);
        }

        Ok(PhaseStepDispatch::StepComplete)
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_agent_step_iteration(
        &mut self,
        user_prompt: &str,
        run_id: &str,
        step: u32,
        started_at: &str,
        messages: &mut Vec<Message>,
        observed_tool_calls: &mut Vec<ToolCall>,
        observed_tool_executions: &mut Vec<ToolExecutionRecord>,
        observed_tool_decisions: &mut Vec<ToolDecisionRecord>,
        last_compaction_report: &mut Option<CompactionReport>,
        hook_invocations: &mut Vec<HookInvocationReport>,
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
        saw_token_usage: &mut bool,
        total_token_usage: &mut TokenUsage,
        taint_state: &mut TaintState,
        runtime_checkpoint: &mut crate::agent_runtime::state::RunCheckpointV1,
        active_plan_step_idx: &mut usize,
        announced_plan_step_id: &mut Option<String>,
        last_user_output: &mut Option<String>,
        step_retry_counts: &mut std::collections::BTreeMap<String, u32>,
        failed_repeat_counts: &mut std::collections::BTreeMap<String, u32>,
        malformed_tool_call_attempts: &mut u32,
        invalid_patch_format_attempts: &mut std::collections::BTreeMap<String, u32>,
        schema_repair_attempts: &mut std::collections::BTreeMap<String, u32>,
        tool_budget_usage: &mut ToolCallBudgetUsage,
        enforce_implementation_integrity_guard: bool,
        expected_mcp_catalog_hash_hex: Option<&String>,
        expected_mcp_docs_hash_hex: Option<&String>,
        allowed_tool_names: &std::collections::BTreeSet<String>,
        run_started: &std::time::Instant,
    ) -> Result<PhaseStepDispatch, AgentOutcome> {
        runtime_checkpoint.step_index = step;
        self.drain_external_operator_queue(run_id, step);
        if let Some(reason) = self.check_wall_time_budget_exceeded(run_id, step, run_started) {
            let final_prompt_size_chars = context_size_chars(messages);
            return Err(self.finalize_budget_exceeded(
                run_id.to_string(),
                started_at.to_string(),
                reason,
                messages.clone(),
                observed_tool_calls.clone(),
                observed_tool_decisions.clone(),
                final_prompt_size_chars,
                last_compaction_report.clone(),
                hook_invocations.clone(),
                *provider_retry_count,
                *provider_error_count,
                *saw_token_usage,
                total_token_usage,
                taint_state,
            ));
        }
        self.emit_plan_step_started_if_needed(
            run_id,
            step,
            *active_plan_step_idx,
            announced_plan_step_id,
        );
        let compacted = self.compact_messages_for_step(
            run_id,
            step,
            messages,
            provider_retry_count,
            provider_error_count,
        );
        let compacted = match compacted {
            Ok(c) => c,
            Err(err_text) => {
                return Err(self.finalize_provider_error_with_end(
                    step,
                    run_id.to_string(),
                    started_at.to_string(),
                    err_text,
                    messages.clone(),
                    observed_tool_calls.clone(),
                    observed_tool_decisions.clone(),
                    0,
                    last_compaction_report.clone(),
                    hook_invocations.clone(),
                    *provider_retry_count,
                    *provider_error_count,
                    *saw_token_usage,
                    total_token_usage,
                    taint_state,
                ));
            }
        };
        if let Some(report) = compacted.report.clone() {
            self.emit_event(
                run_id,
                step,
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
            *last_compaction_report = Some(report);
        }
        *messages = compacted.messages;
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
                run_id,
                step,
                provider_name(self.gate_ctx.provider),
                &self.model,
                &self.gate_ctx.workdir,
                match serde_json::to_value(pre_payload) {
                    Ok(v) => v,
                    Err(e) => {
                        return Err(self.finalize_provider_error_with_end(
                            step,
                            run_id.to_string(),
                            started_at.to_string(),
                            format!("failed to encode pre_model hook payload: {e}"),
                            messages.clone(),
                            observed_tool_calls.clone(),
                            observed_tool_decisions.clone(),
                            0,
                            last_compaction_report.clone(),
                            hook_invocations.clone(),
                            *provider_retry_count,
                            *provider_error_count,
                            *saw_token_usage,
                            total_token_usage,
                            taint_state,
                        ));
                    }
                },
            );
            match self.hooks.run_pre_model_hooks(hook_input).await {
                Ok(result) => {
                    for inv in &result.invocations {
                        self.emit_event(
                            run_id,
                            step,
                            EventKind::HookStart,
                            serde_json::json!({
                                "hook_name": inv.hook_name,
                                "stage": inv.stage
                            }),
                        );
                        self.emit_event(
                            run_id,
                            step,
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
                        let prompt_chars = context_size_chars(messages);
                        return Err(self.finalize_hook_aborted_with_end(
                            step,
                            run_id.to_string(),
                            started_at.to_string(),
                            reason.clone(),
                            reason,
                            messages.clone(),
                            observed_tool_calls.clone(),
                            observed_tool_decisions.clone(),
                            prompt_chars,
                            last_compaction_report.clone(),
                            hook_invocations.clone(),
                            *provider_retry_count,
                            *provider_error_count,
                            *saw_token_usage,
                            total_token_usage,
                            taint_state,
                        ));
                    }
                    if !result.append_messages.is_empty() {
                        messages.extend(result.append_messages);
                        if self.compaction_settings.max_context_chars > 0 {
                            let compacted_again = maybe_compact(messages, &self.compaction_settings)
                                .map_err(|e| format!("compaction failed after hooks: {e}"));
                            match compacted_again {
                                Ok(out) => {
                                    if let Some(report) = out.report.clone() {
                                        self.emit_event(
                                            run_id,
                                            step,
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
                                        *last_compaction_report = Some(report);
                                    }
                                    *messages = out.messages;
                                    if self.compaction_settings.max_context_chars > 0
                                        && context_size_chars(messages)
                                            > self.compaction_settings.max_context_chars
                                    {
                                        let prompt_chars = context_size_chars(messages);
                                        return Err(self.finalize_provider_error_with_end(
                                            step,
                                            run_id.to_string(),
                                            started_at.to_string(),
                                            "hooks caused prompt to exceed budget".to_string(),
                                            messages.clone(),
                                            observed_tool_calls.clone(),
                                            observed_tool_decisions.clone(),
                                            prompt_chars,
                                            last_compaction_report.clone(),
                                            hook_invocations.clone(),
                                            *provider_retry_count,
                                            *provider_error_count,
                                            *saw_token_usage,
                                            total_token_usage,
                                            taint_state,
                                        ));
                                    }
                                }
                                Err(e) => {
                                    let prompt_chars = context_size_chars(messages);
                                    return Err(self.finalize_provider_error_with_end(
                                        step,
                                        run_id.to_string(),
                                        started_at.to_string(),
                                        e,
                                        messages.clone(),
                                        observed_tool_calls.clone(),
                                        observed_tool_decisions.clone(),
                                        prompt_chars,
                                        last_compaction_report.clone(),
                                        hook_invocations.clone(),
                                        *provider_retry_count,
                                        *provider_error_count,
                                        *saw_token_usage,
                                        total_token_usage,
                                        taint_state,
                                    ));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    self.emit_event(
                        run_id,
                        step,
                        EventKind::HookError,
                        serde_json::json!({"stage":"pre_model","error": e.message}),
                    );
                    let prompt_chars = context_size_chars(messages);
                    return Err(self.finalize_hook_aborted_with_end(
                        step,
                        run_id.to_string(),
                        started_at.to_string(),
                        String::new(),
                        e.message,
                        messages.clone(),
                        observed_tool_calls.clone(),
                        observed_tool_decisions.clone(),
                        prompt_chars,
                        last_compaction_report.clone(),
                        hook_invocations.clone(),
                        *provider_retry_count,
                        *provider_error_count,
                        *saw_token_usage,
                        total_token_usage,
                        taint_state,
                    ));
                }
            }
        }

        self.dispatch_runtime_phase_step(
            user_prompt,
            run_id,
            step,
            started_at,
            messages,
            observed_tool_calls,
            observed_tool_executions,
            observed_tool_decisions,
            last_compaction_report,
            hook_invocations,
            provider_retry_count,
            provider_error_count,
            saw_token_usage,
            total_token_usage,
            taint_state,
            runtime_checkpoint,
            active_plan_step_idx,
            last_user_output,
            step_retry_counts,
            failed_repeat_counts,
            malformed_tool_call_attempts,
            invalid_patch_format_attempts,
            schema_repair_attempts,
            tool_budget_usage,
            enforce_implementation_integrity_guard,
            expected_mcp_catalog_hash_hex,
            expected_mcp_docs_hash_hex,
            allowed_tool_names,
            tools_sorted,
        )
        .await
    }

    pub async fn run(
        &mut self,
        user_prompt: &str,
        session_messages: Vec<Message>,
        injected_messages: Vec<Message>,
    ) -> AgentOutcome {
        self.run_with_checkpoint(user_prompt, session_messages, injected_messages, None)
            .await
    }

    pub async fn run_with_checkpoint(
        &mut self,
        user_prompt: &str,
        session_messages: Vec<Message>,
        injected_messages: Vec<Message>,
        initial_runtime_checkpoint: Option<crate::agent_runtime::state::RunCheckpointV1>,
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
        let mut runtime_checkpoint =
            initial_runtime_checkpoint.unwrap_or_else(|| self.initial_runtime_checkpoint(user_prompt));
        let mut active_plan_step_idx: usize = 0;
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
            match self
                .run_agent_step_iteration(
                    user_prompt,
                    &run_id,
                    step as u32,
                    &started_at,
                    &mut messages,
                    &mut observed_tool_calls,
                    &mut observed_tool_executions,
                    &mut observed_tool_decisions,
                    &mut last_compaction_report,
                    &mut hook_invocations,
                    &mut provider_retry_count,
                    &mut provider_error_count,
                    &mut saw_token_usage,
                    &mut total_token_usage,
                    &mut taint_state,
                    &mut runtime_checkpoint,
                    &mut active_plan_step_idx,
                    &mut announced_plan_step_id,
                    &mut last_user_output,
                    &mut step_retry_counts,
                    &mut failed_repeat_counts,
                    &mut malformed_tool_call_attempts,
                    &mut invalid_patch_format_attempts,
                    &mut schema_repair_attempts,
                    &mut tool_budget_usage,
                    enforce_implementation_integrity_guard,
                    expected_mcp_catalog_hash_hex.as_ref(),
                    expected_mcp_docs_hash_hex.as_ref(),
                    &allowed_tool_names,
                    &run_started,
                )
                .await
            {
                Ok(PhaseStepDispatch::StepComplete) => {}
                Ok(PhaseStepDispatch::ContinueStep) => continue,
                Ok(PhaseStepDispatch::ContinueAgentStep) => continue 'agent_steps,
                Err(outcome) => return outcome,
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

fn synthesize_shell_args_from_validation_text(
    raw: &str,
    required_command: &str,
) -> Option<serde_json::Value> {
    if let Some(v) = crate::agent_tool_exec::parse_jsonish(raw) {
        let normalized = crate::tools::normalize_builtin_tool_args("shell", &v);
        if normalized_shell_command(&normalized).as_deref() == Some(required_command) {
            return Some(normalized);
        }
    }

    let trimmed = strip_simple_code_fence(raw).trim();
    if trimmed == required_command {
        return Some(json!({ "command": required_command }));
    }
    for line in trimmed.lines() {
        let line = line.trim();
        if let Some(command) = line.strip_prefix("command=") {
            if command.trim() == required_command {
                return Some(json!({ "command": required_command }));
            }
        }
    }
    if trimmed.matches(required_command).count() == 1 {
        return Some(json!({ "command": required_command }));
    }
    None
}

fn normalized_shell_command(args: &serde_json::Value) -> Option<String> {
    let obj = args.as_object()?;
    if let Some(command) = obj.get("command").and_then(|v| v.as_str()) {
        return Some(command.trim().to_string());
    }
    let cmd = obj.get("cmd").and_then(|v| v.as_str())?;
    let mut parts = vec![cmd.trim().to_string()];
    if let Some(arr) = obj.get("args").and_then(|v| v.as_array()) {
        for item in arr {
            parts.push(item.as_str()?.trim().to_string());
        }
    }
    Some(parts.join(" ").trim().to_string())
}

fn strip_simple_code_fence(raw: &str) -> &str {
    let trimmed = raw.trim();
    if !trimmed.starts_with("```") || !trimmed.ends_with("```") {
        return trimmed;
    }
    let inner = &trimmed[3..trimmed.len() - 3];
    match inner.find('\n') {
        Some(idx) => inner[idx + 1..].trim(),
        None => inner.trim(),
    }
}

#[cfg(test)]
mod validation_shell_shape_tests {
    use super::{normalized_shell_command, synthesize_shell_args_from_validation_text};
    use serde_json::json;

    #[test]
    fn repairs_bare_required_command_to_shell_args() {
        let args = synthesize_shell_args_from_validation_text("node --test", "node --test")
            .expect("shell args");
        assert_eq!(
            normalized_shell_command(&args).as_deref(),
            Some("node --test")
        );
    }

    #[test]
    fn repairs_fenced_required_command_to_shell_args() {
        let args =
            synthesize_shell_args_from_validation_text("```bash\nnode --test\n```", "node --test")
                .expect("shell args");
        assert_eq!(
            normalized_shell_command(&args).as_deref(),
            Some("node --test")
        );
    }

    #[test]
    fn repairs_command_line_in_exact_answer_shape() {
        let args = synthesize_shell_args_from_validation_text(
            "verified=yes\ncommand=node --test\nresult=passed",
            "node --test",
        )
        .expect("shell args");
        assert_eq!(args, json!({ "command": "node --test" }));
    }

    #[test]
    fn repairs_single_required_command_mention_inside_prose() {
        let args = synthesize_shell_args_from_validation_text(
            "<think>I should run node --test now before finalizing.</think>",
            "node --test",
        )
        .expect("shell args");
        assert_eq!(args, json!({ "command": "node --test" }));
    }

    #[test]
    fn repairs_exact_final_answer_prose_because_command_is_known() {
        let args = synthesize_shell_args_from_validation_text(
            "verified=yes\ncommand=node --test\nresult=passed",
            "node --test",
        )
        .expect("shell args");
        assert_eq!(args, json!({ "command": "node --test" }));
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod agent_tests;
