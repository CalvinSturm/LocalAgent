use crate::agent::{self, Agent, AgentExitReason, PlanToolEnforcementMode};
use crate::compaction::CompactionSettings;
use crate::events::EventKind;
use crate::gate::{GateContext, ProviderKind};
use crate::planner;
use crate::planner_runtime;
use crate::providers::ModelProvider;
use crate::runtime_events;
use crate::runtime_paths;
use crate::session;
use crate::store::extract_session_messages;
use crate::store::{self, PlannerRunRecord, WorkerRunRecord};
use crate::trust;
use crate::types::{Message, Role};
use crate::RunArgs;

use super::finalize::{
    build_run_cli_config_fingerprint_bundle, finalize_early_run_result,
    write_run_artifact_with_warning, RunArtifactWriteInput, RunCliFingerprintBuildInput,
};
use super::RunExecutionResult;

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
    pub(super) lsp_context_message: &'a Option<Message>,
    pub(super) pack_guidance_message: &'a Option<Message>,
    pub(super) base_task_memory: &'a Option<Message>,
    pub(super) replan_handoff: String,
    pub(super) resolved_settings: &'a session::RunSettingResolution,
    pub(super) cancel_rx: &'a mut tokio::sync::watch::Receiver<bool>,
}

pub(super) struct PlannerBootstrapInput<'a, P: ModelProvider> {
    pub(super) provider: &'a P,
    pub(super) provider_kind: ProviderKind,
    pub(super) base_url: &'a str,
    pub(super) prompt: &'a str,
    pub(super) args: &'a RunArgs,
    pub(super) paths: &'a store::StatePaths,
    pub(super) run_id: &'a str,
    pub(super) planner_model: &'a str,
    pub(super) worker_model: &'a str,
    pub(super) planner_strict_effective: bool,
    pub(super) effective_plan_tool_enforcement: PlanToolEnforcementMode,
    pub(super) resolved_settings: &'a session::RunSettingResolution,
    pub(super) event_sink: &'a mut Option<Box<dyn crate::events::EventSink>>,
    pub(super) ui_join: &'a mut Option<std::thread::JoinHandle<anyhow::Result<()>>>,
    pub(super) planner_record: &'a mut Option<PlannerRunRecord>,
    pub(super) worker_record: &'a mut Option<WorkerRunRecord>,
    pub(super) planner_injected_message: &'a mut Option<Message>,
    pub(super) plan_step_constraints: &'a mut Vec<agent::PlanStepConstraint>,
    pub(super) gate_ctx: &'a mut GateContext,
    pub(super) hooks_config_path: &'a std::path::Path,
    pub(super) mcp_config_path: &'a std::path::Path,
    pub(super) tool_catalog: &'a [store::ToolCatalogEntry],
    pub(super) mcp_tool_snapshot: &'a [store::McpToolSnapshotEntry],
    pub(super) mcp_tool_catalog_hash_hex: &'a Option<String>,
    pub(super) policy_source: String,
    pub(super) policy_hash_hex: Option<String>,
    pub(super) policy_version: Option<u32>,
    pub(super) includes_resolved: Vec<String>,
    pub(super) mcp_allowlist: Option<crate::trust::policy::McpAllowSummary>,
    pub(super) tool_schema_hash_hex_map: std::collections::BTreeMap<String, String>,
    pub(super) hooks_config_hash_hex: Option<String>,
    pub(super) mcp_pin_snapshot: Option<store::McpPinSnapshotRecord>,
    pub(super) instruction_resolution: &'a crate::instructions::InstructionResolution,
    pub(super) task_contract: &'a crate::agent::task_contract::TaskContractV1,
    pub(super) task_contract_provenance: &'a crate::agent::task_contract::TaskContractProvenanceV1,
    pub(super) execution_tier: crate::agent_runtime::state::ExecutionTier,
    pub(super) project_guidance_resolution:
        Option<&'a crate::project_guidance::ResolvedProjectGuidance>,
    pub(super) repo_map_resolution: Option<&'a crate::repo_map::ResolvedRepoMap>,
    pub(super) lsp_context_resolution: Option<&'a crate::lsp_context::ResolvedLspContext>,
    pub(super) activated_packs: &'a [crate::packs::ActivatedPack],
}

pub(super) struct ReplanOrchestrationInput<'a, P: ModelProvider> {
    pub(super) agent: &'a mut Agent<P>,
    pub(super) args: &'a RunArgs,
    pub(super) run_id: &'a str,
    pub(super) prompt: &'a str,
    pub(super) planner_model: &'a str,
    pub(super) worker_model: &'a str,
    pub(super) planner_strict_effective: bool,
    pub(super) effective_plan_tool_enforcement: PlanToolEnforcementMode,
    pub(super) planner_record: &'a mut Option<PlannerRunRecord>,
    pub(super) worker_record: &'a mut Option<WorkerRunRecord>,
    pub(super) base_instruction_messages: &'a [Message],
    pub(super) project_guidance_message: &'a Option<Message>,
    pub(super) repo_map_message: &'a Option<Message>,
    pub(super) lsp_context_message: &'a Option<Message>,
    pub(super) pack_guidance_message: &'a Option<Message>,
    pub(super) base_task_memory: &'a Option<Message>,
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

pub(super) async fn bootstrap_planner_phase<P: ModelProvider>(
    input: PlannerBootstrapInput<'_, P>,
) -> anyhow::Result<Option<RunExecutionResult>> {
    let planner_out: anyhow::Result<_> = run_planner_phase_with_start_event(
        input.provider,
        PlannerPhaseLaunch {
            run_id: input.run_id,
            planner_model: input.planner_model,
            prompt: input.prompt,
            planner_max_steps: input.args.planner_max_steps,
            planner_output: input.args.planner_output,
            planner_strict: input.planner_strict_effective,
            effective_plan_tool_enforcement: input.effective_plan_tool_enforcement,
        },
        input.event_sink,
    )
    .await;
    match planner_out {
        Ok(out) => {
            if input.planner_strict_effective && !out.ok {
                emit_planner_end_event(
                    input.event_sink,
                    input.run_id,
                    false,
                    &out.plan_hash_hex,
                    &out.error
                        .clone()
                        .unwrap_or_else(|| "planner validation failed".to_string()),
                    None,
                    None,
                );
                let outcome = planner_strict_failure_outcome(
                    input.run_id,
                    input.resolved_settings,
                    out.error.clone(),
                    out.raw_output.clone(),
                );
                *input.planner_record = Some(PlannerRunRecord {
                    model: input.planner_model.to_string(),
                    max_steps: input.args.planner_max_steps,
                    strict: input.planner_strict_effective,
                    output_format: format!("{:?}", input.args.planner_output).to_lowercase(),
                    plan_json: out.plan_json,
                    plan_hash_hex: out.plan_hash_hex,
                    ok: false,
                    raw_output: out.raw_output,
                    error: out.error,
                });
                *input.worker_record = Some(WorkerRunRecord {
                    model: input.worker_model.to_string(),
                    injected_planner_hash_hex: None,
                    step_result_valid: None,
                    step_result_json: None,
                    step_result_error: None,
                });
                let (cli_config, config_fingerprint, cfg_hash) =
                    build_run_cli_config_fingerprint_bundle(RunCliFingerprintBuildInput {
                        provider_kind: input.provider_kind,
                        base_url: input.base_url,
                        worker_model: input.worker_model,
                        args: input.args,
                        paths: input.paths,
                        resolved_settings: input.resolved_settings,
                        hooks_config_path: input.hooks_config_path,
                        mcp_config_path: input.mcp_config_path,
                        tool_catalog: input.tool_catalog,
                        mcp_tool_snapshot: input.mcp_tool_snapshot,
                        mcp_tool_catalog_hash_hex: input.mcp_tool_catalog_hash_hex,
                        policy_version: input.policy_version,
                        includes_resolved: &input.includes_resolved,
                        mcp_allowlist: &input.mcp_allowlist,
                        mode: input.args.mode,
                        planner_model: Some(input.planner_model),
                        worker_model_override: Some(input.worker_model),
                        planner_max_steps: Some(input.args.planner_max_steps),
                        planner_output: Some(
                            format!("{:?}", input.args.planner_output).to_lowercase(),
                        ),
                        planner_strict: Some(input.planner_strict_effective),
                        enforce_plan_tools: Some(
                            format!("{:?}", input.effective_plan_tool_enforcement).to_lowercase(),
                        ),
                        instruction_resolution: input.instruction_resolution,
                        project_guidance_resolution: input.project_guidance_resolution,
                        repo_map_resolution: input.repo_map_resolution,
                        lsp_context_resolution: input.lsp_context_resolution,
                        activated_packs: input.activated_packs,
                    })?;
                let run_artifact_path = write_run_artifact_with_warning(RunArtifactWriteInput {
                    paths: input.paths.clone(),
                    cli_config,
                    policy_info: store::PolicyRecordInfo {
                        source: input.policy_source,
                        hash_hex: input.policy_hash_hex,
                        version: input.policy_version,
                        includes_resolved: input.includes_resolved,
                        mcp_allowlist: input.mcp_allowlist,
                    },
                    config_hash_hex: cfg_hash,
                    outcome: outcome.clone(),
                    mode: input.args.mode,
                    planner_record: input.planner_record.clone(),
                    worker_record: input.worker_record.clone(),
                    tool_schema_hash_hex_map: input.tool_schema_hash_hex_map,
                    hooks_config_hash_hex: input.hooks_config_hash_hex,
                    task_contract: input.task_contract.clone(),
                    task_contract_provenance: input.task_contract_provenance.clone(),
                    tool_facts: Vec::new(),
                    tool_fact_envelopes: Vec::new(),
                    run_checkpoint: super::checkpoint::checkpoint_for_outcome(&outcome),
                    final_checkpoint: Some(super::checkpoint::runtime_state_checkpoint_for_outcome(
                        &outcome,
                        input.prompt,
                        input.execution_tier.clone(),
                        &[],
                    )),
                    execution_tier: input.execution_tier.clone(),
                    interrupt_history: crate::agent::interrupts::interrupt_history_for_outcome(&outcome),
                    phase_summary: super::checkpoint::phase_summary_for_outcome(&outcome),
                    completion_decisions: super::checkpoint::completion_decisions_for_outcome(
                        &outcome,
                        &super::checkpoint::runtime_state_checkpoint_for_outcome(
                            &outcome,
                            input.prompt,
                            input.execution_tier.clone(),
                            &[],
                        ),
                    ),
                    config_fingerprint: Some(config_fingerprint.clone()),
                    repro_record: None,
                    mcp_runtime_trace: Vec::new(),
                    mcp_pin_snapshot: input.mcp_pin_snapshot,
                });
                return finalize_early_run_result(
                    input.ui_join.take(),
                    outcome,
                    run_artifact_path,
                    None,
                )
                    .map(Some);
            }
            emit_planner_end_event(
                input.event_sink,
                input.run_id,
                out.ok,
                &out.plan_hash_hex,
                &out.error.clone().unwrap_or_default(),
                None,
                None,
            );
            let handoff = format!(
                "{}\n\n{}",
                planner::planner_handoff_content(&out.plan_json)?,
                planner::planner_worker_contract_content(&out.plan_json)?
            );
            if matches!(
                input.effective_plan_tool_enforcement,
                PlanToolEnforcementMode::Soft | PlanToolEnforcementMode::Hard
            ) {
                match planner::extract_plan_step_tools(&out.plan_json) {
                    Ok(steps) => {
                        *input.plan_step_constraints = steps
                            .into_iter()
                            .map(|s| agent::PlanStepConstraint {
                                step_id: s.step_id,
                                intended_tools: s.intended_tools,
                            })
                            .collect();
                    }
                    Err(e) => {
                        eprintln!("WARN: failed to extract plan step constraints: {e}");
                    }
                }
            }
            *input.planner_injected_message = Some(Message {
                role: Role::Developer,
                content: Some(handoff),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            });
            input.gate_ctx.planner_hash_hex = Some(out.plan_hash_hex.clone());
            *input.worker_record = Some(WorkerRunRecord {
                model: input.worker_model.to_string(),
                injected_planner_hash_hex: Some(out.plan_hash_hex.clone()),
                step_result_valid: None,
                step_result_json: None,
                step_result_error: None,
            });
            *input.planner_record = Some(PlannerRunRecord {
                model: input.planner_model.to_string(),
                max_steps: input.args.planner_max_steps,
                strict: input.planner_strict_effective,
                output_format: format!("{:?}", input.args.planner_output).to_lowercase(),
                plan_json: out.plan_json,
                plan_hash_hex: out.plan_hash_hex,
                ok: out.ok,
                raw_output: out.raw_output,
                error: out.error,
            });
            emit_worker_start_event(
                input.event_sink,
                input.run_id,
                input.worker_model,
                &input
                    .planner_record
                    .as_ref()
                    .map(|p| p.plan_hash_hex.clone())
                    .unwrap_or_default(),
                input.effective_plan_tool_enforcement,
                None,
            );
            Ok(None)
        }
        Err(e) => {
            let err_short = e.to_string();
            emit_planner_end_event(
                input.event_sink,
                input.run_id,
                false,
                "",
                &err_short,
                None,
                None,
            );
            let outcome = planner_runtime_error_outcome(
                input.run_id,
                input.resolved_settings,
                e.to_string(),
                input.prompt,
            );
            let (cli_config, config_fingerprint, cfg_hash) =
                build_run_cli_config_fingerprint_bundle(RunCliFingerprintBuildInput {
                    provider_kind: input.provider_kind,
                    base_url: input.base_url,
                    worker_model: input.worker_model,
                    args: input.args,
                    paths: input.paths,
                    resolved_settings: input.resolved_settings,
                    hooks_config_path: input.hooks_config_path,
                    mcp_config_path: input.mcp_config_path,
                    tool_catalog: input.tool_catalog,
                    mcp_tool_snapshot: input.mcp_tool_snapshot,
                    mcp_tool_catalog_hash_hex: input.mcp_tool_catalog_hash_hex,
                    policy_version: input.policy_version,
                    includes_resolved: &input.includes_resolved,
                    mcp_allowlist: &input.mcp_allowlist,
                    mode: input.args.mode,
                    planner_model: Some(input.planner_model),
                    worker_model_override: Some(input.worker_model),
                    planner_max_steps: Some(input.args.planner_max_steps),
                    planner_output: Some(format!("{:?}", input.args.planner_output).to_lowercase()),
                    planner_strict: Some(input.planner_strict_effective),
                    enforce_plan_tools: Some(
                        format!("{:?}", input.effective_plan_tool_enforcement).to_lowercase(),
                    ),
                    instruction_resolution: input.instruction_resolution,
                    project_guidance_resolution: input.project_guidance_resolution,
                    repo_map_resolution: input.repo_map_resolution,
                    lsp_context_resolution: input.lsp_context_resolution,
                    activated_packs: input.activated_packs,
                })?;
            let run_artifact_path = write_run_artifact_with_warning(RunArtifactWriteInput {
                paths: input.paths.clone(),
                cli_config,
                policy_info: store::PolicyRecordInfo {
                    source: input.policy_source,
                    hash_hex: input.policy_hash_hex,
                    version: input.policy_version,
                    includes_resolved: input.includes_resolved,
                    mcp_allowlist: input.mcp_allowlist,
                },
                config_hash_hex: cfg_hash,
                outcome: outcome.clone(),
                mode: input.args.mode,
                planner_record: None,
                worker_record: None,
                tool_schema_hash_hex_map: input.tool_schema_hash_hex_map,
                hooks_config_hash_hex: input.hooks_config_hash_hex,
                task_contract: input.task_contract.clone(),
                task_contract_provenance: input.task_contract_provenance.clone(),
                tool_facts: Vec::new(),
                tool_fact_envelopes: Vec::new(),
                run_checkpoint: super::checkpoint::checkpoint_for_outcome(&outcome),
                final_checkpoint: Some(super::checkpoint::runtime_state_checkpoint_for_outcome(
                    &outcome,
                    input.prompt,
                    input.execution_tier.clone(),
                    &[],
                )),
                execution_tier: input.execution_tier.clone(),
                interrupt_history: crate::agent::interrupts::interrupt_history_for_outcome(&outcome),
                phase_summary: super::checkpoint::phase_summary_for_outcome(&outcome),
                completion_decisions: super::checkpoint::completion_decisions_for_outcome(
                    &outcome,
                    &super::checkpoint::runtime_state_checkpoint_for_outcome(
                        &outcome,
                        input.prompt,
                        input.execution_tier.clone(),
                        &[],
                    ),
                ),
                config_fingerprint: Some(config_fingerprint.clone()),
                repro_record: None,
                mcp_runtime_trace: Vec::new(),
                mcp_pin_snapshot: input.mcp_pin_snapshot,
            });
            finalize_early_run_result(input.ui_join.take(), outcome, run_artifact_path, None)
                .map(Some)
        }
    }
}

pub(super) async fn maybe_handle_worker_replan<P: ModelProvider>(
    input: ReplanOrchestrationInput<'_, P>,
    outcome: &mut agent::AgentOutcome,
) -> anyhow::Result<()> {
    if !matches!(input.args.mode, planner::RunMode::PlannerWorker)
        || !matches!(outcome.exit_reason, AgentExitReason::PlannerError)
        || !outcome
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("worker requested replan transition")
    {
        return Ok(());
    }
    let replanner_reason = outcome
        .error
        .clone()
        .unwrap_or_else(|| "worker requested replan transition".to_string());
    let prior_plan_json = input
        .planner_record
        .as_ref()
        .map(|p| p.plan_json.clone())
        .unwrap_or_else(|| serde_json::json!({}));
    let prior_plan_hash = input
        .planner_record
        .as_ref()
        .map(|p| p.plan_hash_hex.clone())
        .unwrap_or_default();
    let prior_plan_text =
        serde_json::to_string_pretty(&prior_plan_json).unwrap_or_else(|_| "{}".to_string());
    let replan_prompt = format!(
        "{prompt}\n\nREPLAN CONTEXT\nPrevious plan hash: {prior_plan_hash}\nPrevious normalized plan:\n{prior_plan_text}\n\nRuntime requested a replan because: {replanner_reason}\nReturn an updated openagent.plan.v1 JSON plan for remaining work only.",
        prompt = input.prompt
    );
    match run_replanner_phase_with_start_event(
        &input.agent.provider,
        ReplannerPhaseLaunch {
            run_id: input.run_id,
            planner_model: input.planner_model,
            replanner_reason: &replanner_reason,
            replan_prompt: &replan_prompt,
            planner_max_steps: input.args.planner_max_steps,
            planner_output: input.args.planner_output,
            planner_strict: input.planner_strict_effective,
        },
        &mut input.agent.event_sink,
    )
    .await
    {
        Ok(replan_out) if !input.planner_strict_effective || replan_out.ok => {
            emit_planner_end_event(
                &mut input.agent.event_sink,
                input.run_id,
                replan_out.ok,
                &replan_out.plan_hash_hex,
                "",
                Some("replan"),
                Some(&prior_plan_hash),
            );
            let ReplanSuccessPrep { replan_handoff } = prepare_replan_success_resume(
                ReplanSuccessPrepInput {
                    agent: input.agent,
                    run_id: input.run_id,
                    planner_model: input.planner_model,
                    worker_model: input.worker_model,
                    planner_max_steps: input.args.planner_max_steps,
                    planner_output: input.args.planner_output,
                    planner_strict_effective: input.planner_strict_effective,
                    effective_plan_tool_enforcement: input.effective_plan_tool_enforcement,
                    worker_record: input.worker_record,
                    planner_record: input.planner_record,
                },
                replan_out,
            )?;
            *outcome = run_replan_resume_with_cancel(ReplanResumeRunInput {
                agent: input.agent,
                prompt: input.prompt,
                prior_outcome: outcome,
                base_instruction_messages: input.base_instruction_messages,
                project_guidance_message: input.project_guidance_message,
                repo_map_message: input.repo_map_message,
                lsp_context_message: input.lsp_context_message,
                pack_guidance_message: input.pack_guidance_message,
                base_task_memory: input.base_task_memory,
                replan_handoff,
                resolved_settings: input.resolved_settings,
                cancel_rx: input.cancel_rx,
            })
            .await;
        }
        Ok(replan_out) => {
            outcome.exit_reason = AgentExitReason::PlannerError;
            outcome.error = Some(format!(
                "replan failed strict validation: {}",
                replan_out
                    .error
                    .unwrap_or_else(|| "planner validation failed".to_string())
            ));
        }
        Err(e) => {
            outcome.exit_reason = AgentExitReason::PlannerError;
            outcome.error = Some(format!("replan failed: {e}"));
        }
    }
    Ok(())
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
        input.lsp_context_message.clone(),
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
        out = input
            .agent
            .run_with_checkpoint(input.prompt, resume_session_messages, replan_injected, None) => out,
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
