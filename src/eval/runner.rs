use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context};
use uuid::Uuid;

#[path = "runner_artifacts.rs"]
mod runner_artifacts;
#[path = "runner_output.rs"]
mod runner_output;
#[path = "runner_rows.rs"]
mod runner_rows;
#[path = "runner_runtime.rs"]
mod runner_runtime;

use crate::eval::cost::{load_cost_model, CostModel};
#[allow(unused_imports)]
pub use crate::eval::metrics::{compute_eval_metrics, count_tool_calls_by_side_effects};
use crate::eval::tasks::{tasks_for_pack, EvalTask, Fixture};
#[allow(unused_imports)]
pub use crate::eval::types::{
    EvalAggregateMetrics, EvalBaselineStatus, EvalConfig, EvalResults, EvalResultsConfig,
    EvalRunMetrics, EvalRunRow, EvalRunStats, EvalSummary, EvalVerifierResult,
};
use crate::mcp::registry::list_servers;
use crate::store::{provider_to_string, resolve_state_paths, StatePaths};
use runner_artifacts::write_synthetic_error_artifact;
use runner_output::{finalize_and_write_eval_results, print_row, push_row};
use runner_rows::{
    build_eval_run_error_row, build_eval_timeout_row, missing_capability_reason,
    missing_required_tool_reason, skipped_row,
};
use runner_runtime::run_single;
#[cfg(test)]
use runner_runtime::run_task_verifier;

#[cfg(test)]
fn finalize_summary(results: &mut EvalResults) {
    runner_output::finalize_summary(results);
}

pub async fn run_eval(config: EvalConfig, cwd: &Path) -> anyhow::Result<PathBuf> {
    if config.models.is_empty() {
        return Err(anyhow!("--models is required and must not be empty"));
    }
    let base_workdir = if let Some(path) = &config.workdir_override {
        std::fs::canonicalize(path)
            .with_context(|| format!("failed to resolve --workdir {}", path.display()))?
    } else {
        std::fs::canonicalize(cwd)
            .with_context(|| "failed to resolve current workdir".to_string())?
    };

    let state_paths = resolve_state_paths(
        &base_workdir,
        config.state_dir_override.clone(),
        config.policy_override.clone(),
        config.approvals_override.clone(),
        config.audit_override.clone(),
    );
    if state_paths.using_legacy_dir {
        eprintln!(
            "WARN: using legacy state dir at {}",
            state_paths.state_dir.display()
        );
    }

    let mcp_config_path = config
        .mcp_config
        .clone()
        .unwrap_or_else(|| state_paths.state_dir.join("mcp_servers.json"));
    let mut enabled_mcp = config.mcp.clone();
    let cost_model = if let Some(path) = &config.cost_model_path {
        Some(load_cost_model(path)?)
    } else {
        None
    };
    let tasks = tasks_for_pack(config.pack);
    let has_browser_tasks = tasks.iter().any(|t| t.needs_playwright && !t.optional);
    if has_browser_tasks
        && !enabled_mcp.iter().any(|m| m == "playwright")
        && list_servers(&mcp_config_path)
            .map(|names| names.iter().any(|n| n == "playwright"))
            .unwrap_or(false)
    {
        enabled_mcp.push("playwright".to_string());
    }

    let out_path = config.out.clone().unwrap_or_else(|| {
        let ts = crate::trust::now_rfc3339().replace(':', "-");
        state_paths
            .state_dir
            .join("eval")
            .join(format!("results_{ts}.json"))
    });

    let mut results = EvalResults {
        schema_version: "openagent.eval.v1".to_string(),
        created_at: crate::trust::now_rfc3339(),
        config: EvalResultsConfig {
            provider: provider_to_string(config.provider),
            base_url: config.base_url.clone(),
            models: config.models.clone(),
            pack: format!("{:?}", config.pack).to_lowercase(),
            runs_per_task: config.runs_per_task,
            max_steps: config.max_steps,
            max_wall_time_ms: config.max_wall_time_ms,
            max_mcp_calls: config.max_mcp_calls,
            tool_exec_timeout_ms: config.tool_exec_timeout_ms,
            post_write_verify_timeout_ms: config.post_write_verify_timeout_ms,
            timeout_seconds: config.timeout_seconds,
            trust_mode: format!("{:?}", config.trust).to_lowercase(),
            approval_mode: format!("{:?}", config.approval_mode).to_lowercase(),
            auto_approve_scope: format!("{:?}", config.auto_approve_scope).to_lowercase(),
            approval_key: config.approval_key.as_str().to_string(),
            allow_shell: config.allow_shell,
            allow_write: config.allow_write,
            enable_write_tools: config.enable_write_tools,
            unsafe_mode: config.unsafe_mode,
            no_limits: config.no_limits,
            unsafe_bypass_allow_flags: config.unsafe_bypass_allow_flags,
            mcp: enabled_mcp.clone(),
            no_session: config.no_session,
            session: config.session.clone(),
            max_context_chars: config.max_context_chars,
            compaction_mode: format!("{:?}", config.compaction_mode).to_lowercase(),
            compaction_keep_last: config.compaction_keep_last,
            tool_result_persist: format!("{:?}", config.tool_result_persist).to_lowercase(),
            hooks_mode: format!("{:?}", config.hooks_mode).to_lowercase(),
            hooks_config_path: config
                .hooks_config
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| {
                    state_paths
                        .state_dir
                        .join("hooks.yaml")
                        .display()
                        .to_string()
                }),
            hooks_strict: config.hooks_strict,
            hooks_timeout_ms: config.hooks_timeout_ms,
            hooks_max_stdout_bytes: config.hooks_max_stdout_bytes,
            tool_args_strict: format!("{:?}", config.tool_args_strict).to_lowercase(),
            tui_enabled: config.tui_enabled,
            tui_refresh_ms: config.tui_refresh_ms,
            tui_max_log_lines: config.tui_max_log_lines,
            http_max_retries: config.http.http_max_retries,
            http_timeout_ms: config.http.request_timeout_ms,
            http_connect_timeout_ms: config.http.connect_timeout_ms,
            http_stream_idle_timeout_ms: config.http.stream_idle_timeout_ms,
            http_max_response_bytes: config.http.max_response_bytes,
            http_max_line_bytes: config.http.max_line_bytes,
            mode: format!("{:?}", config.mode).to_lowercase(),
            planner_model: config.planner_model.clone(),
            worker_model: config.worker_model.clone(),
            resolved_profile_name: config.resolved_profile_name.clone(),
            resolved_profile_path: config.resolved_profile_path.clone(),
            resolved_profile_hash_hex: config.resolved_profile_hash_hex.clone(),
            min_pass_rate: config.min_pass_rate,
            fail_on_any: config.fail_on_any,
            max_avg_steps: config.max_avg_steps,
            cost_model_path: config
                .cost_model_path
                .as_ref()
                .map(|p| p.display().to_string()),
        },
        summary: EvalSummary::default(),
        by_model: BTreeMap::new(),
        runs: Vec::new(),
        metrics: None,
        baseline: None,
        regression: None,
    };

    for model in &config.models {
        for task in &tasks {
            if handle_eval_skip_gates(EvalSkipGateInput {
                config: &config,
                enabled_mcp: &enabled_mcp,
                model,
                task,
                results: &mut results,
            }) {
                continue;
            }

            for run_index in 0..config.runs_per_task {
                let run_dir = prepare_eval_run_workdir(&config, task)?;

                let row = execute_eval_run_once(EvalSingleRunExecInput {
                    config: &config,
                    state_paths: &state_paths,
                    enabled_mcp: &enabled_mcp,
                    model,
                    task,
                    cost_model: cost_model.as_ref(),
                    run_dir: &run_dir,
                    run_index,
                })
                .await;
                if config.workdir_override.is_none() && !config.keep_workdir {
                    let _ = std::fs::remove_dir_all(&run_dir);
                }
                print_row(&row);
                push_row(&mut results, row);
            }
        }
    }

    finalize_and_write_eval_results(&config, &out_path, &mut results)?;
    println!("eval results written: {}", out_path.display());
    Ok(out_path)
}

fn prepare_eval_run_workdir(config: &EvalConfig, task: &EvalTask) -> anyhow::Result<PathBuf> {
    let run_dir = create_run_workdir(config.workdir_override.as_deref())?;
    apply_fixtures(&run_dir, &task.fixtures)?;
    Ok(run_dir)
}

struct EvalSkipGateInput<'a> {
    config: &'a EvalConfig,
    enabled_mcp: &'a [String],
    model: &'a str,
    task: &'a EvalTask,
    results: &'a mut EvalResults,
}

fn handle_eval_skip_gates(input: EvalSkipGateInput<'_>) -> bool {
    if input.task.optional {
        return true;
    }
    let mcp_enabled = input.enabled_mcp.iter().any(|m| m == "playwright");
    if let Some(reason) = missing_capability_reason(input.task, input.config, mcp_enabled) {
        let row = skipped_row(input.model, input.task, 0, &reason);
        print_row(&row);
        push_row(input.results, row);
        return true;
    }
    if let Some(reason) = missing_required_tool_reason(
        input.task,
        input.config.enable_write_tools,
        input.enabled_mcp,
    ) {
        let row = skipped_row(input.model, input.task, 0, &reason);
        print_row(&row);
        push_row(input.results, row);
        return true;
    }
    false
}

struct EvalSingleRunExecInput<'a> {
    config: &'a EvalConfig,
    state_paths: &'a StatePaths,
    enabled_mcp: &'a [String],
    model: &'a str,
    task: &'a EvalTask,
    cost_model: Option<&'a CostModel>,
    run_dir: &'a Path,
    run_index: usize,
}

async fn execute_eval_run_once(input: EvalSingleRunExecInput<'_>) -> EvalRunRow {
    let timeout = Duration::from_secs(input.config.timeout_seconds);
    let exec = run_single(
        input.config,
        input.state_paths,
        input.run_dir,
        input.enabled_mcp,
        input.model,
        input.task,
        input.cost_model,
    );
    match tokio::time::timeout(timeout, exec).await {
        Ok(Ok(mut row)) => {
            row.run_index = input.run_index;
            if input.config.keep_workdir || input.config.workdir_override.is_some() {
                row.workdir = Some(input.run_dir.display().to_string());
            }
            row
        }
        Ok(Err(e)) => {
            let run_id = Uuid::new_v4().to_string();
            let error = format!("run error: {e}");
            write_synthetic_error_artifact(
                input.config,
                input.state_paths,
                input.model,
                &run_id,
                error.clone(),
            );
            build_eval_run_error_row(
                input.config,
                input.model,
                input.task,
                input.run_index,
                input.run_dir,
                run_id,
                error,
            )
        }
        Err(_) => {
            let run_id = Uuid::new_v4().to_string();
            write_synthetic_error_artifact(
                input.config,
                input.state_paths,
                input.model,
                &run_id,
                "timeout".to_string(),
            );
            build_eval_timeout_row(
                input.config,
                input.model,
                input.task,
                input.run_index,
                input.run_dir,
                run_id,
            )
        }
    }
}

fn create_run_workdir(override_path: Option<&Path>) -> anyhow::Result<PathBuf> {
    if let Some(path) = override_path {
        std::fs::create_dir_all(path)?;
        return Ok(path.to_path_buf());
    }
    let path = std::env::temp_dir().join(format!("openagent-eval-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

fn apply_fixtures(workdir: &Path, fixtures: &[Fixture]) -> anyhow::Result<()> {
    for fx in fixtures {
        match fx {
            Fixture::WriteFile { path, content } => {
                let full = workdir.join(path);
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(full, content)?;
            }
            Fixture::CreateDir { path } => {
                std::fs::create_dir_all(workdir.join(path))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        compute_eval_metrics, count_tool_calls_by_side_effects, finalize_summary,
        missing_capability_reason, run_task_verifier, EvalConfig, EvalResults, EvalResultsConfig,
        EvalRunMetrics, EvalRunRow, EvalRunStats, EvalVerifierResult,
    };
    use crate::compaction::{CompactionMode, ToolResultPersist};
    use crate::eval::tasks::{EvalTask, Fixture, RequiredCapabilities, VerifierSpec};
    use crate::eval::types::EvalMetrics;
    use crate::gate::{
        ApprovalKeyVersion, ApprovalMode, AutoApproveScope, ProviderKind, TrustMode,
    };
    use crate::hooks::config::HooksMode;
    use crate::planner::RunMode;
    use crate::providers::http::HttpConfig;
    use crate::tools::ToolArgsStrict;
    use crate::types::ToolCall;

    #[test]
    fn summary_aggregation_counts_pass_fail() {
        let mut results = EvalResults {
            schema_version: "openagent.eval.v1".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            config: EvalResultsConfig::minimal_for_tests(),
            summary: Default::default(),
            by_model: BTreeMap::new(),
            runs: vec![
                EvalRunRow {
                    model: "m".to_string(),
                    task_id: "C1".to_string(),
                    run_index: 0,
                    workdir: None,
                    run_id: "r1".to_string(),
                    exit_reason: "ok".to_string(),
                    status: "passed".to_string(),
                    skip_reason: None,
                    required_flags: vec![],
                    passed: true,
                    failures: vec![],
                    stats: EvalRunStats {
                        steps: 1,
                        tool_calls: 1,
                    },
                    metrics: None,
                    tokens: None,
                    estimated_cost_usd: None,
                    verifier: Some(EvalVerifierResult {
                        ran: false,
                        ok: false,
                        summary: String::new(),
                        stdout_truncated: false,
                        stderr_truncated: false,
                    }),
                },
                EvalRunRow {
                    model: "m".to_string(),
                    task_id: "C2".to_string(),
                    run_index: 0,
                    workdir: None,
                    run_id: "r2".to_string(),
                    exit_reason: "denied".to_string(),
                    status: "failed".to_string(),
                    skip_reason: None,
                    required_flags: vec![],
                    passed: false,
                    failures: vec!["x".to_string()],
                    stats: EvalRunStats {
                        steps: 1,
                        tool_calls: 1,
                    },
                    metrics: None,
                    tokens: None,
                    estimated_cost_usd: None,
                    verifier: Some(EvalVerifierResult {
                        ran: false,
                        ok: false,
                        summary: String::new(),
                        stdout_truncated: false,
                        stderr_truncated: false,
                    }),
                },
            ],
            metrics: None,
            baseline: None,
            regression: None,
        };
        finalize_summary(&mut results);
        assert_eq!(results.summary.total_runs, 2);
        assert_eq!(results.summary.passed, 1);
        assert_eq!(results.summary.failed, 1);
        assert_eq!(results.summary.skipped, 0);
        assert!(results.summary.pass_rate > 0.4 && results.summary.pass_rate < 0.6);
    }

    #[test]
    fn metrics_count_side_effects_and_averages() {
        let calls = vec![
            ToolCall {
                id: "1".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"a"}),
            },
            ToolCall {
                id: "2".to_string(),
                name: "write_file".to_string(),
                arguments: serde_json::json!({"path":"b","content":"x"}),
            },
        ];
        let by = count_tool_calls_by_side_effects(&calls);
        assert_eq!(by.get("filesystem_read"), Some(&1));
        assert_eq!(by.get("filesystem_write"), Some(&1));

        let mut results = EvalResults {
            schema_version: "openagent.eval.v1".to_string(),
            created_at: "x".to_string(),
            config: EvalResultsConfig::minimal_for_tests(),
            summary: Default::default(),
            by_model: BTreeMap::new(),
            runs: vec![EvalRunRow {
                model: "m".to_string(),
                task_id: "C1".to_string(),
                run_index: 0,
                workdir: None,
                run_id: "r".to_string(),
                exit_reason: "ok".to_string(),
                status: "passed".to_string(),
                skip_reason: None,
                required_flags: vec![],
                passed: true,
                failures: vec![],
                stats: EvalRunStats {
                    steps: 2,
                    tool_calls: 2,
                },
                metrics: Some(EvalRunMetrics {
                    steps: 2,
                    tool_calls: 2,
                    tool_sequence: vec![],
                    tool_calls_by_side_effects: by,
                    bytes_read: 10,
                    bytes_written: 20,
                    wall_time_ms: 30,
                    verifier_time_ms: 0,
                    provider: Default::default(),
                    tool_retries: 0,
                    tool_failures_by_class: BTreeMap::new(),
                    step_invariant_violations: 0,
                }),
                tokens: None,
                estimated_cost_usd: None,
                verifier: None,
            }],
            metrics: None,
            baseline: None,
            regression: None,
        };
        finalize_summary(&mut results);
        let m: EvalMetrics = compute_eval_metrics(&results);
        assert!(m.summary.avg_steps > 1.0);
        assert!(m.summary.pass_rate > 0.9);
    }

    #[test]
    fn skip_logic_requires_write_and_shell_flags() {
        let task = EvalTask {
            id: "T".to_string(),
            prompt: String::new(),
            required_tools: vec![],
            assertions: vec![],
            fixtures: vec![Fixture::CreateDir {
                path: "x".to_string(),
            }],
            needs_write: true,
            needs_playwright: false,
            optional: false,
            required_capabilities: RequiredCapabilities {
                needs_write_tools: true,
                needs_shell: true,
                needs_mcp: false,
            },
            verifier: None,
        };
        let cfg = EvalConfig {
            provider: ProviderKind::Ollama,
            base_url: "http://localhost:11434".to_string(),
            api_key: None,
            models: vec!["m".to_string()],
            pack: crate::eval::tasks::EvalPack::Coding,
            out: None,
            runs_per_task: 1,
            max_steps: 1,
            max_wall_time_ms: 0,
            max_mcp_calls: 0,
            tool_exec_timeout_ms: 30_000,
            post_write_verify_timeout_ms: 5_000,
            timeout_seconds: 1,
            trust: TrustMode::Off,
            approval_mode: ApprovalMode::Interrupt,
            auto_approve_scope: AutoApproveScope::Run,
            approval_key: ApprovalKeyVersion::V1,
            enable_write_tools: false,
            allow_write: false,
            allow_shell: false,
            unsafe_mode: false,
            no_limits: false,
            unsafe_bypass_allow_flags: false,
            mcp: vec![],
            mcp_config: None,
            session: "default".to_string(),
            no_session: true,
            max_session_messages: 40,
            max_context_chars: 0,
            compaction_mode: CompactionMode::Off,
            compaction_keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
            hooks_mode: HooksMode::Off,
            hooks_config: None,
            hooks_strict: false,
            hooks_timeout_ms: 1000,
            hooks_max_stdout_bytes: 1000,
            tool_args_strict: ToolArgsStrict::On,
            tui_enabled: false,
            tui_refresh_ms: 50,
            tui_max_log_lines: 100,
            state_dir_override: None,
            policy_override: None,
            approvals_override: None,
            audit_override: None,
            workdir_override: None,
            keep_workdir: false,
            http: HttpConfig::default(),
            mode: RunMode::Single,
            planner_model: None,
            worker_model: None,
            min_pass_rate: 0.0,
            fail_on_any: false,
            max_avg_steps: None,
            resolved_profile_name: None,
            resolved_profile_path: None,
            resolved_profile_hash_hex: None,
            junit: None,
            summary_md: None,
            cost_model_path: None,
        };
        let reason = missing_capability_reason(&task, &cfg, false).expect("reason");
        assert!(reason.contains("--enable-write-tools"));
    }

    #[test]
    fn skip_logic_requires_mcp_playwright() {
        let task = EvalTask {
            id: "B".to_string(),
            prompt: String::new(),
            required_tools: vec![],
            assertions: vec![],
            fixtures: vec![],
            needs_write: false,
            needs_playwright: true,
            optional: false,
            required_capabilities: RequiredCapabilities {
                needs_write_tools: false,
                needs_shell: false,
                needs_mcp: true,
            },
            verifier: None,
        };
        let cfg = EvalConfig {
            provider: ProviderKind::Ollama,
            base_url: "http://localhost:11434".to_string(),
            api_key: None,
            models: vec!["m".to_string()],
            pack: crate::eval::tasks::EvalPack::Browser,
            out: None,
            runs_per_task: 1,
            max_steps: 1,
            max_wall_time_ms: 0,
            max_mcp_calls: 0,
            tool_exec_timeout_ms: 30_000,
            post_write_verify_timeout_ms: 5_000,
            timeout_seconds: 1,
            trust: TrustMode::Off,
            approval_mode: ApprovalMode::Interrupt,
            auto_approve_scope: AutoApproveScope::Run,
            approval_key: ApprovalKeyVersion::V1,
            enable_write_tools: false,
            allow_write: false,
            allow_shell: false,
            unsafe_mode: false,
            no_limits: false,
            unsafe_bypass_allow_flags: false,
            mcp: vec![],
            mcp_config: None,
            session: "default".to_string(),
            no_session: true,
            max_session_messages: 40,
            max_context_chars: 0,
            compaction_mode: CompactionMode::Off,
            compaction_keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
            hooks_mode: HooksMode::Off,
            hooks_config: None,
            hooks_strict: false,
            hooks_timeout_ms: 1000,
            hooks_max_stdout_bytes: 1000,
            tool_args_strict: ToolArgsStrict::On,
            tui_enabled: false,
            tui_refresh_ms: 50,
            tui_max_log_lines: 100,
            state_dir_override: None,
            policy_override: None,
            approvals_override: None,
            audit_override: None,
            workdir_override: None,
            keep_workdir: false,
            http: HttpConfig::default(),
            mode: RunMode::Single,
            planner_model: None,
            worker_model: None,
            min_pass_rate: 0.0,
            fail_on_any: false,
            max_avg_steps: None,
            resolved_profile_name: None,
            resolved_profile_path: None,
            resolved_profile_hash_hex: None,
            junit: None,
            summary_md: None,
            cost_model_path: None,
        };
        let reason = missing_capability_reason(&task, &cfg, false).expect("reason");
        assert!(reason.contains("--mcp playwright"));
    }

    #[test]
    fn verifier_failure_is_deterministic() {
        let tmp = tempfile::tempdir().expect("tmp");
        let spec = VerifierSpec {
            command: "cargo".to_string(),
            args: vec!["--version".to_string()],
            cwd: ".".to_string(),
            summary_success_contains: "__never__".to_string(),
        };
        let out = run_task_verifier(Some(&spec), tmp.path(), 1024).expect("verifier");
        assert!(out.ran);
        assert!(!out.ok);
    }

    #[test]
    fn verifier_can_pass_on_local_fixture() {
        let tmp = tempfile::tempdir().expect("tmp");
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname=\"vpass\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .expect("cargo");
        std::fs::create_dir_all(tmp.path().join("src")).expect("src");
        std::fs::write(
            tmp.path().join("src/lib.rs"),
            "#[cfg(test)] mod tests { #[test] fn ok(){ assert_eq!(2+2,4); } }",
        )
        .expect("lib");
        let spec = VerifierSpec {
            command: "cargo".to_string(),
            args: vec!["test".to_string(), "-q".to_string()],
            cwd: ".".to_string(),
            summary_success_contains: "test result: ok".to_string(),
        };
        let out = run_task_verifier(Some(&spec), tmp.path(), 50_000).expect("verifier");
        assert!(out.ran);
        assert!(out.ok);
    }
}
