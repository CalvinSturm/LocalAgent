use serde_json::Value;

use crate::target::{PatchReq, ReadReq, WriteReq};
use crate::types::SideEffects;

use super::exec_support::{failed_exec, path_is_workdir_scoped, target_to_exec, ToolExecution};
use super::{minimal_builtin_example, ToolErrorCode, ToolErrorDetail, ToolResultMeta, ToolRuntime};

pub(super) async fn run_write_file(rt: &ToolRuntime, args: &Value) -> ToolExecution {
    if !rt.allow_write && !rt.unsafe_bypass_allow_flags {
        return failed_exec(
            rt,
            SideEffects::FilesystemWrite,
            "writes require --allow-write".to_string(),
            Some(ToolErrorDetail {
                code: ToolErrorCode::ToolDisabled,
                message: "Write tools are disabled by runtime flags.".to_string(),
                expected_schema: None,
                received_args: Some(args.clone()),
                minimal_example: minimal_builtin_example("write_file"),
                available_tools: None,
            }),
        );
    }
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if !path_is_workdir_scoped(path) && !rt.unsafe_bypass_allow_flags {
        return failed_exec(
            rt,
            SideEffects::FilesystemWrite,
            "path must stay within workdir (no absolute paths or '..' traversal)".to_string(),
            Some(ToolErrorDetail {
                code: ToolErrorCode::ToolPathDenied,
                message: "Path must stay within workdir.".to_string(),
                expected_schema: None,
                received_args: Some(args.clone()),
                minimal_example: minimal_builtin_example("write_file"),
                available_tools: None,
            }),
        );
    }
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let create_parents = args
        .get("create_parents")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let overwrite_existing = args
        .get("overwrite_existing")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !overwrite_existing {
        let exists_probe = rt
            .exec_target
            .read_file(ReadReq {
                workdir: rt.workdir.clone(),
                path: path.to_string(),
                max_read_bytes: 1,
            })
            .await;
        if exists_probe.ok {
            return failed_exec(
                rt,
                SideEffects::FilesystemWrite,
                "write_file blocked for existing file; use apply_patch for in-place edits or set overwrite_existing=true for explicit full rewrite".to_string(),
                None,
            );
        }
    }
    let out = rt
        .exec_target
        .write_file(WriteReq {
            workdir: rt.workdir.clone(),
            path: path.to_string(),
            content: content.to_string(),
            create_parents,
        })
        .await;
    target_to_exec(SideEffects::FilesystemWrite, out)
}

pub(super) async fn run_apply_patch(rt: &ToolRuntime, args: &Value) -> ToolExecution {
    if !rt.allow_write && !rt.unsafe_bypass_allow_flags {
        return failed_exec(
            rt,
            SideEffects::FilesystemWrite,
            "writes require --allow-write".to_string(),
            Some(ToolErrorDetail {
                code: ToolErrorCode::ToolDisabled,
                message: "Write tools are disabled by runtime flags.".to_string(),
                expected_schema: None,
                received_args: Some(args.clone()),
                minimal_example: minimal_builtin_example("apply_patch"),
                available_tools: None,
            }),
        );
    }
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if !path_is_workdir_scoped(path) && !rt.unsafe_bypass_allow_flags {
        return failed_exec(
            rt,
            SideEffects::FilesystemWrite,
            "path must stay within workdir (no absolute paths or '..' traversal)".to_string(),
            Some(ToolErrorDetail {
                code: ToolErrorCode::ToolPathDenied,
                message: "Path must stay within workdir.".to_string(),
                expected_schema: None,
                received_args: Some(args.clone()),
                minimal_example: minimal_builtin_example("apply_patch"),
                available_tools: None,
            }),
        );
    }
    let patch_text = args
        .get("patch")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let out = rt
        .exec_target
        .apply_patch(PatchReq {
            workdir: rt.workdir.clone(),
            path: path.to_string(),
            patch: patch_text.to_string(),
        })
        .await;
    target_to_exec(SideEffects::FilesystemWrite, out)
}

pub(super) async fn run_str_replace(rt: &ToolRuntime, args: &Value) -> ToolExecution {
    if !rt.allow_write && !rt.unsafe_bypass_allow_flags {
        return failed_exec(
            rt,
            SideEffects::FilesystemWrite,
            "writes require --allow-write".to_string(),
            Some(ToolErrorDetail {
                code: ToolErrorCode::ToolDisabled,
                message: "Write tools are disabled by runtime flags.".to_string(),
                expected_schema: None,
                received_args: Some(args.clone()),
                minimal_example: minimal_builtin_example("str_replace"),
                available_tools: None,
            }),
        );
    }
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if !path_is_workdir_scoped(path) && !rt.unsafe_bypass_allow_flags {
        return failed_exec(
            rt,
            SideEffects::FilesystemWrite,
            "path must stay within workdir (no absolute paths or '..' traversal)".to_string(),
            Some(ToolErrorDetail {
                code: ToolErrorCode::ToolPathDenied,
                message: "Path must stay within workdir.".to_string(),
                expected_schema: None,
                received_args: Some(args.clone()),
                minimal_example: minimal_builtin_example("str_replace"),
                available_tools: None,
            }),
        );
    }
    let old_string = args
        .get("old_string")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let new_string = args
        .get("new_string")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let read_out = rt
        .exec_target
        .read_file(ReadReq {
            workdir: rt.workdir.clone(),
            path: path.to_string(),
            max_read_bytes: 10 * 1024 * 1024,
        })
        .await;
    if !read_out.ok {
        return failed_exec(
            rt,
            SideEffects::FilesystemWrite,
            format!("str_replace: could not read file '{path}': {}", read_out.content),
            None,
        );
    }
    let original = &read_out.content;
    let matches: Vec<_> = original.match_indices(old_string).collect();
    if matches.is_empty() {
        return failed_exec(
            rt,
            SideEffects::FilesystemWrite,
            format!(
                "str_replace: old_string not found in '{path}'. Make sure the string matches exactly, including whitespace and indentation."
            ),
            None,
        );
    }
    if matches.len() > 1 {
        return failed_exec(
            rt,
            SideEffects::FilesystemWrite,
            format!(
                "str_replace: old_string matches {} locations in '{path}'. Include more surrounding context to make it unique.",
                matches.len()
            ),
            None,
        );
    }
    let replaced = original.replacen(old_string, new_string, 1);
    let changed = replaced != *original;
    let write_out = rt
        .exec_target
        .write_file(WriteReq {
            workdir: rt.workdir.clone(),
            path: path.to_string(),
            content: replaced.clone(),
            create_parents: false,
        })
        .await;
    if !write_out.ok {
        return failed_exec(
            rt,
            SideEffects::FilesystemWrite,
            format!("str_replace: could not write file '{path}': {}", write_out.content),
            None,
        );
    }
    let content =
        serde_json::json!({"path": path, "changed": changed, "bytes_written": replaced.len()})
            .to_string();
    ToolExecution {
        ok: true,
        content,
        truncated: false,
        error: None,
        meta: ToolResultMeta {
            side_effects: SideEffects::FilesystemWrite,
            bytes: Some(replaced.len() as u64),
            exit_code: None,
            stderr_truncated: None,
            stdout_truncated: None,
            source: "builtin".to_string(),
            execution_target: match rt.exec_target_kind {
                crate::target::ExecTargetKind::Host => "host".to_string(),
                crate::target::ExecTargetKind::Docker => "docker".to_string(),
            },
            warnings: None,
            warnings_max: None,
            warnings_truncated: None,
            docker: None,
        },
    }
}
