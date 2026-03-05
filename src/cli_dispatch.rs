use anyhow::{anyhow, Context};
use clap::Parser;

use crate::cli_args::*;

use crate::mcp::registry::{doctor_server as mcp_doctor_server, list_servers as mcp_list_servers};

use crate::store::provider_to_string;

use crate::*;

use crate::{
    approvals_ops, chat_repl_runtime, ops_helpers, provider_runtime, runtime_paths, scaffold,
    session_ops, startup_bootstrap, startup_init, taskgraph, tasks_graph_runtime, trust, tui,
};

fn argv_has_flag(argv: &[std::ffi::OsString], flag: &str) -> bool {
    argv.iter().skip(1).any(|arg| {
        arg.to_str()
            .is_some_and(|s| s == flag || s.starts_with(&format!("{flag}=")))
    })
}

fn fresh_ephemeral_state_dir() -> std::path::PathBuf {
    std::env::temp_dir()
        .join("localagent")
        .join("run-state")
        .join(uuid::Uuid::new_v4().to_string())
}

struct EphemeralStateDirGuard {
    path: std::path::PathBuf,
}

impl EphemeralStateDirGuard {
    fn new(path: std::path::PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for EphemeralStateDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub(crate) fn apply_run_command_defaults(
    cli: &mut Cli,
    argv: &[std::ffi::OsString],
    workdir: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let _ = workdir;
    if !matches!(
        cli.command,
        None | Some(Commands::Run) | Some(Commands::Exec)
    ) {
        return None;
    }
    let no_session_explicit = argv_has_flag(argv, "--no-session");
    let state_dir_explicit = argv_has_flag(argv, "--state-dir");
    if !no_session_explicit && !state_dir_explicit {
        cli.run.no_session = true;
    }
    if cli.run.state_dir.is_none() && !state_dir_explicit {
        let path = fresh_ephemeral_state_dir();
        cli.run.state_dir = Some(path.clone());
        return Some(path);
    }
    None
}

pub(crate) fn is_bare_startup_bootstrap_invocation(cli: &Cli) -> bool {
    cli.command.is_none()
        && cli.run.provider.is_none()
        && cli.run.model.is_none()
        && cli.run.prompt.is_none()
}

pub(crate) async fn run_cli() -> anyhow::Result<()> {
    let argv = std::env::args_os().collect::<Vec<_>>();
    let mut cli = Cli::parse_from(argv.clone());
    let run_presence = crate::reliability_profile::detect_run_args_presence_from_argv(&argv);

    if cli.run.reliability_profile.is_some() {
        let _ = crate::reliability_profile::apply_builtin_profile_to_run_args(
            &mut cli.run,
            &run_presence,
        )?;
    }

    if cli.run.no_limits && !cli.run.unsafe_mode {
        return Err(anyhow!("--no-limits requires --unsafe"));
    }
    validate_sampling_args(&cli.run)?;

    if cli.run.unsafe_mode {
        eprintln!("WARN: unsafe mode enabled");
    }

    let workdir = std::fs::canonicalize(&cli.run.workdir)
        .with_context(|| format!("failed to resolve workdir: {}", cli.run.workdir.display()))?;
    let ephemeral_state_dir = apply_run_command_defaults(&mut cli, &argv, &workdir);
    let _ephemeral_state_guard = ephemeral_state_dir.map(EphemeralStateDirGuard::new);

    let paths = resolve_state_paths(
        &workdir,
        cli.run.state_dir.clone(),
        cli.run.policy.clone(),
        cli.run.approvals.clone(),
        cli.run.audit.clone(),
    );

    if paths.using_legacy_dir {
        eprintln!(
            "WARN: using legacy state dir at {}",
            paths.state_dir.display()
        );
    }

    if is_bare_startup_bootstrap_invocation(&cli) {
        startup_bootstrap::run_startup_bootstrap(&cli.run, &paths).await?;
        return Ok(());
    }

    startup_init::maybe_auto_init_state(&cli.command, cli.run.state_dir.clone(), &workdir, &paths)?;

    match &cli.command {
        Some(Commands::Run) | Some(Commands::Exec) => {}

        Some(Commands::Version(args)) => {
            let info = version_info();

            if args.json {
                println!("{}", serde_json::to_string_pretty(&info)?);
            } else {
                println!("LocalAgent {}", info.version);

                println!("git_sha: {}", info.git_sha);

                println!("target: {}", info.target);

                println!("build_time_utc: {}", info.build_time_utc);
            }

            return Ok(());
        }

        Some(Commands::Init(args)) => {
            let init_workdir = if let Some(w) = &args.workdir {
                std::fs::canonicalize(w)
                    .with_context(|| format!("failed to resolve workdir: {}", w.display()))?
            } else {
                workdir.clone()
            };

            let out = scaffold::run_init(&InitOptions {
                workdir: init_workdir,

                state_dir_override: args.state_dir.clone(),

                force: args.force,

                print_only: args.print,
            })?;

            print!("{out}");

            return Ok(());
        }

        Some(Commands::Template(args)) => {
            match &args.command {
                TemplateSubcommand::List => {
                    for name in scaffold::list_templates() {
                        println!("{name}");
                    }
                }

                TemplateSubcommand::Show { name } => {
                    let content = scaffold::template_content(name)
                        .ok_or_else(|| anyhow!("unknown template '{}'", name))?;

                    print!("{content}");
                }

                TemplateSubcommand::Write { name, out, force } => {
                    scaffold::write_template(name, out, *force)?;

                    println!("wrote template {} to {}", name, out.display());
                }
            }

            return Ok(());
        }

        Some(Commands::Chat(args)) => {
            chat_repl_runtime::run_chat_repl(args, &cli.run, &paths).await?;

            return Ok(());
        }

        Some(Commands::Doctor(args)) => {
            crate::cli_dispatch_misc_ops::handle_doctor_command(args, &cli.run, &workdir).await?;
            return Ok(());
        }

        Some(Commands::Mcp(args)) => {
            let mcp_config_path =
                runtime_paths::resolved_mcp_config_path(&cli.run, &paths.state_dir);

            match &args.command {
                McpSubcommand::List => {
                    let names = mcp_list_servers(&mcp_config_path)?;

                    for n in names {
                        println!("{n}");
                    }

                    return Ok(());
                }

                McpSubcommand::Doctor { name } => {
                    match mcp_doctor_server(&mcp_config_path, name).await {
                        Ok(count) => {
                            println!("OK: mcp {} tool_count={}", name, count);

                            return Ok(());
                        }

                        Err(e) => {
                            println!("FAIL: {}", e);

                            std::process::exit(1);
                        }
                    }
                }
            }
        }

        Some(Commands::Check(args)) => {
            crate::cli_dispatch_checks::handle_check_command(args, &cli.run, &workdir, &paths)
                .await?;
            return Ok(());
        }

        Some(Commands::Repo(args)) => {
            crate::cli_dispatch_misc_ops::handle_repo_command(args, &workdir, &paths)?;
            return Ok(());
        }

        Some(Commands::Hooks(args)) => {
            let hooks_path = runtime_paths::resolved_hooks_config_path(&cli.run, &paths.state_dir);

            match &args.command {
                HooksSubcommand::List => {
                    ops_helpers::handle_hooks_list(&hooks_path)?;

                    return Ok(());
                }

                HooksSubcommand::Doctor => {
                    if let Err(e) = ops_helpers::handle_hooks_doctor(
                        &hooks_path,
                        &cli.run,
                        provider_to_string(ProviderKind::Ollama),
                    )
                    .await
                    {
                        println!("FAIL: {e}");

                        std::process::exit(1);
                    }

                    println!("OK: hooks doctor");

                    return Ok(());
                }
            }
        }

        Some(Commands::Policy(args)) => match &args.command {
            PolicySubcommand::Doctor { policy } => {
                let policy_path = policy.clone().unwrap_or_else(|| paths.policy_path.clone());

                match ops_helpers::policy_doctor_output(&policy_path) {
                    Ok(text) => {
                        println!("{text}");

                        return Ok(());
                    }

                    Err(e) => {
                        println!("FAIL: {}", e);

                        std::process::exit(1);
                    }
                }
            }

            PolicySubcommand::PrintEffective { policy, json } => {
                let policy_path = policy.clone().unwrap_or_else(|| paths.policy_path.clone());

                println!(
                    "{}",
                    ops_helpers::policy_effective_output(&policy_path, *json)?
                );

                return Ok(());
            }

            PolicySubcommand::Test {
                cases,

                json,

                policy,
            } => {
                let policy_path = policy.clone().unwrap_or_else(|| paths.policy_path.clone());

                let report = trust::policy_test::run_policy_tests(&policy_path, cases)?;

                if *json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    for case in &report.cases {
                        println!(
                            "{}\t{}\texpected={}\tgot={}\treason={}\tsource={}",
                            if case.pass { "PASS" } else { "FAIL" },
                            case.name,
                            case.expected,
                            case.got,
                            case.reason.as_deref().unwrap_or("-"),
                            case.source.as_deref().unwrap_or("-")
                        );
                    }

                    println!("summary: passed={} failed={}", report.passed, report.failed);
                }

                if report.failed > 0 {
                    std::process::exit(1);
                }

                return Ok(());
            }
        },

        Some(Commands::Approvals(args)) => {
            approvals_ops::handle_approvals_command(&paths.approvals_path, &args.command)?;

            return Ok(());
        }

        Some(Commands::Approve(args)) => {
            let store = ApprovalsStore::new(paths.approvals_path.clone());

            store.approve(&args.id, args.ttl_hours, args.max_uses)?;

            println!("approved {}", args.id);

            return Ok(());
        }

        Some(Commands::Deny(args)) => {
            let store = ApprovalsStore::new(paths.approvals_path.clone());

            store.deny(&args.id)?;

            println!("denied {}", args.id);

            return Ok(());
        }

        Some(Commands::Replay(args)) => {
            crate::cli_dispatch_eval_replay::handle_replay_command(args, &paths).await?;
            return Ok(());
        }

        Some(Commands::Session(args)) => {
            if cli.run.no_session {
                return Err(anyhow!(
                    "session commands require sessions enabled (remove --no-session)"
                ));
            }

            let session_path = paths.sessions_dir.join(format!("{}.json", cli.run.session));

            let store = SessionStore::new(session_path, cli.run.session.clone());

            session_ops::handle_session_command(&store, &args.command)?;

            return Ok(());
        }

        Some(Commands::Profile(args)) => {
            crate::cli_dispatch_misc_ops::handle_profile_command(args)?;
            return Ok(());
        }

        Some(Commands::Pack(args)) => {
            crate::cli_dispatch_misc_ops::handle_pack_command(args, &workdir)?;
            return Ok(());
        }

        Some(Commands::Learn(args)) => {
            crate::cli_dispatch_learn::handle_learn_command(args, &cli.run, &workdir, &paths)
                .await?;
            return Ok(());
        }

        Some(Commands::Eval(eval_cmd)) => {
            crate::cli_dispatch_eval_replay::handle_eval_command(eval_cmd, &paths).await?;
            return Ok(());
        }

        Some(Commands::Tui(args)) => match &args.command {
            TuiSubcommand::Tail { events, refresh_ms } => {
                if let Err(e) = tui::tail::run_tail(events, *refresh_ms) {
                    eprintln!("FAIL: {e}");

                    std::process::exit(1);
                }

                return Ok(());
            }
        },

        Some(Commands::Tasks(args)) => {
            match &args.command {
                TasksSubcommand::Status(s) => {
                    let raw = std::fs::read_to_string(&s.checkpoint).with_context(|| {
                        format!("failed reading checkpoint {}", s.checkpoint.display())
                    })?;

                    let cp: taskgraph::TasksCheckpoint =
                        serde_json::from_str(&raw).context("failed parsing checkpoint JSON")?;

                    println!("{}", serde_json::to_string_pretty(&cp)?);
                }

                TasksSubcommand::Reset(s) => {
                    if s.checkpoint.exists() {
                        std::fs::remove_file(&s.checkpoint).with_context(|| {
                            format!("failed deleting checkpoint {}", s.checkpoint.display())
                        })?;
                    }

                    println!("checkpoint reset: {}", s.checkpoint.display());
                }

                TasksSubcommand::Run(s) => {
                    let exit = tasks_graph_runtime::run_tasks_graph(s, &cli.run, &paths).await?;

                    if exit != 0 {
                        std::process::exit(exit);
                    }
                }
            }

            return Ok(());
        }

        None => {}
    }

    validate_run_output_mode(&cli.run)?;

    let provider_kind = cli
        .run
        .provider
        .ok_or_else(|| anyhow!("--provider is required in run mode"))?;

    let model = cli
        .run
        .model
        .clone()
        .ok_or_else(|| anyhow!("--model is required in run mode"))?;

    let prompt = cli
        .run
        .prompt
        .clone()
        .ok_or_else(|| anyhow!("--prompt is required in run mode"))?;

    let base_url = cli
        .run
        .base_url
        .clone()
        .unwrap_or_else(|| provider_runtime::default_base_url(provider_kind).to_string());

    match provider_kind {
        ProviderKind::Lmstudio | ProviderKind::Llamacpp => {
            let provider = match OpenAiCompatProvider::new(
                base_url.clone(),
                cli.run.api_key.clone(),
                provider_runtime::http_config_from_run_args(&cli.run),
            ) {
                Ok(p) => p,
                Err(e) => {
                    maybe_emit_pre_run_json_failure(&cli.run, &e.to_string());
                    return Err(e);
                }
            };

            let res = match run_agent(
                provider,
                provider_kind,
                &base_url,
                &model,
                &prompt,
                &cli.run,
                &paths,
            )
            .await
            {
                Ok(v) => v,
                Err(e) => {
                    maybe_emit_pre_run_json_failure(&cli.run, &e.to_string());
                    return Err(e);
                }
            };

            if matches!(res.outcome.exit_reason, AgentExitReason::ProviderError) {
                let err = res
                    .outcome
                    .error
                    .unwrap_or_else(|| "provider error".to_string());

                return Err(anyhow!(

                    "{}\nHint: run `localagent doctor --provider {} --base-url {}`\nDefault base URL for {} is {}",

                    err,

                    provider_runtime::provider_cli_name(provider_kind),

                    base_url,

                    provider_runtime::provider_cli_name(provider_kind),

                    provider_runtime::default_base_url(provider_kind)

                ));
            }
        }

        ProviderKind::Ollama => {
            let provider = match OllamaProvider::new(
                base_url.clone(),
                provider_runtime::http_config_from_run_args(&cli.run),
            ) {
                Ok(p) => p,
                Err(e) => {
                    maybe_emit_pre_run_json_failure(&cli.run, &e.to_string());
                    return Err(e);
                }
            };

            let res = match run_agent(
                provider,
                provider_kind,
                &base_url,
                &model,
                &prompt,
                &cli.run,
                &paths,
            )
            .await
            {
                Ok(v) => v,
                Err(e) => {
                    maybe_emit_pre_run_json_failure(&cli.run, &e.to_string());
                    return Err(e);
                }
            };

            if matches!(res.outcome.exit_reason, AgentExitReason::ProviderError) {
                let err = res
                    .outcome
                    .error
                    .unwrap_or_else(|| "provider error".to_string());

                return Err(anyhow!(

                    "{}\nHint: run `localagent doctor --provider {} --base-url {}`\nDefault base URL for {} is {}",

                    err,

                    provider_runtime::provider_cli_name(provider_kind),

                    base_url,

                    provider_runtime::provider_cli_name(provider_kind),

                    provider_runtime::default_base_url(provider_kind)

                ));
            }
        }

        ProviderKind::Mock => {
            let provider = MockProvider::new();

            let _ = match run_agent(
                provider,
                provider_kind,
                &base_url,
                &model,
                &prompt,
                &cli.run,
                &paths,
            )
            .await
            {
                Ok(v) => v,
                Err(e) => {
                    maybe_emit_pre_run_json_failure(&cli.run, &e.to_string());
                    return Err(e);
                }
            };
        }
    }

    Ok(())
}

pub(crate) fn validate_sampling_args(run: &RunArgs) -> anyhow::Result<()> {
    if let Some(top_p) = run.top_p {
        if !(top_p > 0.0 && top_p <= 1.0) {
            return Err(anyhow!("--top-p must be > 0 and <= 1"));
        }
    }
    if matches!(run.max_tokens, Some(0)) {
        return Err(anyhow!("--max-tokens must be > 0"));
    }
    Ok(())
}

pub(crate) fn validate_run_output_mode(run: &RunArgs) -> anyhow::Result<()> {
    if run.tui && matches!(run.output, RunOutputMode::Json) {
        return Err(anyhow!(
            "--output json is incompatible with --tui; use --output human or disable --tui"
        ));
    }
    Ok(())
}

fn maybe_emit_pre_run_json_failure(run: &RunArgs, message: &str) {
    if !matches!(run.output, RunOutputMode::Json) {
        return;
    }
    let projected = crate::events::projected_pre_run_failure_v1(message);
    if let Ok(line) = serde_json::to_string(&projected) {
        println!("{line}");
    }
}
