use serde_json::Value;

use crate::target::{ExecTargetKind, ShellReq, TargetResult};
use crate::types::SideEffects;

use super::exec_support::{failed_exec, ToolExecution};
use super::{minimal_builtin_example, ToolErrorCode, ToolErrorDetail, ToolRuntime};

/// Default wall-clock timeout (ms) applied to a shell command on the host target
/// when the caller omits `timeout_ms`. This keeps unattended/autonomous runs
/// from hanging forever on a runaway command. Adjust here to change the policy
/// globally; callers can always override per-call, and `timeout_ms: 0` opts out
/// entirely (unbounded).
pub(super) const DEFAULT_SHELL_TIMEOUT_MS: u64 = 120_000;

/// Resolve the effective shell timeout from the request args and execution
/// target.
///
/// - Explicit `timeout_ms` (including `0` = unbounded opt-out): honored as-is.
/// - Missing `timeout_ms`: the host target applies [`DEFAULT_SHELL_TIMEOUT_MS`].
///   The docker target instead resolves to `0` (unbounded) because it cannot
///   enforce timeouts (`DockerTarget::exec_shell` rejects `timeout_ms > 0`), so a
///   *missing* timeout must not silently turn into a rejection.
pub(super) fn resolve_shell_timeout_ms(args: &Value, target: ExecTargetKind) -> u64 {
    match args.get("timeout_ms").and_then(|v| v.as_u64()) {
        Some(explicit) => explicit,
        None => match target {
            ExecTargetKind::Host => DEFAULT_SHELL_TIMEOUT_MS,
            ExecTargetKind::Docker => 0,
        },
    }
}

pub(super) async fn run_shell(rt: &ToolRuntime, args: &Value) -> ToolExecution {
    let shell_allowed =
        rt.allow_shell || (rt.allow_shell_in_workdir_only && shell_cwd_is_workdir_scoped(args));
    if !shell_allowed && !rt.unsafe_bypass_allow_flags {
        return failed_exec(
            rt,
            SideEffects::ShellExec,
            "shell tool is disabled. Re-run with --allow-shell or --allow-shell-in-workdir"
                .to_string(),
            Some(ToolErrorDetail {
                code: ToolErrorCode::ShellGateDeny,
                message: "Shell tool is disabled by runtime flags.".to_string(),
                expected_schema: None,
                received_args: Some(args.clone()),
                minimal_example: None,
                available_tools: None,
            }),
        );
    }
    let cmd = args.get("cmd").and_then(|v| v.as_str()).unwrap_or_default();
    let arg_list = args
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let cwd = args
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    // `timeout_ms` is optional. When omitted, the host target applies a safe
    // default so unattended runs cannot hang forever; an explicit `0` opts out
    // (unbounded). The `as_u64` parse makes negative values unrepresentable.
    // Target-aware so a missing timeout never turns into a docker rejection.
    let timeout_ms = resolve_shell_timeout_ms(args, rt.exec_target_kind);
    let req = ShellReq {
        workdir: rt.workdir.clone(),
        cmd: cmd.to_string(),
        args: arg_list.clone(),
        cwd: cwd.clone(),
        max_tool_output_bytes: rt.max_tool_output_bytes,
        timeout_ms,
    };
    let mut out = rt.exec_target.exec_shell(req).await;
    if !out.ok && shell_spawn_not_found(&out.content) {
        if let Some((repair_cmd, repair_args, repair_strategy)) =
            repair_shell_invocation(rt, cmd, &arg_list)
        {
            let repaired = rt
                .exec_target
                .exec_shell(ShellReq {
                    workdir: rt.workdir.clone(),
                    cmd: repair_cmd,
                    args: repair_args,
                    cwd,
                    max_tool_output_bytes: rt.max_tool_output_bytes,
                    timeout_ms,
                })
                .await;
            out = annotate_shell_repair(repaired, repair_strategy);
        }
    }
    super::exec_support::target_to_exec(SideEffects::ShellExec, out)
}

pub(super) fn classify_shell_target_error(
    content: &str,
    exit_code: Option<i32>,
) -> ToolErrorDetail {
    if let Some(detail) = timeout_unsupported_error_from_content(content) {
        return detail;
    }
    if let Some(detail) = timeout_error_from_content(content) {
        return detail;
    }
    let lower = content.to_ascii_lowercase();
    let spawn_failed = lower.contains("shell execution failed:");
    let not_found = lower.contains("program not found")
        || lower.contains("no such file or directory")
        || lower.contains("cannot find the path specified")
        || lower.contains("cannot find the file specified")
        || lower.contains("(os error 2)")
        || lower.contains("(os error 3)");
    let (code, message) = if spawn_failed && not_found {
        (
            ToolErrorCode::ShellExecNotFound,
            "Shell command executable was not found on the execution target.".to_string(),
        )
    } else if spawn_failed {
        (
            ToolErrorCode::ShellExecOsError,
            "Shell execution failed before process start (OS/runtime error).".to_string(),
        )
    } else {
        let status = shell_status_code_from_content(content).or(exit_code);
        (
            ToolErrorCode::ShellExecNonZeroExit,
            match status {
                Some(code) => format!("Shell command exited with non-zero status: {code}."),
                None => "Shell command exited with non-zero status.".to_string(),
            },
        )
    };
    ToolErrorDetail {
        code,
        message,
        expected_schema: None,
        received_args: None,
        minimal_example: minimal_builtin_example("shell"),
        available_tools: None,
    }
}

fn timeout_unsupported_error_from_content(content: &str) -> Option<ToolErrorDetail> {
    let parsed = serde_json::from_str::<Value>(content).ok()?;
    if parsed.get("error").and_then(|v| v.as_str()) != Some("timeout_unsupported") {
        return None;
    }
    let target = parsed
        .get("execution_target")
        .and_then(|v| v.as_str())
        .unwrap_or("this");
    Some(ToolErrorDetail {
        code: ToolErrorCode::ShellExecTimeoutUnsupported,
        message: format!(
            "timeout_ms is not supported on the {target} execution target. Re-run on the host target or omit timeout_ms."
        ),
        expected_schema: None,
        received_args: None,
        minimal_example: minimal_builtin_example("shell"),
        available_tools: None,
    })
}

fn timeout_error_from_content(content: &str) -> Option<ToolErrorDetail> {
    let parsed = serde_json::from_str::<Value>(content).ok()?;
    if parsed.get("timed_out").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    let message = match parsed.get("timeout_ms").and_then(|v| v.as_u64()) {
        Some(ms) => format!("Shell command exceeded the {ms} ms timeout and was terminated."),
        None => "Shell command exceeded its timeout and was terminated.".to_string(),
    };
    Some(ToolErrorDetail {
        code: ToolErrorCode::ShellExecTimeout,
        message,
        expected_schema: None,
        received_args: None,
        minimal_example: minimal_builtin_example("shell"),
        available_tools: None,
    })
}

fn shell_status_code_from_content(content: &str) -> Option<i32> {
    let parsed = serde_json::from_str::<Value>(content).ok()?;
    parsed
        .get("status")
        .and_then(|v| v.as_i64())
        .map(|n| n as i32)
}

fn shell_spawn_not_found(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("shell execution failed:")
        && (lower.contains("program not found")
            || lower.contains("no such file or directory")
            || lower.contains("cannot find the path specified")
            || lower.contains("cannot find the file specified")
            || lower.contains("(os error 2)")
            || lower.contains("(os error 3)")
            || lower.contains("not recognized as an internal or external command"))
}

fn is_windows_exec_target(rt: &ToolRuntime) -> bool {
    match rt.exec_target_kind {
        ExecTargetKind::Docker => false,
        ExecTargetKind::Host => cfg!(windows),
    }
}

fn repair_shell_invocation(
    rt: &ToolRuntime,
    cmd: &str,
    args: &[String],
) -> Option<(String, Vec<String>, &'static str)> {
    let cmd_trimmed = cmd.trim();
    if cmd_trimmed.is_empty() {
        return None;
    }
    let split_tokens = cmd_trimmed
        .split_whitespace()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let has_embedded_whitespace = split_tokens.len() > 1;
    let windows_target = is_windows_exec_target(rt);
    let mut logical_tokens = if has_embedded_whitespace && args.is_empty() {
        split_tokens.clone()
    } else {
        let mut v = Vec::with_capacity(args.len() + 1);
        v.push(cmd_trimmed.to_string());
        v.extend_from_slice(args);
        v
    };
    if windows_target {
        let first = logical_tokens
            .first()
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if is_windows_shell_builtin(&first) {
            let mut wrapped_args = Vec::with_capacity(logical_tokens.len() + 1);
            wrapped_args.push("/c".to_string());
            wrapped_args.append(&mut logical_tokens);
            return Some(("cmd".to_string(), wrapped_args, "windows_cmd_c"));
        }
    }
    if has_embedded_whitespace {
        if !windows_target {
            return Some((
                "sh".to_string(),
                vec!["-lc".to_string(), cmd_trimmed.to_string()],
                "unix_sh_lc",
            ));
        }
        return Some((
            split_tokens[0].clone(),
            split_tokens[1..].to_vec(),
            "split_cmd",
        ));
    }
    None
}

fn is_windows_shell_builtin(cmd: &str) -> bool {
    matches!(
        cmd,
        "assoc"
            | "break"
            | "call"
            | "cd"
            | "chdir"
            | "cls"
            | "color"
            | "copy"
            | "date"
            | "del"
            | "dir"
            | "echo"
            | "endlocal"
            | "erase"
            | "exit"
            | "for"
            | "ftype"
            | "goto"
            | "if"
            | "md"
            | "mkdir"
            | "mklink"
            | "move"
            | "path"
            | "pause"
            | "popd"
            | "prompt"
            | "pushd"
            | "rd"
            | "rem"
            | "ren"
            | "rename"
            | "rmdir"
            | "set"
            | "setlocal"
            | "shift"
            | "start"
            | "time"
            | "title"
            | "type"
            | "ver"
            | "verify"
            | "vol"
    )
}

fn annotate_shell_repair(mut out: TargetResult, strategy: &str) -> TargetResult {
    if let Ok(mut v) = serde_json::from_str::<Value>(&out.content) {
        if let Some(obj) = v.as_object_mut() {
            obj.insert("repair_attempted".to_string(), Value::Bool(true));
            obj.insert(
                "repair_strategy".to_string(),
                Value::String(strategy.to_string()),
            );
            out.content = v.to_string();
        }
    }
    out
}

fn shell_cwd_is_workdir_scoped(args: &Value) -> bool {
    let Some(cwd) = args.get("cwd") else {
        return true;
    };
    let Some(cwd_str) = cwd.as_str() else {
        return false;
    };
    let path = std::path::Path::new(cwd_str);
    if path.is_absolute() {
        return false;
    }
    !path.components().any(|c| {
        matches!(
            c,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    })
}

#[cfg(test)]
mod timeout_policy_tests {
    use super::{resolve_shell_timeout_ms, DEFAULT_SHELL_TIMEOUT_MS};
    use crate::target::ExecTargetKind;
    use serde_json::json;

    #[test]
    fn missing_timeout_uses_host_default() {
        let args = json!({ "cmd": "echo", "args": ["hi"] });
        assert_eq!(
            resolve_shell_timeout_ms(&args, ExecTargetKind::Host),
            DEFAULT_SHELL_TIMEOUT_MS
        );
        assert_eq!(DEFAULT_SHELL_TIMEOUT_MS, 120_000);
    }

    #[test]
    fn explicit_positive_timeout_overrides_default() {
        let args = json!({ "cmd": "echo", "timeout_ms": 500 });
        assert_eq!(resolve_shell_timeout_ms(&args, ExecTargetKind::Host), 500);
        // Explicit values are honored on docker too (DockerTarget rejects > 0).
        assert_eq!(resolve_shell_timeout_ms(&args, ExecTargetKind::Docker), 500);
    }

    #[test]
    fn explicit_zero_timeout_stays_unbounded() {
        let args = json!({ "cmd": "echo", "timeout_ms": 0 });
        assert_eq!(resolve_shell_timeout_ms(&args, ExecTargetKind::Host), 0);
        assert_eq!(resolve_shell_timeout_ms(&args, ExecTargetKind::Docker), 0);
    }

    #[test]
    fn missing_timeout_on_docker_stays_unbounded_not_rejected() {
        // Critical: a missing timeout must not become a docker timeout
        // rejection. Docker resolves missing -> 0 (unbounded), unlike host.
        let args = json!({ "cmd": "echo" });
        assert_eq!(resolve_shell_timeout_ms(&args, ExecTargetKind::Docker), 0);
    }
}
