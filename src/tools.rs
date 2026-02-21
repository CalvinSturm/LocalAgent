use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::types::{Message, Role, SideEffects, ToolCall, ToolDef};

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

#[derive(Debug, Clone)]
pub struct ToolRuntime {
    pub workdir: PathBuf,
    pub allow_shell: bool,
    pub allow_write: bool,
    pub max_tool_output_bytes: usize,
    pub max_read_bytes: usize,
    pub unsafe_bypass_allow_flags: bool,
    pub tool_args_strict: ToolArgsStrict,
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
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolResultEnvelope {
    pub schema_version: String,
    pub tool_name: String,
    pub tool_call_id: String,
    pub ok: bool,
    pub content: String,
    pub truncated: bool,
    pub meta: ToolResultMeta,
}

#[derive(Debug, Clone)]
struct ToolExecution {
    ok: bool,
    content: String,
    truncated: bool,
    meta: ToolResultMeta,
}

pub fn tool_side_effects(tool_name: &str) -> SideEffects {
    match tool_name {
        "list_dir" | "read_file" => SideEffects::FilesystemRead,
        "shell" => SideEffects::ShellExec,
        "write_file" | "apply_patch" => SideEffects::FilesystemWrite,
        _ if tool_name.starts_with("mcp.playwright.") => SideEffects::Browser,
        _ if tool_name.starts_with("mcp.") => SideEffects::Network,
        _ => SideEffects::None,
    }
}

pub fn builtin_tools_enabled(enable_write_tools: bool) -> Vec<ToolDef> {
    let mut tools = vec![
        ToolDef {
            name: "list_dir".to_string(),
            description: "List entries in a directory.".to_string(),
            parameters: json!({
                "type":"object",
                "properties":{"path":{"type":"string"}},
                "required":["path"]
            }),
            side_effects: SideEffects::FilesystemRead,
        },
        ToolDef {
            name: "read_file".to_string(),
            description: "Read a UTF-8 text file (lossy decode allowed).".to_string(),
            parameters: json!({
                "type":"object",
                "properties":{"path":{"type":"string"}},
                "required":["path"]
            }),
            side_effects: SideEffects::FilesystemRead,
        },
        ToolDef {
            name: "shell".to_string(),
            description: "Run a shell command with optional args and cwd.".to_string(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "cmd":{"type":"string"},
                    "args":{"type":"array","items":{"type":"string"}},
                    "cwd":{"type":"string"}
                },
                "required":["cmd"]
            }),
            side_effects: SideEffects::ShellExec,
        },
    ];
    if enable_write_tools {
        tools.push(ToolDef {
            name: "write_file".to_string(),
            description: "Write UTF-8 text content to a file.".to_string(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "content":{"type":"string"},
                    "create_parents":{"type":"boolean"}
                },
                "required":["path","content"]
            }),
            side_effects: SideEffects::FilesystemWrite,
        });
        tools.push(ToolDef {
            name: "apply_patch".to_string(),
            description: "Apply a unified diff patch to a file.".to_string(),
            parameters: json!({
                "type":"object",
                "properties":{"path":{"type":"string"},"patch":{"type":"string"}},
                "required":["path","patch"]
            }),
            side_effects: SideEffects::FilesystemWrite,
        });
    }
    tools
}

pub fn resolve_path(workdir: &Path, input: &str) -> PathBuf {
    let p = PathBuf::from(input);
    if p.is_absolute() {
        p
    } else {
        workdir.join(p)
    }
}

pub fn to_tool_result_envelope(
    tc: &ToolCall,
    source: &str,
    ok: bool,
    content: String,
    truncated: bool,
    mut meta: ToolResultMeta,
) -> ToolResultEnvelope {
    meta.source = source.to_string();
    ToolResultEnvelope {
        schema_version: "openagent.tool_result.v1".to_string(),
        tool_name: tc.name.clone(),
        tool_call_id: tc.id.clone(),
        ok,
        content,
        truncated,
        meta,
    }
}

pub fn envelope_to_message(env: ToolResultEnvelope) -> Message {
    Message {
        role: Role::Tool,
        content: Some(serde_json::to_string(&env).unwrap_or_else(|e| {
            json!({"schema_version":"openagent.tool_result.v1","ok":false,"content":format!("failed to serialize tool result envelope: {e}")}).to_string()
        })),
        tool_call_id: Some(env.tool_call_id.clone()),
        tool_name: Some(env.tool_name.clone()),
        tool_calls: None,
    }
}

pub fn validate_builtin_tool_args(
    tool_name: &str,
    args: &Value,
    strict: ToolArgsStrict,
) -> Result<(), String> {
    let obj = args
        .as_object()
        .ok_or_else(|| "arguments must be a JSON object".to_string())?;
    if !strict.is_enabled() {
        return Ok(());
    }
    match tool_name {
        "list_dir" | "read_file" => require_non_empty_string(obj, "path")?,
        "shell" => {
            require_non_empty_string(obj, "cmd")?;
            if let Some(v) = obj.get("args") {
                let arr = v
                    .as_array()
                    .ok_or_else(|| "args must be an array of strings".to_string())?;
                if arr.iter().any(|x| x.as_str().is_none()) {
                    return Err("args must be an array of strings".to_string());
                }
            }
            if let Some(v) = obj.get("cwd") {
                if v.as_str().is_none() {
                    return Err("cwd must be a string".to_string());
                }
            }
        }
        "write_file" => {
            require_non_empty_string(obj, "path")?;
            require_string(obj, "content")?;
            if let Some(v) = obj.get("create_parents") {
                if v.as_bool().is_none() {
                    return Err("create_parents must be a boolean".to_string());
                }
            }
        }
        "apply_patch" => {
            require_non_empty_string(obj, "path")?;
            require_non_empty_string(obj, "patch")?;
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_schema_args(
    args: &Value,
    schema: Option<&Value>,
    strict: ToolArgsStrict,
) -> Result<(), String> {
    let schema = match schema {
        Some(v) => v,
        None => {
            return args
                .as_object()
                .map(|_| ())
                .ok_or_else(|| "arguments must be a JSON object".to_string());
        }
    };
    let obj = args
        .as_object()
        .ok_or_else(|| "arguments must be a JSON object".to_string())?;
    if !strict.is_enabled() {
        return Ok(());
    }
    let Some(sobj) = schema.as_object() else {
        return Ok(());
    };
    if let Some(req) = sobj.get("required").and_then(|v| v.as_array()) {
        for it in req {
            if let Some(key) = it.as_str() {
                if !obj.contains_key(key) {
                    return Err(format!("missing required field: {key}"));
                }
            }
        }
    }
    let props = sobj
        .get("properties")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    for (k, v) in obj {
        if let Some(schema) = props.get(k) {
            validate_value_type(v, schema).map_err(|e| format!("field '{k}' {e}"))?;
        } else if sobj.get("additionalProperties").and_then(|v| v.as_bool()) == Some(false) {
            return Err(format!("unknown field not allowed: {k}"));
        }
    }
    Ok(())
}

fn validate_value_type(value: &Value, schema: &Value) -> Result<(), String> {
    let Some(kind) = schema.get("type").and_then(|v| v.as_str()) else {
        return Ok(());
    };
    match kind {
        "string" if value.is_string() => Ok(()),
        "number" if value.is_number() => Ok(()),
        "integer" if value.as_i64().is_some() || value.as_u64().is_some() => Ok(()),
        "boolean" if value.is_boolean() => Ok(()),
        "object" if value.is_object() => Ok(()),
        "array" if value.is_array() => {
            if let Some(item_schema) = schema.get("items") {
                if let Some(arr) = value.as_array() {
                    for item in arr {
                        validate_value_type(item, item_schema)?;
                    }
                }
            }
            Ok(())
        }
        "null" if value.is_null() => Ok(()),
        other => Err(format!("has invalid type (expected {other})")),
    }
}

fn require_string(obj: &serde_json::Map<String, Value>, key: &str) -> Result<(), String> {
    match obj.get(key) {
        Some(v) if v.is_string() => Ok(()),
        Some(_) => Err(format!("{key} must be a string")),
        None => Err(format!("missing required field: {key}")),
    }
}

fn require_non_empty_string(obj: &serde_json::Map<String, Value>, key: &str) -> Result<(), String> {
    let v = obj
        .get(key)
        .ok_or_else(|| format!("missing required field: {key}"))?;
    let s = v
        .as_str()
        .ok_or_else(|| format!("{key} must be a string"))?;
    if s.is_empty() {
        return Err(format!("{key} must be a non-empty string"));
    }
    Ok(())
}

pub async fn execute_tool(rt: &ToolRuntime, tc: &ToolCall) -> Message {
    let side_effects = tool_side_effects(&tc.name);
    if let Err(e) = validate_builtin_tool_args(&tc.name, &tc.arguments, rt.tool_args_strict) {
        return envelope_to_message(to_tool_result_envelope(
            tc,
            "builtin",
            false,
            format!("invalid tool arguments: {e}"),
            false,
            ToolResultMeta {
                side_effects,
                bytes: None,
                exit_code: None,
                stderr_truncated: None,
                stdout_truncated: None,
                source: "builtin".to_string(),
            },
        ));
    }
    let exec = match tc.name.as_str() {
        "list_dir" => run_list_dir(rt, &tc.arguments).await,
        "read_file" => run_read_file(rt, &tc.arguments).await,
        "shell" => run_shell(rt, &tc.arguments).await,
        "write_file" => run_write_file(rt, &tc.arguments).await,
        "apply_patch" => run_apply_patch(rt, &tc.arguments).await,
        _ => ToolExecution {
            ok: false,
            content: format!("unknown tool: {}", tc.name),
            truncated: false,
            meta: ToolResultMeta {
                side_effects,
                bytes: None,
                exit_code: None,
                stderr_truncated: None,
                stdout_truncated: None,
                source: "builtin".to_string(),
            },
        },
    };
    envelope_to_message(to_tool_result_envelope(
        tc,
        "builtin",
        exec.ok,
        exec.content,
        exec.truncated,
        exec.meta,
    ))
}

async fn run_list_dir(rt: &ToolRuntime, args: &Value) -> ToolExecution {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    let full_path = resolve_path(&rt.workdir, path);
    let mut entries_out = Vec::new();
    match tokio::fs::read_dir(&full_path).await {
        Ok(mut rd) => loop {
            match rd.next_entry().await {
                Ok(Some(entry)) => {
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    match entry.metadata().await {
                        Ok(meta) => entries_out.push(
                            json!({"name":file_name,"is_dir":meta.is_dir(),"len":meta.len()}),
                        ),
                        Err(e) => entries_out.push(json!({"name":file_name,"error":e.to_string()})),
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    return failed_exec(
                        SideEffects::FilesystemRead,
                        format!("list_dir failed for {}: {e}", full_path.display()),
                    )
                }
            }
        },
        Err(e) => {
            return failed_exec(
                SideEffects::FilesystemRead,
                format!("list_dir failed for {}: {e}", full_path.display()),
            )
        }
    }
    ToolExecution {
        ok: true,
        content: json!({"path":full_path.display().to_string(),"entries":entries_out}).to_string(),
        truncated: false,
        meta: base_meta(SideEffects::FilesystemRead),
    }
}

async fn run_read_file(rt: &ToolRuntime, args: &Value) -> ToolExecution {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let full_path = resolve_path(&rt.workdir, path);
    match tokio::fs::read(&full_path).await {
        Ok(bytes) => {
            let content_raw = String::from_utf8_lossy(&bytes).to_string();
            let (content, truncated) = truncate_utf8_to_bytes(&content_raw, rt.max_read_bytes);
            let mut meta = base_meta(SideEffects::FilesystemRead);
            meta.bytes = Some(bytes.len() as u64);
            ToolExecution {
                ok: true,
                content: json!({"path":full_path.display().to_string(),"content":content,"truncated":truncated,"max_read_bytes":rt.max_read_bytes,"read_bytes":bytes.len()}).to_string(),
                truncated,
                meta,
            }
        }
        Err(e) => failed_exec(
            SideEffects::FilesystemRead,
            format!("read_file failed for {}: {e}", full_path.display()),
        ),
    }
}

async fn run_shell(rt: &ToolRuntime, args: &Value) -> ToolExecution {
    if !rt.allow_shell && !rt.unsafe_bypass_allow_flags {
        return failed_exec(
            SideEffects::ShellExec,
            "shell tool is disabled. Re-run with --allow-shell".to_string(),
        );
    }
    let cmd = args.get("cmd").and_then(|v| v.as_str()).unwrap_or_default();
    let mut command = Command::new(cmd);
    if let Some(raw_args) = args.get("args").and_then(|v| v.as_array()) {
        for arg in raw_args {
            if let Some(s) = arg.as_str() {
                command.arg(s);
            }
        }
    }
    if let Some(cwd) = args.get("cwd").and_then(|v| v.as_str()) {
        command.current_dir(resolve_path(&rt.workdir, cwd));
    } else {
        command.current_dir(&rt.workdir);
    }
    match command.output().await {
        Ok(output) => {
            let stdout_raw = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr_raw = String::from_utf8_lossy(&output.stderr).to_string();
            let (stdout, stdout_truncated) =
                truncate_utf8_to_bytes(&stdout_raw, rt.max_tool_output_bytes);
            let (stderr, stderr_truncated) =
                truncate_utf8_to_bytes(&stderr_raw, rt.max_tool_output_bytes);
            let mut meta = base_meta(SideEffects::ShellExec);
            meta.exit_code = output.status.code();
            meta.bytes = Some((output.stdout.len() + output.stderr.len()) as u64);
            meta.stdout_truncated = Some(stdout_truncated);
            meta.stderr_truncated = Some(stderr_truncated);
            ToolExecution {
                ok: output.status.success(),
                content: json!({"status":output.status.code(),"stdout":stdout,"stderr":stderr,"stdout_truncated":stdout_truncated,"stderr_truncated":stderr_truncated,"max_tool_output_bytes":rt.max_tool_output_bytes}).to_string(),
                truncated: stdout_truncated || stderr_truncated,
                meta,
            }
        }
        Err(e) => failed_exec(
            SideEffects::ShellExec,
            format!("shell execution failed: {e}"),
        ),
    }
}

async fn run_write_file(rt: &ToolRuntime, args: &Value) -> ToolExecution {
    if !rt.allow_write && !rt.unsafe_bypass_allow_flags {
        return failed_exec(
            SideEffects::FilesystemWrite,
            "writes require --allow-write".to_string(),
        );
    }
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let content = args
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let create_parents = args
        .get("create_parents")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let full_path = resolve_path(&rt.workdir, path);
    if create_parents {
        if let Some(parent) = full_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return failed_exec(
                    SideEffects::FilesystemWrite,
                    format!("write_file failed for {}: {e}", full_path.display()),
                );
            }
        }
    }
    match tokio::fs::write(&full_path, content.as_bytes()).await {
        Ok(()) => {
            let mut meta = base_meta(SideEffects::FilesystemWrite);
            meta.bytes = Some(content.len() as u64);
            ToolExecution {
                ok: true,
                content:
                    json!({"path":full_path.display().to_string(),"bytes_written":content.len()})
                        .to_string(),
                truncated: false,
                meta,
            }
        }
        Err(e) => failed_exec(
            SideEffects::FilesystemWrite,
            format!("write_file failed for {}: {e}", full_path.display()),
        ),
    }
}

async fn run_apply_patch(rt: &ToolRuntime, args: &Value) -> ToolExecution {
    if !rt.allow_write && !rt.unsafe_bypass_allow_flags {
        return failed_exec(
            SideEffects::FilesystemWrite,
            "writes require --allow-write".to_string(),
        );
    }
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let patch_text = args
        .get("patch")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let full_path = resolve_path(&rt.workdir, path);
    let original_bytes = match tokio::fs::read(&full_path).await {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            return failed_exec(
                SideEffects::FilesystemWrite,
                format!("apply_patch failed for {}: {e}", full_path.display()),
            )
        }
    };
    let original_text = String::from_utf8_lossy(&original_bytes).to_string();
    let patch = match diffy::Patch::from_str(patch_text) {
        Ok(p) => p,
        Err(e) => return failed_exec(SideEffects::FilesystemWrite, format!("invalid patch: {e}")),
    };
    let patched = match diffy::apply(&original_text, &patch) {
        Ok(p) => p,
        Err(e) => {
            return failed_exec(
                SideEffects::FilesystemWrite,
                format!("failed to apply patch: {e}"),
            )
        }
    };
    if let Some(parent) = full_path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return failed_exec(
                SideEffects::FilesystemWrite,
                format!("apply_patch failed for {}: {e}", full_path.display()),
            );
        }
    }
    match tokio::fs::write(&full_path, patched.as_bytes()).await {
        Ok(()) => {
            let mut meta = base_meta(SideEffects::FilesystemWrite);
            meta.bytes = Some(patched.len() as u64);
            ToolExecution {
                ok: true,
                content: json!({"path":full_path.display().to_string(),"changed":patched!=original_text,"bytes_written":patched.len()}).to_string(),
                truncated: false,
                meta,
            }
        }
        Err(e) => failed_exec(
            SideEffects::FilesystemWrite,
            format!("apply_patch failed for {}: {e}", full_path.display()),
        ),
    }
}

fn base_meta(side_effects: SideEffects) -> ToolResultMeta {
    ToolResultMeta {
        side_effects,
        bytes: None,
        exit_code: None,
        stderr_truncated: None,
        stdout_truncated: None,
        source: "builtin".to_string(),
    }
}

fn failed_exec(side_effects: SideEffects, content: String) -> ToolExecution {
    ToolExecution {
        ok: false,
        content,
        truncated: false,
        meta: base_meta(side_effects),
    }
}

pub fn truncate_utf8_to_bytes(input: &str, max_bytes: usize) -> (String, bool) {
    if max_bytes == 0 {
        return (input.to_string(), false);
    }
    if input.len() <= max_bytes {
        return (input.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    (input[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::{json, Value};
    use tempfile::tempdir;

    use super::{
        builtin_tools_enabled, execute_tool, resolve_path, tool_side_effects,
        validate_builtin_tool_args, ToolArgsStrict, ToolRuntime,
    };
    use crate::types::{SideEffects, ToolCall};

    #[test]
    fn resolves_relative_path_from_workdir() {
        let base = PathBuf::from("some_workdir");
        let out = resolve_path(&base, "nested/file.txt");
        assert_eq!(out, base.join("nested/file.txt"));
    }

    #[test]
    fn write_tools_not_exposed_by_default() {
        let tools = builtin_tools_enabled(false);
        let names = tools.into_iter().map(|t| t.name).collect::<Vec<_>>();
        assert!(!names.iter().any(|n| n == "write_file"));
        assert!(!names.iter().any(|n| n == "apply_patch"));
    }

    #[test]
    fn side_effects_map_builtin_and_mcp() {
        assert_eq!(tool_side_effects("list_dir"), SideEffects::FilesystemRead);
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
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
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
            allow_write: true,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
        };
        let tc = ToolCall {
            id: "bad_w".to_string(),
            name: "write_file".to_string(),
            arguments: json!({"path":"x.txt"}),
        };
        let msg = execute_tool(&rt, &tc).await;
        let content = msg.content.unwrap_or_default();
        assert!(content.contains("invalid tool arguments"));
        assert!(!tmp.path().join("x.txt").exists());
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

    #[tokio::test]
    async fn apply_patch_updates_file() {
        let tmp = tempdir().expect("tempdir");
        let file = tmp.path().join("a.txt");
        tokio::fs::write(&file, "hello\n").await.expect("write");
        let rt = ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_write: true,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
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
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 5,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
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
}
