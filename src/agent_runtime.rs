use std::path::PathBuf;
use std::sync::mpsc::Sender;

use anyhow::{anyhow, Context};

use crate::agent::{
    self, Agent, AgentExitReason, PlanToolEnforcementMode, PolicyLoadedInfo, ToolCallBudget,
};
use crate::compaction::CompactionSettings;
use crate::events::{Event, EventKind};
use crate::gate::ProviderKind;
use crate::mcp::registry::McpRegistry;
use crate::packs;
use crate::planner;
use crate::project_guidance;
use crate::providers::ModelProvider;
use crate::repo_map;
use crate::run_prep;
use crate::runtime_events;
use crate::runtime_flags;
use crate::runtime_paths;
use crate::runtime_wiring;
use crate::store::{self, PlannerRunRecord, WorkerRunRecord};
use crate::tools::ToolRuntime;
use crate::trust::policy::Policy;
use crate::types::{Message, Role};
use crate::RunArgs;
mod finalize;
mod planner_phase;
mod setup;
use finalize::{
    build_and_emit_repro_snapshot, build_run_cli_config_fingerprint_bundle,
    finalize_early_run_result, finalize_ui_and_session_state,
    normalize_and_record_worker_step_result, write_run_artifact_with_warning,
    ReproSnapshotBuildInput, RunArtifactWriteInput, RunCliFingerprintBuildInput,
};
use planner_phase::{
    cancelled_outcome, emit_planner_end_event, emit_worker_start_event,
    planner_runtime_error_outcome, planner_strict_failure_outcome, prepare_replan_success_resume,
    run_planner_phase_with_start_event, run_replan_resume_with_cancel,
    run_replanner_phase_with_start_event, PlannerPhaseLaunch, ReplannerPhaseLaunch,
    ReplanResumeRunInput, ReplanSuccessPrep, ReplanSuccessPrepInput,
};
use setup::{
    build_context_augmentations, build_exec_target, build_gate_context, build_hook_and_tool_setup,
    build_session_bootstrap, build_ui_runtime_setup, resolve_mcp_runtime_registry,
    ContextAugmentations, HookToolSetup, SessionBootstrap, UiRuntimeSetup, UiRuntimeSetupInput,
};

fn task_kind_enforces_implementation_guard(
    task_kind: Option<&str>,
    selected_task_profile: Option<&str>,
) -> bool {
    let is_coding_like = |s: &str| {
        let t = s.to_ascii_lowercase();
        t.contains("coding")
            || t.contains("code")
            || t.contains("implement")
            || t.contains("fix")
            || t.contains("refactor")
            || t.contains("patch")
            || t.contains("edit")
            || t.contains("bugfix")
    };
    task_kind.is_some_and(is_coding_like) || selected_task_profile.is_some_and(is_coding_like)
}

fn should_enable_implementation_guard(args: &RunArgs, selected_task_profile: Option<&str>) -> bool {
    if args.disable_implementation_guard {
        return false;
    }
    if matches!(args.agent_mode, crate::AgentMode::Build) {
        return true;
    }
    task_kind_enforces_implementation_guard(args.task_kind.as_deref(), selected_task_profile)
}

fn maybe_append_implementation_guard_message(
    base_instruction_messages: &mut Vec<Message>,
    args: &RunArgs,
    selected_task_profile: Option<&str>,
) {
    if should_enable_implementation_guard(args, selected_task_profile) {
        base_instruction_messages.push(Message {
            role: Role::System,
            content: Some(agent::INTERNAL_ENFORCE_IMPLEMENTATION_GUARD_FLAG.to_string()),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        });
    }
}

fn validate_runtime_owned_http_timeouts(
    args: &RunArgs,
    planner_strict_effective: bool,
    selected_task_profile: Option<&str>,
) -> anyhow::Result<()> {
    let planner_strict_runtime_owned =
        planner_strict_effective && matches!(args.mode, planner::RunMode::PlannerWorker);
    let runtime_owned_mode = planner_strict_runtime_owned
        || should_enable_implementation_guard(args, selected_task_profile);
    if !runtime_owned_mode {
        return Ok(());
    }
    if args.http_timeout_ms == 0 {
        return Err(anyhow!(
            "invalid timeout config for strict/runtime-owned mode: --http-timeout-ms must be > 0"
        ));
    }
    if args.http_stream_idle_timeout_ms == 0 {
        return Err(anyhow!(
            "invalid timeout config for strict/runtime-owned mode: --http-stream-idle-timeout-ms must be > 0"
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_agent<P: ModelProvider>(
    provider: P,
    provider_kind: ProviderKind,
    base_url: &str,
    default_model: &str,
    prompt: &str,
    args: &RunArgs,
    paths: &store::StatePaths,
) -> anyhow::Result<RunExecutionResult> {
    run_agent_with_ui(
        provider,
        provider_kind,
        base_url,
        default_model,
        prompt,
        args,
        paths,
        None,
        None,
        None,
        false,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_agent_with_ui<P: ModelProvider>(
    provider: P,
    provider_kind: ProviderKind,
    base_url: &str,
    default_model: &str,
    prompt: &str,
    args: &RunArgs,
    paths: &store::StatePaths,
    external_ui_tx: Option<Sender<Event>>,
    external_operator_queue_rx: Option<
        std::sync::mpsc::Receiver<crate::operator_queue::QueueSubmitRequest>,
    >,
    shared_mcp_registry: Option<std::sync::Arc<McpRegistry>>,
    suppress_stdout_stream: bool,
) -> anyhow::Result<RunExecutionResult> {
    let mut effective_args = args.clone();
    runtime_flags::apply_agent_mode_capability_baseline(
        &mut effective_args,
        runtime_flags::parse_capability_explicit_flags(),
    );
    let args = &effective_args;
    let workdir = std::fs::canonicalize(&args.workdir)
        .with_context(|| format!("failed to resolve workdir: {}", args.workdir.display()))?;
    let exec_target = build_exec_target(args)?;
    let resolved_target_kind = exec_target.kind();
    let _target_desc = exec_target.describe();
    let mut gate_ctx = build_gate_context(
        args,
        &workdir,
        provider_kind,
        default_model,
        resolved_target_kind,
    );
    let gate_build = runtime_wiring::build_gate(args, paths)?;
    let policy_hash_hex = gate_build.policy_hash_hex.clone();
    let policy_source = gate_build.policy_source.to_string();
    let policy_version = gate_build.policy_version;
    let includes_resolved = gate_build.includes_resolved.clone();
    let mcp_allowlist = gate_build.mcp_allowlist.clone();
    let policy_loaded_info = policy_version.map(|version| PolicyLoadedInfo {
        version,
        rules_count: gate_build
            .policy_for_exposure
            .as_ref()
            .map(Policy::rules_len)
            .unwrap_or(0),
        includes_count: includes_resolved.len(),
        includes_resolved: includes_resolved.clone(),
        mcp_allowlist: mcp_allowlist.clone(),
    });
    let gate = gate_build.gate;

    let planner_strict_effective = if args.no_planner_strict {
        false
    } else {
        args.planner_strict
    };
    let planner_model = args
        .planner_model
        .clone()
        .unwrap_or_else(|| default_model.to_string());
    let worker_model = args
        .worker_model
        .clone()
        .unwrap_or_else(|| default_model.to_string());
    let plan_enforcement_explicit = runtime_flags::has_explicit_plan_tool_enforcement_flag();
    let effective_plan_tool_enforcement = runtime_flags::resolve_plan_tool_enforcement(
        args.mode,
        args.enforce_plan_tools,
        plan_enforcement_explicit,
    );
    gate_ctx.model = worker_model.clone();

    let SessionBootstrap {
        session_store,
        mut session_data,
        resolved_settings,
        session_messages,
        task_memory,
    } = build_session_bootstrap(args, paths)?;
    let ContextAugmentations {
        instruction_resolution,
        project_guidance_resolution,
        repo_map_resolution,
        activated_packs,
    } = build_context_augmentations(args, paths, &worker_model)?;
    validate_runtime_owned_http_timeouts(
        args,
        planner_strict_effective,
        instruction_resolution.selected_task_profile.as_deref(),
    )?;

    let (mcp_config_path, mcp_registry) =
        resolve_mcp_runtime_registry(args, paths, shared_mcp_registry).await?;

    let prep = run_prep::prepare_tools_and_qualification(run_prep::PrepareToolsInput {
        provider: &provider,
        provider_kind,
        base_url,
        worker_model: &worker_model,
        args,
        state_dir: &paths.state_dir,
        mcp_config_path: &mcp_config_path,
        mcp_registry: mcp_registry.as_ref(),
        policy_for_exposure: gate_build.policy_for_exposure.as_ref(),
    })
    .await?;
    let all_tools = prep.all_tools;
    let mcp_tool_snapshot = prep.mcp_tool_snapshot;
    let qualification_fallback_note = prep.qualification_fallback_note;
    if let Some(note) = &qualification_fallback_note {
        eprintln!("WARN: {note}");
    }
    let mcp_tool_catalog_hash_hex = prep.mcp_tool_catalog_hash_hex;
    let mcp_tool_docs_hash_hex = prep.mcp_tool_docs_hash_hex;
    let mcp_config_hash_hex = prep.mcp_config_hash_hex;
    let mcp_startup_live_catalog_hash_hex = prep.mcp_startup_live_catalog_hash_hex;
    let mcp_startup_live_docs_hash_hex = prep.mcp_startup_live_docs_hash_hex;
    let mcp_snapshot_pinned = prep.mcp_snapshot_pinned;
    let mcp_pin_enforcement = format!("{:?}", args.mcp_pin_enforcement).to_lowercase();
    let HookToolSetup {
        hooks_config_path,
        tool_schema_hash_hex_map,
        hooks_config_hash_hex,
        hook_manager,
        tool_catalog,
    } = build_hook_and_tool_setup(args, paths, &resolved_settings, &all_tools)?;
    gate_ctx.tool_schema_hashes = tool_schema_hash_hex_map.clone();
    gate_ctx.hooks_config_hash_hex = hooks_config_hash_hex.clone();

    let UiRuntimeSetup {
        mut event_sink,
        _cancel_tx: _cancel_tx_guard,
        mut cancel_rx,
        ui_join,
    } = build_ui_runtime_setup(UiRuntimeSetupInput {
        args,
        paths,
        provider_kind,
        worker_model: &worker_model,
        mcp_pin_enforcement: &mcp_pin_enforcement,
        resolved_settings: &resolved_settings,
        policy_hash_hex: &policy_hash_hex,
        mcp_tool_catalog_hash_hex: &mcp_tool_catalog_hash_hex,
        external_ui_tx,
        suppress_stdout_stream,
    })?;

    let run_id = uuid::Uuid::new_v4().to_string();
    let mut planner_record: Option<PlannerRunRecord> = None;
    let mut worker_record: Option<WorkerRunRecord> = None;
    let mut planner_injected_message: Option<Message> = None;
    let mut plan_step_constraints: Vec<agent::PlanStepConstraint> = Vec::new();
    let mcp_pin_snapshot = if mcp_tool_catalog_hash_hex.is_some()
        || mcp_startup_live_catalog_hash_hex.is_some()
        || mcp_tool_docs_hash_hex.is_some()
        || mcp_startup_live_docs_hash_hex.is_some()
    {
        Some(store::McpPinSnapshotRecord {
            enforcement: mcp_pin_enforcement.clone(),
            configured_catalog_hash_hex: mcp_tool_catalog_hash_hex.clone().unwrap_or_default(),
            startup_live_catalog_hash_hex: mcp_startup_live_catalog_hash_hex.clone(),
            configured_docs_hash_hex: mcp_tool_docs_hash_hex.clone(),
            startup_live_docs_hash_hex: mcp_startup_live_docs_hash_hex.clone(),
            mcp_config_hash_hex: mcp_config_hash_hex.clone(),
            pinned: mcp_snapshot_pinned,
        })
    } else {
        None
    };
    runtime_events::emit_event(
        &mut event_sink,
        &run_id,
        0,
        EventKind::McpPinned,
        serde_json::json!({
            "enforcement": mcp_pin_enforcement,
            "configured_hash_hex": mcp_tool_catalog_hash_hex,
            "startup_live_hash_hex": mcp_startup_live_catalog_hash_hex,
            "configured_docs_hash_hex": mcp_tool_docs_hash_hex,
            "startup_live_docs_hash_hex": mcp_startup_live_docs_hash_hex,
            "mcp_config_hash_hex": mcp_config_hash_hex,
            "pinned": mcp_snapshot_pinned
        }),
    );
    for p in &activated_packs {
        runtime_events::emit_event(
            &mut event_sink,
            &run_id,
            0,
            EventKind::PackActivated,
            serde_json::json!({
                "schema": "openagent.pack_activated.v1",
                "pack_id": p.pack_id,
                "pack_hash_hex": p.pack_hash_hex,
                "truncated": p.truncated,
                "bytes_kept": p.bytes_kept
            }),
        );
    }
    if let Some(note) = &qualification_fallback_note {
        runtime_events::emit_event(
            &mut event_sink,
            &run_id,
            0,
            EventKind::Error,
            serde_json::json!({
                "error": note,
                "source": "orchestrator_qualification_fallback"
            }),
        );
    }

    if matches!(args.mode, planner::RunMode::PlannerWorker) {
        let planner_out = run_planner_phase_with_start_event(
            &provider,
            PlannerPhaseLaunch {
                run_id: &run_id,
                planner_model: &planner_model,
                prompt,
                planner_max_steps: args.planner_max_steps,
                planner_output: args.planner_output,
                planner_strict: planner_strict_effective,
                effective_plan_tool_enforcement,
            },
            &mut event_sink,
        )
        .await;
        match planner_out {
            Ok(out) => {
                if planner_strict_effective && !out.ok {
                    emit_planner_end_event(
                        &mut event_sink,
                        &run_id,
                        false,
                        &out.plan_hash_hex,
                        &out.error
                            .clone()
                            .unwrap_or_else(|| "planner validation failed".to_string()),
                        None,
                        None,
                    );
                    let outcome = planner_strict_failure_outcome(
                        &run_id,
                        &resolved_settings,
                        out.error.clone(),
                        out.raw_output.clone(),
                    );
                    planner_record = Some(PlannerRunRecord {
                        model: planner_model.clone(),
                        max_steps: args.planner_max_steps,
                        strict: planner_strict_effective,
                        output_format: format!("{:?}", args.planner_output).to_lowercase(),
                        plan_json: out.plan_json,
                        plan_hash_hex: out.plan_hash_hex,
                        ok: false,
                        raw_output: out.raw_output,
                        error: out.error,
                    });
                    worker_record = Some(WorkerRunRecord {
                        model: worker_model.clone(),
                        injected_planner_hash_hex: None,
                        step_result_valid: None,
                        step_result_json: None,
                        step_result_error: None,
                    });
                    let (cli_config, config_fingerprint, cfg_hash) =
                        build_run_cli_config_fingerprint_bundle(RunCliFingerprintBuildInput {
                            provider_kind,
                            base_url,
                            worker_model: &worker_model,
                            args,
                            paths,
                            resolved_settings: &resolved_settings,
                            hooks_config_path: &hooks_config_path,
                            mcp_config_path: &mcp_config_path,
                            tool_catalog: &tool_catalog,
                            mcp_tool_snapshot: &mcp_tool_snapshot,
                            mcp_tool_catalog_hash_hex: &mcp_tool_catalog_hash_hex,
                            policy_version,
                            includes_resolved: &includes_resolved,
                            mcp_allowlist: &mcp_allowlist,
                            mode: args.mode,
                            planner_model: Some(&planner_model),
                            worker_model_override: Some(&worker_model),
                            planner_max_steps: Some(args.planner_max_steps),
                            planner_output: Some(
                                format!("{:?}", args.planner_output).to_lowercase(),
                            ),
                            planner_strict: Some(planner_strict_effective),
                            enforce_plan_tools: Some(
                                format!("{:?}", effective_plan_tool_enforcement).to_lowercase(),
                            ),
                            instruction_resolution: &instruction_resolution,
                            project_guidance_resolution: project_guidance_resolution.as_ref(),
                            repo_map_resolution: repo_map_resolution.as_ref(),
                            activated_packs: &activated_packs,
                        })?;
                    let run_artifact_path =
                        write_run_artifact_with_warning(RunArtifactWriteInput {
                            paths: paths.clone(),
                            cli_config,
                            policy_info: store::PolicyRecordInfo {
                                source: policy_source,
                                hash_hex: policy_hash_hex,
                                version: policy_version,
                                includes_resolved,
                                mcp_allowlist,
                            },
                            config_hash_hex: cfg_hash,
                            outcome: outcome.clone(),
                            mode: args.mode,
                            planner_record,
                            worker_record,
                            tool_schema_hash_hex_map: tool_schema_hash_hex_map.clone(),
                            hooks_config_hash_hex: hooks_config_hash_hex.clone(),
                            config_fingerprint: Some(config_fingerprint.clone()),
                            repro_record: None,
                            mcp_runtime_trace: Vec::new(),
                            mcp_pin_snapshot: mcp_pin_snapshot.clone(),
                        });
                    return finalize_early_run_result(ui_join, outcome, run_artifact_path);
                }
                emit_planner_end_event(
                    &mut event_sink,
                    &run_id,
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
                    effective_plan_tool_enforcement,
                    PlanToolEnforcementMode::Soft | PlanToolEnforcementMode::Hard
                ) {
                    match planner::extract_plan_step_tools(&out.plan_json) {
                        Ok(steps) => {
                            plan_step_constraints = steps
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
                planner_injected_message = Some(Message {
                    role: Role::Developer,
                    content: Some(handoff),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                });
                gate_ctx.planner_hash_hex = Some(out.plan_hash_hex.clone());
                worker_record = Some(WorkerRunRecord {
                    model: worker_model.clone(),
                    injected_planner_hash_hex: Some(out.plan_hash_hex.clone()),
                    step_result_valid: None,
                    step_result_json: None,
                    step_result_error: None,
                });
                planner_record = Some(PlannerRunRecord {
                    model: planner_model.clone(),
                    max_steps: args.planner_max_steps,
                    strict: planner_strict_effective,
                    output_format: format!("{:?}", args.planner_output).to_lowercase(),
                    plan_json: out.plan_json,
                    plan_hash_hex: out.plan_hash_hex,
                    ok: out.ok,
                    raw_output: out.raw_output,
                    error: out.error,
                });
                emit_worker_start_event(
                    &mut event_sink,
                    &run_id,
                    &worker_model,
                    &planner_record
                        .as_ref()
                        .map(|p| p.plan_hash_hex.clone())
                        .unwrap_or_default(),
                    effective_plan_tool_enforcement,
                    None,
                );
            }
            Err(e) => {
                let err_short = e.to_string();
                emit_planner_end_event(&mut event_sink, &run_id, false, "", &err_short, None, None);
                let outcome = planner_runtime_error_outcome(
                    &run_id,
                    &resolved_settings,
                    e.to_string(),
                    prompt,
                );
                let (cli_config, config_fingerprint, cfg_hash) =
                    build_run_cli_config_fingerprint_bundle(RunCliFingerprintBuildInput {
                        provider_kind,
                        base_url,
                        worker_model: &worker_model,
                        args,
                        paths,
                        resolved_settings: &resolved_settings,
                        hooks_config_path: &hooks_config_path,
                        mcp_config_path: &mcp_config_path,
                        tool_catalog: &tool_catalog,
                        mcp_tool_snapshot: &mcp_tool_snapshot,
                        mcp_tool_catalog_hash_hex: &mcp_tool_catalog_hash_hex,
                        policy_version,
                        includes_resolved: &includes_resolved,
                        mcp_allowlist: &mcp_allowlist,
                        mode: args.mode,
                        planner_model: Some(&planner_model),
                        worker_model_override: Some(&worker_model),
                        planner_max_steps: Some(args.planner_max_steps),
                        planner_output: Some(format!("{:?}", args.planner_output).to_lowercase()),
                        planner_strict: Some(planner_strict_effective),
                        enforce_plan_tools: Some(
                            format!("{:?}", effective_plan_tool_enforcement).to_lowercase(),
                        ),
                        instruction_resolution: &instruction_resolution,
                        project_guidance_resolution: project_guidance_resolution.as_ref(),
                        repo_map_resolution: repo_map_resolution.as_ref(),
                        activated_packs: &activated_packs,
                    })?;
                let run_artifact_path = write_run_artifact_with_warning(RunArtifactWriteInput {
                    paths: paths.clone(),
                    cli_config,
                    policy_info: store::PolicyRecordInfo {
                        source: policy_source,
                        hash_hex: policy_hash_hex,
                        version: policy_version,
                        includes_resolved,
                        mcp_allowlist,
                    },
                    config_hash_hex: cfg_hash,
                    outcome: outcome.clone(),
                    mode: args.mode,
                    planner_record,
                    worker_record,
                    tool_schema_hash_hex_map: tool_schema_hash_hex_map.clone(),
                    hooks_config_hash_hex: hooks_config_hash_hex.clone(),
                    config_fingerprint: Some(config_fingerprint.clone()),
                    repro_record: None,
                    mcp_runtime_trace: Vec::new(),
                    mcp_pin_snapshot: mcp_pin_snapshot.clone(),
                });
                return finalize_early_run_result(ui_join, outcome, run_artifact_path);
            }
        }
    }

    let mut agent = Agent {
        provider,
        model: worker_model.clone(),
        temperature: args.temperature,
        top_p: args.top_p,
        max_tokens: args.max_tokens,
        seed: args.seed,
        tools: all_tools,
        max_steps: args.max_steps,
        tool_rt: ToolRuntime {
            workdir,
            allow_shell: args.allow_shell,
            allow_shell_in_workdir_only: args.allow_shell_in_workdir,
            allow_write: args.allow_write,
            max_tool_output_bytes: if args.no_limits {
                0
            } else {
                args.max_tool_output_bytes
            },
            max_read_bytes: if args.no_limits {
                0
            } else {
                args.max_read_bytes
            },
            unsafe_bypass_allow_flags: args.unsafe_bypass_allow_flags,
            tool_args_strict: resolved_settings.tool_args_strict,
            exec_target_kind: resolved_target_kind,
            exec_target,
        },
        gate,
        gate_ctx,
        mcp_registry,
        stream: args.stream,
        event_sink,
        compaction_settings: CompactionSettings {
            max_context_chars: resolved_settings.max_context_chars,
            mode: resolved_settings.compaction_mode,
            keep_last: resolved_settings.compaction_keep_last,
            tool_result_persist: resolved_settings.tool_result_persist,
        },
        hooks: hook_manager,
        policy_loaded: policy_loaded_info,
        policy_for_taint: gate_build.policy_for_exposure.clone(),
        taint_toggle: args.taint,
        taint_mode: args.taint_mode,
        taint_digest_bytes: args.taint_digest_bytes,
        run_id_override: Some(run_id.clone()),
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: effective_plan_tool_enforcement,
        mcp_pin_enforcement: args.mcp_pin_enforcement,
        plan_step_constraints,
        tool_call_budget: ToolCallBudget {
            max_wall_time_ms: if args.no_limits {
                0
            } else {
                args.max_wall_time_ms
            },
            max_total_tool_calls: args.max_total_tool_calls,
            max_mcp_calls: args.max_mcp_calls,
            max_filesystem_read_calls: args.max_filesystem_read_calls,
            max_filesystem_write_calls: args.max_filesystem_write_calls,
            max_shell_calls: args.max_shell_calls,
            max_network_calls: args.max_network_calls,
            max_browser_calls: args.max_browser_calls,
            tool_exec_timeout_ms: if args.no_limits {
                0
            } else {
                args.tool_exec_timeout_ms
            },
            post_write_verify_timeout_ms: if args.no_limits {
                0
            } else {
                args.post_write_verify_timeout_ms
            },
        },
        mcp_runtime_trace: Vec::new(),
        operator_queue: crate::operator_queue::PendingMessageQueue::default(),
        operator_queue_limits: crate::operator_queue::QueueLimits::default(),
        operator_queue_rx: external_operator_queue_rx,
    };

    let mut base_instruction_messages = instruction_resolution.messages.clone();
    maybe_append_implementation_guard_message(
        &mut base_instruction_messages,
        args,
        instruction_resolution.selected_task_profile.as_deref(),
    );
    let project_guidance_message = project_guidance_resolution
        .as_ref()
        .and_then(project_guidance::project_guidance_message);
    let repo_map_message = repo_map_resolution
        .as_ref()
        .and_then(repo_map::repo_map_message);
    let pack_guidance_message = packs::pack_guidance_message(&activated_packs);
    let base_task_memory = task_memory.clone();
    let initial_injected_messages = runtime_paths::merge_injected_messages(
        base_instruction_messages.clone(),
        project_guidance_message.clone(),
        repo_map_message.clone(),
        pack_guidance_message.clone(),
        base_task_memory.clone(),
        planner_injected_message.clone(),
    );

    let mut outcome = tokio::select! {
        out = agent.run(
            prompt,
            session_messages.clone(),
            initial_injected_messages,
        ) => out,
        _ = tokio::signal::ctrl_c() => {
            cancelled_outcome(&resolved_settings)
        },
        _ = async {
            let _ = cancel_rx.changed().await;
        } => {
            cancelled_outcome(&resolved_settings)
        }
    };

    if matches!(args.mode, planner::RunMode::PlannerWorker)
        && matches!(outcome.exit_reason, AgentExitReason::PlannerError)
        && outcome
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("worker requested replan transition")
    {
        let replanner_reason = outcome
            .error
            .clone()
            .unwrap_or_else(|| "worker requested replan transition".to_string());
        let prior_plan_json = planner_record
            .as_ref()
            .map(|p| p.plan_json.clone())
            .unwrap_or_else(|| serde_json::json!({}));
        let prior_plan_hash = planner_record
            .as_ref()
            .map(|p| p.plan_hash_hex.clone())
            .unwrap_or_default();
        let prior_plan_text =
            serde_json::to_string_pretty(&prior_plan_json).unwrap_or_else(|_| "{}".to_string());
        let replan_prompt = format!(
            "{prompt}\n\nREPLAN CONTEXT\nPrevious plan hash: {prior_plan_hash}\nPrevious normalized plan:\n{prior_plan_text}\n\nRuntime requested a replan because: {replanner_reason}\nReturn an updated openagent.plan.v1 JSON plan for remaining work only."
        );
        match run_replanner_phase_with_start_event(
            &agent.provider,
            ReplannerPhaseLaunch {
                run_id: &run_id,
                planner_model: &planner_model,
                replanner_reason: &replanner_reason,
                replan_prompt: &replan_prompt,
                planner_max_steps: args.planner_max_steps,
                planner_output: args.planner_output,
                planner_strict: planner_strict_effective,
            },
            &mut agent.event_sink,
        )
        .await
        {
            Ok(replan_out) if !planner_strict_effective || replan_out.ok => {
                emit_planner_end_event(
                    &mut agent.event_sink,
                    &run_id,
                    replan_out.ok,
                    &replan_out.plan_hash_hex,
                    "",
                    Some("replan"),
                    Some(&prior_plan_hash),
                );
                let ReplanSuccessPrep { replan_handoff } = prepare_replan_success_resume(
                    ReplanSuccessPrepInput {
                        agent: &mut agent,
                        run_id: &run_id,
                        planner_model: &planner_model,
                        worker_model: &worker_model,
                        planner_max_steps: args.planner_max_steps,
                        planner_output: args.planner_output,
                        planner_strict_effective,
                        effective_plan_tool_enforcement,
                        worker_record: &mut worker_record,
                        planner_record: &mut planner_record,
                    },
                    replan_out,
                )?;
                outcome = run_replan_resume_with_cancel(ReplanResumeRunInput {
                    agent: &mut agent,
                    prompt,
                    prior_outcome: &outcome,
                    base_instruction_messages: &base_instruction_messages,
                    project_guidance_message: &project_guidance_message,
                    repo_map_message: &repo_map_message,
                    pack_guidance_message: &pack_guidance_message,
                    base_task_memory: &base_task_memory,
                    replan_handoff,
                    resolved_settings: &resolved_settings,
                    cancel_rx: &mut cancel_rx,
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
    }

    if matches!(outcome.exit_reason, AgentExitReason::Cancelled) {
        if let Some(sink) = &mut agent.event_sink {
            if let Err(e) = sink.emit(Event::new(
                outcome.run_id.clone(),
                0,
                EventKind::RunEnd,
                serde_json::json!({"exit_reason":"cancelled"}),
            )) {
                eprintln!("WARN: failed to emit cancellation event: {e}");
            }
        }
    }
    if matches!(args.mode, planner::RunMode::PlannerWorker) {
        normalize_and_record_worker_step_result(
            &mut outcome,
            planner_record.as_ref(),
            &mut worker_record,
            planner_strict_effective,
        );
    }
    finalize_ui_and_session_state(
        ui_join,
        args,
        &session_store,
        &mut session_data,
        &resolved_settings,
        &outcome,
    );

    if worker_record.is_none() {
        worker_record = Some(WorkerRunRecord {
            model: worker_model.clone(),
            injected_planner_hash_hex: planner_record.as_ref().map(|p| p.plan_hash_hex.clone()),
            step_result_valid: None,
            step_result_json: None,
            step_result_error: None,
        });
    }
    let (cli_config, config_fingerprint, config_hash_hex) =
        build_run_cli_config_fingerprint_bundle(RunCliFingerprintBuildInput {
            provider_kind,
            base_url,
            worker_model: &worker_model,
            args,
            paths,
            resolved_settings: &resolved_settings,
            hooks_config_path: &hooks_config_path,
            mcp_config_path: &mcp_config_path,
            tool_catalog: &tool_catalog,
            mcp_tool_snapshot: &mcp_tool_snapshot,
            mcp_tool_catalog_hash_hex: &mcp_tool_catalog_hash_hex,
            policy_version,
            includes_resolved: &includes_resolved,
            mcp_allowlist: &mcp_allowlist,
            mode: args.mode,
            planner_model: Some(&planner_model),
            worker_model_override: Some(&worker_model),
            planner_max_steps: Some(args.planner_max_steps),
            planner_output: Some(format!("{:?}", args.planner_output).to_lowercase()),
            planner_strict: Some(planner_strict_effective),
            enforce_plan_tools: Some(
                format!("{:?}", effective_plan_tool_enforcement).to_lowercase(),
            ),
            instruction_resolution: &instruction_resolution,
            project_guidance_resolution: project_guidance_resolution.as_ref(),
            repo_map_resolution: repo_map_resolution.as_ref(),
            activated_packs: &activated_packs,
        })?;
    let repro_record = build_and_emit_repro_snapshot(
        &mut agent.event_sink,
        ReproSnapshotBuildInput {
            args,
            provider_kind,
            base_url,
            worker_model: &worker_model,
            resolved_settings: &resolved_settings,
            policy_hash_hex: &policy_hash_hex,
            includes_resolved: &includes_resolved,
            hooks_config_hash_hex: &hooks_config_hash_hex,
            tool_schema_hash_hex_map: &tool_schema_hash_hex_map,
            tool_catalog: &tool_catalog,
            config_hash_hex: &config_hash_hex,
            run_id: &outcome.run_id,
        },
    )?;
    agent.event_sink = None;
    let run_artifact_path = write_run_artifact_with_warning(RunArtifactWriteInput {
        paths: paths.clone(),
        cli_config,
        policy_info: store::PolicyRecordInfo {
            source: policy_source,
            hash_hex: policy_hash_hex,
            version: policy_version,
            includes_resolved,
            mcp_allowlist,
        },
        config_hash_hex,
        outcome: outcome.clone(),
        mode: args.mode,
        planner_record,
        worker_record,
        tool_schema_hash_hex_map,
        hooks_config_hash_hex,
        config_fingerprint: Some(config_fingerprint.clone()),
        repro_record,
        mcp_runtime_trace: agent.mcp_runtime_trace.clone(),
        mcp_pin_snapshot,
    });

    if !suppress_stdout_stream {
        if args.tui {
            if !outcome.final_output.is_empty() {
                println!("{}", outcome.final_output);
            }
        } else if !args.stream && !matches!(args.output, crate::RunOutputMode::Json) {
            println!("{}", outcome.final_output);
        }
    }

    Ok(RunExecutionResult {
        outcome,
        run_artifact_path,
    })
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use tempfile::tempdir;

    use super::{
        maybe_append_implementation_guard_message, should_enable_implementation_guard,
        task_kind_enforces_implementation_guard,
    };
    use crate::gate::ProviderKind;
    use crate::providers::mock::MockProvider;
    use crate::types::{Message, Role};

    #[test]
    fn build_mode_enables_implementation_guard_by_default() {
        let args = crate::RunArgs::parse_from(["localagent", "--agent-mode", "build"]);
        assert!(should_enable_implementation_guard(&args, None));
    }

    #[test]
    fn coding_task_kind_enables_implementation_guard_in_plan_mode() {
        let args = crate::RunArgs::parse_from([
            "localagent",
            "--agent-mode",
            "plan",
            "--task-kind",
            "coding",
        ]);
        assert!(task_kind_enforces_implementation_guard(
            args.task_kind.as_deref(),
            None
        ));
        assert!(should_enable_implementation_guard(&args, None));
    }

    #[test]
    fn explicit_opt_out_disables_implementation_guard() {
        let args = crate::RunArgs::parse_from([
            "localagent",
            "--agent-mode",
            "build",
            "--disable-implementation-guard",
        ]);
        assert!(!should_enable_implementation_guard(&args, Some("coding")));
    }

    #[test]
    fn guard_message_is_injected_only_when_enabled() {
        let mut enabled_msgs = vec![Message {
            role: Role::System,
            content: Some("base".to_string()),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        }];
        let enabled_args = crate::RunArgs::parse_from(["localagent", "--agent-mode", "build"]);
        maybe_append_implementation_guard_message(&mut enabled_msgs, &enabled_args, None);
        assert!(enabled_msgs.iter().any(|m| {
            m.content
                .as_deref()
                .is_some_and(|c| c == crate::agent::INTERNAL_ENFORCE_IMPLEMENTATION_GUARD_FLAG)
        }));

        let mut disabled_msgs = vec![Message {
            role: Role::System,
            content: Some("base".to_string()),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        }];
        let disabled_args = crate::RunArgs::parse_from([
            "localagent",
            "--agent-mode",
            "build",
            "--disable-implementation-guard",
        ]);
        maybe_append_implementation_guard_message(
            &mut disabled_msgs,
            &disabled_args,
            Some("coding"),
        );
        assert!(!disabled_msgs.iter().any(|m| {
            m.content
                .as_deref()
                .is_some_and(|c| c == crate::agent::INTERNAL_ENFORCE_IMPLEMENTATION_GUARD_FLAG)
        }));
    }

    #[test]
    fn runtime_owned_mode_rejects_zero_http_timeouts() {
        let args = crate::RunArgs::parse_from([
            "localagent",
            "--agent-mode",
            "build",
            "--http-timeout-ms",
            "0",
            "--http-stream-idle-timeout-ms",
            "0",
        ]);
        let err =
            super::validate_runtime_owned_http_timeouts(&args, false, None).expect_err("must fail");
        assert!(err.to_string().contains("--http-timeout-ms must be > 0"));
    }

    #[test]
    fn runtime_owned_mode_accepts_nonzero_http_timeouts() {
        let args = crate::RunArgs::parse_from([
            "localagent",
            "--agent-mode",
            "build",
            "--http-timeout-ms",
            "60000",
            "--http-stream-idle-timeout-ms",
            "15000",
        ]);
        super::validate_runtime_owned_http_timeouts(&args, false, None).expect("valid timeouts");
    }

    #[test]
    fn explicit_opt_out_allows_zero_http_timeouts() {
        let args = crate::RunArgs::parse_from([
            "localagent",
            "--agent-mode",
            "build",
            "--disable-implementation-guard",
            "--http-timeout-ms",
            "0",
            "--http-stream-idle-timeout-ms",
            "0",
        ]);
        super::validate_runtime_owned_http_timeouts(&args, false, None)
            .expect("opt-out allows non-strict zero timeout");
    }

    #[test]
    fn planner_strict_mode_rejects_zero_http_timeouts() {
        let args = crate::RunArgs::parse_from([
            "localagent",
            "--agent-mode",
            "plan",
            "--mode",
            "planner-worker",
            "--http-timeout-ms",
            "0",
            "--http-stream-idle-timeout-ms",
            "0",
        ]);
        let err =
            super::validate_runtime_owned_http_timeouts(&args, true, None).expect_err("must fail");
        assert!(err.to_string().contains("--http-timeout-ms must be > 0"));
    }

    #[tokio::test]
    async fn run_agent_rejects_zero_http_timeouts_in_runtime_owned_mode() {
        let tmp = tempdir().expect("tempdir");
        let paths = crate::store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut args = crate::RunArgs::parse_from(["localagent", "--agent-mode", "build"]);
        args.http_timeout_ms = 0;
        args.http_stream_idle_timeout_ms = 0;
        args.workdir = tmp.path().to_path_buf();
        let err = super::run_agent(
            MockProvider::new(),
            ProviderKind::Mock,
            "mock://local",
            "mock-model",
            "say hi",
            &args,
            &paths,
        )
        .await
        .expect_err("must fail for zero timeout in runtime-owned mode");
        assert!(err.to_string().contains("--http-timeout-ms must be > 0"));
    }

    #[tokio::test]
    async fn run_agent_allows_zero_http_timeouts_with_explicit_opt_out() {
        let tmp = tempdir().expect("tempdir");
        let paths = crate::store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut args = crate::RunArgs::parse_from([
            "localagent",
            "--agent-mode",
            "build",
            "--disable-implementation-guard",
        ]);
        args.http_timeout_ms = 0;
        args.http_stream_idle_timeout_ms = 0;
        args.workdir = tmp.path().to_path_buf();
        let out = super::run_agent(
            MockProvider::new(),
            ProviderKind::Mock,
            "mock://local",
            "mock-model",
            "say hi",
            &args,
            &paths,
        )
        .await
        .expect("opt-out should allow zero timeout");
        assert!(matches!(
            out.outcome.exit_reason,
            crate::AgentExitReason::Ok
        ));
    }
}


#[derive(Debug, Clone)]
pub(crate) struct RunExecutionResult {
    pub(crate) outcome: agent::AgentOutcome,
    pub(crate) run_artifact_path: Option<PathBuf>,
}
