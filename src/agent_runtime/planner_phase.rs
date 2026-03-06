use crate::agent::{self, Agent, AgentExitReason, PlanToolEnforcementMode};
use crate::compaction::CompactionSettings;
use crate::events::EventKind;
use crate::planner;
use crate::planner_runtime;
use crate::providers::ModelProvider;
use crate::runtime_events;
use crate::runtime_paths;
use crate::session;
use crate::store::{PlannerRunRecord, WorkerRunRecord};
use crate::store::extract_session_messages;
use crate::trust;
use crate::types::{Message, Role};

pub(super) struct PlannerPhaseLaunch<'a> {
    pub(super) run_id: &'a str,
    pub(super) planner_model: &'a str,
    pub(super) prompt: &'a str,
    pub(super) planner_max_steps: u32,
    pub(super) planner_output: planner::PlannerOutput,
    pub(super) planner_strict: bool,
    pub(super) effective_plan_tool_enforcement: PlanToolEnforcementMode,
}

pub(super) struct ReplannerPhaseLaunch<'a> {
    pub(super) run_id: &'a str,
    pub(super) planner_model: &'a str,
    pub(super) replanner_reason: &'a str,
    pub(super) replan_prompt: &'a str,
    pub(super) planner_max_steps: u32,
    pub(super) planner_output: planner::PlannerOutput,
    pub(super) planner_strict: bool,
}

pub(super) struct ReplanSuccessPrepInput<'a, P: ModelProvider> {
    pub(super) agent: &'a mut Agent<P>,
    pub(super) run_id: &'a str,
    pub(super) planner_model: &'a str,
    pub(super) worker_model: &'a str,
    pub(super) planner_max_steps: u32,
    pub(super) planner_output: planner::PlannerOutput,
    pub(super) planner_strict_effective: bool,
    pub(super) effective_plan_tool_enforcement: PlanToolEnforcementMode,
    pub(super) worker_record: &'a mut Option<WorkerRunRecord>,
    pub(super) planner_record: &'a mut Option<PlannerRunRecord>,
}

pub(super) struct ReplanSuccessPrep {
    pub(super) replan_handoff: String,
}

pub(super) struct ReplanResumeRunInput<'a, P: ModelProvider> {
    pub(super) agent: &'a mut Agent<P>,
    pub(super) prompt: &'a str,
    pub(super) prior_outcome: &'a agent::AgentOutcome,
    pub(super) base_instruction_messages: &'a [Message],
    pub(super) project_guidance_message: &'a Option<Message>,
    pub(super) repo_map_message: &'a Option<Message>,
    pub(super) pack_guidance_message: &'a Option<Message>,
    pub(super) base_task_memory: &'a Option<Message>,
    pub(super) replan_handoff: String,
    pub(super) resolved_settings: &'a session::RunSettingResolution,
    pub(super) cancel_rx: &'a mut tokio::sync::watch::Receiver<bool>,
}

pub(super) async fn run_planner_phase_with_start_event<P: ModelProvider>(
    provider: &P,
    launch: PlannerPhaseLaunch<'_>,
    event_sink: &mut Option<Box<dyn crate::events::EventSink>>,
) -> anyhow::Result<planner_runtime::PlannerPhaseOutput> {
    runtime_events::emit_event(
        event_sink,
        launch.run_id,
        0,
        EventKind::PlannerStart,
        serde_json::json!({
            "planner_model": launch.planner_model,
            "enforce_plan_tools_effective": format!("{:?}", launch.effective_plan_tool_enforcement).to_lowercase(),
        }),
    );
    planner_runtime::run_planner_phase(
        provider,
        launch.run_id,
        launch.planner_model,
        launch.prompt,
        launch.planner_max_steps,
        launch.planner_output,
        launch.planner_strict,
        event_sink,
    )
    .await
}

pub(super) async fn run_replanner_phase_with_start_event<P: ModelProvider>(
    provider: &P,
    launch: ReplannerPhaseLaunch<'_>,
    event_sink: &mut Option<Box<dyn crate::events::EventSink>>,
) -> anyhow::Result<planner_runtime::PlannerPhaseOutput> {
    runtime_events::emit_event(
        event_sink,
        launch.run_id,
        0,
        EventKind::PlannerStart,
        serde_json::json!({
            "phase": "replan",
            "reason": launch.replanner_reason
        }),
    );
    planner_runtime::run_planner_phase(
        provider,
        launch.run_id,
        launch.planner_model,
        launch.replan_prompt,
        launch.planner_max_steps,
        launch.planner_output,
        launch.planner_strict,
        event_sink,
    )
    .await
}

pub(super) fn prepare_replan_success_resume<P: ModelProvider>(
    input: ReplanSuccessPrepInput<'_, P>,
    replan_out: planner_runtime::PlannerPhaseOutput,
) -> anyhow::Result<ReplanSuccessPrep> {
    let replan_handoff = format!(
        "{}\n\n{}",
        planner::planner_handoff_content(&replan_out.plan_json)?,
        planner::planner_worker_contract_content(&replan_out.plan_json)?
    );
    if matches!(
        input.effective_plan_tool_enforcement,
        PlanToolEnforcementMode::Soft | PlanToolEnforcementMode::Hard
    ) {
        if let Ok(steps) = planner::extract_plan_step_tools(&replan_out.plan_json) {
            input.agent.plan_step_constraints = steps
                .into_iter()
                .map(|s| agent::PlanStepConstraint {
                    step_id: s.step_id,
                    intended_tools: s.intended_tools,
                })
                .collect();
        }
    }
    *input.planner_record = Some(PlannerRunRecord {
        model: input.planner_model.to_string(),
        max_steps: input.planner_max_steps,
        strict: input.planner_strict_effective,
        output_format: format!("{:?}", input.planner_output).to_lowercase(),
        plan_json: replan_out.plan_json.clone(),
        plan_hash_hex: replan_out.plan_hash_hex.clone(),
        ok: replan_out.ok,
        raw_output: replan_out.raw_output,
        error: replan_out.error,
    });
    input.agent.gate_ctx.planner_hash_hex = Some(replan_out.plan_hash_hex.clone());
    if let Some(worker) = input.worker_record.as_mut() {
        worker.injected_planner_hash_hex = Some(replan_out.plan_hash_hex.clone());
    }
    emit_worker_start_event(
        &mut input.agent.event_sink,
        input.run_id,
        input.worker_model,
        &replan_out.plan_hash_hex,
        input.effective_plan_tool_enforcement,
        Some("replan_resume"),
    );
    Ok(ReplanSuccessPrep { replan_handoff })
}

pub(super) async fn run_replan_resume_with_cancel<P: ModelProvider>(
    input: ReplanResumeRunInput<'_, P>,
) -> agent::AgentOutcome {
    let resume_session_messages = extract_session_messages(&input.prior_outcome.messages);
    let replan_injected = runtime_paths::merge_injected_messages(
        input.base_instruction_messages.to_vec(),
        input.project_guidance_message.clone(),
        input.repo_map_message.clone(),
        input.pack_guidance_message.clone(),
        input.base_task_memory.clone(),
        Some(Message {
            role: Role::Developer,
            content: Some(input.replan_handoff),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        }),
    );
    tokio::select! {
        out = input.agent.run(input.prompt, resume_session_messages, replan_injected) => out,
        _ = tokio::signal::ctrl_c() => {
            cancelled_outcome(input.resolved_settings)
        },
        _ = async {
            let _ = input.cancel_rx.changed().await;
        } => {
            cancelled_outcome(input.resolved_settings)
        }
    }
}

pub(super) fn emit_planner_end_event(
    event_sink: &mut Option<Box<dyn crate::events::EventSink>>,
    run_id: &str,
    ok: bool,
    planner_hash_hex: &str,
    error_short: &str,
    phase: Option<&str>,
    lineage_parent_plan_hash_hex: Option<&str>,
) {
    let mut payload = serde_json::Map::new();
    if let Some(phase) = phase {
        payload.insert(
            "phase".to_string(),
            serde_json::Value::String(phase.to_string()),
        );
    }
    payload.insert("ok".to_string(), serde_json::Value::Bool(ok));
    payload.insert(
        "planner_hash_hex".to_string(),
        serde_json::Value::String(planner_hash_hex.to_string()),
    );
    if !error_short.is_empty() {
        payload.insert(
            "error_short".to_string(),
            serde_json::Value::String(error_short.to_string()),
        );
    } else if phase.is_none() {
        payload.insert(
            "error_short".to_string(),
            serde_json::Value::String(String::new()),
        );
    }
    if let Some(parent) = lineage_parent_plan_hash_hex {
        payload.insert(
            "lineage_parent_plan_hash_hex".to_string(),
            serde_json::Value::String(parent.to_string()),
        );
    }
    runtime_events::emit_event(
        event_sink,
        run_id,
        0,
        EventKind::PlannerEnd,
        serde_json::Value::Object(payload),
    );
}

pub(super) fn emit_worker_start_event(
    event_sink: &mut Option<Box<dyn crate::events::EventSink>>,
    run_id: &str,
    worker_model: &str,
    planner_hash_hex: &str,
    effective_plan_tool_enforcement: PlanToolEnforcementMode,
    phase: Option<&str>,
) {
    let mut payload = serde_json::Map::new();
    if let Some(phase) = phase {
        payload.insert(
            "phase".to_string(),
            serde_json::Value::String(phase.to_string()),
        );
    }
    payload.insert(
        "worker_model".to_string(),
        serde_json::Value::String(worker_model.to_string()),
    );
    payload.insert(
        "planner_hash_hex".to_string(),
        serde_json::Value::String(planner_hash_hex.to_string()),
    );
    payload.insert(
        "enforce_plan_tools_effective".to_string(),
        serde_json::Value::String(format!("{:?}", effective_plan_tool_enforcement).to_lowercase()),
    );
    runtime_events::emit_event(
        event_sink,
        run_id,
        0,
        EventKind::WorkerStart,
        serde_json::Value::Object(payload),
    );
}

pub(super) fn cancelled_outcome(
    resolved_settings: &session::RunSettingResolution,
) -> agent::AgentOutcome {
    agent::AgentOutcome {
        run_id: uuid::Uuid::new_v4().to_string(),
        started_at: trust::now_rfc3339(),
        finished_at: trust::now_rfc3339(),
        exit_reason: AgentExitReason::Cancelled,
        final_output: String::new(),
        error: Some("cancelled".to_string()),
        messages: Vec::new(),
        tool_calls: Vec::new(),
        tool_decisions: Vec::new(),
        compaction_settings: CompactionSettings {
            max_context_chars: resolved_settings.max_context_chars,
            mode: resolved_settings.compaction_mode,
            keep_last: resolved_settings.compaction_keep_last,
            tool_result_persist: resolved_settings.tool_result_persist,
        },
        final_prompt_size_chars: 0,
        compaction_report: None,
        hook_invocations: Vec::new(),
        provider_retry_count: 0,
        provider_error_count: 0,
        token_usage: None,
        taint: None,
    }
}

pub(super) fn planner_strict_failure_outcome(
    run_id: &str,
    resolved_settings: &session::RunSettingResolution,
    error: Option<String>,
    raw_output: Option<String>,
) -> agent::AgentOutcome {
    agent::AgentOutcome {
        run_id: run_id.to_string(),
        started_at: trust::now_rfc3339(),
        finished_at: trust::now_rfc3339(),
        exit_reason: AgentExitReason::PlannerError,
        final_output: String::new(),
        error,
        messages: vec![Message {
            role: Role::Assistant,
            content: raw_output,
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        }],
        tool_calls: Vec::new(),
        tool_decisions: Vec::new(),
        compaction_settings: CompactionSettings {
            max_context_chars: resolved_settings.max_context_chars,
            mode: resolved_settings.compaction_mode,
            keep_last: resolved_settings.compaction_keep_last,
            tool_result_persist: resolved_settings.tool_result_persist,
        },
        final_prompt_size_chars: 0,
        compaction_report: None,
        hook_invocations: Vec::new(),
        provider_retry_count: 0,
        provider_error_count: 0,
        token_usage: None,
        taint: None,
    }
}

pub(super) fn planner_runtime_error_outcome(
    run_id: &str,
    resolved_settings: &session::RunSettingResolution,
    error: String,
    prompt: &str,
) -> agent::AgentOutcome {
    agent::AgentOutcome {
        run_id: run_id.to_string(),
        started_at: trust::now_rfc3339(),
        finished_at: trust::now_rfc3339(),
        exit_reason: AgentExitReason::PlannerError,
        final_output: String::new(),
        error: Some(error),
        messages: vec![Message {
            role: Role::User,
            content: Some(prompt.to_string()),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        }],
        tool_calls: Vec::new(),
        tool_decisions: Vec::new(),
        compaction_settings: CompactionSettings {
            max_context_chars: resolved_settings.max_context_chars,
            mode: resolved_settings.compaction_mode,
            keep_last: resolved_settings.compaction_keep_last,
            tool_result_persist: resolved_settings.tool_result_persist,
        },
        final_prompt_size_chars: 0,
        compaction_report: None,
        hook_invocations: Vec::new(),
        provider_retry_count: 0,
        provider_error_count: 0,
        token_usage: None,
        taint: None,
    }
}
