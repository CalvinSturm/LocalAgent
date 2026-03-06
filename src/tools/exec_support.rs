use std::path::Path;

use crate::target::{ExecTargetKind, TargetResult};
use crate::types::SideEffects;

use super::{ToolErrorDetail, ToolResultMeta, ToolRuntime};

#[derive(Debug, Clone)]
pub(super) struct ToolExecution {
    pub(super) ok: bool,
    pub(super) content: String,
    pub(super) truncated: bool,
    pub(super) error: Option<ToolErrorDetail>,
    pub(super) meta: ToolResultMeta,
}

pub(super) fn has_git_segment(path: &Path) -> bool {
    path.components().any(|c| match c {
        std::path::Component::Normal(s) => s == ".git",
        _ => false,
    })
}

pub(super) fn path_is_workdir_scoped(path: &str) -> bool {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return false;
    }
    !p.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    })
}

pub(super) fn target_to_exec(side_effects: SideEffects, out: TargetResult) -> ToolExecution {
    let shell_error = if matches!(side_effects, SideEffects::ShellExec) && !out.ok {
        Some(super::exec_shell::classify_shell_target_error(
            &out.content,
            out.exit_code,
        ))
    } else {
        None
    };
    ToolExecution {
        ok: out.ok,
        content: out.content,
        truncated: out.truncated,
        error: shell_error,
        meta: ToolResultMeta {
            side_effects,
            bytes: out.bytes,
            exit_code: out.exit_code,
            stderr_truncated: out.stderr_truncated,
            stdout_truncated: out.stdout_truncated,
            source: "builtin".to_string(),
            execution_target: match out.execution_target {
                ExecTargetKind::Host => "host".to_string(),
                ExecTargetKind::Docker => "docker".to_string(),
            },
            warnings: None,
            warnings_max: None,
            warnings_truncated: None,
            docker: out.docker,
        },
    }
}

pub(super) fn base_meta(rt: &ToolRuntime, side_effects: SideEffects) -> ToolResultMeta {
    ToolResultMeta {
        side_effects,
        bytes: None,
        exit_code: None,
        stderr_truncated: None,
        stdout_truncated: None,
        source: "builtin".to_string(),
        execution_target: match rt.exec_target_kind {
            ExecTargetKind::Host => "host".to_string(),
            ExecTargetKind::Docker => "docker".to_string(),
        },
        warnings: None,
        warnings_max: None,
        warnings_truncated: None,
        docker: None,
    }
}

pub(super) fn failed_exec(
    rt: &ToolRuntime,
    side_effects: SideEffects,
    content: String,
    error: Option<ToolErrorDetail>,
) -> ToolExecution {
    ToolExecution {
        ok: false,
        content,
        truncated: false,
        error,
        meta: base_meta(rt, side_effects),
    }
}
