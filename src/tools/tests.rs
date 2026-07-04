use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use tempfile::tempdir;

use super::{
    builtin_tools_enabled, builtin_tools_enabled_with_lsp, execute_tool,
    load_tool_result_artifact_manifest, resolve_tool_result_artifact_ref,
    to_model_observation_message, to_tool_result_envelope, to_tool_result_envelope_with_error,
    tool_side_effects, validate_builtin_tool_args, validate_schema_args, ToolArgsStrict,
    ToolErrorCode, ToolErrorDetail, ToolResultContentRef, ToolResultMeta, ToolRuntime,
};
use crate::target::{ExecTargetKind, HostTarget};
use crate::types::{SideEffects, ToolCall};

fn resolve_path(workdir: &Path, input: &str) -> PathBuf {
    let p = PathBuf::from(input);
    if p.is_absolute() {
        p
    } else {
        workdir.join(p)
    }
}

fn normalize_builtin_tool_args(tool_name: &str, args: &Value) -> Value {
    super::catalog::normalize_builtin_tool_args(tool_name, args)
}

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
    assert!(names.iter().any(|n| n == "update_plan"));
    assert!(!names.iter().any(|n| n == "shell"));
    assert!(!names.iter().any(|n| n == "edit"));
    assert!(!names.iter().any(|n| n == "write_file"));
    assert!(!names.iter().any(|n| n == "apply_patch"));
    assert!(!names.iter().any(|n| n == "lsp.diagnostics"));
}

#[test]
fn lsp_tools_exposed_only_when_enabled() {
    let tools = builtin_tools_enabled_with_lsp(false, false, true);
    let names = tools.into_iter().map(|t| t.name).collect::<Vec<_>>();
    for expected in [
        "lsp.diagnostics",
        "lsp.document_symbols",
        "lsp.goto_definition",
        "lsp.find_references",
        "lsp.hover",
    ] {
        assert!(names.iter().any(|n| n == expected));
    }
}

#[test]
fn write_tool_descriptions_bias_toward_relative_paths_and_apply_patch_recovery() {
    let tools = builtin_tools_enabled(true, false);
    let edit = tools.iter().find(|tool| tool.name == "edit").expect("edit");
    assert!(edit.description.contains("workdir-relative path"));
    assert!(edit
        .description
        .contains("default tool for ordinary small in-place fixes"));
    assert!(edit.description.contains("OpenCode-style aliases"));

    let apply_patch = tools
        .iter()
        .find(|tool| tool.name == "apply_patch")
        .expect("apply_patch");
    assert!(apply_patch.description.contains("workdir-relative path"));
    assert!(apply_patch
        .description
        .contains("larger or multi-hunk edits"));
    assert!(apply_patch.description.contains("edit/str_replace"));

    let str_replace = tools
        .iter()
        .find(|tool| tool.name == "str_replace")
        .expect("str_replace");
    assert!(str_replace.description.contains("workdir-relative path"));
    assert!(str_replace
        .description
        .contains("only for trivial exact unique matches"));
    assert!(str_replace
        .description
        .contains("switch to edit or apply_patch"));

    let write_file = tools
        .iter()
        .find(|tool| tool.name == "write_file")
        .expect("write_file");
    assert!(write_file
        .description
        .contains("new files or explicit full rewrites"));
}

#[test]
fn builtin_tool_order_biases_toward_read_edit_verify_validate_flow() {
    let names = super::schema::sorted_builtin_tool_names();
    assert_eq!(
        names,
        vec![
            "list_dir",
            "glob",
            "grep",
            "lsp.diagnostics",
            "lsp.document_symbols",
            "lsp.find_references",
            "lsp.goto_definition",
            "lsp.hover",
            "update_plan",
            "read_file",
            "edit",
            "apply_patch",
            "shell",
            "str_replace",
            "write_file",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>()
    );
}

#[test]
fn side_effects_map_builtin_and_mcp() {
    assert_eq!(tool_side_effects("list_dir"), SideEffects::FilesystemRead);
    assert_eq!(tool_side_effects("glob"), SideEffects::FilesystemRead);
    assert_eq!(tool_side_effects("grep"), SideEffects::FilesystemRead);
    assert_eq!(
        tool_side_effects("lsp.goto_definition"),
        SideEffects::FilesystemRead
    );
    assert_eq!(tool_side_effects("update_plan"), SideEffects::None);
    assert_eq!(
        tool_side_effects("mcp.playwright.browser_snapshot"),
        SideEffects::Browser
    );
    assert_eq!(tool_side_effects("mcp.other.echo"), SideEffects::Network);
}

#[tokio::test]
async fn update_plan_returns_compact_side_effect_free_result() {
    let rt = ToolRuntime {
        workdir: PathBuf::from("."),
        allow_shell: false,
        allow_shell_in_workdir_only: false,
        allow_write: false,
        max_tool_output_bytes: 200_000,
        max_read_bytes: 200_000,
        unsafe_bypass_allow_flags: false,
        tool_args_strict: ToolArgsStrict::On,
        lsp: None,
        exec_target_kind: ExecTargetKind::Host,
        exec_target: std::sync::Arc::new(HostTarget),
    };
    let tc = ToolCall {
        id: "plan_1".to_string(),
        name: "update_plan".to_string(),
        arguments: json!({
            "items": [
                {"step":"Inspect code", "status":"completed"},
                {"step":"Implement update_plan", "status":"in_progress"},
                {"step":"Run tests", "status":"pending"}
            ]
        }),
    };
    let msg = execute_tool(&rt, &tc).await;
    let parsed: Value = serde_json::from_str(&msg.content.unwrap_or_default()).expect("json");
    assert_eq!(parsed["ok"], json!(true));
    assert_eq!(parsed["meta"]["side_effects"], json!("none"));
    let inner: Value = serde_json::from_str(parsed["content"].as_str().unwrap()).expect("inner");
    assert_eq!(inner["items"], json!(3));
    assert_eq!(inner["completed"], json!(1));
    assert_eq!(inner["pending"], json!(1));
    assert_eq!(inner["in_progress"], json!("Implement update_plan"));
}

#[test]
fn model_observation_for_shell_failure_preserves_tail_without_raw_dump() {
    let tc = ToolCall {
        id: "tc_shell_obs".to_string(),
        name: "shell".to_string(),
        arguments: json!({"cmd":"cargo","args":["test"],"cwd":"crates/demo"}),
    };
    let stdout = format!(
        "compile start\n{}\nnot ok 7 - parser trims whitespace\nExpected 7 but got NaN\n",
        "middle noise\n".repeat(800)
    );
    let stderr = format!(
        "{}error: test failed, to rerun pass --lib\n",
        "stderr noise\n".repeat(500)
    );
    let raw_inner = json!({
        "status": 1,
        "stdout": stdout,
        "stderr": stderr,
        "stdout_truncated": false,
        "stderr_truncated": false
    })
    .to_string();
    let raw_msg = super::envelope_to_message(to_tool_result_envelope_with_error(
        &tc,
        "builtin",
        false,
        raw_inner,
        false,
        Some(ToolErrorDetail {
            code: ToolErrorCode::ShellExecNonZeroExit,
            message: "Shell command exited with non-zero status: 1.".to_string(),
            expected_schema: None,
            received_args: None,
            minimal_example: None,
            available_tools: None,
        }),
        ToolResultMeta {
            side_effects: SideEffects::ShellExec,
            duration_ms: Some(123),
            bytes: Some(12_000),
            exit_code: Some(1),
            stderr_truncated: Some(false),
            stdout_truncated: Some(false),
            source: "builtin".to_string(),
            execution_target: "host".to_string(),
            warnings: None,
            warnings_max: None,
            warnings_truncated: None,
            docker: None,
        },
    ));

    let artifact_dir = tempdir().expect("artifact dir");
    let raw_content = raw_msg.content.as_deref().expect("raw content").to_string();
    let compact = to_model_observation_message(
        &raw_msg,
        &tc,
        Path::new("C:/repo"),
        Some(artifact_dir.path()),
    );
    let envelope: Value = serde_json::from_str(&compact.content.unwrap()).expect("envelope");
    let observation = envelope.get("observation").expect("observation");
    assert_eq!(
        observation.get("schema_version").and_then(Value::as_str),
        Some("openagent.observation.v1")
    );
    assert_eq!(
        observation.get("exit_code").and_then(Value::as_i64),
        Some(1)
    );
    assert_eq!(
        observation.get("duration_ms").and_then(Value::as_u64),
        Some(123)
    );
    assert!(observation
        .get("stdout_tail")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .contains("Expected 7 but got NaN"));
    let raw_result_ref = observation.get("raw_result_ref").expect("raw_result_ref");
    assert_eq!(
        raw_result_ref.get("kind").and_then(Value::as_str),
        Some("run_artifact_path")
    );
    let raw_result_path = raw_result_ref
        .get("path")
        .and_then(Value::as_str)
        .expect("raw result path");
    assert_eq!(
        std::fs::read_to_string(raw_result_path).expect("raw artifact"),
        raw_content
    );
    let manifest =
        load_tool_result_artifact_manifest(artifact_dir.path()).expect("artifact manifest");
    assert_eq!(
        manifest.schema_version,
        "openagent.tool_result_artifacts_manifest.v1"
    );
    let raw_entry = manifest
        .artifacts
        .iter()
        .find(|entry| entry.label == "raw_result")
        .expect("raw manifest entry");
    assert_eq!(raw_entry.sequence, 1);
    assert_eq!(raw_entry.tool_name, "shell");
    assert_eq!(raw_entry.tool_call_id, "tc_shell_obs");
    assert_eq!(raw_entry.content_kind, "application/json");
    assert_eq!(raw_entry.size_bytes, raw_content.len() as u64);
    assert_eq!(raw_entry.artifact_ref.path, raw_result_path);
    assert_eq!(raw_entry.artifact_ref.bytes, raw_content.len() as u64);
    assert_eq!(raw_entry.sha16.len(), 16);
    assert_eq!(raw_entry.sha256.len(), 64);
    assert!(observation.get("stdout_ref").is_some());
    let stderr_ref = observation.get("stderr_ref").expect("stderr_ref");
    let stderr_path = stderr_ref
        .get("path")
        .and_then(Value::as_str)
        .expect("stderr path");
    assert!(std::fs::read_to_string(stderr_path)
        .expect("stderr artifact")
        .contains("error: test failed"));
    assert_eq!(
        observation.get("truncated").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        observation.get("truncation_policy").and_then(Value::as_str),
        Some("head_1200_tail_4000")
    );
    assert!(!envelope
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .contains("middle noise\nmiddle noise\nmiddle noise"));
    assert_eq!(
        observation.get("failure_class").and_then(Value::as_str),
        Some("E_NONZERO_EXIT")
    );
    assert!(observation
        .get("suggested_next_action")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .contains("rerun the command"));
}

#[test]
fn tool_result_artifact_resolver_accepts_manifested_ref_and_rejects_bad_refs() {
    let tc = ToolCall {
        id: "tc_resolver".to_string(),
        name: "read_file".to_string(),
        arguments: json!({"path":"src/main.rs"}),
    };
    let raw_msg = super::envelope_to_message(to_tool_result_envelope(
        &tc,
        "builtin",
        true,
        json!({"path":"src/main.rs","content":"hello"}).to_string(),
        false,
        ToolResultMeta {
            side_effects: SideEffects::FilesystemRead,
            duration_ms: Some(3),
            bytes: Some(5),
            exit_code: None,
            stderr_truncated: None,
            stdout_truncated: None,
            source: "builtin".to_string(),
            execution_target: "host".to_string(),
            warnings: None,
            warnings_max: None,
            warnings_truncated: None,
            docker: None,
        },
    ));
    let artifact_dir = tempdir().expect("artifact dir");
    let compact =
        to_model_observation_message(&raw_msg, &tc, Path::new("."), Some(artifact_dir.path()));
    let envelope: Value = serde_json::from_str(&compact.content.unwrap()).expect("envelope");
    let raw_ref: ToolResultContentRef =
        serde_json::from_value(envelope["observation"]["raw_result_ref"].clone()).expect("raw ref");

    let resolved =
        resolve_tool_result_artifact_ref(artifact_dir.path(), &raw_ref).expect("resolve ref");
    assert_eq!(
        std::fs::read_to_string(resolved).expect("artifact"),
        raw_msg.content.unwrap()
    );

    let mut unknown_ref = raw_ref.clone();
    unknown_ref.path = artifact_dir
        .path()
        .join("missing-artifact.json")
        .to_string_lossy()
        .to_string();
    assert!(resolve_tool_result_artifact_ref(artifact_dir.path(), &unknown_ref).is_err());

    let mut malicious_ref = raw_ref;
    malicious_ref.path = "../outside.json".to_string();
    assert!(resolve_tool_result_artifact_ref(artifact_dir.path(), &malicious_ref).is_err());
}

#[test]
fn model_observation_for_write_reports_changed_file() {
    let tc = ToolCall {
        id: "tc_edit_obs".to_string(),
        name: "edit".to_string(),
        arguments: json!({"path":"src/main.rs","old_string":"old","new_string":"new"}),
    };
    let raw_msg = super::envelope_to_message(to_tool_result_envelope(
        &tc,
        "builtin",
        true,
        json!({"path":"src/main.rs","changed":true,"bytes_written":42}).to_string(),
        false,
        ToolResultMeta {
            side_effects: SideEffects::FilesystemWrite,
            duration_ms: Some(7),
            bytes: Some(42),
            exit_code: None,
            stderr_truncated: None,
            stdout_truncated: None,
            source: "builtin".to_string(),
            execution_target: "host".to_string(),
            warnings: None,
            warnings_max: None,
            warnings_truncated: None,
            docker: None,
        },
    ));

    let compact = to_model_observation_message(&raw_msg, &tc, Path::new("."), None);
    let envelope: Value = serde_json::from_str(&compact.content.unwrap()).expect("envelope");
    let changed = envelope
        .get("observation")
        .and_then(|o| o.get("changed_files"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(changed, vec![json!("src/main.rs")]);
}

#[test]
fn update_plan_rejects_multiple_in_progress_items() {
    let err = validate_builtin_tool_args(
        "update_plan",
        &json!({
            "items": [
                {"step":"A", "status":"in_progress"},
                {"step":"B", "status":"in_progress"}
            ]
        }),
        ToolArgsStrict::On,
    )
    .expect_err("invalid plan rejected");
    assert!(err.contains("at most one"));
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
        lsp: None,
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
        lsp: None,
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
        lsp: None,
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
        lsp: None,
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
        json!("edit"),
        json!("glob"),
        json!("grep"),
        json!("list_dir"),
        json!("lsp.diagnostics"),
        json!("lsp.document_symbols"),
        json!("lsp.find_references"),
        json!("lsp.goto_definition"),
        json!("lsp.hover"),
        json!("read_file"),
        json!("shell"),
        json!("str_replace"),
        json!("update_plan"),
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
        lsp: None,
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
        lsp: None,
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
        lsp: None,
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
        lsp: None,
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
        lsp: None,
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
        lsp: None,
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
fn edit_aliases_normalize_to_internal_shape() {
    let normalized = normalize_builtin_tool_args(
        "edit",
        &json!({
            "filePath":"src/main.rs",
            "oldString":"before",
            "newString":"after"
        }),
    );
    assert_eq!(
        normalized,
        json!({
            "filePath":"src/main.rs",
            "oldString":"before",
            "newString":"after",
            "path":"src/main.rs",
            "old_string":"before",
            "new_string":"after"
        })
    );
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
        lsp: None,
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
async fn edit_updates_file_with_opencode_style_aliases() {
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
        lsp: None,
        exec_target_kind: ExecTargetKind::Host,
        exec_target: std::sync::Arc::new(HostTarget),
    };
    let tc = ToolCall {
        id: "tc_edit".to_string(),
        name: "edit".to_string(),
        arguments: json!({"filePath":"a.txt","oldString":"hello","newString":"world"}),
    };
    let msg = execute_tool(&rt, &tc).await;
    let body = msg.content.expect("content");
    assert!(body.contains("\"ok\":true"));
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
        lsp: None,
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
        lsp: None,
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
    let normalized =
        normalize_builtin_tool_args("shell", &json!({"command":"cmd /c echo should-be-blocked"}));
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
        lsp: None,
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
        lsp: None,
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
        lsp: None,
        exec_target_kind: ExecTargetKind::Host,
        exec_target: std::sync::Arc::new(HostTarget),
    };
    // Use `ver` rather than `echo`: both are cmd builtins, but `echo` is often
    // shadowed by an MSYS/Git `echo.exe` on PATH, which lets the direct spawn
    // succeed and bypasses the repair path. `ver` has no standalone executable
    // counterpart, so the initial spawn deterministically fails regardless of
    // host PATH contents, forcing the windows_cmd_c auto-repair.
    let tc = ToolCall {
        id: "tc_shell_auto_repair_win".to_string(),
        name: "shell".to_string(),
        arguments: json!({"cmd":"ver"}),
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
    // `cmd /c ver` prints a line like "Microsoft Windows [Version 10.0.x]".
    assert!(
        stdout.to_ascii_lowercase().contains("windows"),
        "expected `ver` output to mention Windows, got: {stdout}"
    );
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
        lsp: None,
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
async fn shell_without_timeout_still_runs_fast_command_under_default_policy() {
    // A host shell call that omits timeout_ms now gets the 120s default. A fast
    // command must still complete normally (regression guard: the default must
    // not break ordinary usage), proving missing -> default is wired into the
    // execute_tool path without turning fast commands into timeouts.
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
        lsp: None,
        exec_target_kind: ExecTargetKind::Host,
        exec_target: std::sync::Arc::new(HostTarget),
    };
    let arguments = if cfg!(windows) {
        json!({"cmd":"cmd","args":["/C","echo default-policy-ok"]})
    } else {
        json!({"cmd":"sh","args":["-c","echo default-policy-ok"]})
    };
    let tc = ToolCall {
        id: "tc_shell_default_timeout".to_string(),
        name: "shell".to_string(),
        arguments,
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
    assert!(stdout.contains("default-policy-ok"));
    // Fast command must not be reported as timed out.
    assert!(inner.get("timed_out").is_none());
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
        lsp: None,
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
        lsp: None,
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
    assert!(content.contains("Use a workdir-relative path"));
    assert!(content.contains("\"ok\":false"));
}

#[tokio::test]
async fn str_replace_not_found_suggests_apply_patch_recovery() {
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
        lsp: None,
        exec_target_kind: ExecTargetKind::Host,
        exec_target: std::sync::Arc::new(HostTarget),
    };
    let tc = ToolCall {
        id: "tc_str_replace_missing".to_string(),
        name: "str_replace".to_string(),
        arguments: json!({"path":"a.txt","old_string":"goodbye","new_string":"world"}),
    };
    let msg = execute_tool(&rt, &tc).await;
    let content = msg.content.expect("content");
    assert!(content.contains("old_string not found"));
    assert!(content.contains("switch to apply_patch"));
}
