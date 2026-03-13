use std::path::PathBuf;
use std::sync::{mpsc::Sender, Arc};

use anyhow::Context;
use tokio::sync::watch;

use crate::agent::{PlanToolEnforcementMode, PolicyLoadedInfo};
use crate::events::{Event, EventKind};
use crate::gate::{GateContext, ProviderKind};
use crate::mcp::registry::McpRegistry;
use crate::packs;
use crate::providers::ModelProvider;
use crate::run_prep;
use crate::runtime_events;
use crate::runtime_flags;
use crate::runtime_wiring::{self, GateBuild};
use crate::session::{self, SessionStore};
use crate::store;
use crate::target::{ExecTarget, ExecTargetKind};
use crate::trust::policy::Policy;
use crate::types::{Message, SideEffects};
use crate::RunArgs;

use super::guard::{should_enable_implementation_guard, validate_runtime_owned_http_timeouts};
use super::setup::{
    build_context_augmentations, build_exec_target, build_gate_context, build_hook_and_tool_setup,
    build_session_bootstrap, build_ui_runtime_setup, resolve_mcp_runtime_registry,
    ContextAugmentations, HookToolSetup, SessionBootstrap, UiRuntimeSetup, UiRuntimeSetupInput,
};

pub(super) struct RuntimeLaunch {
    pub(super) args: RunArgs,
    pub(super) workdir: PathBuf,
    pub(super) all_tools: Vec<crate::types::ToolDef>,
    pub(super) exec_target: Arc<dyn ExecTarget>,
    pub(super) resolved_target_kind: ExecTargetKind,
    pub(super) gate_ctx: GateContext,
    pub(super) gate_build: GateBuild,
    pub(super) policy_loaded_info: Option<PolicyLoadedInfo>,
    pub(super) planner_strict_effective: bool,
    pub(super) planner_model: String,
    pub(super) worker_model: String,
    pub(super) effective_plan_tool_enforcement: PlanToolEnforcementMode,
    pub(super) session_store: SessionStore,
    pub(super) session_data: session::SessionData,
    pub(super) resolved_settings: session::RunSettingResolution,
    pub(super) session_messages: Vec<Message>,
    pub(super) task_memory: Option<Message>,
    pub(super) instruction_resolution: crate::instructions::InstructionResolution,
    pub(super) task_contract: crate::agent::TaskContractV1,
    pub(super) task_contract_provenance: crate::agent::TaskContractProvenanceV1,
    pub(super) execution_tier: crate::agent_runtime::state::ExecutionTier,
    pub(super) project_guidance_resolution:
        Option<crate::project_guidance::ResolvedProjectGuidance>,
    pub(super) repo_map_resolution: Option<crate::repo_map::ResolvedRepoMap>,
    pub(super) lsp_context_resolution: Option<crate::lsp_context::ResolvedLspContext>,
    pub(super) activated_packs: Vec<packs::ActivatedPack>,
    pub(super) mcp_config_path: PathBuf,
    pub(super) mcp_registry: Option<Arc<McpRegistry>>,
    pub(super) mcp_tool_snapshot: Vec<store::McpToolSnapshotEntry>,
    pub(super) qualification_fallback_note: Option<String>,
    pub(super) mcp_tool_catalog_hash_hex: Option<String>,
    pub(super) mcp_tool_docs_hash_hex: Option<String>,
    pub(super) mcp_config_hash_hex: Option<String>,
    pub(super) mcp_startup_live_catalog_hash_hex: Option<String>,
    pub(super) mcp_startup_live_docs_hash_hex: Option<String>,
    pub(super) mcp_snapshot_pinned: bool,
    pub(super) mcp_pin_enforcement: String,
    pub(super) hooks_config_path: PathBuf,
    pub(super) tool_schema_hash_hex_map: std::collections::BTreeMap<String, String>,
    pub(super) hooks_config_hash_hex: Option<String>,
    pub(super) hook_manager: crate::hooks::runner::HookManager,
    pub(super) tool_catalog: Vec<store::ToolCatalogEntry>,
    pub(super) event_sink: Option<Box<dyn crate::events::EventSink>>,
    pub(super) _cancel_tx: tokio::sync::watch::Sender<bool>,
    pub(super) cancel_rx: tokio::sync::watch::Receiver<bool>,
    pub(super) ui_join: Option<std::thread::JoinHandle<anyhow::Result<()>>>,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn prepare_runtime_launch<P: ModelProvider>(
    provider: &P,
    provider_kind: ProviderKind,
    base_url: &str,
    default_model: &str,
    prompt: &str,
    args: &RunArgs,
    paths: &store::StatePaths,
    external_ui_tx: Option<Sender<Event>>,
    external_cancel_pair: Option<(watch::Sender<bool>, watch::Receiver<bool>)>,
    shared_mcp_registry: Option<Arc<McpRegistry>>,
    suppress_stdout_stream: bool,
) -> anyhow::Result<RuntimeLaunch> {
    let mut effective_args = args.clone();
    runtime_flags::apply_agent_mode_capability_baseline(
        &mut effective_args,
        runtime_flags::parse_capability_explicit_flags(),
    );
    let mut args = effective_args;
    let workdir = std::fs::canonicalize(&args.workdir)
        .with_context(|| format!("failed to resolve workdir: {}", args.workdir.display()))?;
    let exec_target = build_exec_target(&args)?;
    let resolved_target_kind = exec_target.kind();
    let _target_desc = exec_target.describe();
    let mut gate_ctx = build_gate_context(
        &args,
        &workdir,
        provider_kind,
        default_model,
        resolved_target_kind,
    );
    let gate_build = runtime_wiring::build_gate(&args, paths)?;
    let policy_loaded_info = gate_build.policy_version.map(|version| PolicyLoadedInfo {
        version,
        rules_count: gate_build
            .policy_for_exposure
            .as_ref()
            .map(Policy::rules_len)
            .unwrap_or(0),
        includes_count: gate_build.includes_resolved.len(),
        includes_resolved: gate_build.includes_resolved.clone(),
        mcp_allowlist: gate_build.mcp_allowlist.clone(),
    });

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
        session_data,
        resolved_settings,
        session_messages,
        task_memory,
    } = build_session_bootstrap(&args, paths)?;
    let ContextAugmentations {
        instruction_resolution,
        project_guidance_resolution,
        repo_map_resolution,
        lsp_context_resolution,
        activated_packs,
    } = build_context_augmentations(prompt, &args, paths, &worker_model)?;
    validate_runtime_owned_http_timeouts(
        &args,
        planner_strict_effective,
        instruction_resolution.selected_task_profile.as_deref(),
    )?;
    let implementation_guard_enabled = should_enable_implementation_guard(
        &args,
        instruction_resolution.selected_task_profile.as_deref(),
    );

    let (mcp_config_path, mcp_registry) =
        resolve_mcp_runtime_registry(&args, paths, shared_mcp_registry).await?;

    let prep = run_prep::prepare_tools_and_qualification(run_prep::PrepareToolsInput {
        provider,
        provider_kind,
        base_url,
        worker_model: &worker_model,
        args: &args,
        state_dir: &paths.state_dir,
        mcp_config_path: &mcp_config_path,
        mcp_registry: mcp_registry.as_ref(),
        policy_for_exposure: gate_build.policy_for_exposure.as_ref(),
    })
    .await?;
    if let Some(note) = &prep.qualification_fallback_note {
        eprintln!("WARN: {note}");
    }
    if prep.write_disabled_by_qualification {
        args.allow_write = false;
        gate_ctx.allow_write = false;
    }

    let mcp_pin_enforcement = format!("{:?}", args.mcp_pin_enforcement).to_lowercase();
    let HookToolSetup {
        hooks_config_path,
        tool_schema_hash_hex_map,
        hooks_config_hash_hex,
        hook_manager,
        tool_catalog,
    } = build_hook_and_tool_setup(&args, paths, &resolved_settings, &prep.all_tools)?;
    let task_contract_resolution = crate::agent::resolve_task_contract(
        &args,
        prompt,
        instruction_resolution.selected_task_profile.as_deref(),
        implementation_guard_enabled,
        &prep.all_tools,
    );
    let execution_tier = resolve_execution_tier(
        resolved_target_kind,
        args.allow_shell,
        args.allow_write || args.enable_write_tools,
        &prep.all_tools,
    );
    gate_ctx.tool_schema_hashes = tool_schema_hash_hex_map.clone();
    gate_ctx.hooks_config_hash_hex = hooks_config_hash_hex.clone();

    let UiRuntimeSetup {
        event_sink,
        _cancel_tx,
        cancel_rx,
        ui_join,
    } = build_ui_runtime_setup(UiRuntimeSetupInput {
        args: &args,
        paths,
        provider_kind,
        worker_model: &worker_model,
        mcp_pin_enforcement: &mcp_pin_enforcement,
        resolved_settings: &resolved_settings,
        policy_hash_hex: &gate_build.policy_hash_hex,
        mcp_tool_catalog_hash_hex: &prep.mcp_tool_catalog_hash_hex,
        external_ui_tx,
        external_cancel_pair,
        suppress_stdout_stream,
    })?;

    Ok(RuntimeLaunch {
        args,
        workdir,
        all_tools: prep.all_tools,
        exec_target,
        resolved_target_kind,
        gate_ctx,
        gate_build,
        policy_loaded_info,
        planner_strict_effective,
        planner_model,
        worker_model,
        effective_plan_tool_enforcement,
        session_store,
        session_data,
        resolved_settings,
        session_messages,
        task_memory,
        instruction_resolution,
        task_contract: task_contract_resolution.contract,
        task_contract_provenance: task_contract_resolution.provenance,
        execution_tier,
        project_guidance_resolution,
        repo_map_resolution,
        lsp_context_resolution,
        activated_packs,
        mcp_config_path,
        mcp_registry,
        mcp_tool_snapshot: prep.mcp_tool_snapshot,
        qualification_fallback_note: prep.qualification_fallback_note,
        mcp_tool_catalog_hash_hex: prep.mcp_tool_catalog_hash_hex,
        mcp_tool_docs_hash_hex: prep.mcp_tool_docs_hash_hex,
        mcp_config_hash_hex: prep.mcp_config_hash_hex,
        mcp_startup_live_catalog_hash_hex: prep.mcp_startup_live_catalog_hash_hex,
        mcp_startup_live_docs_hash_hex: prep.mcp_startup_live_docs_hash_hex,
        mcp_snapshot_pinned: prep.mcp_snapshot_pinned,
        mcp_pin_enforcement,
        hooks_config_path,
        tool_schema_hash_hex_map,
        hooks_config_hash_hex,
        hook_manager,
        tool_catalog,
        event_sink,
        _cancel_tx,
        cancel_rx,
        ui_join,
    })
}

fn resolve_execution_tier(
    resolved_target_kind: ExecTargetKind,
    allow_shell: bool,
    allow_write: bool,
    all_tools: &[crate::types::ToolDef],
) -> crate::agent_runtime::state::ExecutionTier {
    if matches!(resolved_target_kind, ExecTargetKind::Docker) {
        return crate::agent_runtime::state::ExecutionTier::DockerIsolated;
    }
    if allow_shell {
        return crate::agent_runtime::state::ExecutionTier::ScopedHostShell;
    }
    if allow_write {
        return crate::agent_runtime::state::ExecutionTier::ScopedHostWrite;
    }
    let has_only_mcp_tools = !all_tools.is_empty()
        && all_tools.iter().all(|tool| {
            !matches!(
                tool.side_effects,
                SideEffects::FilesystemRead
                    | SideEffects::FilesystemWrite
                    | SideEffects::ShellExec
                    | SideEffects::Browser
            )
        })
        && all_tools.iter().any(|tool| matches!(tool.side_effects, SideEffects::Network));
    if has_only_mcp_tools {
        return crate::agent_runtime::state::ExecutionTier::McpOnly;
    }
    let has_no_side_effects = all_tools
        .iter()
        .all(|tool| matches!(tool.side_effects, SideEffects::None));
    if has_no_side_effects {
        return crate::agent_runtime::state::ExecutionTier::NoSideEffects;
    }
    crate::agent_runtime::state::ExecutionTier::ReadOnlyHost
}

pub(super) fn build_mcp_pin_snapshot(
    launch: &RuntimeLaunch,
) -> Option<store::McpPinSnapshotRecord> {
    if launch.mcp_tool_catalog_hash_hex.is_some()
        || launch.mcp_startup_live_catalog_hash_hex.is_some()
        || launch.mcp_tool_docs_hash_hex.is_some()
        || launch.mcp_startup_live_docs_hash_hex.is_some()
    {
        Some(store::McpPinSnapshotRecord {
            enforcement: launch.mcp_pin_enforcement.clone(),
            configured_catalog_hash_hex: launch
                .mcp_tool_catalog_hash_hex
                .clone()
                .unwrap_or_default(),
            startup_live_catalog_hash_hex: launch.mcp_startup_live_catalog_hash_hex.clone(),
            configured_docs_hash_hex: launch.mcp_tool_docs_hash_hex.clone(),
            startup_live_docs_hash_hex: launch.mcp_startup_live_docs_hash_hex.clone(),
            mcp_config_hash_hex: launch.mcp_config_hash_hex.clone(),
            pinned: launch.mcp_snapshot_pinned,
        })
    } else {
        None
    }
}

pub(super) fn emit_startup_runtime_events(launch: &mut RuntimeLaunch, run_id: &str) {
    runtime_events::emit_event(
        &mut launch.event_sink,
        run_id,
        0,
        EventKind::ExecutionTierSelected,
        serde_json::json!({
            "execution_tier": launch.execution_tier
        }),
    );
    runtime_events::emit_event(
        &mut launch.event_sink,
        run_id,
        0,
        EventKind::TaskContractResolved,
        serde_json::json!({
            "task_kind": launch.task_contract.task_kind,
            "validation_requirement": launch.task_contract.validation_requirement,
            "allowed_tools_semantics": launch.task_contract.allowed_tools_semantics,
            "task_kind_source": launch.task_contract_provenance.task_kind
        }),
    );
    runtime_events::emit_event(
        &mut launch.event_sink,
        run_id,
        0,
        EventKind::McpPinned,
        serde_json::json!({
            "enforcement": launch.mcp_pin_enforcement,
            "configured_hash_hex": launch.mcp_tool_catalog_hash_hex,
            "startup_live_hash_hex": launch.mcp_startup_live_catalog_hash_hex,
            "configured_docs_hash_hex": launch.mcp_tool_docs_hash_hex,
            "startup_live_docs_hash_hex": launch.mcp_startup_live_docs_hash_hex,
            "mcp_config_hash_hex": launch.mcp_config_hash_hex,
            "pinned": launch.mcp_snapshot_pinned
        }),
    );
    for pack in &launch.activated_packs {
        runtime_events::emit_event(
            &mut launch.event_sink,
            run_id,
            0,
            EventKind::PackActivated,
            serde_json::json!({
                "schema": "openagent.pack_activated.v1",
                "pack_id": pack.pack_id,
                "pack_hash_hex": pack.pack_hash_hex,
                "truncated": pack.truncated,
                "bytes_kept": pack.bytes_kept
            }),
        );
    }
    if let Some(note) = &launch.qualification_fallback_note {
        runtime_events::emit_event(
            &mut launch.event_sink,
            run_id,
            0,
            EventKind::Error,
            serde_json::json!({
                "error": note,
                "source": "orchestrator_qualification_fallback"
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use tempfile::tempdir;

    use super::prepare_runtime_launch;
    use crate::agent::{
        AllowedToolsSemantics, ContractValueSource, FinalAnswerMode, ValidationRequirement,
        WriteRequirement,
    };
    use crate::gate::ProviderKind;
    use crate::providers::mock::MockProvider;

    async fn launch_for_args(
        argv: &[&str],
        prompt: &str,
    ) -> anyhow::Result<super::RuntimeLaunch> {
        let tmp = tempdir().expect("tempdir");
        let paths = crate::store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut args = crate::RunArgs::parse_from(argv);
        args.workdir = tmp.path().to_path_buf();
        prepare_runtime_launch(
            &MockProvider::new(),
            ProviderKind::Mock,
            "mock://local",
            "mock-model",
            prompt,
            &args,
            &paths,
            None,
            None,
            None,
            true,
        )
        .await
    }

    #[tokio::test]
    async fn launch_resolves_explicit_task_kind_contract() {
        let launch = launch_for_args(
            &["localagent", "--agent-mode", "plan", "--task-kind", "code fix"],
            "Inspect and fix the project.",
        )
        .await
        .expect("launch");
        assert_eq!(launch.task_contract.task_kind, "coding");
        assert_eq!(
            launch.task_contract_provenance.task_kind,
            ContractValueSource::Explicit
        );
        assert_eq!(
            launch.task_contract.allowed_tools_semantics,
            AllowedToolsSemantics::ExposedSnapshot
        );
    }

    #[tokio::test]
    async fn launch_disabling_implementation_guard_relaxes_write_contract() {
        let launch = launch_for_args(
            &[
                "localagent",
                "--agent-mode",
                "build",
                "--disable-implementation-guard",
            ],
            "Inspect the repository and summarize risks.",
        )
        .await
        .expect("launch");
        assert_eq!(launch.task_contract.write_requirement, WriteRequirement::Optional);
        assert!(!launch.task_contract.completion_policy.require_pre_write_read);
        assert!(!launch.task_contract.completion_policy.require_post_write_readback);
        assert!(!launch.task_contract.completion_policy.require_effective_write);
    }

    #[tokio::test]
    async fn launch_infers_validation_requirement_from_prompt() {
        let launch = launch_for_args(
            &["localagent", "--agent-mode", "plan"],
            "Make the fix. Before finishing, run cargo test successfully.",
        )
        .await
        .expect("launch");
        assert_eq!(
            launch.task_contract.validation_requirement,
            ValidationRequirement::Command {
                command: "cargo test".to_string(),
            }
        );
        assert_eq!(
            launch.task_contract_provenance.validation_requirement,
            ContractValueSource::Inferred
        );
    }

    #[tokio::test]
    async fn launch_infers_exact_final_answer_from_prompt() {
        let launch = launch_for_args(
            &["localagent", "--agent-mode", "plan"],
            "Update the file.\n\nReply with exactly:\n\nverified fix\n",
        )
        .await
        .expect("launch");
        assert_eq!(
            launch.task_contract.final_answer_mode,
            FinalAnswerMode::Exact {
                required_text: "verified fix".to_string(),
            }
        );
        assert_eq!(
            launch.task_contract_provenance.final_answer_mode,
            ContractValueSource::Inferred
        );
    }

    #[tokio::test]
    async fn launch_resolves_execution_tier_for_host_read_only_mode() {
        let launch = launch_for_args(
            &["localagent", "--agent-mode", "build"],
            "Explain the repository.",
        )
        .await
        .expect("launch");
        assert_eq!(
            launch.execution_tier,
            crate::agent_runtime::state::ExecutionTier::ReadOnlyHost
        );
    }
}
