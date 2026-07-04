use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::store::{ensure_dir, sha256_hex, write_json_atomic};
use crate::tools::ToolResultContentRef;
use crate::types::{Message, ToolCall};

const OBSERVATION_SCHEMA_VERSION: &str = "openagent.observation.v1";
const TOOL_RESULT_ARTIFACT_MANIFEST_SCHEMA_VERSION: &str =
    "openagent.tool_result_artifacts_manifest.v1";
const MANIFEST_FILE_NAME: &str = "manifest.json";
const HEAD_BYTES: usize = 1200;
const TAIL_BYTES: usize = 4000;
const MAX_DIAGNOSTICS: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolResultArtifactManifestV1 {
    pub schema_version: String,
    pub artifacts: Vec<ToolResultArtifactManifestEntryV1>,
}

impl Default for ToolResultArtifactManifestV1 {
    fn default() -> Self {
        Self {
            schema_version: TOOL_RESULT_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
            artifacts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolResultArtifactManifestEntryV1 {
    pub sequence: u64,
    #[serde(rename = "ref")]
    pub artifact_ref: ToolResultContentRef,
    pub path: String,
    pub tool_name: String,
    pub tool_call_id: String,
    pub label: String,
    pub content_kind: String,
    pub size_bytes: u64,
    pub sha16: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolResultArtifactWarning {
    pub code: &'static str,
    pub message: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ToolResultArtifactResolveError {
    InvalidRefKind(String),
    PathTraversal(String),
    UnknownRef,
    InvalidManifestPath(String),
    Io(String),
    Parse(String),
}

impl fmt::Display for ToolResultArtifactResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRefKind(kind) => write!(f, "unsupported artifact ref kind: {kind}"),
            Self::PathTraversal(path) => write!(f, "artifact ref path is unsafe: {path}"),
            Self::UnknownRef => write!(f, "unknown tool-result artifact ref"),
            Self::InvalidManifestPath(path) => {
                write!(f, "manifest artifact path is unsafe: {path}")
            }
            Self::Io(err) => write!(f, "failed to read tool-result artifact manifest: {err}"),
            Self::Parse(err) => write!(f, "failed to parse tool-result artifact manifest: {err}"),
        }
    }
}

impl std::error::Error for ToolResultArtifactResolveError {}

#[derive(Debug, Clone, Serialize)]
struct ObservationV1 {
    schema_version: &'static str,
    tool_name: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stdout_head: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stdout_tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stderr_tail: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    changed_files: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<ObservationDiagnosticV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggested_next_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_result_ref: Option<ToolResultContentRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stdout_ref: Option<ToolResultContentRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stderr_ref: Option<ToolResultContentRef>,
    truncated: bool,
    truncation_policy: ObservationTruncationPolicyV1,
    raw_content_sha256: String,
    raw_content_bytes: usize,
    raw_content_truncated: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
enum ObservationTruncationPolicyV1 {
    #[serde(rename = "head_1200_tail_4000")]
    Head1200Tail4000,
}

#[derive(Debug, Clone, Serialize)]
struct ObservationDiagnosticV1 {
    stream: &'static str,
    line: String,
}

#[cfg(test)]
pub(crate) fn to_model_observation_message(
    raw_tool_msg: &Message,
    tc: &ToolCall,
    workdir: &Path,
    raw_tool_result_dir: Option<&Path>,
) -> Message {
    to_model_observation_message_with_warnings(raw_tool_msg, tc, workdir, raw_tool_result_dir).0
}

pub(crate) fn to_model_observation_message_with_warnings(
    raw_tool_msg: &Message,
    tc: &ToolCall,
    workdir: &Path,
    raw_tool_result_dir: Option<&Path>,
) -> (Message, Vec<ToolResultArtifactWarning>) {
    if tc.name.starts_with("mcp.") {
        return (raw_tool_msg.clone(), Vec::new());
    }
    let Some(raw_content) = raw_tool_msg.content.as_deref() else {
        return (raw_tool_msg.clone(), Vec::new());
    };
    let Ok(mut envelope) = serde_json::from_str::<Value>(raw_content) else {
        return (raw_tool_msg.clone(), Vec::new());
    };
    let mut warnings = Vec::new();
    let observation = build_observation(
        &envelope,
        tc,
        workdir,
        raw_content,
        raw_tool_result_dir,
        &mut warnings,
    );
    let Some(obj) = envelope.as_object_mut() else {
        return (raw_tool_msg.clone(), warnings);
    };
    let Ok(observation_value) = serde_json::to_value(&observation) else {
        return (raw_tool_msg.clone(), warnings);
    };
    let Ok(observation_text) = serde_json::to_string(&observation_value) else {
        return (raw_tool_msg.clone(), warnings);
    };
    obj.insert("content".to_string(), Value::String(observation_text));
    obj.insert("observation".to_string(), observation_value);
    let compact_content = match serde_json::to_string(&envelope) {
        Ok(content) => content,
        Err(_) => return (raw_tool_msg.clone(), warnings),
    };
    let mut out = raw_tool_msg.clone();
    out.content = Some(compact_content);
    (out, warnings)
}

pub(crate) fn set_tool_result_duration(message: &mut Message, duration_ms: u64) {
    let Some(content) = message.content.as_deref() else {
        return;
    };
    let Ok(mut envelope) = serde_json::from_str::<Value>(content) else {
        return;
    };
    if let Some(meta) = envelope.get_mut("meta").and_then(Value::as_object_mut) {
        meta.insert("duration_ms".to_string(), Value::from(duration_ms));
    }
    if let Ok(updated) = serde_json::to_string(&envelope) {
        message.content = Some(updated);
    }
}

fn build_observation(
    envelope: &Value,
    tc: &ToolCall,
    workdir: &Path,
    raw_content: &str,
    raw_tool_result_dir: Option<&Path>,
    warnings: &mut Vec<ToolResultArtifactWarning>,
) -> ObservationV1 {
    let ok = envelope.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let meta = envelope.get("meta").unwrap_or(&Value::Null);
    let raw_tool_content = envelope
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let inner = serde_json::from_str::<Value>(raw_tool_content).ok();
    let stdout = inner
        .as_ref()
        .and_then(|v| v.get("stdout"))
        .and_then(Value::as_str);
    let stderr = inner
        .as_ref()
        .and_then(|v| v.get("stderr"))
        .and_then(Value::as_str);
    let target_path = target_path(envelope, tc, inner.as_ref());
    let changed_files = changed_files(tc, target_path.as_deref(), inner.as_ref(), ok);
    let diagnostics = diagnostics(stdout, stderr);
    let failure_class = (!ok).then(|| failure_class(envelope, inner.as_ref()));
    let raw_content_truncated = envelope
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let stdout_truncated = stream_truncated(meta, inner.as_ref(), "stdout", stdout, HEAD_BYTES);
    let stderr_truncated = stream_truncated(meta, inner.as_ref(), "stderr", stderr, TAIL_BYTES);
    ObservationV1 {
        schema_version: OBSERVATION_SCHEMA_VERSION,
        tool_name: tc.name.clone(),
        status: if ok { "ok" } else { "failed" },
        exit_code: meta
            .get("exit_code")
            .and_then(Value::as_i64)
            .or_else(|| inner.as_ref()?.get("status")?.as_i64()),
        duration_ms: meta.get("duration_ms").and_then(Value::as_u64),
        cwd: shell_cwd(tc, workdir),
        target_path,
        command: shell_command(tc),
        stdout_head: stdout.and_then(|s| non_empty(head_utf8_bytes(s, HEAD_BYTES))),
        stdout_tail: stdout.and_then(|s| tail_if_useful(s, TAIL_BYTES)),
        stderr_tail: stderr.and_then(|s| non_empty(tail_utf8_bytes(s, TAIL_BYTES))),
        changed_files,
        diagnostics,
        suggested_next_action: suggested_next_action(envelope, inner.as_ref(), tc, ok),
        failure_class,
        raw_result_ref: spool_text_artifact(
            raw_tool_result_dir,
            tc,
            "raw_result",
            "json",
            "application/json",
            raw_content,
            warnings,
        ),
        stdout_ref: stdout.and_then(|s| {
            stdout_truncated.then(|| {
                spool_text_artifact(
                    raw_tool_result_dir,
                    tc,
                    "stdout",
                    "txt",
                    "text/plain",
                    s,
                    warnings,
                )
            })?
        }),
        stderr_ref: stderr.and_then(|s| {
            stderr_truncated.then(|| {
                spool_text_artifact(
                    raw_tool_result_dir,
                    tc,
                    "stderr",
                    "txt",
                    "text/plain",
                    s,
                    warnings,
                )
            })?
        }),
        truncated: raw_content_truncated || stdout_truncated || stderr_truncated,
        truncation_policy: ObservationTruncationPolicyV1::Head1200Tail4000,
        raw_content_sha256: sha256_hex(raw_content.as_bytes()),
        raw_content_bytes: raw_content.len(),
        raw_content_truncated,
    }
}

fn stream_truncated(
    meta: &Value,
    inner: Option<&Value>,
    stream: &str,
    value: Option<&str>,
    model_limit_bytes: usize,
) -> bool {
    let meta_key = format!("{stream}_truncated");
    meta.get(&meta_key)
        .and_then(Value::as_bool)
        .or_else(|| inner?.get(&meta_key).and_then(Value::as_bool))
        .unwrap_or(false)
        || value
            .map(|s| model_limit_bytes > 0 && s.len() > model_limit_bytes)
            .unwrap_or(false)
}

fn spool_text_artifact(
    artifact_dir: Option<&Path>,
    tc: &ToolCall,
    label: &str,
    extension: &str,
    content_kind: &str,
    content: &str,
    warnings: &mut Vec<ToolResultArtifactWarning>,
) -> Option<ToolResultContentRef> {
    let artifact_dir = artifact_dir?;
    if ensure_dir(artifact_dir).is_err() {
        warnings.push(ToolResultArtifactWarning {
            code: "tool_result_artifact_dir_create_failed",
            message: format!(
                "failed to create tool-result artifact directory {}",
                artifact_dir.display()
            ),
            path: Some(artifact_dir.display().to_string()),
        });
        return None;
    }
    let bytes = content.as_bytes();
    let sha256 = sha256_hex(bytes);
    let tool = sanitize_filename_component(&tc.name);
    let id = sanitize_filename_component(&tc.id);
    let label = sanitize_filename_component(label);
    let extension = sanitize_filename_component(extension);
    let file_name = format!("{tool}__{id}__{label}__{}.{}", &sha256[..16], extension);
    let path = artifact_dir.join(file_name);
    if let Err(err) = std::fs::write(&path, bytes) {
        warnings.push(ToolResultArtifactWarning {
            code: "tool_result_artifact_write_failed",
            message: format!(
                "failed to write tool-result artifact {}: {err}",
                path.display()
            ),
            path: Some(path.display().to_string()),
        });
        return None;
    }
    let artifact_ref = ToolResultContentRef {
        kind: "run_artifact_path".to_string(),
        path: path.to_string_lossy().to_string(),
        sha256,
        bytes: bytes.len() as u64,
    };
    if let Err(err) =
        record_tool_result_artifact(artifact_dir, tc, &label, content_kind, &path, &artifact_ref)
    {
        warnings.push(ToolResultArtifactWarning {
            code: "tool_result_artifact_manifest_write_failed",
            message: err,
            path: Some(manifest_path(artifact_dir).display().to_string()),
        });
    }
    Some(artifact_ref)
}

fn record_tool_result_artifact(
    artifact_dir: &Path,
    tc: &ToolCall,
    label: &str,
    content_kind: &str,
    artifact_path: &Path,
    artifact_ref: &ToolResultContentRef,
) -> Result<(), String> {
    let manifest_path = manifest_path(artifact_dir);
    let mut manifest = if manifest_path.exists() {
        let content = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("failed to read {}: {e}", manifest_path.display()))?;
        serde_json::from_str::<ToolResultArtifactManifestV1>(&content)
            .map_err(|e| format!("failed to parse {}: {e}", manifest_path.display()))?
    } else {
        ToolResultArtifactManifestV1::default()
    };
    if manifest
        .artifacts
        .iter()
        .any(|entry| entry.artifact_ref == *artifact_ref)
    {
        return Ok(());
    }
    let relative_path = artifact_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            format!(
                "artifact path has no valid file name: {}",
                artifact_path.display()
            )
        })?
        .to_string();
    let sequence = manifest.artifacts.len() as u64 + 1;
    manifest.artifacts.push(ToolResultArtifactManifestEntryV1 {
        sequence,
        artifact_ref: artifact_ref.clone(),
        path: relative_path,
        tool_name: tc.name.clone(),
        tool_call_id: tc.id.clone(),
        label: label.to_string(),
        content_kind: content_kind.to_string(),
        size_bytes: artifact_ref.bytes,
        sha16: artifact_ref.sha256[..16].to_string(),
        sha256: artifact_ref.sha256.clone(),
    });
    write_json_atomic(&manifest_path, &manifest)
        .map_err(|e| format!("failed to write {}: {e}", manifest_path.display()))
}

fn manifest_path(artifact_dir: &Path) -> PathBuf {
    artifact_dir.join(MANIFEST_FILE_NAME)
}

#[allow(dead_code)]
pub fn load_tool_result_artifact_manifest(
    artifact_dir: &Path,
) -> Result<ToolResultArtifactManifestV1, ToolResultArtifactResolveError> {
    let path = manifest_path(artifact_dir);
    let content = std::fs::read_to_string(&path)
        .map_err(|e| ToolResultArtifactResolveError::Io(e.to_string()))?;
    serde_json::from_str::<ToolResultArtifactManifestV1>(&content)
        .map_err(|e| ToolResultArtifactResolveError::Parse(e.to_string()))
}

#[allow(dead_code)]
pub fn resolve_tool_result_artifact_ref(
    artifact_dir: &Path,
    artifact_ref: &ToolResultContentRef,
) -> Result<PathBuf, ToolResultArtifactResolveError> {
    if artifact_ref.kind != "run_artifact_path" {
        return Err(ToolResultArtifactResolveError::InvalidRefKind(
            artifact_ref.kind.clone(),
        ));
    }
    reject_traversal_path(&artifact_ref.path)?;
    let manifest = load_tool_result_artifact_manifest(artifact_dir)?;
    let entry = manifest
        .artifacts
        .iter()
        .find(|entry| entry.artifact_ref == *artifact_ref)
        .ok_or(ToolResultArtifactResolveError::UnknownRef)?;
    let relative = safe_manifest_relative_path(&entry.path)?;
    let resolved = artifact_dir.join(relative);
    if resolved.exists() {
        let root = std::fs::canonicalize(artifact_dir)
            .map_err(|e| ToolResultArtifactResolveError::Io(e.to_string()))?;
        let candidate = std::fs::canonicalize(&resolved)
            .map_err(|e| ToolResultArtifactResolveError::Io(e.to_string()))?;
        if !candidate.starts_with(&root) {
            return Err(ToolResultArtifactResolveError::InvalidManifestPath(
                entry.path.clone(),
            ));
        }
        return Ok(candidate);
    }
    Ok(resolved)
}

#[allow(dead_code)]
fn reject_traversal_path(path: &str) -> Result<(), ToolResultArtifactResolveError> {
    if Path::new(path)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(ToolResultArtifactResolveError::PathTraversal(
            path.to_string(),
        ));
    }
    Ok(())
}

#[allow(dead_code)]
fn safe_manifest_relative_path(path: &str) -> Result<PathBuf, ToolResultArtifactResolveError> {
    let path_buf = PathBuf::from(path);
    if path_buf.components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        )
    }) {
        return Err(ToolResultArtifactResolveError::InvalidManifestPath(
            path.to_string(),
        ));
    }
    Ok(path_buf)
}

fn sanitize_filename_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        let ok = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_');
        out.push(if ok { ch } else { '_' });
    }
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

fn shell_cwd(tc: &ToolCall, workdir: &Path) -> Option<String> {
    if tc.name != "shell" {
        return None;
    }
    tc.arguments
        .get("cwd")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| Some(workdir.display().to_string()))
}

fn shell_command(tc: &ToolCall) -> Option<String> {
    if tc.name != "shell" {
        return None;
    }
    if let Some(command) = tc.arguments.get("command").and_then(Value::as_str) {
        return Some(command.to_string());
    }
    let cmd = tc.arguments.get("cmd").and_then(Value::as_str)?;
    let mut parts = vec![cmd.to_string()];
    if let Some(args) = tc.arguments.get("args").and_then(Value::as_array) {
        for arg in args {
            if let Some(arg) = arg.as_str() {
                parts.push(arg.to_string());
            }
        }
    }
    Some(parts.join(" "))
}

fn target_path(envelope: &Value, tc: &ToolCall, inner: Option<&Value>) -> Option<String> {
    tc.arguments
        .get("path")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            inner?
                .get("path")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            envelope
                .get("error")
                .and_then(|e| e.get("received_args"))
                .and_then(|a| a.get("path"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn changed_files(
    tc: &ToolCall,
    path: Option<&str>,
    inner: Option<&Value>,
    ok: bool,
) -> Vec<String> {
    if !ok
        || !matches!(
            tc.name.as_str(),
            "apply_patch" | "edit" | "write_file" | "str_replace"
        )
    {
        return Vec::new();
    }
    if inner
        .and_then(|v| v.get("changed"))
        .and_then(Value::as_bool)
        .is_some_and(|changed| !changed)
    {
        return Vec::new();
    }
    path.map(|p| vec![p.to_string()]).unwrap_or_default()
}

fn diagnostics(stdout: Option<&str>, stderr: Option<&str>) -> Vec<ObservationDiagnosticV1> {
    let mut out = Vec::new();
    collect_diagnostics("stdout", stdout.unwrap_or_default(), &mut out);
    collect_diagnostics("stderr", stderr.unwrap_or_default(), &mut out);
    out
}

fn collect_diagnostics(stream: &'static str, text: &str, out: &mut Vec<ObservationDiagnosticV1>) {
    for line in text.lines() {
        if out.len() >= MAX_DIAGNOSTICS {
            return;
        }
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        let is_diagnostic = lower.starts_with("error")
            || lower.starts_with("failed")
            || lower.starts_with("not ok ")
            || lower.contains(" assertion")
            || lower.contains(" panicked")
            || lower.contains("panic")
            || lower.contains("exception")
            || lower.contains("traceback")
            || lower.contains("expected")
            || lower.contains("mismatched types")
            || lower.contains("cannot find");
        if is_diagnostic && !trimmed.is_empty() {
            out.push(ObservationDiagnosticV1 {
                stream,
                line: truncate_chars(trimmed, 500),
            });
        }
    }
}

fn failure_class(envelope: &Value, inner: Option<&Value>) -> String {
    let code = envelope
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    match code {
        "tool_args_invalid" | "tool_args_malformed_json" => "E_SCHEMA".to_string(),
        "tool_path_denied" | "path_out_of_scope" | "tool_disabled" | "shell_gate_deny" => {
            "E_POLICY".to_string()
        }
        "shell_exec_timeout" => "E_TIMEOUT".to_string(),
        "shell_exec_not_found" => "E_EXEC_NOT_FOUND".to_string(),
        "shell_exec_non_zero_exit" => "E_NONZERO_EXIT".to_string(),
        _ => {
            if inner
                .and_then(|v| v.get("timed_out"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "E_TIMEOUT".to_string()
            } else {
                "E_TOOL_FAILED".to_string()
            }
        }
    }
}

fn suggested_next_action(
    envelope: &Value,
    inner: Option<&Value>,
    tc: &ToolCall,
    ok: bool,
) -> Option<String> {
    if ok {
        return None;
    }
    let code = envelope
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if code == "tool_args_invalid" || code == "tool_args_malformed_json" {
        return Some("Emit a corrected tool call with valid arguments.".to_string());
    }
    if code == "tool_path_denied" || code == "path_out_of_scope" {
        return Some(
            "Use a workdir-relative path and avoid absolute paths or '..' traversal.".to_string(),
        );
    }
    if inner
        .and_then(|v| v.get("timed_out"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || code == "shell_exec_timeout"
    {
        return Some(
            "Narrow the command, make it non-interactive, or rerun with a larger timeout_ms."
                .to_string(),
        );
    }
    match tc.name.as_str() {
        "shell" => Some(
            "Inspect the failing diagnostics, edit the relevant source, then rerun the command."
                .to_string(),
        ),
        "apply_patch" | "edit" | "str_replace" => Some(
            "Re-read the current file, adjust the edit to match the actual content, then retry."
                .to_string(),
        ),
        "read_file" | "grep" | "glob" | "list_dir" => Some(
            "Check the path or query shape, then retry with a scoped workdir-relative target."
                .to_string(),
        ),
        _ => None,
    }
}

fn head_utf8_bytes(input: &str, max_bytes: usize) -> String {
    truncate_utf8_prefix(input, max_bytes).0
}

fn tail_if_useful(input: &str, max_bytes: usize) -> Option<String> {
    if input.len() <= HEAD_BYTES {
        return None;
    }
    non_empty(tail_utf8_bytes(input, max_bytes))
}

fn tail_utf8_bytes(input: &str, max_bytes: usize) -> String {
    if max_bytes == 0 || input.len() <= max_bytes {
        return input.to_string();
    }
    let mut start = input.len().saturating_sub(max_bytes);
    while start < input.len() && !input.is_char_boundary(start) {
        start += 1;
    }
    input[start..].to_string()
}

fn truncate_utf8_prefix(input: &str, max_bytes: usize) -> (String, bool) {
    if max_bytes == 0 || input.len() <= max_bytes {
        return (input.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    (input[..end].to_string(), true)
}

fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    input.chars().take(max_chars).collect()
}
