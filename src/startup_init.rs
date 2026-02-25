use std::path::PathBuf;

use crate::scaffold::{self, InitOptions};
use crate::store;
use crate::Commands;

pub(crate) fn should_auto_init_state(command: &Option<Commands>) -> bool {
    !matches!(
        command,
        Some(Commands::Version(_)) | Some(Commands::Init(_)) | Some(Commands::Template(_))
    )
}

pub(crate) fn maybe_auto_init_state(
    command: &Option<Commands>,
    state_dir_override: Option<PathBuf>,
    workdir: &std::path::Path,
    paths: &store::StatePaths,
) -> anyhow::Result<()> {
    if !should_auto_init_state(command) || paths.state_dir.exists() {
        return Ok(());
    }
    let _ = scaffold::run_init(&InitOptions {
        workdir: workdir.to_path_buf(),
        state_dir_override,
        force: false,
        print_only: false,
    })?;
    eprintln!(
        "INFO: initialized LocalAgent state at {}",
        paths.state_dir.display()
    );
    Ok(())
}
