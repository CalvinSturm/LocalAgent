use std::path::PathBuf;

use crate::instructions;
use crate::instructions::InstructionResolution;
use crate::RunArgs;

fn resolved_instructions_config_path(args: &RunArgs, state_dir: &std::path::Path) -> PathBuf {
    args.instructions_config
        .clone()
        .unwrap_or_else(|| instructions::default_config_path(state_dir))
}

pub(crate) fn resolve_instruction_messages(
    args: &RunArgs,
    state_dir: &std::path::Path,
    model: &str,
) -> anyhow::Result<InstructionResolution> {
    let cfg_path = resolved_instructions_config_path(args, state_dir);
    if !cfg_path.exists() {
        return Ok(InstructionResolution::empty());
    }
    let (cfg, hash_hex) = instructions::load_config(&cfg_path)?;
    let (messages, selected_model, selected_task) = instructions::resolve_messages(
        &cfg,
        model,
        args.task_kind.as_deref(),
        args.instruction_model_profile.as_deref(),
        args.instruction_task_profile.as_deref(),
    )?;
    Ok(InstructionResolution {
        config_path: Some(cfg_path),
        config_hash_hex: Some(hash_hex),
        selected_model_profile: selected_model,
        selected_task_profile: selected_task,
        messages,
    })
}
