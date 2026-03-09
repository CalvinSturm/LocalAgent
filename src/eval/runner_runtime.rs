use std::path::Path;
use std::time::Duration;

use anyhow::Context;

use super::runner_artifacts::{write_run_artifact_for_eval, EvalPolicyMeta};
use crate::agent::{Agent, AgentExitReason, PolicyLoadedInfo, ToolCallBudget};
use crate::compaction::CompactionSettings;
use crate::eval::assert::evaluate_assertions;
use crate::eval::cost::{estimate_cost_usd, CostModel};
use crate::eval::fixtures::FixtureServer;
use crate::eval::metrics::{
    count_tool_calls_by_side_effects, derive_io_bytes_from_messages,
    derive_step_invariant_violations, derive_tool_retry_metrics,
};
use crate::eval::tasks::{EvalTask, VerifierSpec};
use crate::eval::types::{
    EvalConfig, EvalProviderMetrics, EvalRunMetrics, EvalRunRow, EvalRunStats, EvalTokenMetrics,
    EvalVerifierResult,
};
use crate::events::{Event, EventSink};
use crate::gate::{
    compute_policy_hash_hex, GateContext, NoGate, ProviderKind, ToolGate, TrustGate, TrustMode,
};
use crate::hooks::config::HooksMode;
use crate::hooks::runner::{HookManager, HookRuntimeConfig};
use crate::mcp::registry::McpRegistry;
use crate::providers::http::HttpConfig;
use crate::providers::mock::MockProvider;
use crate::providers::ollama::OllamaProvider;
use crate::providers::openai_compat::OpenAiCompatProvider;
use crate::providers::ModelProvider;
use crate::store::StatePaths;
use crate::target::{ExecTargetKind, HostTarget};
use crate::tools::{builtin_tools_enabled, ToolRuntime};
use crate::trust::approvals::ApprovalsStore;
use crate::trust::audit::AuditLog;
use crate::trust::policy::{McpAllowSummary, Policy};
use crate::types::{Message, Role};

pub(crate) fn compute_hooks_config_hash_hex(mode: HooksMode, path: &Path) -> Option<String> {
    if matches!(mode, HooksMode::Off) || !path.exists() {
        return None;
    }
    std::fs::read(path)
        .ok()
        .map(|bytes| crate::store::sha256_hex(&bytes))
}

struct EvalEventCaptureSink {
    events: std::sync::Arc<std::sync::Mutex<Vec<Event>>>,
}

impl EventSink for EvalEventCaptureSink {
    fn emit(&mut self, event: Event) -> anyhow::Result<()> {
        self.events.lock().expect("event lock").push(event);
        Ok(())
    }
}

pub(crate) fn run_task_verifier(
    spec: Option<&VerifierSpec>,
    workdir: &Path,
    max_bytes: usize,
) -> anyhow::Result<EvalVerifierResult> {
    let Some(spec) = spec else {
        return Ok(EvalVerifierResult {
            ran: false,
            ok: false,
            summary: "not configured".to_string(),
            stdout_truncated: false,
            stderr_truncated: false,
        });
    };
    let cwd = workdir.join(&spec.cwd);
    let output = std::process::Command::new(&spec.command)
        .args(&spec.args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed running verifier command {}", spec.command))?;
    let (stdout, stdout_truncated) = truncate_bytes_lossy(&output.stdout, max_bytes);
    let (stderr, stderr_truncated) = truncate_bytes_lossy(&output.stderr, max_bytes);
    let combined = format!("{stdout}\n{stderr}");
    let ok = output.status.success() && combined.contains(&spec.summary_success_contains);
    Ok(EvalVerifierResult {
        ran: true,
        ok,
        summary: if ok {
            "ok".to_string()
        } else {
            format!(
                "{} failed (status={:?})",
                spec.command,
                output.status.code().unwrap_or(-1)
            )
        },
        stdout_truncated,
        stderr_truncated,
    })
}

fn truncate_bytes_lossy(bytes: &[u8], max: usize) -> (String, bool) {
    if bytes.len() <= max {
        return (String::from_utf8_lossy(bytes).into_owned(), false);
    }
    (String::from_utf8_lossy(&bytes[..max]).into_owned(), true)
}

struct GateBuild {
    gate: Box<dyn ToolGate>,
    policy_hash_hex: Option<String>,
    policy_source: &'static str,
    policy_for_exposure: Option<Policy>,
    policy_version: Option<u32>,
    includes_resolved: Vec<String>,
    mcp_allowlist: Option<McpAllowSummary>,
}

fn build_gate(trust: TrustMode, paths: &StatePaths) -> anyhow::Result<GateBuild> {
    match trust {
        TrustMode::Off => Ok(GateBuild {
            gate: Box::new(NoGate::new()),
            policy_hash_hex: None,
            policy_source: "none",
            policy_for_exposure: None,
            policy_version: None,
            includes_resolved: Vec::new(),
            mcp_allowlist: None,
        }),
        TrustMode::Auto => {
            if !paths.policy_path.exists() {
                return Ok(GateBuild {
                    gate: Box::new(NoGate::new()),
                    policy_hash_hex: None,
                    policy_source: "none",
                    policy_for_exposure: None,
                    policy_version: None,
                    includes_resolved: Vec::new(),
                    mcp_allowlist: None,
                });
            }
            let bytes = std::fs::read(&paths.policy_path)?;
            let policy = Policy::from_path(&paths.policy_path).with_context(|| {
                format!("failed parsing policy {}", paths.policy_path.display())
            })?;
            let hash = compute_policy_hash_hex(&bytes);
            let policy_version = policy.version();
            let includes_resolved = policy.includes_resolved().to_vec();
            let mcp_allowlist = policy.mcp_allowlist_summary();
            Ok(GateBuild {
                gate: Box::new(TrustGate::new(
                    policy.clone(),
                    ApprovalsStore::new(paths.approvals_path.clone()),
                    AuditLog::new(paths.audit_path.clone()),
                    TrustMode::Auto,
                    hash.clone(),
                )),
                policy_hash_hex: Some(hash),
                policy_source: "file",
                policy_for_exposure: Some(policy),
                policy_version: Some(policy_version),
                includes_resolved,
                mcp_allowlist,
            })
        }
        TrustMode::On => {
            let (policy, hash, src) = if paths.policy_path.exists() {
                let bytes = std::fs::read(&paths.policy_path)?;
                let policy = Policy::from_path(&paths.policy_path).with_context(|| {
                    format!("failed parsing policy {}", paths.policy_path.display())
                })?;
                (policy, compute_policy_hash_hex(&bytes), "file")
            } else {
                let repr = crate::trust::policy::safe_default_policy_repr();
                (
                    Policy::safe_default(),
                    compute_policy_hash_hex(repr.as_bytes()),
                    "default",
                )
            };
            let policy_version = policy.version();
            let includes_resolved = policy.includes_resolved().to_vec();
            let mcp_allowlist = policy.mcp_allowlist_summary();
            Ok(GateBuild {
                gate: Box::new(TrustGate::new(
                    policy.clone(),
                    ApprovalsStore::new(paths.approvals_path.clone()),
                    AuditLog::new(paths.audit_path.clone()),
                    TrustMode::On,
                    hash.clone(),
                )),
                policy_hash_hex: Some(hash),
                policy_source: src,
                policy_for_exposure: Some(policy),
                policy_version: Some(policy_version),
                includes_resolved,
                mcp_allowlist,
            })
        }
    }
}

enum EvalProvider {
    OpenAiCompat(OpenAiCompatProvider),
    Ollama(OllamaProvider),
    Mock(MockProvider),
}

#[async_trait::async_trait]
impl ModelProvider for EvalProvider {
    async fn generate(
        &self,
        req: crate::types::GenerateRequest,
    ) -> anyhow::Result<crate::types::GenerateResponse> {
        match self {
            EvalProvider::OpenAiCompat(p) => p.generate(req).await,
            EvalProvider::Ollama(p) => p.generate(req).await,
            EvalProvider::Mock(p) => p.generate(req).await,
        }
    }
}

fn make_provider(
    provider: ProviderKind,
    base_url: &str,
    api_key: Option<String>,
    http: HttpConfig,
) -> anyhow::Result<EvalProvider> {
    match provider {
        ProviderKind::Lmstudio | ProviderKind::Llamacpp => Ok(EvalProvider::OpenAiCompat(
            OpenAiCompatProvider::new(provider, base_url.to_string(), api_key, http)?,
        )),
        ProviderKind::Ollama => Ok(EvalProvider::Ollama(OllamaProvider::new(
            base_url.to_string(),
            http,
        )?)),
        ProviderKind::Mock => Ok(EvalProvider::Mock(MockProvider::new())),
    }
}

pub(crate) async fn run_single(
    config: &EvalConfig,
    state_paths: &StatePaths,
    workdir: &Path,
    enabled_mcp: &[String],
    model: &str,
    task: &EvalTask,
    cost_model: Option<&CostModel>,
) -> anyhow::Result<EvalRunRow> {
    let run_started = std::time::Instant::now();
    let fixture_server = if task.needs_playwright {
        Some(FixtureServer::start().context("failed to start local browser fixture server")?)
    } else {
        None
    };
    let prompt = if let Some(s) = &fixture_server {
        task.prompt.replace("{FIXTURE_BASE_URL}", s.base_url())
    } else {
        task.prompt.clone()
    };
    let gate_ctx = GateContext {
        workdir: workdir.to_path_buf(),
        allow_shell: config.allow_shell,
        allow_write: config.allow_write,
        approval_mode: config.approval_mode,
        auto_approve_scope: config.auto_approve_scope,
        approval_key_version: config.approval_key,
        tool_schema_hashes: std::collections::BTreeMap::new(),
        hooks_config_hash_hex: None,
        planner_hash_hex: None,
        unsafe_mode: config.unsafe_mode,
        unsafe_bypass_allow_flags: config.unsafe_bypass_allow_flags,
        run_id: None,
        enable_write_tools: config.enable_write_tools,
        max_tool_output_bytes: if config.no_limits { 0 } else { 200_000 },
        max_read_bytes: if config.no_limits { 0 } else { 200_000 },
        provider: config.provider,
        model: model.to_string(),
        exec_target: ExecTargetKind::Host,
        taint_enabled: false,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_overall: crate::taint::TaintLevel::Clean,
        taint_sources: Vec::new(),
    };
    let gate_build = build_gate(config.trust, state_paths)?;
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

    let mcp_config_path = config
        .mcp_config
        .clone()
        .unwrap_or_else(|| state_paths.state_dir.join("mcp_servers.json"));
    let mcp_registry = if enabled_mcp.is_empty() {
        None
    } else {
        Some(std::sync::Arc::new(
            McpRegistry::from_config_path(&mcp_config_path, enabled_mcp, Duration::from_secs(30))
                .await?,
        ))
    };

    let mut tools = builtin_tools_enabled(
        config.enable_write_tools,
        config.allow_shell || config.unsafe_bypass_allow_flags,
    );
    if let Some(reg) = &mcp_registry {
        let mut mcp_defs = reg.tool_defs();
        if let Some(policy) = &gate_build.policy_for_exposure {
            mcp_defs.retain(|t| policy.mcp_tool_allowed(&t.name).is_ok());
        }
        tools.extend(mcp_defs);
    }

    let tool_catalog = tools
        .iter()
        .map(|t| crate::store::ToolCatalogEntry {
            name: t.name.clone(),
            side_effects: t.side_effects,
        })
        .collect::<Vec<_>>();
    let tool_schema_hash_hex_map = crate::store::tool_schema_hash_hex_map(&tools);
    let resolved_hooks_config_path = config
        .hooks_config
        .clone()
        .unwrap_or_else(|| state_paths.state_dir.join("hooks.yaml"));
    let hooks_config_hash_hex =
        compute_hooks_config_hash_hex(config.hooks_mode, &resolved_hooks_config_path);

    let is_c2 = task.id == "C2";
    let task_max_steps = if is_c2 {
        std::cmp::min(config.max_steps, 6).max(1)
    } else {
        config.max_steps
    };
    let task_max_wall_time_ms = if is_c2 {
        if config.max_wall_time_ms == 0 {
            45_000
        } else {
            std::cmp::min(config.max_wall_time_ms, 45_000)
        }
    } else {
        config.max_wall_time_ms
    };
    let task_max_tokens = if is_c2 { Some(256) } else { None };
    let mut task_http = config.http;
    if is_c2 && (task_http.stream_idle_timeout_ms == 0 || task_http.stream_idle_timeout_ms > 15_000)
    {
        task_http.stream_idle_timeout_ms = 15_000;
    }
    let provider = make_provider(
        config.provider,
        &config.base_url,
        config.api_key.clone(),
        task_http,
    )?;
    let captured_events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Event>::new()));
    let mut agent = Agent {
        provider,
        model: model.to_string(),
        temperature: None,
        top_p: None,
        max_tokens: task_max_tokens,
        seed: None,
        tools,
        max_steps: task_max_steps,
        tool_rt: ToolRuntime {
            workdir: workdir.to_path_buf(),
            allow_shell: config.allow_shell,
            allow_shell_in_workdir_only: false,
            allow_write: config.allow_write,
            max_tool_output_bytes: if config.no_limits { 0 } else { 200_000 },
            max_read_bytes: if config.no_limits { 0 } else { 200_000 },
            unsafe_bypass_allow_flags: config.unsafe_bypass_allow_flags,
            tool_args_strict: config.tool_args_strict,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: gate_build.gate,
        gate_ctx: GateContext {
            tool_schema_hashes: tool_schema_hash_hex_map.clone(),
            hooks_config_hash_hex: hooks_config_hash_hex.clone(),
            ..gate_ctx
        },
        mcp_registry,
        stream: false,
        event_sink: Some(Box::new(EvalEventCaptureSink {
            events: captured_events.clone(),
        })),
        compaction_settings: CompactionSettings {
            max_context_chars: config.max_context_chars,
            mode: config.compaction_mode,
            keep_last: config.compaction_keep_last,
            tool_result_persist: config.tool_result_persist,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: config.hooks_mode,
            config_path: resolved_hooks_config_path,
            strict: config.hooks_strict,
            timeout_ms: config.hooks_timeout_ms,
            max_stdout_bytes: config.hooks_max_stdout_bytes,
        })?,
        policy_loaded: policy_loaded_info,
        policy_for_taint: gate_build.policy_for_exposure.clone(),
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: crate::agent::PlanToolEnforcementMode::Off,
        mcp_pin_enforcement: crate::agent::McpPinEnforcementMode::Hard,
        plan_step_constraints: Vec::new(),
        tool_call_budget: ToolCallBudget {
            max_wall_time_ms: task_max_wall_time_ms,
            max_total_tool_calls: 0,
            max_mcp_calls: config.max_mcp_calls,
            max_filesystem_read_calls: 0,
            max_filesystem_write_calls: 0,
            max_shell_calls: 0,
            max_network_calls: 0,
            max_browser_calls: 0,
            tool_exec_timeout_ms: config.tool_exec_timeout_ms,
            post_write_verify_timeout_ms: config.post_write_verify_timeout_ms,
        },
        mcp_runtime_trace: Vec::new(),
        operator_queue: crate::operator_queue::PendingMessageQueue::default(),
        operator_queue_limits: crate::operator_queue::QueueLimits::default(),
        operator_queue_rx: None,
    };
    let session_messages = Vec::new();
    let injected_messages = vec![Message {
        role: Role::System,
        content: Some(crate::agent::INTERNAL_ENFORCE_IMPLEMENTATION_GUARD_FLAG.to_string()),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
    }];
    let outcome = agent
        .run(&prompt, session_messages, injected_messages)
        .await;
    let wall_time_ms = run_started.elapsed().as_millis() as u64;
    let mut failures = evaluate_assertions(&task.assertions, workdir, &outcome);
    let verifier_started = std::time::Instant::now();
    let verifier = run_task_verifier(task.verifier.as_ref(), workdir, 200_000)?;
    let verifier_time_ms = verifier_started.elapsed().as_millis() as u64;
    if verifier.ran && !verifier.ok {
        failures.push(format!("verifier failed: {}", verifier.summary));
    }
    let passed = failures.is_empty() && matches!(outcome.exit_reason, AgentExitReason::Ok);
    let steps = outcome
        .messages
        .iter()
        .filter(|m| matches!(m.role, crate::types::Role::Assistant))
        .count();
    let tool_calls = outcome.tool_calls.len();
    let tool_calls_by_side_effects = count_tool_calls_by_side_effects(&outcome.tool_calls);
    let (tool_retries, tool_failures_by_class) =
        derive_tool_retry_metrics(&captured_events.lock().expect("event lock"));
    let step_invariant_violations =
        derive_step_invariant_violations(&captured_events.lock().expect("event lock"));
    let (bytes_read, bytes_written) = derive_io_bytes_from_messages(&outcome.messages);
    let tokens = Some(match outcome.token_usage.clone() {
        Some(t) => EvalTokenMetrics {
            prompt_tokens: t.prompt_tokens,
            completion_tokens: t.completion_tokens,
            total_tokens: t.total_tokens,
            source: "provider".to_string(),
        },
        None => EvalTokenMetrics {
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
            source: "unknown".to_string(),
        },
    });
    let estimated_cost_usd = match (cost_model, outcome.token_usage.as_ref()) {
        (Some(cm), Some(t)) => estimate_cost_usd(model, t, cm),
        _ => None,
    };
    let run_metrics = EvalRunMetrics {
        steps: steps as u32,
        tool_calls: tool_calls as u32,
        tool_sequence: outcome.tool_calls.iter().map(|t| t.name.clone()).collect(),
        tool_calls_by_side_effects,
        bytes_read,
        bytes_written,
        wall_time_ms,
        verifier_time_ms,
        provider: EvalProviderMetrics {
            http_retries: outcome.provider_retry_count,
            provider_errors: outcome.provider_error_count,
        },
        tool_retries,
        tool_failures_by_class,
        step_invariant_violations,
    };

    write_run_artifact_for_eval(
        config,
        state_paths,
        model,
        &outcome,
        tool_catalog,
        EvalPolicyMeta {
            source: policy_source,
            hash_hex: policy_hash_hex,
            version: policy_version,
            includes_resolved,
            mcp_allowlist,
        },
        tool_schema_hash_hex_map,
        hooks_config_hash_hex,
    )?;

    Ok(EvalRunRow {
        model: model.to_string(),
        task_id: task.id.clone(),
        run_index: 0,
        workdir: None,
        run_id: outcome.run_id.clone(),
        exit_reason: outcome.exit_reason.as_str().to_string(),
        status: if passed {
            "passed".to_string()
        } else {
            "failed".to_string()
        },
        skip_reason: None,
        required_flags: task.required_flags(),
        passed,
        failures,
        stats: EvalRunStats { steps, tool_calls },
        metrics: Some(run_metrics),
        tokens,
        estimated_cost_usd,
        verifier: Some(verifier),
    })
}
