use anyhow::{anyhow, Context};

use crate::cli_args::{
    LearnArgs, LearnCategoryArg, LearnPromoteTargetArg, LearnStatusArg, LearnSubcommand, RunArgs,
};
use crate::learning;
use crate::providers::ModelProvider;
use crate::store::StatePaths;
use crate::types::{GenerateRequest, Message, Role};

pub(crate) async fn handle_learn_command(
    args: &LearnArgs,
    cli_run: &RunArgs,
    workdir: &std::path::Path,
    paths: &StatePaths,
) -> anyhow::Result<()> {
    match &args.command {
        LearnSubcommand::Capture {
            run,
            assist,
            write,
            category,
            summary,
            task_summary,
            profile,
            guidance_text,
            check_text,
            tags,
            evidence,
            evidence_notes,
        } => {
            validate_capture_assist_flags(*assist, *write)?;
            let category = match category {
                LearnCategoryArg::WorkflowHint => learning::LearningCategoryV1::WorkflowHint,
                LearnCategoryArg::PromptGuidance => learning::LearningCategoryV1::PromptGuidance,
                LearnCategoryArg::CheckCandidate => learning::LearningCategoryV1::CheckCandidate,
            };
            let input = learning::build_capture_input(
                run.clone(),
                category,
                summary.clone(),
                task_summary.clone(),
                profile.clone(),
                guidance_text.clone(),
                check_text.clone(),
                tags.clone(),
                evidence.clone(),
                evidence_notes.clone(),
            );
            if *assist {
                let assisted = generate_assisted_capture_preview(cli_run, &input).await?;
                println!(
                    "{}",
                    learning::render_assist_capture_preview(&assisted.preview)
                );
                if !*write {
                    return Ok(());
                }
                let assist_meta = learning::build_assist_capture_meta(
                    &assisted.preview.provider,
                    &assisted.preview.model,
                    &assisted.preview.input_hash_hex,
                    input.run_id.as_deref(),
                    assisted.output_truncated,
                );
                let input = learning::apply_assisted_draft_to_capture_input(
                    input,
                    &assisted.preview.draft,
                    assist_meta,
                );
                let out = learning::capture_learning_entry(&paths.state_dir, input)
                    .context("failed to capture assisted learning entry")?;
                learning::emit_learning_captured_event(&paths.state_dir, &out.entry)
                    .context("failed to emit learning_captured event")?;
                println!("{}", learning::render_capture_confirmation(&out.entry));
                return Ok(());
            }
            let out = learning::capture_learning_entry(&paths.state_dir, input)
                .context("failed to capture learning entry")?;
            learning::emit_learning_captured_event(&paths.state_dir, &out.entry)
                .context("failed to emit learning_captured event")?;
            println!("{}", learning::render_capture_confirmation(&out.entry));
            Ok(())
        }
        LearnSubcommand::List {
            statuses,
            categories,
            limit,
            show_archived,
            format,
        } => {
            let mut entries = learning::list_learning_entries(&paths.state_dir)
                .context("failed to list learning entries")?;
            if !categories.is_empty() {
                let wanted = categories
                    .iter()
                    .map(|c| match c {
                        LearnCategoryArg::WorkflowHint => {
                            learning::LearningCategoryV1::WorkflowHint
                        }
                        LearnCategoryArg::PromptGuidance => {
                            learning::LearningCategoryV1::PromptGuidance
                        }
                        LearnCategoryArg::CheckCandidate => {
                            learning::LearningCategoryV1::CheckCandidate
                        }
                    })
                    .collect::<Vec<_>>();
                entries.retain(|e| wanted.contains(&e.category));
            }
            if !statuses.is_empty() {
                let wanted = statuses
                    .iter()
                    .map(|s| match s {
                        LearnStatusArg::Captured => learning::LearningStatusV1::Captured,
                        LearnStatusArg::Promoted => learning::LearningStatusV1::Promoted,
                        LearnStatusArg::Archived => learning::LearningStatusV1::Archived,
                    })
                    .collect::<Vec<_>>();
                entries.retain(|e| wanted.contains(&e.status));
            } else if !show_archived {
                entries.retain(|e| e.status != learning::LearningStatusV1::Archived);
            }
            let limit = *limit;
            if entries.len() > limit {
                entries.truncate(limit);
            }
            match format.as_str() {
                "table" => {
                    println!("{}", learning::render_learning_list_table(&entries));
                    Ok(())
                }
                "json" => {
                    println!(
                        "{}",
                        learning::render_learning_list_json_preview(&entries)
                            .context("failed to render learn list JSON preview")?
                    );
                    Ok(())
                }
                other => Err(anyhow!(
                    "unsupported learn list format '{other}' (expected table|json)"
                )),
            }
        }
        LearnSubcommand::Show {
            id,
            format,
            show_evidence,
            show_proposed,
        } => {
            let entry = learning::load_learning_entry(&paths.state_dir, id)
                .with_context(|| format!("failed to load learning entry {id}"))?;
            match format.as_str() {
                "text" => {
                    println!(
                        "{}",
                        learning::render_learning_show_text(&entry, *show_evidence, *show_proposed)
                    );
                    Ok(())
                }
                "json" => {
                    println!(
                        "{}",
                        learning::render_learning_show_json_preview(
                            &entry,
                            *show_evidence,
                            *show_proposed
                        )
                        .context("failed to render learn show JSON preview")?
                    );
                    Ok(())
                }
                other => Err(anyhow!(
                    "unsupported learn show format '{other}' (expected text|json)"
                )),
            }
        }
        LearnSubcommand::Archive { id } => {
            let out = learning::archive_learning_entry(&paths.state_dir, id)
                .with_context(|| format!("failed to archive learning entry {id}"))?;
            println!("{}", learning::render_archive_confirmation(&out));
            Ok(())
        }
        LearnSubcommand::Promote {
            id,
            to,
            slug,
            pack_id,
            force,
            check_run,
            replay_verify,
            replay_verify_run_id,
            replay_verify_strict,
        } => match to {
            LearnPromoteTargetArg::Check => {
                let slug = slug
                    .as_deref()
                    .ok_or_else(|| anyhow!("--slug is required for --to check"))?;
                validate_promote_chain_flags(
                    *to,
                    *check_run,
                    *replay_verify,
                    replay_verify_run_id,
                )?;
                let out = learning::promote_learning_to_check(&paths.state_dir, id, slug, *force)
                    .with_context(|| {
                    format!("failed to promote learning entry {id} to check")
                })?;
                println!("{}", learning::render_promote_to_check_confirmation(&out));
                if *check_run {
                    let check_out = crate::cli_dispatch_checks::run_check_command(
                        Some(out.target_path.clone()),
                        Some(1),
                        cli_run,
                        workdir,
                        paths,
                    )
                    .await
                    .context("chained check run failed")?;
                    crate::cli_dispatch_checks::write_check_run_outputs(&check_out, None, None)?;
                    if check_out.exit != crate::checks::runner::CheckRunExit::Ok {
                        std::process::exit(check_out.exit as i32);
                    }
                }
                if *replay_verify {
                    run_chained_replay_verify(
                        paths,
                        id,
                        replay_verify_run_id.as_deref(),
                        *replay_verify_strict,
                    )?;
                }
                Ok(())
            }
            LearnPromoteTargetArg::Pack => {
                validate_promote_chain_flags(
                    *to,
                    *check_run,
                    *replay_verify,
                    replay_verify_run_id,
                )?;
                let pack_id = pack_id
                    .as_deref()
                    .ok_or_else(|| anyhow!("--pack-id is required for --to pack"))?;
                let out = learning::promote_learning_to_pack(&paths.state_dir, id, pack_id, *force)
                    .with_context(|| format!("failed to promote learning entry {id} to pack"))?;
                println!("{}", learning::render_promote_to_target_confirmation(&out));
                if *replay_verify {
                    run_chained_replay_verify(
                        paths,
                        id,
                        replay_verify_run_id.as_deref(),
                        *replay_verify_strict,
                    )?;
                }
                Ok(())
            }
            LearnPromoteTargetArg::Agents => {
                validate_promote_chain_flags(
                    *to,
                    *check_run,
                    *replay_verify,
                    replay_verify_run_id,
                )?;
                let out = learning::promote_learning_to_agents(&paths.state_dir, id, *force)
                    .with_context(|| format!("failed to promote learning entry {id} to agents"))?;
                println!("{}", learning::render_promote_to_target_confirmation(&out));
                if *replay_verify {
                    run_chained_replay_verify(
                        paths,
                        id,
                        replay_verify_run_id.as_deref(),
                        *replay_verify_strict,
                    )?;
                }
                Ok(())
            }
        },
    }
}

fn validate_capture_assist_flags(assist: bool, write: bool) -> anyhow::Result<()> {
    if write && !assist {
        return Err(anyhow!(
            "{}: --write requires --assist",
            learning::LEARN_ASSIST_WRITE_REQUIRES_ASSIST
        ));
    }
    Ok(())
}

struct AssistedPreviewBuild {
    preview: learning::AssistedCapturePreview,
    output_truncated: bool,
}

async fn generate_assisted_capture_preview(
    cli_run: &RunArgs,
    input: &learning::CaptureLearningInput,
) -> anyhow::Result<AssistedPreviewBuild> {
    let provider_kind = cli_run.provider.ok_or_else(|| {
        anyhow!(
            "{}: --provider is required for assisted capture",
            learning::LEARN_ASSIST_PROVIDER_REQUIRED
        )
    })?;
    let model = cli_run.model.clone().ok_or_else(|| {
        anyhow!(
            "{}: --model is required for assisted capture",
            learning::LEARN_ASSIST_MODEL_REQUIRED
        )
    })?;
    let provider_name = crate::provider_runtime::provider_cli_name(provider_kind).to_string();
    let base_url = cli_run
        .base_url
        .clone()
        .unwrap_or_else(|| crate::provider_runtime::default_base_url(provider_kind).to_string());

    let canonical = learning::build_assist_capture_input_canonical(input);
    let canonical_json = serde_json::to_string_pretty(&canonical)?;
    let input_hash_hex = learning::compute_assist_input_hash_hex(&canonical)?;

    let raw = call_assist_model(cli_run, provider_kind, &base_url, &model, &canonical_json).await?;
    let raw_trimmed = raw.trim().to_string();
    let draft = learning::parse_assisted_capture_draft(&raw_trimmed);
    Ok(AssistedPreviewBuild {
        preview: learning::AssistedCapturePreview {
            provider: provider_name,
            model,
            prompt_version: learning::LEARN_ASSIST_PROMPT_VERSION_V1.to_string(),
            input_hash_hex,
            draft,
            raw_model_output: raw_trimmed,
        },
        output_truncated: false,
    })
}

async fn call_assist_model(
    cli_run: &RunArgs,
    provider_kind: crate::ProviderKind,
    base_url: &str,
    model: &str,
    canonical_json: &str,
) -> anyhow::Result<String> {
    let req = GenerateRequest {
        model: model.to_string(),
        messages: vec![
            Message {
                role: Role::System,
                content: Some("You draft a LocalAgent learning capture. Return a JSON object with optional keys: category, summary, guidance_text, check_text. Keep outputs concise.".to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
            Message {
                role: Role::User,
                content: Some(format!(
                    "Draft a learning capture from this canonical input JSON:\n{}",
                    canonical_json
                )),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
        ],
        tools: None,
    };

    let resp = match provider_kind {
        crate::ProviderKind::Lmstudio | crate::ProviderKind::Llamacpp => {
            let provider = crate::OpenAiCompatProvider::new(
                base_url.to_string(),
                cli_run.api_key.clone(),
                crate::provider_runtime::http_config_from_run_args(cli_run),
            )?;
            provider.generate(req).await?
        }
        crate::ProviderKind::Ollama => {
            let provider = crate::OllamaProvider::new(
                base_url.to_string(),
                crate::provider_runtime::http_config_from_run_args(cli_run),
            )?;
            provider.generate(req).await?
        }
        crate::ProviderKind::Mock => {
            let provider = crate::MockProvider::new();
            provider.generate(req).await?
        }
    };
    Ok(resp.assistant.content.unwrap_or_default())
}

fn run_chained_replay_verify(
    paths: &StatePaths,
    learning_id: &str,
    run_id_override: Option<&str>,
    strict: bool,
) -> anyhow::Result<()> {
    let source_run_id = if run_id_override.is_none() {
        let entry =
            learning::load_learning_entry(&paths.state_dir, learning_id).with_context(|| {
                format!("failed to load learning entry {learning_id} for chained replay verify")
            })?;
        entry.source.run_id
    } else {
        None
    };
    let run_id =
        resolve_replay_verify_run_id(learning_id, run_id_override, source_run_id.as_deref())?;

    let record = crate::store::load_run_record(&paths.state_dir, &run_id).map_err(|e| {
        anyhow!(
            "failed to load run '{}': {}. runs dir: {}",
            run_id,
            e,
            paths.runs_dir.display()
        )
    })?;
    let report = crate::repro::verify_run_record(&record, strict)?;
    print!("{}", crate::repro::render_verify_report(&report));
    if report.status == "fail" {
        std::process::exit(1);
    }
    Ok(())
}

fn validate_promote_chain_flags(
    target: LearnPromoteTargetArg,
    check_run: bool,
    replay_verify: bool,
    replay_verify_run_id: &Option<String>,
) -> anyhow::Result<()> {
    if check_run && !matches!(target, LearnPromoteTargetArg::Check) {
        return Err(anyhow!("--check-run is only valid with --to check"));
    }
    if replay_verify_run_id.is_some() && !replay_verify {
        return Err(anyhow!("--replay-verify-run-id requires --replay-verify"));
    }
    Ok(())
}

fn resolve_replay_verify_run_id(
    learning_id: &str,
    run_id_override: Option<&str>,
    source_run_id: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(run_id) = run_id_override {
        return Ok(run_id.to_string());
    }
    if let Some(run_id) = source_run_id {
        return Ok(run_id.to_string());
    }
    Err(anyhow!(
        "no source run_id on learning entry {}; pass --replay-verify-run-id",
        learning_id
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    use clap::Parser;
    use tempfile::tempdir;

    use super::*;
    use crate::cli_args::{Cli, Commands};

    #[test]
    fn validate_promote_chain_flags_rejects_check_run_for_non_check_targets() {
        let err = validate_promote_chain_flags(LearnPromoteTargetArg::Pack, true, false, &None)
            .expect_err("invalid");
        assert!(err
            .to_string()
            .contains("--check-run is only valid with --to check"));

        validate_promote_chain_flags(LearnPromoteTargetArg::Check, true, false, &None)
            .expect("check target allows --check-run");
    }

    #[test]
    fn validate_promote_chain_flags_requires_replay_verify_for_override_run_id() {
        let err = validate_promote_chain_flags(
            LearnPromoteTargetArg::Agents,
            false,
            false,
            &Some("run_123".to_string()),
        )
        .expect_err("invalid");
        assert!(err
            .to_string()
            .contains("--replay-verify-run-id requires --replay-verify"));
    }

    #[test]
    fn resolve_replay_verify_run_id_prefers_override_then_source() {
        let resolved = resolve_replay_verify_run_id("L1", Some("override_run"), Some("source_run"))
            .expect("override");
        assert_eq!(resolved, "override_run");

        let resolved =
            resolve_replay_verify_run_id("L1", None, Some("source_run")).expect("source");
        assert_eq!(resolved, "source_run");
    }

    #[test]
    fn resolve_replay_verify_run_id_errors_when_missing() {
        let err = resolve_replay_verify_run_id("L1", None, None).expect_err("missing");
        assert!(err
            .to_string()
            .contains("no source run_id on learning entry L1; pass --replay-verify-run-id"));
    }

    #[test]
    fn validate_capture_assist_flags_requires_assist_for_write() {
        let err = validate_capture_assist_flags(false, true).expect_err("invalid");
        assert!(err
            .to_string()
            .contains("LEARN_ASSIST_WRITE_REQUIRES_ASSIST"));
        validate_capture_assist_flags(false, false).expect("plain capture");
        validate_capture_assist_flags(true, false).expect("assist preview");
        validate_capture_assist_flags(true, true).expect("assist write");
    }

    fn parse_learn_cli(args: &[&str]) -> (LearnArgs, RunArgs) {
        let mut argv = vec!["localagent"];
        argv.extend_from_slice(args);
        let cli = Cli::parse_from(argv);
        let learn = match cli.command {
            Some(Commands::Learn(args)) => args,
            other => panic!("expected learn command, got {:?}", other.map(|_| "other")),
        };
        (learn, cli.run)
    }

    fn collect_state_files(state_dir: &Path) -> BTreeSet<String> {
        fn walk(dir: &Path, root: &Path, out: &mut BTreeSet<String>) {
            if let Ok(rd) = fs::read_dir(dir) {
                for ent in rd.flatten() {
                    let path = ent.path();
                    if path.is_dir() {
                        walk(&path, root, out);
                    } else if path.is_file() {
                        out.insert(
                            path.strip_prefix(root)
                                .unwrap_or(&path)
                                .to_string_lossy()
                                .replace('\\', "/"),
                        );
                    }
                }
            }
        }
        let mut out = BTreeSet::new();
        if state_dir.exists() {
            walk(state_dir, state_dir, &mut out);
        }
        out
    }

    #[tokio::test]
    async fn assist_preview_without_write_performs_zero_filesystem_writes() {
        let tmp = tempdir().expect("tempdir");
        let workdir = tmp.path();
        let paths = crate::store::resolve_state_paths(workdir, None, None, None, None);
        let before = collect_state_files(&paths.state_dir);
        let (learn_args, run_args) = parse_learn_cli(&[
            "--provider",
            "mock",
            "--model",
            "mock-model",
            "learn",
            "capture",
            "--assist",
            "--category",
            "prompt-guidance",
            "--summary",
            "Investigate repeated retries",
            "--evidence",
            "run_id:run_123",
        ]);

        handle_learn_command(&learn_args, &run_args, workdir, &paths)
            .await
            .expect("assist preview");

        let after = collect_state_files(&paths.state_dir);
        assert_eq!(before, after, "assist preview should not write state");
        assert!(after.is_empty());
    }

    #[tokio::test]
    async fn assist_write_persists_assist_metadata_and_write_failure_emits_no_event() {
        let tmp = tempdir().expect("tempdir");
        let workdir = tmp.path();
        let paths = crate::store::resolve_state_paths(workdir, None, None, None, None);

        let (learn_args, run_args) = parse_learn_cli(&[
            "--provider",
            "mock",
            "--model",
            "mock-model",
            "learn",
            "capture",
            "--assist",
            "--write",
            "--category",
            "prompt-guidance",
            "--summary",
            "Base summary",
            "--run",
            "run_123",
        ]);

        handle_learn_command(&learn_args, &run_args, workdir, &paths)
            .await
            .expect("assist write");
        let entries = learning::list_learning_entries(&paths.state_dir).expect("list");
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        let assist = e.assist.as_ref().expect("assist metadata");
        assert!(assist.enabled);
        assert_eq!(assist.provider, "mock");
        assert_eq!(assist.model, "mock-model");
        assert_eq!(
            assist.prompt_version,
            learning::LEARN_ASSIST_PROMPT_VERSION_V1
        );
        assert!(!assist.input_hash_hex.is_empty());
        let events_path = learning::learning_events_path(&paths.state_dir);
        assert!(
            events_path.exists(),
            "capture event should be emitted on success"
        );

        let tmp_fail = tempdir().expect("tempdir");
        let fail_workdir = tmp_fail.path();
        let fail_paths = crate::store::resolve_state_paths(fail_workdir, None, None, None, None);
        let entries_dir = learning::learning_entries_dir(&fail_paths.state_dir);
        fs::create_dir_all(entries_dir.parent().expect("learn parent")).expect("mk learn dir");
        fs::write(&entries_dir, "poison").expect("poison entries dir path");

        let (fail_learn_args, fail_run_args) = parse_learn_cli(&[
            "--provider",
            "mock",
            "--model",
            "mock-model",
            "learn",
            "capture",
            "--assist",
            "--write",
            "--category",
            "prompt-guidance",
            "--summary",
            "Will fail write",
        ]);
        let err = handle_learn_command(&fail_learn_args, &fail_run_args, fail_workdir, &fail_paths)
            .await
            .expect_err("write failure");
        assert!(err
            .to_string()
            .contains("failed to capture assisted learning entry"));
        assert!(
            !learning::learning_events_path(&fail_paths.state_dir).exists(),
            "no capture event should be emitted when entry write fails"
        );
    }
}
