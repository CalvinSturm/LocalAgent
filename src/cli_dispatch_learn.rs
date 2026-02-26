use anyhow::anyhow;

use crate::cli_args::LearnArgs;

pub(crate) async fn handle_learn_command(_args: &LearnArgs) -> anyhow::Result<()> {
    Err(anyhow!(
        "learn commands are not implemented yet (PR1 in progress: capture/list/show)"
    ))
}
