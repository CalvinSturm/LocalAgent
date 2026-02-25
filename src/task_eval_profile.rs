use crate::eval::profile::load_profile;
use crate::eval::tasks::EvalPack;
use crate::gate::{ApprovalMode, AutoApproveScope, ProviderKind, TrustMode};
use crate::session::CapsMode;
use crate::EvalArgs;

fn cli_has_flag(flag: &str) -> bool {
    std::env::args().any(|a| a == flag || a.starts_with(&format!("{flag}=")))
}

pub(crate) fn apply_eval_profile_overrides(
    args: &mut EvalArgs,
    state_dir: &std::path::Path,
) -> anyhow::Result<Option<crate::eval::profile::LoadedProfile>> {
    let loaded = if args.profile.is_some() || args.profile_path.is_some() {
        Some(load_profile(
            state_dir,
            args.profile.as_deref(),
            args.profile_path.as_deref(),
        )?)
    } else {
        None
    };
    let Some(loaded) = loaded else {
        return Ok(None);
    };
    let p = &loaded.profile;

    if !cli_has_flag("--provider") {
        if let Some(v) = &p.provider {
            args.provider = match v.as_str() {
                "lmstudio" => ProviderKind::Lmstudio,
                "llamacpp" => ProviderKind::Llamacpp,
                "mock" => ProviderKind::Mock,
                _ => ProviderKind::Ollama,
            };
        }
    }
    if !cli_has_flag("--base-url") {
        if let Some(v) = &p.base_url {
            args.base_url = Some(v.clone());
        }
    }
    if !cli_has_flag("--models") {
        if let Some(v) = &p.models {
            args.models = Some(v.join(","));
        }
    }
    if !cli_has_flag("--pack") {
        if let Some(v) = &p.pack {
            args.pack = match v.as_str() {
                "coding" => EvalPack::Coding,
                "browser" => EvalPack::Browser,
                _ => EvalPack::All,
            };
        }
    }
    if !cli_has_flag("--runs-per-task") {
        if let Some(v) = p.runs_per_task {
            args.runs_per_task = v;
        }
    }
    if !cli_has_flag("--caps") {
        if let Some(v) = &p.caps {
            args.caps = match v.as_str() {
                "off" => CapsMode::Off,
                "strict" => CapsMode::Strict,
                _ => CapsMode::Auto,
            };
        }
    }
    if !cli_has_flag("--trust") {
        if let Some(v) = &p.trust {
            args.trust = match v.as_str() {
                "off" => TrustMode::Off,
                "auto" => TrustMode::Auto,
                _ => TrustMode::On,
            };
        }
    }
    if !cli_has_flag("--approval-mode") {
        if let Some(v) = &p.approval_mode {
            args.approval_mode = match v.as_str() {
                "interrupt" => ApprovalMode::Interrupt,
                "fail" => ApprovalMode::Fail,
                _ => ApprovalMode::Auto,
            };
        }
    }
    if !cli_has_flag("--auto-approve-scope") {
        if let Some(v) = &p.auto_approve_scope {
            args.auto_approve_scope = match v.as_str() {
                "session" => AutoApproveScope::Session,
                _ => AutoApproveScope::Run,
            };
        }
    }
    if !cli_has_flag("--mcp") {
        if let Some(v) = &p.mcp {
            args.mcp = v.clone();
        }
    }
    if let Some(flags) = &p.flags {
        if !cli_has_flag("--enable-write-tools") {
            if let Some(v) = flags.enable_write_tools {
                args.enable_write_tools = v;
            }
        }
        if !cli_has_flag("--allow-write") {
            if let Some(v) = flags.allow_write {
                args.allow_write = v;
            }
        }
        if !cli_has_flag("--allow-shell") {
            if let Some(v) = flags.allow_shell {
                args.allow_shell = v;
            }
        }
    }
    if let Some(th) = &p.thresholds {
        if !cli_has_flag("--min-pass-rate") {
            if let Some(v) = th.min_pass_rate {
                args.min_pass_rate = v;
            }
        }
        if !cli_has_flag("--fail-on-any") {
            if let Some(v) = th.fail_on_any {
                args.fail_on_any = v;
            }
        }
        if !cli_has_flag("--max-avg-steps") {
            if let Some(v) = th.max_avg_steps {
                args.max_avg_steps = Some(v);
            }
        }
    }
    Ok(Some(loaded))
}
