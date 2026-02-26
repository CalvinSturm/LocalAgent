use crate::cli_args::*;
use crate::{provider_runtime, repo_map, store};

pub(crate) async fn handle_doctor_command(
    args: &DoctorArgs,
    cli_run: &RunArgs,
    workdir: &std::path::Path,
) -> anyhow::Result<()> {
    if args.docker {
        match crate::target::DockerTarget::validate_available().and_then(|_| {
            crate::target::DockerTarget::validate_image_present_local(&cli_run.docker_image)
        }) {
            Ok(()) => {
                println!(
                    "OK: docker daemon reachable; image={} present locally; network={} ; workdir_mount={}",
                    cli_run.docker_image,
                    format!("{:?}", cli_run.docker_network).to_lowercase(),
                    workdir.display()
                );
                return Ok(());
            }
            Err(e) => {
                println!("FAIL: {e}");
                std::process::exit(1);
            }
        }
    }

    match provider_runtime::doctor_check(args).await {
        Ok(ok_msg) => {
            println!("{ok_msg}");
            Ok(())
        }
        Err(fail_reason) => {
            println!("FAIL: {fail_reason}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn handle_repo_command(
    args: &RepoArgs,
    workdir: &std::path::Path,
    paths: &store::StatePaths,
) -> anyhow::Result<()> {
    match &args.command {
        RepoSubcommand::Map {
            print_content,
            no_write,
            max_files,
            max_scan_bytes,
            max_out_bytes,
        } => {
            let map = repo_map::resolve_repo_map(
                workdir,
                repo_map::RepoMapLimits {
                    max_files: *max_files,
                    max_scan_bytes: *max_scan_bytes,
                    max_out_bytes: *max_out_bytes,
                    ..repo_map::RepoMapLimits::default()
                },
            )?;

            let cache_path = if *no_write {
                None
            } else {
                Some(repo_map::write_repo_map_cache(&paths.state_dir, &map)?)
            };

            print!(
                "{}",
                repo_map::render_repo_map_summary_text(&map, cache_path.as_deref())
            );
            if *print_content {
                println!("repo_map:");
                print!("{}", map.content);
            }
            Ok(())
        }
    }
}

pub(crate) fn handle_profile_command(args: &ProfileArgs) -> anyhow::Result<()> {
    match &args.command {
        ProfileSubcommand::List => {
            for p in crate::reliability_profile::list_builtin_profiles_sorted() {
                println!("{}\t{}", p.name, p.description);
            }
        }
        ProfileSubcommand::Show { name } => {
            println!("{}", crate::reliability_profile::render_profile_show(name)?);
        }
    }
    Ok(())
}

pub(crate) fn handle_pack_command(
    args: &PackArgs,
    workdir: &std::path::Path,
) -> anyhow::Result<()> {
    match &args.command {
        PackSubcommand::List => {
            let packs = crate::packs::discover_packs(workdir, crate::packs::PackLimits::default())?;
            println!("{}", crate::packs::render_pack_list_text(&packs));
        }
        PackSubcommand::Show { pack_id } => {
            println!(
                "{}",
                crate::packs::render_pack_show_text(
                    workdir,
                    pack_id,
                    crate::packs::PackLimits::default()
                )?
            );
        }
    }
    Ok(())
}
