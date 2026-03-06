use serde_json::Value;

use crate::target::{PatchReq, ReadReq, WriteReq};
use crate::types::SideEffects;

use super::{
    failed_exec, minimal_builtin_example, path_is_workdir_scoped, target_to_exec,
    ToolErrorCode, ToolErrorDetail, ToolExecution, ToolRuntime,
};

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
