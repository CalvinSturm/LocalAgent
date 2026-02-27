use std::path::PathBuf;

use crate::provider_runtime;
use crate::store;
use crate::*;

pub(crate) struct CheckRunCommandOutput {
    pub(crate) report: checks::report::CheckRunReport,
    pub(crate) exit: checks::runner::CheckRunExit,
}

pub(crate) async fn handle_check_command(
    args: &CheckArgs,
    cli_run: &RunArgs,
    workdir: &std::path::Path,
    paths: &store::StatePaths,
) -> anyhow::Result<()> {
    match &args.command {
        CheckSubcommand::Run {
            path,
            json_out,
            junit_out,
            max_checks,
        } => {
            let out = run_check_command(path.clone(), *max_checks, cli_run, workdir, paths).await?;
            write_check_run_outputs(&out, json_out.as_ref(), junit_out.as_ref())?;
            match out.exit {
                checks::runner::CheckRunExit::Ok => Ok(()),
                _ => std::process::exit(out.exit as i32),
            }
        }
    }
}

pub(crate) async fn run_check_command(
    path: Option<PathBuf>,
    max_checks: Option<usize>,
    cli_run: &RunArgs,
    workdir: &std::path::Path,
    paths: &store::StatePaths,
) -> anyhow::Result<CheckRunCommandOutput> {
    let provider_kind = match cli_run.provider {
        Some(p) => p,
        None => {
            let mut report = checks::runner::report_single_error(
                "CHECK_RUNNER_CONFIG_INVALID",
                "--provider is required for `localagent check run`",
            );
            apply_check_runner_report_meta(&mut report, cli_run, None, None);
            return Ok(CheckRunCommandOutput {
                report,
                exit: checks::runner::CheckRunExit::InvalidChecks,
            });
        }
    };
    let model = match &cli_run.model {
        Some(m) => m.clone(),
        None => {
            let mut report = checks::runner::report_single_error(
                "CHECK_RUNNER_CONFIG_INVALID",
                "--model is required for `localagent check run`",
            );
            apply_check_runner_report_meta(&mut report, cli_run, Some(provider_kind), None);
            return Ok(CheckRunCommandOutput {
                report,
                exit: checks::runner::CheckRunExit::InvalidChecks,
            });
        }
    };
    let base_url = cli_run
        .base_url
        .clone()
        .unwrap_or_else(|| provider_runtime::default_base_url(provider_kind).to_string());
    let checks = match checks::runner::load_checks_for_run(
        workdir,
        &checks::runner::CheckRunArgs { path, max_checks },
    ) {
        Ok(c) => c,
        Err(boxed) => {
            let (mut report, exit) = *boxed;
            apply_check_runner_report_meta(&mut report, cli_run, Some(provider_kind), Some(&model));
            return Ok(CheckRunCommandOutput { report, exit });
        }
    };

    let mut results = Vec::new();
    for check in checks {
        if let Some((status, summary)) = check_capability_denial(&check, cli_run) {
            results.push(checks::report::CheckRunResult {
                name: check.name,
                path: check.path,
                description: check.description,
                status: status.to_string(),
                reason_code: Some("CHECK_CAPABILITY_DENIED".to_string()),
                summary,
                required: check.required,
                file_bytes_hash_hex: check.file_bytes_hash_hex,
                frontmatter_hash_hex: check.frontmatter_hash_hex,
                check_hash_hex: check.check_hash_hex,
            });
            continue;
        }

        let mut run_args = cli_run.clone();
        run_args.no_session = true;
        run_args.reset_session = false;
        run_args.approval_mode = crate::gate::ApprovalMode::Fail;
        if let Some(b) = &check.frontmatter.budget {
            if let Some(ms) = b.max_steps {
                run_args.max_steps = ms as usize;
            }
            if let Some(mt) = b.max_tool_calls {
                run_args.max_total_tool_calls = mt as usize;
            }
            if let Some(t) = b.max_time_ms {
                run_args.max_wall_time_ms = t;
            }
        }

        let mut isolated_paths = None;
        let mut _scratch_guard = None;
        if check_requires_scratch_isolation(&check) {
            match prepare_check_scratch_workspace(workdir) {
                Ok((scratch_guard, scratch_workdir)) => {
                    run_args.workdir = scratch_workdir.clone();
                    isolated_paths = Some(resolve_state_paths(
                        &scratch_workdir,
                        None,
                        None,
                        None,
                        None,
                    ));
                    _scratch_guard = Some(scratch_guard);
                }
                Err(e) => {
                    results.push(checks::report::CheckRunResult {
                        name: check.name,
                        path: check.path,
                        description: check.description,
                        status: "error".to_string(),
                        reason_code: Some("CHECK_RUNNER_INTERNAL_ERROR".to_string()),
                        summary: format!("failed to prepare isolated scratch workspace: {e}"),
                        required: check.required,
                        file_bytes_hash_hex: check.file_bytes_hash_hex,
                        frontmatter_hash_hex: check.frontmatter_hash_hex,
                        check_hash_hex: check.check_hash_hex,
                    });
                    continue;
                }
            }
        }

        let run_res = execute_check_agent_run(
            provider_kind,
            &base_url,
            &model,
            &check.body,
            &run_args,
            isolated_paths.as_ref().unwrap_or(paths),
        )
        .await;

        match run_res {
            Ok(res) => {
                let outcome = res.outcome;
                if let Some(msg) = check_allowed_tools_violation(&check, &outcome) {
                    results.push(checks::report::CheckRunResult {
                        name: check.name,
                        path: check.path,
                        description: check.description,
                        status: "failed".to_string(),
                        reason_code: Some("CHECK_ALLOWED_TOOLS_VIOLATION".to_string()),
                        summary: msg,
                        required: check.required,
                        file_bytes_hash_hex: check.file_bytes_hash_hex,
                        frontmatter_hash_hex: check.frontmatter_hash_hex,
                        check_hash_hex: check.check_hash_hex,
                    });
                    continue;
                }
                match checks::runner::evaluate_final_output(&check, &outcome.final_output) {
                    Ok(()) => results.push(checks::report::CheckRunResult {
                        name: check.name,
                        path: check.path,
                        description: check.description,
                        status: "passed".to_string(),
                        reason_code: None,
                        summary: format!(
                            "exit_reason={} final_output_len={}",
                            outcome.exit_reason.as_str(),
                            outcome.final_output.len()
                        ),
                        required: check.required,
                        file_bytes_hash_hex: check.file_bytes_hash_hex,
                        frontmatter_hash_hex: check.frontmatter_hash_hex,
                        check_hash_hex: check.check_hash_hex,
                    }),
                    Err(msg) => results.push(checks::report::CheckRunResult {
                        name: check.name,
                        path: check.path,
                        description: check.description,
                        status: "failed".to_string(),
                        reason_code: Some("CHECK_PASS_CRITERIA_FAILED".to_string()),
                        summary: msg,
                        required: check.required,
                        file_bytes_hash_hex: check.file_bytes_hash_hex,
                        frontmatter_hash_hex: check.frontmatter_hash_hex,
                        check_hash_hex: check.check_hash_hex,
                    }),
                }
            }
            Err(e) => {
                results.push(checks::report::CheckRunResult {
                    name: check.name,
                    path: check.path,
                    description: check.description,
                    status: "error".to_string(),
                    reason_code: Some("CHECK_RUNNER_INTERNAL_ERROR".to_string()),
                    summary: e.to_string(),
                    required: check.required,
                    file_bytes_hash_hex: check.file_bytes_hash_hex,
                    frontmatter_hash_hex: check.frontmatter_hash_hex,
                    check_hash_hex: check.check_hash_hex,
                });
            }
        }
    }

    let mut report = checks::report::CheckRunReport::from_results(results);
    apply_check_runner_report_meta(&mut report, cli_run, Some(provider_kind), Some(&model));
    let exit = if report.errors > 0 {
        checks::runner::CheckRunExit::RunnerError
    } else if report.failed > 0 {
        checks::runner::CheckRunExit::FailedChecks
    } else {
        checks::runner::CheckRunExit::Ok
    };
    Ok(CheckRunCommandOutput { report, exit })
}

pub(crate) fn write_check_run_outputs(
    out: &CheckRunCommandOutput,
    json_out: Option<&PathBuf>,
    junit_out: Option<&PathBuf>,
) -> anyhow::Result<()> {
    let json = render_check_run_output(out)?;
    if let Some(path) = json_out {
        std::fs::write(path, &json)?;
    } else {
        println!("{json}");
    }
    if let Some(junit) = junit_out {
        checks::report::write_junit(junit, &out.report)?;
    }
    Ok(())
}

pub(crate) fn render_check_run_output(out: &CheckRunCommandOutput) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(&out.report)?)
}

fn check_capability_denial(
    check: &checks::loader::LoadedCheck,
    run: &RunArgs,
) -> Option<(&'static str, String)> {
    for flag in &check.frontmatter.required_flags {
        match flag.as_str() {
            "shell" => {
                if !(run.allow_shell || run.allow_shell_in_workdir) {
                    return Some((
                        if check.required { "failed" } else { "skipped" },
                        "shell capability not enabled (requires --allow-shell or --allow-shell-in-workdir)".to_string(),
                    ));
                }
            }
            "write" => {
                if !(run.allow_write && run.enable_write_tools) {
                    return Some((
                        if check.required { "failed" } else { "skipped" },
                        "write capability not enabled (requires --allow-write and --enable-write-tools)"
                            .to_string(),
                    ));
                }
            }
            other => {
                if let Some(server) = other.strip_prefix("mcp:") {
                    if !run.mcp.iter().any(|m| m == server) {
                        return Some((
                            if check.required { "failed" } else { "skipped" },
                            format!("required MCP server not enabled: {server}"),
                        ));
                    }
                } else {
                    return Some((
                        if check.required { "failed" } else { "skipped" },
                        format!("unknown required flag: {other}"),
                    ));
                }
            }
        }
    }
    None
}

fn apply_check_runner_report_meta(
    report: &mut checks::report::CheckRunReport,
    run: &RunArgs,
    provider_kind: Option<ProviderKind>,
    model: Option<&str>,
) {
    let provider = provider_kind
        .or(run.provider)
        .map(provider_runtime::provider_cli_name)
        .unwrap_or("unset");
    let base_url = run.base_url.clone().unwrap_or_else(|| {
        provider_kind
            .or(run.provider)
            .map(provider_runtime::default_base_url)
            .unwrap_or("unset")
            .to_string()
    });
    let cfg = serde_json::json!({
        "schema": "localagent.check_runner.config.v1",
        "provider": provider,
        "base_url": base_url,
        "model": model.or(run.model.as_deref()).unwrap_or("unset"),
        "approval_mode": "fail",
        "no_session": true,
        "reset_session": false,
        "allow_shell": run.allow_shell,
        "allow_shell_in_workdir": run.allow_shell_in_workdir,
        "allow_write": run.allow_write,
        "enable_write_tools": run.enable_write_tools,
        "trust_mode": format!("{:?}", run.trust).to_lowercase(),
        "tool_args_strict": format!("{:?}", run.tool_args_strict).to_lowercase(),
        "mcp": run.mcp,
        "max_tool_output_bytes": run.max_tool_output_bytes,
        "max_read_bytes": run.max_read_bytes,
        "max_steps": run.max_steps,
        "max_total_tool_calls": run.max_total_tool_calls,
        "max_wall_time_ms": run.max_wall_time_ms
    });
    let canonical = serde_json::to_string(&cfg).unwrap_or_else(|_| "{}".to_string());
    report.runner_profile = "localagent_check_v1".to_string();
    report.runner_config_hash_hex = crate::store::sha256_hex(canonical.as_bytes());
}

fn check_allowed_tools_violation(
    check: &checks::loader::LoadedCheck,
    outcome: &crate::agent::AgentOutcome,
) -> Option<String> {
    let Some(allowed_tools) = &check.frontmatter.allowed_tools else {
        return None;
    };
    let mut used_tools = outcome
        .tool_decisions
        .iter()
        .map(|d| d.tool.as_str())
        .collect::<Vec<_>>();
    if used_tools.is_empty() {
        used_tools.extend(outcome.tool_calls.iter().map(|c| c.name.as_str()));
    }
    for tool_name in used_tools {
        if !allowed_tools.iter().any(|t| t == tool_name) {
            let allowed = if allowed_tools.is_empty() {
                "(no tools allowed)".to_string()
            } else {
                allowed_tools.join(", ")
            };
            return Some(format!(
                "tool '{}' is not allowed by check allowed_tools [{}]",
                tool_name, allowed
            ));
        }
    }
    None
}

fn check_requires_scratch_isolation(check: &checks::loader::LoadedCheck) -> bool {
    check
        .frontmatter
        .required_flags
        .iter()
        .any(|f| f == "shell" || f == "write")
}

struct CheckScratchWorkspace {
    root: PathBuf,
}

impl Drop for CheckScratchWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn prepare_check_scratch_workspace(
    workdir: &std::path::Path,
) -> anyhow::Result<(CheckScratchWorkspace, PathBuf)> {
    let root = std::env::temp_dir().join(format!("localagent-check-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root)?;
    let repo_copy = root.join("repo");
    std::fs::create_dir_all(&repo_copy)?;
    copy_check_workspace_tree(workdir, &repo_copy)?;
    Ok((CheckScratchWorkspace { root }, repo_copy))
}

fn copy_check_workspace_tree(
    src_root: &std::path::Path,
    dst_root: &std::path::Path,
) -> anyhow::Result<()> {
    let mut stack = vec![(src_root.to_path_buf(), dst_root.to_path_buf())];
    while let Some((src_dir, dst_dir)) = stack.pop() {
        std::fs::create_dir_all(&dst_dir)?;
        let mut entries = std::fs::read_dir(&src_dir)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
        for entry in entries {
            let src_path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }

            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if src_dir == src_root
                && (name_str == ".git"
                    || name_str == ".localagent"
                    || name_str == "target"
                    || name_str == "node_modules")
            {
                continue;
            }

            let dst_path = dst_dir.join(&name);
            if file_type.is_dir() {
                stack.push((src_path, dst_path));
            } else if file_type.is_file() {
                std::fs::copy(src_path, dst_path)?;
            }
        }
    }
    Ok(())
}

async fn execute_check_agent_run(
    provider_kind: ProviderKind,
    base_url: &str,
    model: &str,
    prompt: &str,
    run_args: &RunArgs,
    paths: &store::StatePaths,
) -> anyhow::Result<RunExecutionResult> {
    match provider_kind {
        ProviderKind::Lmstudio | ProviderKind::Llamacpp => {
            let provider = OpenAiCompatProvider::new(
                base_url.to_string(),
                run_args.api_key.clone(),
                provider_runtime::http_config_from_run_args(run_args),
            )?;
            run_agent_with_ui(
                provider,
                provider_kind,
                base_url,
                model,
                prompt,
                run_args,
                paths,
                None,
                None,
                None,
                true,
            )
            .await
        }
        ProviderKind::Ollama => {
            let provider = OllamaProvider::new(
                base_url.to_string(),
                provider_runtime::http_config_from_run_args(run_args),
            )?;
            run_agent_with_ui(
                provider,
                provider_kind,
                base_url,
                model,
                prompt,
                run_args,
                paths,
                None,
                None,
                None,
                true,
            )
            .await
        }
        ProviderKind::Mock => {
            let provider = MockProvider::new();
            run_agent_with_ui(
                provider,
                provider_kind,
                base_url,
                model,
                prompt,
                run_args,
                paths,
                None,
                None,
                None,
                true,
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{check_allowed_tools_violation, copy_check_workspace_tree};
    use crate::agent::{AgentExitReason, AgentOutcome, ToolDecisionRecord};
    use crate::checks::loader::LoadedCheck;
    use crate::checks::schema::{CheckFrontmatter, PassCriteria, PassCriteriaType};
    use crate::compaction::{CompactionMode, CompactionSettings, ToolResultPersist};
    use crate::types::ToolCall;

    fn sample_loaded_check(allowed_tools: Option<Vec<&str>>) -> LoadedCheck {
        LoadedCheck {
            path: "x.md".to_string(),
            name: "x".to_string(),
            description: None,
            required: true,
            body: "body".to_string(),
            file_bytes_hash_hex: "a".to_string(),
            frontmatter_hash_hex: "b".to_string(),
            check_hash_hex: "c".to_string(),
            frontmatter: CheckFrontmatter {
                schema_version: 1,
                name: "x".to_string(),
                description: None,
                required: true,
                allowed_tools: allowed_tools
                    .map(|v| v.into_iter().map(|s| s.to_string()).collect()),
                required_flags: Vec::new(),
                pass_criteria: PassCriteria {
                    kind: PassCriteriaType::Equals,
                    value: "ok".to_string(),
                },
                budget: None,
            },
        }
    }

    fn sample_outcome() -> AgentOutcome {
        AgentOutcome {
            run_id: "r".to_string(),
            started_at: "s".to_string(),
            finished_at: "f".to_string(),
            exit_reason: AgentExitReason::Ok,
            final_output: "ok".to_string(),
            error: None,
            messages: Vec::new(),
            tool_calls: Vec::new(),
            tool_decisions: Vec::new(),
            compaction_settings: CompactionSettings {
                max_context_chars: 0,
                mode: CompactionMode::Off,
                keep_last: 0,
                tool_result_persist: ToolResultPersist::Digest,
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

    #[test]
    fn allowed_tools_violation_uses_tool_decisions_first() {
        let check = sample_loaded_check(Some(vec!["read_file"]));
        let mut outcome = sample_outcome();
        outcome.tool_decisions.push(ToolDecisionRecord {
            step: 1,
            tool_call_id: "tc1".to_string(),
            tool: "shell".to_string(),
            decision: "allow".to_string(),
            reason: None,
            source: None,
            taint_overall: None,
            taint_enforced: false,
            escalated: false,
            escalation_reason: None,
        });

        let got = check_allowed_tools_violation(&check, &outcome).expect("violation");
        assert!(got.contains("tool 'shell'"));
        assert!(got.contains("read_file"));
    }

    #[test]
    fn allowed_tools_violation_falls_back_to_tool_calls_when_no_decisions() {
        let check = sample_loaded_check(Some(vec!["read_file"]));
        let mut outcome = sample_outcome();
        outcome.tool_calls.push(ToolCall {
            id: "tc1".to_string(),
            name: "write_file".to_string(),
            arguments: serde_json::json!({"path":"x","content":"y"}),
        });

        let got = check_allowed_tools_violation(&check, &outcome).expect("violation");
        assert!(got.contains("tool 'write_file'"));
    }

    #[test]
    fn allowed_tools_none_disables_post_run_filtering() {
        let check = sample_loaded_check(None);
        let mut outcome = sample_outcome();
        outcome.tool_decisions.push(ToolDecisionRecord {
            step: 1,
            tool_call_id: "tc1".to_string(),
            tool: "shell".to_string(),
            decision: "allow".to_string(),
            reason: None,
            source: None,
            taint_overall: None,
            taint_enforced: false,
            escalated: false,
            escalation_reason: None,
        });

        assert!(check_allowed_tools_violation(&check, &outcome).is_none());
    }

    #[test]
    fn scratch_copy_excludes_root_internal_dirs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src_repo");
        let dst = tmp.path().join("dst_repo");
        std::fs::create_dir_all(src.join(".git")).expect("git dir");
        std::fs::create_dir_all(src.join(".localagent")).expect("state dir");
        std::fs::create_dir_all(src.join("target")).expect("target dir");
        std::fs::create_dir_all(src.join("node_modules")).expect("node_modules dir");
        std::fs::create_dir_all(src.join("subdir")).expect("subdir");
        std::fs::write(src.join("README.md"), "ok").expect("readme");
        std::fs::write(src.join("subdir").join("file.txt"), "ok").expect("subfile");
        std::fs::write(src.join(".git").join("config"), "x").expect("git file");
        std::fs::write(src.join(".localagent").join("state.json"), "x").expect("state file");

        copy_check_workspace_tree(&src, &dst).expect("copy");

        assert!(dst.join("README.md").exists());
        assert!(dst.join("subdir").join("file.txt").exists());
        assert!(!dst.join(".git").exists());
        assert!(!dst.join(".localagent").exists());
        assert!(!dst.join("target").exists());
        assert!(!dst.join("node_modules").exists());
    }
}
