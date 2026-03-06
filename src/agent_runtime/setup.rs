use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::Context;
use tokio::sync::watch;

use crate::events::Event;
use crate::gate::{ApprovalMode, GateContext, ProviderKind};
use crate::hooks::runner::{HookManager, HookRuntimeConfig};
use crate::mcp::registry::McpRegistry;
use crate::ops_helpers;
use crate::packs;
use crate::project_guidance;
use crate::repo_map;
use crate::runtime_flags;
use crate::runtime_paths;
use crate::runtime_wiring;
use crate::session::{self, task_memory_message, RunSettingInputs, SessionStore};
use crate::store::{self, provider_to_string};
use crate::taint;
use crate::taint::TaintToggle;
use crate::target::{DockerTarget, ExecTarget, ExecTargetKind, HostTarget};
use crate::types::Message;
use crate::{instruction_runtime, tui, DockerNetwork, RunArgs};

pub(super) struct SessionBootstrap {
    pub(super) session_store: SessionStore,
    pub(super) session_data: session::SessionData,
    pub(super) resolved_settings: session::RunSettingResolution,
    pub(super) session_messages: Vec<Message>,
    pub(super) task_memory: Option<Message>,
}

pub(super) struct ContextAugmentations {
    pub(super) instruction_resolution: crate::instructions::InstructionResolution,
    pub(super) project_guidance_resolution: Option<project_guidance::ResolvedProjectGuidance>,
    pub(super) repo_map_resolution: Option<repo_map::ResolvedRepoMap>,
    pub(super) activated_packs: Vec<packs::ActivatedPack>,
}

pub(super) struct UiRuntimeSetup {
    pub(super) event_sink: Option<Box<dyn crate::events::EventSink>>,
    pub(super) _cancel_tx: watch::Sender<bool>,
    pub(super) cancel_rx: watch::Receiver<bool>,
    pub(super) ui_join: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
}

pub(super) struct UiRuntimeSetupInput<'a> {
    pub(super) args: &'a RunArgs,
    pub(super) paths: &'a store::StatePaths,
    pub(super) provider_kind: ProviderKind,
    pub(super) worker_model: &'a str,
    pub(super) mcp_pin_enforcement: &'a str,
    pub(super) resolved_settings: &'a session::RunSettingResolution,
    pub(super) policy_hash_hex: &'a Option<String>,
    pub(super) mcp_tool_catalog_hash_hex: &'a Option<String>,
    pub(super) external_ui_tx: Option<Sender<Event>>,
    pub(super) suppress_stdout_stream: bool,
}

pub(super) struct HookToolSetup {
    pub(super) hooks_config_path: std::path::PathBuf,
    pub(super) tool_schema_hash_hex_map: std::collections::BTreeMap<String, String>,
    pub(super) hooks_config_hash_hex: Option<String>,
    pub(super) hook_manager: HookManager,
    pub(super) tool_catalog: Vec<store::ToolCatalogEntry>,
}

pub(super) fn build_exec_target(args: &RunArgs) -> anyhow::Result<std::sync::Arc<dyn ExecTarget>> {
    match args.exec_target {
        ExecTargetKind::Host => Ok(std::sync::Arc::new(HostTarget)),
        ExecTargetKind::Docker => {
            DockerTarget::validate_available().with_context(|| {
                "docker execution target requested. Install/start Docker or re-run with --exec-target host"
            })?;
            DockerTarget::validate_image_present_local(&args.docker_image).with_context(|| {
                "docker execution target requested. Ensure the configured image is present locally or re-run with --exec-target host"
            })?;
            Ok(std::sync::Arc::new(DockerTarget::new(
                args.docker_image.clone(),
                args.docker_workdir.clone(),
                match args.docker_network {
                    DockerNetwork::None => "none",
                    DockerNetwork::Bridge => "bridge",
                }
                .to_string(),
                args.docker_user.clone(),
            )))
        }
    }
}

pub(super) fn build_gate_context(
    args: &RunArgs,
    workdir: &std::path::Path,
    provider_kind: ProviderKind,
    default_model: &str,
    resolved_target_kind: ExecTargetKind,
) -> GateContext {
    GateContext {
        workdir: workdir.to_path_buf(),
        allow_shell: args.allow_shell || args.allow_shell_in_workdir,
        allow_write: args.allow_write,
        approval_mode: args.approval_mode,
        auto_approve_scope: args.auto_approve_scope,
        unsafe_mode: args.unsafe_mode,
        unsafe_bypass_allow_flags: args.unsafe_bypass_allow_flags,
        run_id: None,
        enable_write_tools: args.enable_write_tools,
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
        provider: provider_kind,
        model: default_model.to_string(),
        exec_target: resolved_target_kind,
        approval_key_version: args.approval_key,
        tool_schema_hashes: std::collections::BTreeMap::new(),
        hooks_config_hash_hex: None,
        planner_hash_hex: None,
        taint_enabled: matches!(args.taint, TaintToggle::On),
        taint_mode: args.taint_mode,
        taint_overall: taint::TaintLevel::Clean,
        taint_sources: Vec::new(),
    }
}

pub(super) async fn resolve_mcp_runtime_registry(
    args: &RunArgs,
    paths: &store::StatePaths,
    shared_mcp_registry: Option<std::sync::Arc<McpRegistry>>,
) -> anyhow::Result<(std::path::PathBuf, Option<std::sync::Arc<McpRegistry>>)> {
    let mcp_config_path = runtime_paths::resolved_mcp_config_path(args, &paths.state_dir);
    let mcp_registry = if let Some(reg) = shared_mcp_registry {
        Some(reg)
    } else if args.mcp.is_empty() {
        None
    } else {
        Some(std::sync::Arc::new(
            McpRegistry::from_config_path(&mcp_config_path, &args.mcp, Duration::from_secs(30))
                .await?,
        ))
    };
    Ok((mcp_config_path, mcp_registry))
}

pub(super) fn build_session_bootstrap(
    args: &RunArgs,
    paths: &store::StatePaths,
) -> anyhow::Result<SessionBootstrap> {
    let session_path = paths.sessions_dir.join(format!("{}.json", args.session));
    let session_store = SessionStore::new(session_path, args.session.clone());
    if !args.no_session && args.reset_session {
        session_store.reset()?;
    }
    let session_data = if args.no_session {
        session::SessionData::empty(&args.session)
    } else {
        session_store.load()?
    };
    let explicit_flags = runtime_flags::parse_explicit_flags();
    let resolved_settings = session::resolve_run_settings(
        args.use_session_settings,
        !args.no_session,
        &session_data,
        &explicit_flags,
        RunSettingInputs {
            max_context_chars: args.max_context_chars,
            compaction_mode: args.compaction_mode,
            compaction_keep_last: args.compaction_keep_last,
            tool_result_persist: args.tool_result_persist,
            tool_args_strict: args.tool_args_strict,
            caps_mode: args.caps,
            hooks_mode: args.hooks,
        },
    );
    let session_messages = if args.no_session {
        Vec::new()
    } else {
        session_data.messages.clone()
    };
    let task_memory = if args.no_session {
        None
    } else {
        task_memory_message(&session_data.task_memory)
    };
    Ok(SessionBootstrap {
        session_store,
        session_data,
        resolved_settings,
        session_messages,
        task_memory,
    })
}

pub(super) fn build_context_augmentations(
    args: &RunArgs,
    paths: &store::StatePaths,
    worker_model: &str,
) -> anyhow::Result<ContextAugmentations> {
    let instruction_resolution =
        instruction_runtime::resolve_instruction_messages(args, &paths.state_dir, worker_model)?;
    let project_guidance_resolution = project_guidance::resolve_project_guidance(
        &args.workdir,
        project_guidance::ProjectGuidanceLimits::default(),
    )
    .ok()
    .filter(|g| !g.merged_text.is_empty());
    let repo_map_resolution = if args.use_repomap {
        repo_map::resolve_repo_map(
            &args.workdir,
            repo_map::RepoMapLimits {
                max_out_bytes: args.repomap_max_bytes,
                ..repo_map::RepoMapLimits::default()
            },
        )
        .ok()
        .filter(|m| !m.content.is_empty())
    } else {
        None
    };
    let activated_packs = if args.packs.is_empty() {
        Vec::new()
    } else {
        packs::activate_packs(&args.workdir, &args.packs, packs::PackLimits::default())?
    };
    Ok(ContextAugmentations {
        instruction_resolution,
        project_guidance_resolution,
        repo_map_resolution,
        activated_packs,
    })
}

pub(super) fn build_ui_runtime_setup(input: UiRuntimeSetupInput<'_>) -> anyhow::Result<UiRuntimeSetup> {
    let (ui_tx, ui_rx) = if input.args.tui {
        let (tx, rx) = std::sync::mpsc::channel();
        (Some(tx), Some(rx))
    } else {
        (input.external_ui_tx, None)
    };
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let cancel_tx_for_tui = cancel_tx.clone();
    let ui_join = if let Some(rx) = ui_rx {
        let approvals_path = input.paths.approvals_path.clone();
        let cfg = tui::TuiConfig {
            refresh_ms: input.args.tui_refresh_ms,
            max_log_lines: input.args.tui_max_log_lines,
            provider: provider_to_string(input.provider_kind),
            model: input.worker_model.to_string(),
            mode_label: format!(
                "{}·{}",
                if !input.args.allow_shell
                    && !input.args.allow_write
                    && !input.args.enable_write_tools
                {
                    "SAFE".to_string()
                } else {
                    "CODE".to_string()
                },
                format!("{:?}", input.args.agent_mode).to_ascii_uppercase()
            ),
            authority_label: if input.args.approval_mode == ApprovalMode::Auto {
                "EXEC".to_string()
            } else {
                "VETO".to_string()
            },
            mcp_pin_enforcement: input.mcp_pin_enforcement.to_ascii_uppercase(),
            caps_source: format!("{:?}", input.resolved_settings.caps_mode).to_lowercase(),
            policy_hash: input.policy_hash_hex.clone().unwrap_or_default(),
            mcp_catalog_hash: input.mcp_tool_catalog_hash_hex.clone().unwrap_or_default(),
        };
        Some(std::thread::spawn(move || {
            tui::run_live(rx, approvals_path, cfg, cancel_tx_for_tui)
        }))
    } else {
        None
    };
    let event_sink = runtime_wiring::build_event_sink(
        input.args.stream,
        input.args.output,
        input.args.events.as_deref(),
        input.args.tui,
        ui_tx,
        input.suppress_stdout_stream,
    )?;
    Ok(UiRuntimeSetup {
        event_sink,
        _cancel_tx: cancel_tx,
        cancel_rx,
        ui_join,
    })
}

pub(super) fn build_hook_and_tool_setup(
    args: &RunArgs,
    paths: &store::StatePaths,
    resolved_settings: &session::RunSettingResolution,
    all_tools: &[crate::types::ToolDef],
) -> anyhow::Result<HookToolSetup> {
    let hooks_config_path = runtime_paths::resolved_hooks_config_path(args, &paths.state_dir);
    let tool_schema_hash_hex_map = store::tool_schema_hash_hex_map(all_tools);
    let hooks_config_hash_hex = ops_helpers::compute_hooks_config_hash_hex(
        resolved_settings.hooks_mode,
        &hooks_config_path,
    );
    let hook_manager = HookManager::build(HookRuntimeConfig {
        mode: resolved_settings.hooks_mode,
        config_path: hooks_config_path.clone(),
        strict: args.hooks_strict,
        timeout_ms: args.hooks_timeout_ms,
        max_stdout_bytes: args.hooks_max_stdout_bytes,
    })?;
    let tool_catalog = all_tools
        .iter()
        .map(|t| store::ToolCatalogEntry {
            name: t.name.clone(),
            side_effects: t.side_effects,
        })
        .collect::<Vec<_>>();
    Ok(HookToolSetup {
        hooks_config_path,
        tool_schema_hash_hex_map,
        hooks_config_hash_hex,
        hook_manager,
        tool_catalog,
    })
}
