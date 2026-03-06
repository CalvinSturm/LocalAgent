use std::path::PathBuf;
use std::sync::Arc;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::target::{
    DockerMeta, ExecTarget, ExecTargetKind,
};
use crate::types::{Message, SideEffects, ToolCall};

mod catalog;
mod envelope;
mod exec_fs;
mod exec_shell;
mod exec_support;
mod exec_write;
mod schema;

pub use catalog::{builtin_tools_enabled, tool_side_effects};
pub use envelope::{
    envelope_to_message, invalid_args_tool_message, to_tool_result_envelope,
    to_tool_result_envelope_with_error,
};
pub use schema::{
    compact_builtin_schema, invalid_args_detail, minimal_builtin_example,
    sorted_builtin_tool_names, validate_builtin_tool_args, validate_schema_args,
};
use exec_support::ToolExecution;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ToolArgsStrict {
    On,
    Off,
}

impl ToolArgsStrict {
    pub fn is_enabled(self) -> bool {
        matches!(self, Self::On)
    }
}

#[derive(Clone)]
pub struct ToolRuntime {
    pub workdir: PathBuf,
    pub allow_shell: bool,
    pub allow_shell_in_workdir_only: bool,
    pub allow_write: bool,
    pub max_tool_output_bytes: usize,
    pub max_read_bytes: usize,
    pub unsafe_bypass_allow_flags: bool,
    pub tool_args_strict: ToolArgsStrict,
    pub exec_target_kind: ExecTargetKind,
    pub exec_target: Arc<dyn ExecTarget>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolResultMeta {
    pub side_effects: SideEffects,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout_truncated: Option<bool>,
    pub source: String,
    pub execution_target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings: Option<Vec<ToolWarningDetail>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings_max: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warnings_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docker: Option<DockerMeta>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolWarningDetail {
    pub code: String,
    pub path: String,
    pub target: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolResultContentRef {
    pub kind: String,
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolResultEnvelope {
    pub schema_version: String,
    pub tool_name: String,
    pub tool_call_id: String,
    pub ok: bool,
    pub content: String,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncate_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_output_ref: Option<ToolResultContentRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ToolErrorDetail>,
    pub meta: ToolResultMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum ToolErrorCode {
    ToolArgsInvalid,
    ToolUnknown,
    ToolPathDenied,
    PathOutOfScope,
    ToolDisabled,
    ToolArgsMalformedJson,
    InvalidPattern,
    IoError,
    ShellGateDeny,
    ShellToolUnavailable,
    ShellExecNotFound,
    ShellExecOsError,
    ShellExecNonZeroExit,
}

impl ToolErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ToolArgsInvalid => "tool_args_invalid",
            Self::ToolUnknown => "tool_unknown",
            Self::ToolPathDenied => "tool_path_denied",
            Self::PathOutOfScope => "path_out_of_scope",
            Self::ToolDisabled => "tool_disabled",
            Self::ToolArgsMalformedJson => "tool_args_malformed_json",
            Self::InvalidPattern => "invalid_pattern",
            Self::IoError => "io_error",
            Self::ShellGateDeny => "shell_gate_deny",
            Self::ShellToolUnavailable => "shell_tool_unavailable",
            Self::ShellExecNotFound => "shell_exec_not_found",
            Self::ShellExecOsError => "shell_exec_os_error",
            Self::ShellExecNonZeroExit => "shell_exec_non_zero_exit",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolErrorDetail {
    pub code: ToolErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub received_args: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimal_example: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_tools: Option<Vec<String>>,
}

#[cfg(test)]
pub fn resolve_path(workdir: &std::path::Path, input: &str) -> PathBuf {
    let p = PathBuf::from(input);
    if p.is_absolute() {
        p
    } else {
        workdir.join(p)
    }
}


pub async fn execute_tool(rt: &ToolRuntime, tc: &ToolCall) -> Message {
    let normalized_args = catalog::normalize_builtin_tool_args(&tc.name, &tc.arguments);
    let side_effects = tool_side_effects(&tc.name);
    if let Err(e) = validate_builtin_tool_args(&tc.name, &normalized_args, rt.tool_args_strict) {
        return invalid_args_tool_message(
            tc,
            "builtin",
            &e,
            match rt.exec_target_kind {
                ExecTargetKind::Host => "host".to_string(),
                ExecTargetKind::Docker => "docker".to_string(),
            },
        );
    }
    let exec = match tc.name.as_str() {
        "list_dir" => exec_fs::run_list_dir(rt, &normalized_args).await,
        "read_file" => exec_fs::run_read_file(rt, &normalized_args).await,
        "glob" => exec_fs::run_glob(rt, &normalized_args).await,
        "grep" => exec_fs::run_grep(rt, &normalized_args).await,
        "shell" => exec_shell::run_shell(rt, &normalized_args).await,
        "write_file" => exec_write::run_write_file(rt, &normalized_args).await,
        "apply_patch" => exec_write::run_apply_patch(rt, &normalized_args).await,
        _ => ToolExecution {
            ok: false,
            content: format!("unknown tool: {}", tc.name),
            truncated: false,
            error: Some(ToolErrorDetail {
                code: ToolErrorCode::ToolUnknown,
                message: format!("Unknown tool '{}'.", tc.name),
                expected_schema: None,
                received_args: Some(tc.arguments.clone()),
                minimal_example: None,
                available_tools: Some(sorted_builtin_tool_names()),
            }),
            meta: ToolResultMeta {
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
            },
        },
    };
    envelope_to_message(to_tool_result_envelope_with_error(
        tc,
        "builtin",
        exec.ok,
        exec.content,
        exec.truncated,
        exec.error,
        exec.meta,
    ))
}

#[cfg(test)]
fn normalize_builtin_tool_args(tool_name: &str, args: &Value) -> Value {
    catalog::normalize_builtin_tool_args(tool_name, args)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::{
        builtin_tools_enabled, execute_tool, normalize_builtin_tool_args, resolve_path,
        tool_side_effects, validate_builtin_tool_args, validate_schema_args, ToolArgsStrict,
        ToolRuntime,
    };
    use crate::target::{ExecTargetKind, HostTarget};
    use crate::types::{SideEffects, ToolCall};

    #[test]
    fn resolves_relative_path_from_workdir() {
        let base = PathBuf::from("some_workdir");
        let out = resolve_path(&base, "nested/file.txt");
        assert_eq!(out, base.join("nested/file.txt"));
    }

    #[test]
    fn write_tools_not_exposed_by_default() {
        let tools = builtin_tools_enabled(false, false);
        let names = tools.into_iter().map(|t| t.name).collect::<Vec<_>>();
        assert!(names.iter().any(|n| n == "glob"));
        assert!(names.iter().any(|n| n == "grep"));
        assert!(!names.iter().any(|n| n == "shell"));
        assert!(!names.iter().any(|n| n == "write_file"));
        assert!(!names.iter().any(|n| n == "apply_patch"));
    }

    #[test]
    fn side_effects_map_builtin_and_mcp() {
        assert_eq!(tool_side_effects("list_dir"), SideEffects::FilesystemRead);
        assert_eq!(tool_side_effects("glob"), SideEffects::FilesystemRead);
        assert_eq!(tool_side_effects("grep"), SideEffects::FilesystemRead);
        assert_eq!(
            tool_side_effects("mcp.playwright.browser_snapshot"),
            SideEffects::Browser
        );
        assert_eq!(tool_side_effects("mcp.other.echo"), SideEffects::Network);
    }

    #[tokio::test]
    async fn write_file_denied_when_allow_write_false() {
        let rt = ToolRuntime {
            workdir: PathBuf::from("."),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_w".to_string(),
            name: "write_file".to_string(),
            arguments: json!({"path":"foo.txt", "content":"hello"}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let content = msg.content.unwrap_or_default();
        assert!(content.contains("writes require --allow-write"));
        assert!(content.contains("\"ok\":false"));
    }

    #[tokio::test]
    async fn invalid_args_do_not_write_file() {
        let tmp = tempdir().expect("tempdir");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: true,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "bad_w".to_string(),
            name: "write_file".to_string(),
            arguments: json!({"path":"x.txt"}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let content = msg.content.unwrap_or_default();
        assert!(content.contains("invalid tool arguments"));
        let parsed: Value = serde_json::from_str(&content).expect("json");
        assert_eq!(
            parsed
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(|v| v.as_str()),
            Some("tool_args_invalid")
        );
        assert!(!tmp.path().join("x.txt").exists());
    }

    #[tokio::test]
    async fn invalid_args_payload_is_structured_and_deterministic() {
        let rt = ToolRuntime {
            workdir: PathBuf::from("."),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "bad_read".to_string(),
            name: "read_file".to_string(),
            arguments: json!({}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let parsed: Value = serde_json::from_str(&msg.content.unwrap_or_default()).expect("json");
        assert_eq!(
            parsed
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(|v| v.as_str()),
            Some("tool_args_invalid")
        );
        assert_eq!(
            parsed
                .get("error")
                .and_then(|e| e.get("expected_schema"))
                .and_then(|s| s.get("required"))
                .and_then(|r| r.get(0))
                .and_then(|v| v.as_str()),
            Some("path")
        );
        assert_eq!(
            parsed
                .get("error")
                .and_then(|e| e.get("minimal_example"))
                .and_then(|m| m.get("path"))
                .and_then(|v| v.as_str()),
            Some("src/main.rs")
        );
    }

    #[tokio::test]
    async fn unknown_tool_payload_includes_sorted_available_tools() {
        let rt = ToolRuntime {
            workdir: PathBuf::from("."),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_unknown".to_string(),
            name: "grep_search".to_string(),
            arguments: json!({"path":"."}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let parsed: Value = serde_json::from_str(&msg.content.unwrap_or_default()).expect("json");
        assert_eq!(
            parsed
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(|v| v.as_str()),
            Some("tool_unknown")
        );
        let got = parsed
            .get("error")
            .and_then(|e| e.get("available_tools"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let expected = vec![
            json!("apply_patch"),
            json!("glob"),
            json!("grep"),
            json!("list_dir"),
            json!("read_file"),
            json!("shell"),
            json!("write_file"),
        ];
        assert_eq!(got, expected);
    }

    #[tokio::test]
    async fn glob_returns_sorted_matches_and_truncates() {
        let tmp = tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("src")).expect("mkdir");
        std::fs::write(tmp.path().join("src").join("b.rs"), "fn b() {}\n").expect("write");
        std::fs::write(tmp.path().join("src").join("a.rs"), "fn a() {}\n").expect("write");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_glob".to_string(),
            name: "glob".to_string(),
            arguments: json!({"pattern":"src/*.rs","max_results":1}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let env: Value = serde_json::from_str(&msg.content.unwrap_or_default()).expect("env");
        assert_eq!(env.get("ok").and_then(|v| v.as_bool()), Some(true));
        let content = env
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let body: Value = serde_json::from_str(content).expect("body");
        assert_eq!(
            body.get("matches")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str()),
            Some("src/a.rs")
        );
        assert_eq!(body.get("truncated").and_then(|v| v.as_bool()), Some(true));
    }

    #[tokio::test]
    async fn grep_returns_byte_columns_multi_match_and_skips_non_utf8() {
        let tmp = tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("f.txt"), "aba\r\naba\n").expect("write");
        std::fs::write(tmp.path().join("bin.dat"), vec![0, 159, 146, 150]).expect("write");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_grep".to_string(),
            name: "grep".to_string(),
            arguments: json!({"pattern":"a","path":".","max_results":10}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let env: Value = serde_json::from_str(&msg.content.unwrap_or_default()).expect("env");
        let content = env
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let body: Value = serde_json::from_str(content).expect("body");
        assert_eq!(
            body.get("skipped_binary_or_non_utf8_files")
                .and_then(|v| v.as_u64()),
            Some(1)
        );
        let matches = body
            .get("matches")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(matches.len() >= 4);
        assert_eq!(
            matches
                .first()
                .and_then(|m| m.get("line"))
                .and_then(|v| v.as_u64()),
            Some(1)
        );
        assert_eq!(
            matches
                .first()
                .and_then(|m| m.get("column"))
                .and_then(|v| v.as_u64()),
            Some(1)
        );
        assert_eq!(
            matches
                .first()
                .and_then(|m| m.get("text"))
                .and_then(|v| v.as_str()),
            Some("aba")
        );
    }

    #[tokio::test]
    async fn glob_rejects_out_of_scope_path() {
        let tmp = tempdir().expect("tempdir");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_glob_oos".to_string(),
            name: "glob".to_string(),
            arguments: json!({"pattern":"*.rs","path":"../"}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let env: Value = serde_json::from_str(&msg.content.unwrap_or_default()).expect("env");
        assert_eq!(
            env.get("error")
                .and_then(|e| e.get("code"))
                .and_then(|v| v.as_str()),
            Some("path_out_of_scope")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn grep_symlink_out_of_scope_adds_warning_metadata() {
        use std::os::unix::fs::symlink;

        let tmp = tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("inner")).expect("mkdir");
        std::fs::write(tmp.path().join("inner").join("ok.txt"), "hello\n").expect("write");
        let outside = tempdir().expect("outside");
        std::fs::write(outside.path().join("x.txt"), "world\n").expect("write");
        symlink(outside.path(), tmp.path().join("inner").join("escape")).expect("symlink");

        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_warn".to_string(),
            name: "grep".to_string(),
            arguments: json!({"pattern":"hello","path":"inner"}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let env: Value = serde_json::from_str(&msg.content.unwrap_or_default()).expect("env");
        let warnings = env
            .get("meta")
            .and_then(|m| m.get("warnings"))
            .and_then(|w| w.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(!warnings.is_empty());
        assert_eq!(
            warnings
                .first()
                .and_then(|w| w.get("target"))
                .and_then(|v| v.as_str()),
            Some("OUT_OF_SCOPE")
        );
    }

    #[tokio::test]
    async fn write_file_blocks_existing_file_without_overwrite_flag() {
        let tmp = tempdir().expect("tempdir");
        let existing = tmp.path().join("x.txt");
        std::fs::write(&existing, "old").expect("seed file");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: true,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_overwrite_block".to_string(),
            name: "write_file".to_string(),
            arguments: json!({"path":"x.txt","content":"new"}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let body = msg.content.unwrap_or_default();
        assert!(body.contains("\"ok\":false"));
        assert!(body.contains("use apply_patch"));
        let after = std::fs::read_to_string(existing).expect("read file");
        assert_eq!(after, "old");
    }

    #[tokio::test]
    async fn write_file_allows_existing_file_with_overwrite_flag() {
        let tmp = tempdir().expect("tempdir");
        let existing = tmp.path().join("x.txt");
        std::fs::write(&existing, "old").expect("seed file");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: true,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_overwrite_allowed".to_string(),
            name: "write_file".to_string(),
            arguments: json!({"path":"x.txt","content":"new","overwrite_existing":true}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let body = msg.content.unwrap_or_default();
        assert!(body.contains("\"ok\":true"));
        let after = std::fs::read_to_string(existing).expect("read file");
        assert_eq!(after, "new");
    }

    #[test]
    fn wrong_type_args_rejected() {
        let err = validate_builtin_tool_args(
            "shell",
            &json!({"cmd":"echo", "args":[1,2]}),
            ToolArgsStrict::On,
        )
        .expect_err("expected error");
        assert!(err.contains("array of strings"));
    }

    #[test]
    fn unknown_schema_allows_empty_object_only() {
        assert!(validate_schema_args(&json!({}), None, ToolArgsStrict::On).is_ok());
        let err = validate_schema_args(&json!({"x":1}), None, ToolArgsStrict::On)
            .expect_err("expected unknown-schema arg error");
        assert_eq!(err, "arguments not allowed for tool with unknown schema");
    }

    #[test]
    fn unknown_schema_still_requires_object() {
        let err = validate_schema_args(&json!(["x"]), None, ToolArgsStrict::On)
            .expect_err("expected object error");
        assert_eq!(err, "arguments must be a JSON object");
    }

    #[tokio::test]
    async fn apply_patch_updates_file() {
        let tmp = tempdir().expect("tempdir");
        let file = tmp.path().join("a.txt");
        tokio::fs::write(&file, "hello\n").await.expect("write");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: true,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_p".to_string(),
            name: "apply_patch".to_string(),
            arguments: json!({"path":"a.txt","patch":"@@ -1 +1 @@\n-hello\n+world\n"}),
        };
        let _ = execute_tool(&rt, &tc).await;
        let updated = tokio::fs::read_to_string(&file).await.expect("read");
        assert_eq!(updated, "world\n");
    }

    #[tokio::test]
    async fn read_file_envelope_sets_truncation() {
        let tmp = tempdir().expect("tempdir");
        let file = tmp.path().join("c.txt");
        tokio::fs::write(&file, "abcdefghijklmnopqrstuvwxyz")
            .await
            .expect("write");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 5,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_t".to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path":"c.txt"}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let content = msg.content.expect("content");
        let parsed: Value = serde_json::from_str(&content).expect("json");
        assert_eq!(
            parsed.get("schema_version").and_then(|v| v.as_str()),
            Some("openagent.tool_result.v1")
        );
        assert_eq!(parsed.get("ok").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            parsed.get("truncated").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[tokio::test]
    async fn shell_in_workdir_flag_rejects_escaping_cwd() {
        let tmp = tempdir().expect("tempdir");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: true,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_shell".to_string(),
            name: "shell".to_string(),
            arguments: json!({"cmd":"echo","args":["hi"],"cwd":"../"}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let content = msg.content.expect("content");
        assert!(content.contains("--allow-shell-in-workdir"));
        assert!(content.contains("\"ok\":false"));
    }

    #[test]
    fn shell_command_alias_normalizes_to_cmd_and_args() {
        let normalized = normalize_builtin_tool_args(
            "shell",
            &json!({"command":"cmd /c echo should-be-blocked"}),
        );
        assert_eq!(
            normalized,
            json!({"cmd":"cmd","args":["/c","echo","should-be-blocked"]})
        );
    }

    #[tokio::test]
    async fn shell_disabled_uses_shell_gate_deny_code() {
        let tmp = tempdir().expect("tempdir");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_shell_disabled".to_string(),
            name: "shell".to_string(),
            arguments: json!({"command":"cmd /c echo hi"}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let content = msg.content.expect("content");
        let parsed: Value = serde_json::from_str(&content).expect("json");
        assert_eq!(
            parsed
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(|v| v.as_str()),
            Some("shell_gate_deny")
        );
    }

    #[tokio::test]
    async fn shell_spawn_not_found_sets_not_found_error_code() {
        let tmp = tempdir().expect("tempdir");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: true,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_shell_missing".to_string(),
            name: "shell".to_string(),
            arguments: json!({"cmd":"definitely_missing_localagent_cmd_12345"}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let content = msg.content.expect("content");
        let parsed: Value = serde_json::from_str(&content).expect("json");
        assert_eq!(parsed.get("ok").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(
            parsed
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(|v| v.as_str()),
            Some("shell_exec_not_found")
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn shell_auto_repair_wraps_windows_builtin() {
        let tmp = tempdir().expect("tempdir");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: true,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_shell_auto_repair_win".to_string(),
            name: "shell".to_string(),
            arguments: json!({"cmd":"echo","args":["hi-manual-test"]}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let envelope: Value = serde_json::from_str(&msg.content.expect("content")).expect("json");
        assert_eq!(envelope.get("ok").and_then(|v| v.as_bool()), Some(true));
        let inner = envelope
            .get("content")
            .and_then(|v| v.as_str())
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .expect("inner shell json");
        let stdout = inner
            .get("stdout")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(stdout.contains("hi-manual-test"));
        assert_eq!(
            inner.get("repair_attempted").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            inner.get("repair_strategy").and_then(|v| v.as_str()),
            Some("windows_cmd_c")
        );
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn shell_auto_repair_uses_sh_lc_for_embedded_command() {
        let tmp = tempdir().expect("tempdir");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: true,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_shell_auto_repair_unix".to_string(),
            name: "shell".to_string(),
            arguments: json!({"cmd":"echo hi-manual-test"}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let envelope: Value = serde_json::from_str(&msg.content.expect("content")).expect("json");
        assert_eq!(envelope.get("ok").and_then(|v| v.as_bool()), Some(true));
        let inner = envelope
            .get("content")
            .and_then(|v| v.as_str())
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .expect("inner shell json");
        let stdout = inner
            .get("stdout")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(stdout.contains("hi-manual-test"));
        assert_eq!(
            inner.get("repair_attempted").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            inner.get("repair_strategy").and_then(|v| v.as_str()),
            Some("unix_sh_lc")
        );
    }

    #[tokio::test]
    async fn read_file_rejects_path_traversal() {
        let tmp = tempdir().expect("tempdir");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_read_escape".to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path":"../secret.txt"}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let content = msg.content.expect("content");
        assert!(content.contains("path must stay within workdir"));
        assert!(content.contains("\"ok\":false"));
        let parsed: Value = serde_json::from_str(&content).expect("json");
        assert_eq!(
            parsed
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(|v| v.as_str()),
            Some("tool_path_denied")
        );
    }

    #[tokio::test]
    async fn write_file_rejects_absolute_path() {
        let tmp = tempdir().expect("tempdir");
        let absolute = tmp.path().join("outside.txt").display().to_string();
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: true,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        };
        let tc = ToolCall {
            id: "tc_write_abs".to_string(),
            name: "write_file".to_string(),
            arguments: json!({"path":absolute, "content":"hello"}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let content = msg.content.expect("content");
        assert!(content.contains("path must stay within workdir"));
        assert!(content.contains("\"ok\":false"));
    }
}
