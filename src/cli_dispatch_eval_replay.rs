use anyhow::{anyhow, Context};

use crate::cli_args::*;
use crate::eval::tasks::EvalPack;
use crate::gate::ProviderKind;
use crate::providers::ModelProvider;
use crate::*;
use crate::{eval, provider_runtime, store, task_eval_profile};

pub(crate) async fn handle_replay_command(
    args: &ReplayArgs,
    paths: &store::StatePaths,
) -> anyhow::Result<()> {
    match &args.command {
        Some(ReplaySubcommand::Verify {
            run_id,
            strict,
            json,
        }) => {
            let record = store::load_run_record(&paths.state_dir, run_id).map_err(|e| {
                anyhow!(
                    "failed to load run '{}': {}. runs dir: {}",
                    run_id,
                    e,
                    paths.runs_dir.display()
                )
            })?;

            let report = verify_run_record(&record, *strict)?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", render_verify_report(&report));
            }

            if report.status == "fail" {
                std::process::exit(1);
            }

            Ok(())
        }
        Some(ReplaySubcommand::Resume { run_id }) => {
            let checkpoint = store::load_runtime_checkpoint_record(paths, run_id).map_err(|e| {
                anyhow!(
                    "failed to load runtime checkpoint '{}': {}. checkpoints dir: {}",
                    run_id,
                    e,
                    paths.checkpoints_dir.display()
                )
            })?;
            resume_from_runtime_checkpoint(&checkpoint, paths).await
        }
        None => {
            let run_id = args
                .run_id
                .as_ref()
                .ok_or_else(|| anyhow!("missing run_id. use `localagent replay <run_id>`"))?;

            match store::load_run_record(&paths.state_dir, run_id) {
                Ok(record) => {
                    print!("{}", store::render_replay(&record));
                    Ok(())
                }
                Err(e) => Err(anyhow!(
                    "failed to load run '{}': {}. runs dir: {}",
                    run_id,
                    e,
                    paths.runs_dir.display()
                )),
            }
        }
    }
}

async fn resume_from_runtime_checkpoint(
    checkpoint: &store::RuntimeRunCheckpointRecordV1,
    paths: &store::StatePaths,
) -> anyhow::Result<()> {
    let run_args = load_resume_run_args(checkpoint)?;
    let provider_kind = run_args.provider.unwrap_or(ProviderKind::Ollama);
    let model = run_args
        .model
        .clone()
        .ok_or_else(|| anyhow!("checkpoint resume is missing model configuration"))?;
    let base_url = run_args
        .base_url
        .clone()
        .unwrap_or_else(|| provider_runtime::default_base_url(provider_kind).to_string());

    match provider_kind {
        ProviderKind::Lmstudio | ProviderKind::Llamacpp => {
            let provider = OpenAiCompatProvider::new(
                provider_kind,
                base_url.clone(),
                run_args.api_key.clone(),
                provider_runtime::http_config_from_run_args(&run_args),
            )
            .with_context(|| {
                format!(
                    "failed to initialize {} provider for checkpoint resume",
                    provider_runtime::provider_cli_name(provider_kind)
                )
            })?;
            resume_from_runtime_checkpoint_with_provider(
                checkpoint,
                paths,
                provider,
                provider_kind,
                &base_url,
                &model,
                run_args,
            )
            .await?;
        }
        ProviderKind::Ollama => {
            let provider = OllamaProvider::new(
                base_url.clone(),
                provider_runtime::http_config_from_run_args(&run_args),
            )
            .with_context(|| "failed to initialize ollama provider for checkpoint resume")?;
            resume_from_runtime_checkpoint_with_provider(
                checkpoint,
                paths,
                provider,
                provider_kind,
                &base_url,
                &model,
                run_args,
            )
            .await?;
        }
        ProviderKind::Mock => {
            resume_from_runtime_checkpoint_with_provider(
                checkpoint,
                paths,
                MockProvider::new(),
                provider_kind,
                &base_url,
                &model,
                run_args,
            )
            .await?;
        }
    }

    Ok(())
}

fn load_resume_run_args(checkpoint: &store::RuntimeRunCheckpointRecordV1) -> anyhow::Result<RunArgs> {
    ensure_resumable_checkpoint(checkpoint)?;
    let mut run_args = crate::agent_runtime::checkpoint::parse_resume_args(&checkpoint.resume_argv)?;
    let resume_session = format!("resume-{}", checkpoint.runtime_run_id);
    run_args.no_session = false;
    run_args.reset_session = false;
    run_args.session = resume_session;
    run_args.prompt = Some(checkpoint.prompt.clone());
    Ok(run_args)
}

fn ensure_resumable_checkpoint(
    checkpoint: &store::RuntimeRunCheckpointRecordV1,
) -> anyhow::Result<()> {
    match checkpoint.checkpoint.phase {
        store::RunCheckpointPhase::WaitingForApproval => {}
        store::RunCheckpointPhase::Interrupted => return Ok(()),
    }
    if checkpoint
        .checkpoint
        .pending_interrupt
        .as_ref()
        .map(|it| it.kind != store::RunCheckpointInterruptKind::ApprovalRequired)
        .unwrap_or(true)
    {
        return Err(anyhow!(
            "runtime checkpoint '{}' is not resumable: phase is {:?}",
            checkpoint.runtime_run_id,
            checkpoint.checkpoint.phase
        ));
    }
    Ok(())
}

async fn resume_from_runtime_checkpoint_with_provider<P: ModelProvider>(
    checkpoint: &store::RuntimeRunCheckpointRecordV1,
    paths: &store::StatePaths,
    provider: P,
    provider_kind: ProviderKind,
    base_url: &str,
    model: &str,
    run_args: RunArgs,
) -> anyhow::Result<crate::agent_runtime::RunExecutionResult> {
    ensure_resumable_checkpoint(checkpoint)?;
    approve_checkpoint_boundary_if_present(checkpoint, paths)?;
    seed_resume_session(checkpoint, paths, &run_args)?;
    let resume_prompt = checkpoint_resume_prompt(checkpoint);
    crate::agent_runtime::run_agent(
        provider,
        provider_kind,
        base_url,
        model,
        &resume_prompt,
        &run_args,
        paths,
    )
    .await
}

fn approve_checkpoint_boundary_if_present(
    checkpoint: &store::RuntimeRunCheckpointRecordV1,
    paths: &store::StatePaths,
) -> anyhow::Result<()> {
    if checkpoint.checkpoint.phase != store::RunCheckpointPhase::WaitingForApproval {
        return Ok(());
    }
    let Some(approval_id) = checkpoint
        .pending_tool_call
        .as_ref()
        .and_then(|tool| tool.approval_id.as_deref())
    else {
        return Ok(());
    };
    let approvals = crate::trust::approvals::ApprovalsStore::new(paths.approvals_path.clone());
    approvals.approve(approval_id, None, None).with_context(|| {
        format!(
            "failed to mark approval '{}' approved for checkpoint resume",
            approval_id
        )
    })
}

fn seed_resume_session(
    checkpoint: &store::RuntimeRunCheckpointRecordV1,
    paths: &store::StatePaths,
    run_args: &RunArgs,
) -> anyhow::Result<()> {
    let resume_session = run_args.session.clone();
    let session_path = paths.sessions_dir.join(format!("{resume_session}.json"));
    let session_store = crate::session::SessionStore::new(session_path, resume_session.clone());
    let mut messages = checkpoint.resume_session_messages.clone();
    messages.push(resume_boundary_message(checkpoint));
    session_store.save(
        &crate::session::SessionData {
            name: resume_session,
            updated_at: crate::trust::now_rfc3339(),
            messages,
            settings: crate::session::SessionSettings::default(),
            task_memory: Vec::new(),
        },
        std::cmp::max(
            run_args.max_session_messages,
            checkpoint.resume_session_messages.len().saturating_add(1).max(1),
        ),
    )
}

fn resume_boundary_message(checkpoint: &store::RuntimeRunCheckpointRecordV1) -> crate::types::Message {
    let tool_summary = checkpoint
        .pending_tool_call
        .as_ref()
        .map(|tool| {
            format!(
                "pending_tool={} tool_call_id={} approval_id={} args={}",
                tool.tool_name,
                tool.tool_call_id,
                tool.approval_id.as_deref().unwrap_or("unknown"),
                tool.arguments
            )
        })
        .unwrap_or_else(|| "pending_tool=none".to_string());
    let boundary = checkpoint
        .boundary_output
        .as_deref()
        .unwrap_or("interrupted");
    let phase = match checkpoint.checkpoint.phase {
        store::RunCheckpointPhase::WaitingForApproval => "waiting_for_approval",
        store::RunCheckpointPhase::Interrupted => "interrupted",
    };
    let phase_note = match checkpoint.checkpoint.phase {
        store::RunCheckpointPhase::WaitingForApproval => {
            "Approval has been granted for the stored pending tool call."
        }
        store::RunCheckpointPhase::Interrupted => {
            "Resume from the stored interrupted boundary and continue the prior run."
        }
    };
    crate::types::Message {
        role: crate::types::Role::Developer,
        content: Some(format!(
            "RUNTIME CHECKPOINT RESUME HANDOFF\n\
Boundary phase: {phase}\n\
{phase_note}\n\
Boundary output: {boundary}\n\
{tool_summary}\n\
Continue from this boundary without re-planning from scratch."
        )),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
    }
}

fn checkpoint_resume_prompt(checkpoint: &store::RuntimeRunCheckpointRecordV1) -> String {
    if checkpoint.checkpoint.phase == store::RunCheckpointPhase::WaitingForApproval {
        if let Some(tool) = &checkpoint.pending_tool_call {
            return format!(
                "Resume the interrupted run. Approval has been granted for the pending tool call. \
Continue from the prior conversation state. If the pending step is still needed, call tool \
`{}` with these exact arguments: {}. Then continue the task to completion.",
                tool.tool_name, tool.arguments
            );
        }
        return "Resume the interrupted run. Approval has been granted for the pending step. Continue from the prior conversation state and complete the task.".to_string();
    }
    if let Some(boundary_output) = checkpoint.boundary_output.as_deref() {
        format!(
            "Resume the interrupted run from the stored boundary. Prior boundary output: {}. Continue the task to completion without re-planning from scratch unless the current state requires it.",
            boundary_output
        )
    } else {
        "Resume the interrupted run from the stored boundary and complete the task.".to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{
        checkpoint_resume_prompt, load_resume_run_args, resume_boundary_message,
        resume_from_runtime_checkpoint_with_provider,
    };
    use crate::agent::AgentExitReason;
    use crate::gate::ProviderKind;
    use crate::providers::ModelProvider;
    use crate::store::{
        PendingApprovalToolCallV1, RunCheckpointInterruptKind, RunCheckpointInterruptV1,
        RunCheckpointPhase, RunCheckpointV1, RuntimeRunCheckpointRecordV1,
    };
    use crate::target::ExecTargetKind;
    use crate::trust::approvals::{ApprovalProvenance, ApprovalsStore, StoredStatus};
    use crate::trust::policy::safe_default_policy_repr;
    use crate::types::{GenerateRequest, GenerateResponse, Message, Role, ToolCall};
    use async_trait::async_trait;
    use serde_json::json;
    use tempfile::tempdir;

    #[derive(Clone, Default)]
    struct ResumeScriptedProvider {
        seen: Arc<Mutex<Vec<Vec<Message>>>>,
    }

    #[async_trait]
    impl ModelProvider for ResumeScriptedProvider {
        async fn generate(&self, req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
            self.seen
                .lock()
                .expect("lock seen")
                .push(req.messages.clone());
            let saw_tool_result = req.messages.iter().any(|msg| matches!(msg.role, Role::Tool));
            if saw_tool_result {
                return Ok(GenerateResponse {
                    assistant: Message {
                        role: Role::Assistant,
                        content: Some("resumed ok".to_string()),
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                    },
                    tool_calls: Vec::new(),
                    usage: None,
                });
            }
            Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: None,
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: vec![ToolCall {
                    id: "resume_tc_1".to_string(),
                    name: "shell".to_string(),
                    arguments: json!({"command":"Write-Output resumed"}),
                }],
                usage: None,
            })
        }
    }

    fn approval_checkpoint_record(approval_id: &str) -> RuntimeRunCheckpointRecordV1 {
        RuntimeRunCheckpointRecordV1 {
            schema_version: "openagent.runtime_checkpoint.v1".to_string(),
            runtime_run_id: "checkpoint-run-1".to_string(),
            prompt: "fix the task".to_string(),
            resume_argv: vec![
                "localagent".to_string(),
                "--provider".to_string(),
                "mock".to_string(),
                "--model".to_string(),
                "resume-model".to_string(),
                "--prompt".to_string(),
                "fix the task".to_string(),
                "--allow-shell".to_string(),
                "--trust".to_string(),
                "on".to_string(),
                "--approval-mode".to_string(),
                "interrupt".to_string(),
            ],
            checkpoint: RunCheckpointV1 {
                schema_version: "openagent.run_checkpoint.v1".to_string(),
                phase: RunCheckpointPhase::WaitingForApproval,
                terminal_boundary: true,
                pending_interrupt: Some(RunCheckpointInterruptV1 {
                    kind: RunCheckpointInterruptKind::ApprovalRequired,
                    reason: Some("approval required".to_string()),
                }),
            },
            resume_session_messages: vec![Message {
                role: Role::User,
                content: Some("continue".to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            }],
            tool_facts: Vec::new(),
            tool_fact_envelopes: Vec::new(),
            pending_tool_call: Some(PendingApprovalToolCallV1 {
                tool_call_id: "resume_tc_1".to_string(),
                tool_name: "shell".to_string(),
                arguments: "{\"command\":\"Write-Output resumed\"}".to_string(),
                approval_id: Some(approval_id.to_string()),
                reason: Some("shell approval required".to_string()),
            }),
            boundary_output: Some("approval required".to_string()),
        }
    }

    fn interrupted_checkpoint_record() -> RuntimeRunCheckpointRecordV1 {
        RuntimeRunCheckpointRecordV1 {
            schema_version: "openagent.runtime_checkpoint.v1".to_string(),
            runtime_run_id: "checkpoint-run-2".to_string(),
            prompt: "continue the interrupted task".to_string(),
            resume_argv: vec![
                "localagent".to_string(),
                "--provider".to_string(),
                "mock".to_string(),
                "--model".to_string(),
                "resume-model".to_string(),
                "--prompt".to_string(),
                "continue the interrupted task".to_string(),
                "--allow-shell".to_string(),
                "--trust".to_string(),
                "on".to_string(),
            ],
            checkpoint: RunCheckpointV1 {
                schema_version: "openagent.run_checkpoint.v1".to_string(),
                phase: RunCheckpointPhase::Interrupted,
                terminal_boundary: true,
                pending_interrupt: Some(RunCheckpointInterruptV1 {
                    kind: RunCheckpointInterruptKind::OperatorInterrupt,
                    reason: Some("operator paused run".to_string()),
                }),
            },
            resume_session_messages: vec![Message {
                role: Role::User,
                content: Some("continue".to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            }],
            tool_facts: Vec::new(),
            tool_fact_envelopes: Vec::new(),
            pending_tool_call: None,
            boundary_output: Some("run interrupted by operator".to_string()),
        }
    }

    #[tokio::test]
    async fn replay_resume_approval_checkpoint_runs_to_completion() {
        let tmp = tempdir().expect("tempdir");
        let workdir = tmp.path().join("workdir");
        std::fs::create_dir_all(&workdir).expect("workdir");
        let paths = crate::store::resolve_state_paths(&workdir, None, None, None, None);
        let approvals = ApprovalsStore::new(paths.approvals_path.clone());
        let pending_args = json!({"command":"Write-Output resumed"});
        let approval_key = crate::gate::compute_approval_key(
            "shell",
            &pending_args,
            &workdir,
            &crate::gate::compute_policy_hash_hex(safe_default_policy_repr().as_bytes()),
        );
        let approval_id = approvals
            .create_pending(
                "shell",
                &pending_args,
                Some(approval_key),
                Some(ApprovalProvenance {
                    approval_key_version: "v1".to_string(),
                    tool_schema_hash_hex: None,
                    hooks_config_hash_hex: None,
                    exec_target: Some(
                        match ExecTargetKind::Host {
                            ExecTargetKind::Host => "host",
                            ExecTargetKind::Docker => "docker",
                        }
                        .to_string(),
                    ),
                    planner_hash_hex: None,
                }),
            )
            .expect("create approval");
        let checkpoint = approval_checkpoint_record(&approval_id);
        let mut run_args = load_resume_run_args(&checkpoint).expect("resume args");
        run_args.workdir = workdir.clone();
        let provider = ResumeScriptedProvider::default();

        let result = resume_from_runtime_checkpoint_with_provider(
            &checkpoint,
            &paths,
            provider.clone(),
            ProviderKind::Mock,
            "mock://resume",
            "resume-model",
            run_args,
        )
        .await
        .expect("resume succeeds");

        assert!(matches!(result.outcome.exit_reason, AgentExitReason::Ok));
        assert_eq!(result.outcome.final_output, "resumed ok");
        assert!(result.runtime_checkpoint_path.is_none());

        let approvals_data = approvals.list().expect("approvals list");
        let stored = approvals_data
            .requests
            .get(&approval_id)
            .expect("stored approval");
        assert_eq!(stored.status, StoredStatus::Approved);
        assert_eq!(stored.uses, Some(1));

        let seen = provider.seen.lock().expect("seen lock");
        assert!(seen.iter().any(|messages| {
            messages.iter().any(|message| {
                message.role == Role::Developer
                    && message
                        .content
                        .as_deref()
                        .is_some_and(|content| content.contains("RUNTIME CHECKPOINT RESUME HANDOFF"))
            })
        }));
    }

    #[test]
    fn interrupted_checkpoint_resume_metadata_is_supported() {
        let checkpoint = interrupted_checkpoint_record();
        let run_args = load_resume_run_args(&checkpoint).expect("resume args");
        assert_eq!(run_args.prompt.as_deref(), Some("continue the interrupted task"));

        let prompt = checkpoint_resume_prompt(&checkpoint);
        assert!(prompt.contains("stored boundary"));

        let message = resume_boundary_message(&checkpoint);
        let body = message.content.expect("boundary message");
        assert!(body.contains("Boundary phase: interrupted"));
        assert!(body.contains("run interrupted by operator"));
    }

    #[test]
    fn resume_prompt_and_boundary_message_include_pending_tool_details() {
        let checkpoint = approval_checkpoint_record("approval-1");
        let prompt = checkpoint_resume_prompt(&checkpoint);
        assert!(prompt.contains("Approval has been granted"));
        assert!(prompt.contains("shell"));

        let message = resume_boundary_message(&checkpoint);
        let body = message.content.expect("boundary message");
        assert!(body.contains("RUNTIME CHECKPOINT RESUME HANDOFF"));
        assert!(body.contains("approval_id=approval-1"));
    }
}

pub(crate) async fn handle_eval_command(
    eval_cmd: &EvalCmd,
    paths: &store::StatePaths,
) -> anyhow::Result<()> {
    if let Some(sub) = &eval_cmd.command {
        match sub {
            EvalSubcommand::Profile { command } => {
                match command {
                    EvalProfileSubcommand::List => {
                        for p in list_profiles(&paths.state_dir)? {
                            println!("{p}");
                        }
                    }
                    EvalProfileSubcommand::Show {
                        name,
                        json,
                        profile_path,
                    } => {
                        let loaded = load_profile(
                            &paths.state_dir,
                            Some(name.as_str()),
                            profile_path.as_deref(),
                        )?;

                        if *json {
                            println!("{}", serde_json::to_string_pretty(&loaded.profile)?);
                        } else {
                            println!("{}", serde_yaml::to_string(&loaded.profile)?);
                        }
                    }
                    EvalProfileSubcommand::Doctor { name, profile_path } => {
                        let loaded = load_profile(
                            &paths.state_dir,
                            Some(name.as_str()),
                            profile_path.as_deref(),
                        )?;

                        let req = doctor_profile(&loaded.profile)?;

                        let provider = match loaded.profile.provider.as_deref() {
                            Some("lmstudio") => ProviderKind::Lmstudio,
                            Some("llamacpp") => ProviderKind::Llamacpp,
                            Some("mock") => ProviderKind::Mock,
                            _ => ProviderKind::Ollama,
                        };

                        let base_url = loaded.profile.base_url.clone().unwrap_or_else(|| {
                            provider_runtime::default_base_url(provider).to_string()
                        });

                        match provider_runtime::doctor_check(&DoctorArgs {
                            docker: false,
                            provider: Some(provider),
                            base_url: Some(base_url.clone()),
                            api_key: None,
                        })
                        .await
                        {
                            Ok(ok) => println!("{ok}"),
                            Err(e) => {
                                eprintln!("FAIL: {e}");
                                std::process::exit(1);
                            }
                        }

                        if req.is_empty() {
                            println!("Required flags: (none)");
                        } else {
                            println!("Required flags: {}", req.join(" "));
                        }
                    }
                }

                return Ok(());
            }

            EvalSubcommand::Baseline { command } => {
                match command {
                    EvalBaselineSubcommand::Create { name, from } => {
                        let path = create_baseline_from_results(&paths.state_dir, name, from)?;
                        println!("created baseline {} at {}", name, path.display());
                    }
                    EvalBaselineSubcommand::Show { name } => {
                        let b = load_baseline(&paths.state_dir, name)?;
                        println!("{}", serde_json::to_string_pretty(&b)?);
                    }
                    EvalBaselineSubcommand::Delete { name } => {
                        delete_baseline(&paths.state_dir, name)?;
                        println!("deleted baseline {name}");
                    }
                    EvalBaselineSubcommand::List => {
                        for n in list_baselines(&paths.state_dir)? {
                            println!("{n}");
                        }
                    }
                }

                return Ok(());
            }

            EvalSubcommand::Report { command } => {
                match command {
                    EvalReportSubcommand::Compare { a, b, out, json } => {
                        compare_results_files(a, b, out, json.as_deref())?;

                        println!("compare report written: {}", out.display());

                        if let Some(j) = json {
                            println!("compare json written: {}", j.display());
                        }
                    }
                }

                return Ok(());
            }
        }
    }

    let mut args = eval_cmd.run.clone();

    let loaded_profile =
        task_eval_profile::apply_eval_profile_overrides(&mut args, &paths.state_dir)?;

    if args.no_limits && !args.unsafe_mode {
        return Err(anyhow!("--no-limits requires --unsafe"));
    }

    if args.unsafe_mode {
        eprintln!("WARN: unsafe mode enabled");
    }

    let models = args
        .models
        .clone()
        .ok_or_else(|| anyhow!("--models is required and must not be empty"))?
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if models.is_empty() {
        return Err(anyhow!("--models is required and must not be empty"));
    }

    let mut enable_write_tools = args.enable_write_tools;

    if matches!(args.pack, EvalPack::Coding | EvalPack::All) && !args.enable_write_tools {
        enable_write_tools = true;
    }

    let cfg = EvalConfig {
        provider: args.provider,
        base_url: args
            .base_url
            .clone()
            .unwrap_or_else(|| provider_runtime::default_base_url(args.provider).to_string()),
        api_key: args.api_key.clone(),
        models,
        pack: args.pack,
        out: args.out.clone(),
        junit: args.junit.clone(),
        summary_md: args.summary_md.clone(),
        cost_model_path: args.cost_model.clone(),
        runs_per_task: args.runs_per_task,
        max_steps: args.max_steps,
        max_wall_time_ms: args.max_wall_time_ms,
        max_mcp_calls: args.max_mcp_calls,
        tool_exec_timeout_ms: args.tool_exec_timeout_ms,
        post_write_verify_timeout_ms: args.post_write_verify_timeout_ms,
        timeout_seconds: args.timeout_seconds,
        trust: args.trust,
        approval_mode: args.approval_mode,
        auto_approve_scope: args.auto_approve_scope,
        approval_key: args.approval_key,
        enable_write_tools,
        allow_write: args.allow_write,
        allow_shell: args.allow_shell,
        unsafe_mode: args.unsafe_mode,
        no_limits: args.no_limits,
        unsafe_bypass_allow_flags: args.unsafe_bypass_allow_flags,
        mcp: args.mcp.clone(),
        mcp_config: args.mcp_config.clone(),
        session: args.session.clone(),
        no_session: args.no_session,
        max_session_messages: args.max_session_messages,
        max_context_chars: args.max_context_chars,
        compaction_mode: args.compaction_mode,
        compaction_keep_last: args.compaction_keep_last,
        tool_result_persist: args.tool_result_persist,
        hooks_mode: args.hooks,
        hooks_config: args.hooks_config.clone(),
        hooks_strict: args.hooks_strict,
        hooks_timeout_ms: args.hooks_timeout_ms,
        hooks_max_stdout_bytes: args.hooks_max_stdout_bytes,
        tool_args_strict: args.tool_args_strict,
        tui_enabled: false,
        tui_refresh_ms: 50,
        tui_max_log_lines: 200,
        state_dir_override: args.state_dir.clone(),
        policy_override: args.policy.clone(),
        approvals_override: args.approvals.clone(),
        audit_override: args.audit.clone(),
        workdir_override: args.workdir.clone(),
        keep_workdir: args.keep_workdir,
        http: provider_runtime::http_config_from_eval_args(&args),
        mode: args.mode,
        planner_model: args.planner_model.clone(),
        worker_model: args.worker_model.clone(),
        min_pass_rate: args.min_pass_rate,
        fail_on_any: args.fail_on_any,
        max_avg_steps: args.max_avg_steps,
        resolved_profile_name: args.profile.clone(),
        resolved_profile_path: loaded_profile
            .as_ref()
            .map(|p| stable_path_string(&p.path))
            .or_else(|| args.profile_path.as_ref().map(|p| stable_path_string(p))),
        resolved_profile_hash_hex: loaded_profile.as_ref().map(|p| p.hash_hex.clone()),
    };

    let cwd = std::env::current_dir().with_context(|| "failed to read current dir")?;
    let results_path = run_eval(cfg.clone(), &cwd).await?;

    let mut exit_fail = false;
    let mut results: eval::runner::EvalResults =
        serde_json::from_slice(&std::fs::read(&results_path)?)?;

    if let Some(name) = args.baseline.clone() {
        let created = create_baseline_from_results(&paths.state_dir, &name, &results_path)?;
        println!("baseline created: {} ({})", name, created.display());
    }

    let avg_steps = eval::baseline::avg_steps(&results);
    let mut threshold_failures = Vec::new();

    if results.summary.pass_rate < args.min_pass_rate {
        threshold_failures.push(format!(
            "pass_rate {} < min_pass_rate {}",
            results.summary.pass_rate, args.min_pass_rate
        ));
    }

    if let Some(max_avg) = args.max_avg_steps {
        if avg_steps > max_avg {
            threshold_failures.push(format!(
                "avg_steps {} > max_avg_steps {}",
                avg_steps, max_avg
            ));
        }
    }

    if args.fail_on_any && results.summary.failed > 0 {
        threshold_failures.push(format!("failed runs present: {}", results.summary.failed));
    }

    if !threshold_failures.is_empty() {
        exit_fail = true;
        eprintln!("THRESHOLDS: FAIL");
        for f in &threshold_failures {
            eprintln!(" - {f}");
        }
    }

    if let Some(name) = args.compare_baseline.clone() {
        let path = baseline_path(&paths.state_dir, &name);
        let baseline = load_baseline(&paths.state_dir, &name)?;
        let mut profile_hash_mismatch = false;

        if baseline.profile_hash_hex != results.config.resolved_profile_hash_hex {
            profile_hash_mismatch = true;
            eprintln!(
                "WARN: baseline profile hash mismatch (baseline={:?}, current={:?})",
                baseline.profile_hash_hex, results.config.resolved_profile_hash_hex
            );
        }

        let reg = compare_results(&baseline, &results);

        println!(
            "REGRESSION: {}",
            if reg.passed {
                "PASS".to_string()
            } else {
                format!("FAIL ({} failures)", reg.failures.len())
            }
        );

        if args.fail_on_regression && !reg.passed {
            exit_fail = true;
        }

        results.baseline = Some(eval::runner::EvalBaselineStatus {
            name,
            path: stable_path_string(&path),
            loaded: true,
            profile_hash_mismatch,
        });

        results.regression = Some(reg);
        std::fs::write(&results_path, serde_json::to_string_pretty(&results)?)?;
    }

    if let Some(bundle_path) = args.bundle.clone() {
        let should_bundle = !args.bundle_on_fail || exit_fail;
        if should_bundle {
            let out = create_bundle(&BundleSpec {
                bundle_path,
                state_dir: paths.state_dir.clone(),
                results_path: results_path.clone(),
                junit_path: args.junit.clone(),
                summary_md_path: args.summary_md.clone(),
                baseline_name: args.compare_baseline.clone(),
                profile_name: args.profile.clone(),
                profile_hash_hex: results.config.resolved_profile_hash_hex.clone(),
            })?;

            println!("bundle written: {}", out.display());
        }
    }

    if exit_fail {
        std::process::exit(1);
    }

    Ok(())
}
