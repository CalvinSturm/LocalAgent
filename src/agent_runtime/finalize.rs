use crate::agent::{self, AgentExitReason};
use crate::events::EventKind;
use crate::gate::ProviderKind;
use crate::planner;
use crate::repro;
use crate::repro::ReproEnvMode;
use crate::runtime_events;
use crate::runtime_paths;
use crate::session::{self, settings_from_run, SessionStore};
use crate::store::{self, config_hash_hex, extract_session_messages, provider_to_string};
use crate::RunArgs;

use super::RunExecutionResult;
use crate::store::{PlannerRunRecord, WorkerRunRecord};

pub(super) struct ReproSnapshotBuildInput<'a> {
    pub(super) args: &'a RunArgs,
    pub(super) provider_kind: ProviderKind,
    pub(super) base_url: &'a str,
    pub(super) worker_model: &'a str,
    pub(super) resolved_settings: &'a session::RunSettingResolution,
    pub(super) policy_hash_hex: &'a Option<String>,
    pub(super) includes_resolved: &'a Vec<String>,
    pub(super) hooks_config_hash_hex: &'a Option<String>,
    pub(super) tool_schema_hash_hex_map: &'a std::collections::BTreeMap<String, String>,
    pub(super) tool_catalog: &'a [store::ToolCatalogEntry],
    pub(super) config_hash_hex: &'a str,
    pub(super) run_id: &'a str,
}

pub(super) struct RunArtifactWriteInput {
    pub(super) paths: store::StatePaths,
    pub(super) cli_config: store::RunCliConfig,
    pub(super) policy_info: store::PolicyRecordInfo,
    pub(super) config_hash_hex: String,
    pub(super) outcome: agent::AgentOutcome,
    pub(super) mode: planner::RunMode,
    pub(super) planner_record: Option<PlannerRunRecord>,
    pub(super) worker_record: Option<WorkerRunRecord>,
    pub(super) tool_schema_hash_hex_map: std::collections::BTreeMap<String, String>,
    pub(super) hooks_config_hash_hex: Option<String>,
    pub(super) task_contract: crate::agent::TaskContractV1,
    pub(super) task_contract_provenance: crate::agent::TaskContractProvenanceV1,
    pub(super) tool_facts: Vec<crate::agent::ToolFactV1>,
    pub(super) tool_fact_envelopes: Vec<crate::agent::ToolFactEnvelopeV1>,
    pub(super) run_checkpoint: Option<store::RunCheckpointV1>,
    pub(super) config_fingerprint: Option<store::ConfigFingerprintV1>,
    pub(super) repro_record: Option<crate::repro::RunReproRecord>,
    pub(super) mcp_runtime_trace: Vec<crate::agent::McpRuntimeTraceEntry>,
    pub(super) mcp_pin_snapshot: Option<store::McpPinSnapshotRecord>,
}

pub(super) struct RunCliFingerprintBuildInput<'a> {
    pub(super) provider_kind: ProviderKind,
    pub(super) base_url: &'a str,
    pub(super) worker_model: &'a str,
    pub(super) args: &'a RunArgs,
    pub(super) paths: &'a store::StatePaths,
    pub(super) resolved_settings: &'a session::RunSettingResolution,
    pub(super) hooks_config_path: &'a std::path::Path,
    pub(super) mcp_config_path: &'a std::path::Path,
    pub(super) tool_catalog: &'a [store::ToolCatalogEntry],
    pub(super) mcp_tool_snapshot: &'a [store::McpToolSnapshotEntry],
    pub(super) mcp_tool_catalog_hash_hex: &'a Option<String>,
    pub(super) policy_version: Option<u32>,
    pub(super) includes_resolved: &'a [String],
    pub(super) mcp_allowlist: &'a Option<crate::trust::policy::McpAllowSummary>,
    pub(super) mode: planner::RunMode,
    pub(super) planner_model: Option<&'a str>,
    pub(super) worker_model_override: Option<&'a str>,
    pub(super) planner_max_steps: Option<u32>,
    pub(super) planner_output: Option<String>,
    pub(super) planner_strict: Option<bool>,
    pub(super) enforce_plan_tools: Option<String>,
    pub(super) instruction_resolution: &'a crate::instructions::InstructionResolution,
    pub(super) project_guidance_resolution:
        Option<&'a crate::project_guidance::ResolvedProjectGuidance>,
    pub(super) repo_map_resolution: Option<&'a crate::repo_map::ResolvedRepoMap>,
    pub(super) lsp_context_resolution: Option<&'a crate::lsp_context::ResolvedLspContext>,
    pub(super) activated_packs: &'a [crate::packs::ActivatedPack],
}

pub(super) struct FinalizeRunArtifactsInput<'a> {
    pub(super) event_sink: &'a mut Option<Box<dyn crate::events::EventSink>>,
    pub(super) args: &'a RunArgs,
    pub(super) prompt: &'a str,
    pub(super) paths: &'a store::StatePaths,
    pub(super) provider_kind: ProviderKind,
    pub(super) base_url: &'a str,
    pub(super) worker_model: &'a str,
    pub(super) planner_model: &'a str,
    pub(super) planner_strict_effective: bool,
    pub(super) effective_plan_tool_enforcement: crate::agent::PlanToolEnforcementMode,
    pub(super) resolved_settings: &'a session::RunSettingResolution,
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
    pub(super) instruction_resolution: &'a crate::instructions::InstructionResolution,
    pub(super) task_contract: &'a crate::agent::TaskContractV1,
    pub(super) task_contract_provenance: &'a crate::agent::TaskContractProvenanceV1,
    pub(super) project_guidance_resolution:
        Option<&'a crate::project_guidance::ResolvedProjectGuidance>,
    pub(super) repo_map_resolution: Option<&'a crate::repo_map::ResolvedRepoMap>,
    pub(super) lsp_context_resolution: Option<&'a crate::lsp_context::ResolvedLspContext>,
    pub(super) activated_packs: &'a [crate::packs::ActivatedPack],
    pub(super) outcome: &'a agent::AgentOutcome,
    pub(super) planner_record: Option<PlannerRunRecord>,
    pub(super) worker_record: Option<WorkerRunRecord>,
    pub(super) mcp_runtime_trace: Vec<crate::agent::McpRuntimeTraceEntry>,
    pub(super) mcp_pin_snapshot: Option<store::McpPinSnapshotRecord>,
}

pub(super) fn write_run_artifact_with_warning(
    input: RunArtifactWriteInput,
) -> Option<std::path::PathBuf> {
    match store::write_run_record(
        &input.paths,
        input.cli_config,
        input.policy_info,
        input.config_hash_hex,
        &input.outcome,
        input.mode,
        input.planner_record,
        input.worker_record,
        input.tool_schema_hash_hex_map,
        input.hooks_config_hash_hex,
        Some(input.task_contract),
        Some(input.task_contract_provenance),
        input.tool_facts,
        input.tool_fact_envelopes,
        input.run_checkpoint,
        input.config_fingerprint,
        input.repro_record,
        input.mcp_runtime_trace,
        input.mcp_pin_snapshot,
    ) {
        Ok(p) => Some(p),
        Err(e) => {
            eprintln!("WARN: failed to write run artifact: {e}");
            None
        }
    }
}

pub(super) fn finalize_early_run_result(
    ui_join: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
    outcome: agent::AgentOutcome,
    run_artifact_path: Option<std::path::PathBuf>,
    runtime_checkpoint_path: Option<std::path::PathBuf>,
) -> anyhow::Result<RunExecutionResult> {
    if let Some(h) = ui_join {
        let _ = h.join();
    }
    Ok(RunExecutionResult {
        outcome,
        run_artifact_path,
        runtime_checkpoint_path,
    })
}

pub(super) fn finalize_ui_and_session_state(
    ui_join: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
    args: &RunArgs,
    session_store: &SessionStore,
    session_data: &mut session::SessionData,
    resolved_settings: &session::RunSettingResolution,
    outcome: &agent::AgentOutcome,
) {
    if let Some(h) = ui_join {
        if let Err(_e) = h.join() {
            eprintln!("WARN: tui thread ended unexpectedly");
        }
    }
    if !args.no_session {
        session_data.messages = extract_session_messages(&outcome.messages);
        session_data.settings = settings_from_run(resolved_settings);
        if let Err(e) = session_store.save(session_data, args.max_session_messages) {
            eprintln!("WARN: failed to save session: {e}");
        }
    }
}

pub(super) fn normalize_and_record_worker_step_result(
    outcome: &mut agent::AgentOutcome,
    planner_record: Option<&PlannerRunRecord>,
    worker_record: &mut Option<WorkerRunRecord>,
    planner_strict_effective: bool,
) {
    let mut step_result_json = None;
    let mut step_result_error = None;
    let mut step_result_valid = None;
    if let Some(plan) = planner_record {
        match planner::normalize_worker_step_result(&outcome.final_output, &plan.plan_json) {
            Ok(v) => {
                step_result_json = Some(v);
                step_result_valid = Some(true);
            }
            Err(e) => {
                let err = e.to_string();
                if planner_strict_effective && matches!(outcome.exit_reason, AgentExitReason::Ok) {
                    outcome.exit_reason = AgentExitReason::PlannerError;
                    outcome.error = Some(format!(
                        "worker step result validation failed in strict planner_worker mode: {err}"
                    ));
                }
                step_result_error = Some(err);
                step_result_valid = Some(false);
            }
        }
    }
    if let Some(worker) = worker_record.as_mut() {
        worker.step_result_valid = step_result_valid;
        worker.step_result_json = step_result_json;
        worker.step_result_error = step_result_error;
    }
}

pub(super) fn build_and_emit_repro_snapshot(
    event_sink: &mut Option<Box<dyn crate::events::EventSink>>,
    input: ReproSnapshotBuildInput<'_>,
) -> anyhow::Result<Option<crate::repro::RunReproRecord>> {
    let repro_record = repro::build_repro_record(
        input.args.repro,
        input.args.repro_env,
        repro::ReproBuildInput {
            run_id: input.run_id.to_string(),
            created_at: crate::trust::now_rfc3339(),
            provider: provider_to_string(input.provider_kind),
            base_url: input.base_url.to_string(),
            model: input.worker_model.to_string(),
            caps_source: format!("{:?}", input.resolved_settings.caps_mode).to_lowercase(),
            trust_mode: store::cli_trust_mode(input.args.trust),
            approval_mode: format!("{:?}", input.args.approval_mode).to_lowercase(),
            approval_key: input.args.approval_key.as_str().to_string(),
            policy_hash_hex: input.policy_hash_hex.clone(),
            includes_resolved: input.includes_resolved.clone(),
            hooks_mode: format!("{:?}", input.resolved_settings.hooks_mode).to_lowercase(),
            hooks_config_hash_hex: input.hooks_config_hash_hex.clone(),
            taint_mode: format!("{:?}", input.args.taint_mode).to_lowercase(),
            taint_policy_globs_hash_hex: input.policy_hash_hex.clone(),
            tool_schema_hash_hex_map: input.tool_schema_hash_hex_map.clone(),
            tool_catalog: input.tool_catalog.to_vec(),
            exec_target: format!("{:?}", input.args.exec_target).to_lowercase(),
            docker: if matches!(
                input.args.exec_target,
                crate::target::ExecTargetKind::Docker
            ) {
                Some(repro::ReproDocker {
                    image: input.args.docker_image.clone(),
                    workdir: input.args.docker_workdir.clone(),
                    network: format!("{:?}", input.args.docker_network).to_lowercase(),
                    user: input.args.docker_user.clone(),
                })
            } else {
                None
            },
            workdir: repro::stable_workdir_string(&input.args.workdir),
            config_hash_hex: input.config_hash_hex.to_string(),
        },
    )?;
    if let Some(r) = &repro_record {
        runtime_events::emit_event(
            event_sink,
            input.run_id,
            0,
            EventKind::ReproSnapshot,
            serde_json::json!({
                "enabled": true,
                "env_mode": r.env_mode,
                "repro_hash_hex": r.repro_hash_hex
            }),
        );
        if matches!(input.args.repro_env, ReproEnvMode::All) {
            eprintln!(
                "WARN: repro-env=all enabled; sensitive-like env vars are excluded from hash material."
            );
        }
        if let Some(path) = &input.args.repro_out {
            if let Err(e) = repro::write_repro_out(path, r) {
                eprintln!("WARN: failed to write repro snapshot: {e}");
            }
        }
    }
    Ok(repro_record)
}

pub(super) fn build_run_cli_config_fingerprint_bundle(
    input: RunCliFingerprintBuildInput<'_>,
) -> anyhow::Result<(store::RunCliConfig, store::ConfigFingerprintV1, String)> {
    let cli_config = runtime_paths::build_run_cli_config(runtime_paths::RunCliConfigInput {
        provider_kind: input.provider_kind,
        base_url: input.base_url,
        model: input.worker_model,
        args: input.args,
        resolved_settings: input.resolved_settings,
        hooks_config_path: input.hooks_config_path,
        mcp_config_path: input.mcp_config_path,
        tool_catalog: input.tool_catalog.to_vec(),
        mcp_tool_snapshot: input.mcp_tool_snapshot.to_vec(),
        mcp_tool_catalog_hash_hex: input.mcp_tool_catalog_hash_hex.clone(),
        policy_version: input.policy_version,
        includes_resolved: input.includes_resolved.to_vec(),
        mcp_allowlist: input.mcp_allowlist.clone(),
        mode: input.mode,
        planner_model: input.planner_model.map(ToOwned::to_owned),
        worker_model: input.worker_model_override.map(ToOwned::to_owned),
        planner_max_steps: input.planner_max_steps,
        planner_output: input.planner_output,
        planner_strict: input.planner_strict,
        enforce_plan_tools: input.enforce_plan_tools,
        instructions: input.instruction_resolution,
        project_guidance: input.project_guidance_resolution,
        repo_map: input.repo_map_resolution,
        lsp_context: input.lsp_context_resolution,
        activated_packs: input.activated_packs,
    });
    let config_fingerprint = runtime_paths::build_config_fingerprint(
        &cli_config,
        input.args,
        input.worker_model,
        input.paths,
    );
    let cfg_hash = config_hash_hex(&config_fingerprint)?;
    Ok((cli_config, config_fingerprint, cfg_hash))
}

pub(super) fn finalize_run_artifacts(
    input: FinalizeRunArtifactsInput<'_>,
) -> anyhow::Result<(Option<std::path::PathBuf>, Option<std::path::PathBuf>)> {
    let worker_record = input.worker_record.or_else(|| {
        Some(WorkerRunRecord {
            model: input.worker_model.to_string(),
            injected_planner_hash_hex: input
                .planner_record
                .as_ref()
                .map(|p| p.plan_hash_hex.clone()),
            step_result_valid: None,
            step_result_json: None,
            step_result_error: None,
        })
    });
    let (cli_config, config_fingerprint, config_hash_hex) =
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
    let repro_record = build_and_emit_repro_snapshot(
        input.event_sink,
        ReproSnapshotBuildInput {
            args: input.args,
            provider_kind: input.provider_kind,
            base_url: input.base_url,
            worker_model: input.worker_model,
            resolved_settings: input.resolved_settings,
            policy_hash_hex: &input.policy_hash_hex,
            includes_resolved: &input.includes_resolved,
            hooks_config_hash_hex: &input.hooks_config_hash_hex,
            tool_schema_hash_hex_map: &input.tool_schema_hash_hex_map,
            tool_catalog: input.tool_catalog,
            config_hash_hex: &config_hash_hex,
            run_id: &input.outcome.run_id,
        },
    )?;
    *input.event_sink = None;
    let run_checkpoint = super::checkpoint::checkpoint_for_outcome(input.outcome);
    let tool_facts =
        crate::agent::tool_facts_from_transcript(input.prompt, &input.outcome.tool_calls, &input.outcome.messages);
    let tool_fact_envelopes = crate::agent::tool_fact_envelopes_from_facts(
        &tool_facts,
        crate::agent::ToolFactSourceV1::Transcript,
        Some("finalize"),
        run_checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint_phase_name(&checkpoint.phase)),
    );
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
        config_hash_hex,
        outcome: input.outcome.clone(),
        mode: input.args.mode,
        planner_record: input.planner_record,
        worker_record,
        tool_schema_hash_hex_map: input.tool_schema_hash_hex_map,
        hooks_config_hash_hex: input.hooks_config_hash_hex,
        task_contract: input.task_contract.clone(),
        task_contract_provenance: input.task_contract_provenance.clone(),
        tool_facts: tool_facts.clone(),
        tool_fact_envelopes,
        run_checkpoint: run_checkpoint.clone(),
        config_fingerprint: Some(config_fingerprint),
        repro_record,
        mcp_runtime_trace: input.mcp_runtime_trace,
        mcp_pin_snapshot: input.mcp_pin_snapshot,
    });
    let runtime_checkpoint_path = if let Some(record) =
        super::checkpoint::runtime_checkpoint_record_for_outcome(
            input.outcome,
            input.prompt,
            input.args,
            &tool_facts,
        )
    {
        match store::write_runtime_checkpoint_record(input.paths, &record) {
            Ok(path) => Some(path),
            Err(e) => {
                eprintln!("WARN: failed to write runtime checkpoint: {e}");
                None
            }
        }
    } else {
        let _ = store::delete_runtime_checkpoint_record(input.paths, &input.outcome.run_id);
        None
    };
    Ok((run_artifact_path, runtime_checkpoint_path))
}

fn checkpoint_phase_name(phase: &store::RunCheckpointPhase) -> &'static str {
    match phase {
        store::RunCheckpointPhase::WaitingForApproval => "waiting_for_approval",
        store::RunCheckpointPhase::Interrupted => "interrupted",
    }
}
