use anyhow::{anyhow, Context};

use crate::cli_args::{
    LearnArgs, LearnCategoryArg, LearnPromoteTargetArg, LearnStatusArg, LearnSubcommand, RunArgs,
};
use crate::learning;
use crate::store::StatePaths;

pub(crate) async fn handle_learn_command(
    args: &LearnArgs,
    cli_run: &RunArgs,
    workdir: &std::path::Path,
    paths: &StatePaths,
) -> anyhow::Result<()> {
    match &args.command {
        LearnSubcommand::Capture {
            run,
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
    use super::*;

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
}
