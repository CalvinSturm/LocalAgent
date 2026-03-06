use std::path::PathBuf;
use std::sync::mpsc::Sender;

use crate::agent::{self, Agent, AgentExitReason, ToolCallBudget};
use crate::compaction::CompactionSettings;
use crate::events::{Event, EventKind};
use crate::gate::ProviderKind;
use crate::mcp::registry::McpRegistry;
use crate::packs;
use crate::planner;
use crate::project_guidance;
use crate::providers::ModelProvider;
use crate::repo_map;
use crate::runtime_paths;
use crate::store::{self, PlannerRunRecord, WorkerRunRecord};
use crate::tools::ToolRuntime;
use crate::types::Message;
use crate::RunArgs;
mod finalize;
mod guard;
mod launch;
mod planner_phase;
mod setup;
use finalize::{
    build_and_emit_repro_snapshot, build_run_cli_config_fingerprint_bundle,
    finalize_ui_and_session_state, normalize_and_record_worker_step_result,
    write_run_artifact_with_warning, ReproSnapshotBuildInput, RunArtifactWriteInput,
    RunCliFingerprintBuildInput,
};
use guard::maybe_append_implementation_guard_message;
use launch::{build_mcp_pin_snapshot, emit_startup_runtime_events, prepare_runtime_launch};
use planner_phase::{
    bootstrap_planner_phase, cancelled_outcome, maybe_handle_worker_replan, PlannerBootstrapInput,
    ReplanOrchestrationInput,
};

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
    let mut launch = prepare_runtime_launch(
        &provider,
        provider_kind,
        base_url,
        default_model,
        args,
        paths,
        external_ui_tx,
        shared_mcp_registry,
        suppress_stdout_stream,
    )
    .await?;
    let mcp_pin_snapshot = build_mcp_pin_snapshot(&launch);
    let run_id = uuid::Uuid::new_v4().to_string();
    emit_startup_runtime_events(&mut launch, &run_id);
    let launch::RuntimeLaunch {
        args,
        workdir,
        all_tools,
        exec_target,
        resolved_target_kind,
        mut gate_ctx,
        gate_build,
        policy_loaded_info,
        planner_strict_effective,
        planner_model,
        worker_model,
        effective_plan_tool_enforcement,
        session_store,
        mut session_data,
        resolved_settings,
        session_messages,
        task_memory,
        instruction_resolution,
        project_guidance_resolution,
        repo_map_resolution,
        activated_packs,
        mcp_config_path,
        mcp_registry,
        mcp_tool_snapshot,
        qualification_fallback_note: _qualification_fallback_note,
        mcp_tool_catalog_hash_hex,
        mcp_tool_docs_hash_hex: _mcp_tool_docs_hash_hex,
        mcp_config_hash_hex: _mcp_config_hash_hex,
        mcp_startup_live_catalog_hash_hex: _mcp_startup_live_catalog_hash_hex,
        mcp_startup_live_docs_hash_hex: _mcp_startup_live_docs_hash_hex,
        mcp_snapshot_pinned: _mcp_snapshot_pinned,
        mcp_pin_enforcement: _mcp_pin_enforcement,
        hooks_config_path,
        tool_schema_hash_hex_map,
        hooks_config_hash_hex,
        hook_manager,
        tool_catalog,
        mut event_sink,
        _cancel_tx: _cancel_tx_guard,
        mut cancel_rx,
        mut ui_join,
    } = launch;
    let policy_hash_hex = gate_build.policy_hash_hex.clone();
    let policy_source = gate_build.policy_source.to_string();
    let policy_version = gate_build.policy_version;
    let includes_resolved = gate_build.includes_resolved.clone();
    let mcp_allowlist = gate_build.mcp_allowlist.clone();
    let gate = gate_build.gate;
    let mut planner_record: Option<PlannerRunRecord> = None;
    let mut worker_record: Option<WorkerRunRecord> = None;
    let mut planner_injected_message: Option<Message> = None;
    let mut plan_step_constraints: Vec<agent::PlanStepConstraint> = Vec::new();
    if matches!(args.mode, planner::RunMode::PlannerWorker) {
        if let Some(result) = bootstrap_planner_phase(PlannerBootstrapInput {
            provider: &provider,
            provider_kind,
            base_url,
            prompt,
            args: &args,
            paths,
            run_id: &run_id,
            planner_model: &planner_model,
            worker_model: &worker_model,
            planner_strict_effective,
            effective_plan_tool_enforcement,
            resolved_settings: &resolved_settings,
            event_sink: &mut event_sink,
            ui_join: &mut ui_join,
            planner_record: &mut planner_record,
            worker_record: &mut worker_record,
            planner_injected_message: &mut planner_injected_message,
            plan_step_constraints: &mut plan_step_constraints,
            gate_ctx: &mut gate_ctx,
            hooks_config_path: &hooks_config_path,
            mcp_config_path: &mcp_config_path,
            tool_catalog: &tool_catalog,
            mcp_tool_snapshot: &mcp_tool_snapshot,
            mcp_tool_catalog_hash_hex: &mcp_tool_catalog_hash_hex,
            policy_source: policy_source.clone(),
            policy_hash_hex: policy_hash_hex.clone(),
            policy_version,
            includes_resolved: includes_resolved.clone(),
            mcp_allowlist: mcp_allowlist.clone(),
            tool_schema_hash_hex_map: tool_schema_hash_hex_map.clone(),
            hooks_config_hash_hex: hooks_config_hash_hex.clone(),
            mcp_pin_snapshot: mcp_pin_snapshot.clone(),
            instruction_resolution: &instruction_resolution,
            project_guidance_resolution: project_guidance_resolution.as_ref(),
            repo_map_resolution: repo_map_resolution.as_ref(),
            activated_packs: &activated_packs,
        })
        .await?
        {
            return Ok(result);
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
        &args,
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
    maybe_handle_worker_replan(
        ReplanOrchestrationInput {
            agent: &mut agent,
            args: &args,
            run_id: &run_id,
            prompt,
            planner_model: &planner_model,
            worker_model: &worker_model,
            planner_strict_effective,
            effective_plan_tool_enforcement,
            planner_record: &mut planner_record,
            worker_record: &mut worker_record,
            base_instruction_messages: &base_instruction_messages,
            project_guidance_message: &project_guidance_message,
            repo_map_message: &repo_map_message,
            pack_guidance_message: &pack_guidance_message,
            base_task_memory: &base_task_memory,
            resolved_settings: &resolved_settings,
            cancel_rx: &mut cancel_rx,
        },
        &mut outcome,
    )
    .await?;

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
        &args,
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
            args: &args,
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
            args: &args,
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

    use super::guard::{
        maybe_append_implementation_guard_message, should_enable_implementation_guard,
        task_kind_enforces_implementation_guard, validate_runtime_owned_http_timeouts,
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
        let err = validate_runtime_owned_http_timeouts(&args, false, None).expect_err("must fail");
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
        validate_runtime_owned_http_timeouts(&args, false, None).expect("valid timeouts");
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
        validate_runtime_owned_http_timeouts(&args, false, None)
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
        let err = validate_runtime_owned_http_timeouts(&args, true, None).expect_err("must fail");
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
