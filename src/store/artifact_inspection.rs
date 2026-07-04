use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::{
    load_tool_result_artifact_manifest, resolve_tool_result_artifact_ref, ToolResultContentRef,
};
use crate::types::Role;

use super::RunRecord;

const OBSERVATION_SCHEMA_VERSION: &str = "openagent.observation.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayArtifactRefSummary {
    pub run_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub label: String,
    #[serde(rename = "ref")]
    pub artifact_ref: ToolResultContentRef,
    pub ref_value: String,
    pub size_bytes: Option<u64>,
    pub sha16: Option<String>,
    pub sha256: Option<String>,
    pub relative_path: Option<String>,
    pub resolved_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayArtifactList {
    pub run_id: String,
    pub artifact_dir: String,
    pub artifacts: Vec<ReplayArtifactRefSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayArtifactInspection {
    pub summary: ReplayArtifactRefSummary,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayArtifactInspectionError {
    MissingManifest {
        path: String,
    },
    UnknownRef {
        artifact_ref: String,
    },
    UnsafeRef {
        artifact_ref: String,
        reason: String,
    },
    InvalidRefKind {
        kind: String,
    },
    MalformedRef {
        artifact_ref: String,
        error: String,
    },
    MissingArtifactFile {
        path: String,
    },
    Io {
        path: String,
        error: String,
    },
}

impl fmt::Display for ReplayArtifactInspectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingManifest { path } => {
                write!(f, "missing tool-result artifact manifest: {path}")
            }
            Self::UnknownRef { artifact_ref } => {
                write!(f, "unknown tool-result artifact ref: {artifact_ref}")
            }
            Self::UnsafeRef {
                artifact_ref,
                reason,
            } => write!(
                f,
                "unsafe tool-result artifact ref '{artifact_ref}': {reason}"
            ),
            Self::InvalidRefKind { kind } => {
                write!(f, "invalid tool-result artifact ref kind: {kind}")
            }
            Self::MalformedRef {
                artifact_ref,
                error,
            } => write!(
                f,
                "malformed tool-result artifact ref '{artifact_ref}': {error}"
            ),
            Self::MissingArtifactFile { path } => {
                write!(f, "tool-result artifact file missing: {path}")
            }
            Self::Io { path, error } => write!(f, "failed to read artifact {path}: {error}"),
        }
    }
}

impl std::error::Error for ReplayArtifactInspectionError {}

fn run_tool_result_artifact_dir(state_dir: &Path, run_id: &str) -> PathBuf {
    state_dir
        .join("runs")
        .join(format!("{run_id}.tool-results"))
}

pub fn list_replay_artifacts(
    state_dir: &Path,
    record: &RunRecord,
) -> Result<ReplayArtifactList, ReplayArtifactInspectionError> {
    let artifact_dir = run_tool_result_artifact_dir(state_dir, &record.metadata.run_id);
    let extracted = extract_observation_artifact_refs(record);
    if extracted.is_empty() {
        return Ok(ReplayArtifactList {
            run_id: record.metadata.run_id.clone(),
            artifact_dir: artifact_dir.display().to_string(),
            artifacts: Vec::new(),
        });
    }

    let manifest_path = artifact_dir.join("manifest.json");
    if !manifest_path.exists() {
        return Err(ReplayArtifactInspectionError::MissingManifest {
            path: manifest_path.display().to_string(),
        });
    }
    let manifest = load_tool_result_artifact_manifest(&artifact_dir).map_err(|err| {
        ReplayArtifactInspectionError::Io {
            path: manifest_path.display().to_string(),
            error: err.to_string(),
        }
    })?;

    let mut artifacts = Vec::new();
    for extracted_ref in extracted {
        let entry = manifest
            .artifacts
            .iter()
            .find(|entry| entry.artifact_ref == extracted_ref.artifact_ref)
            .ok_or_else(|| ReplayArtifactInspectionError::UnknownRef {
                artifact_ref: extracted_ref.artifact_ref.path.clone(),
            })?;
        let resolved = resolve_ref_for_replay(&artifact_dir, &extracted_ref.artifact_ref)
            .map_err(|err| map_resolve_error(&extracted_ref.artifact_ref.path, err))?;
        ensure_artifact_file_exists(&resolved)?;
        artifacts.push(ReplayArtifactRefSummary {
            run_id: record.metadata.run_id.clone(),
            tool_call_id: extracted_ref.tool_call_id,
            tool_name: extracted_ref.tool_name,
            label: extracted_ref.label,
            ref_value: extracted_ref.artifact_ref.path.clone(),
            artifact_ref: extracted_ref.artifact_ref,
            size_bytes: Some(entry.size_bytes),
            sha16: Some(entry.sha16.clone()),
            sha256: Some(entry.sha256.clone()),
            relative_path: Some(entry.path.clone()),
            resolved_path: resolved.display().to_string(),
        });
    }

    Ok(ReplayArtifactList {
        run_id: record.metadata.run_id.clone(),
        artifact_dir: artifact_dir.display().to_string(),
        artifacts,
    })
}

pub fn inspect_replay_artifact(
    state_dir: &Path,
    record: &RunRecord,
    artifact_ref_arg: &str,
) -> Result<ReplayArtifactInspection, ReplayArtifactInspectionError> {
    let artifact_dir = run_tool_result_artifact_dir(state_dir, &record.metadata.run_id);
    if let Some(parsed_ref) = parse_ref_object(artifact_ref_arg)? {
        let summary = summary_for_explicit_ref(state_dir, record, &artifact_dir, &parsed_ref)?;
        let content = read_artifact_content(Path::new(&summary.resolved_path))?;
        return Ok(ReplayArtifactInspection { summary, content });
    }

    let list = list_replay_artifacts(state_dir, record)?;
    if let Some(summary) = list
        .artifacts
        .into_iter()
        .find(|artifact| artifact_matches_arg(artifact, artifact_ref_arg))
    {
        let content = read_artifact_content(Path::new(&summary.resolved_path))?;
        return Ok(ReplayArtifactInspection { summary, content });
    }

    let unresolved_probe = ToolResultContentRef {
        kind: "run_artifact_path".to_string(),
        path: artifact_ref_arg.to_string(),
        sha256: String::new(),
        bytes: 0,
    };
    match resolve_ref_for_replay(&artifact_dir, &unresolved_probe) {
        Ok(path) => {
            if path.exists() {
                Err(ReplayArtifactInspectionError::UnknownRef {
                    artifact_ref: artifact_ref_arg.to_string(),
                })
            } else {
                Err(ReplayArtifactInspectionError::MissingArtifactFile {
                    path: path.display().to_string(),
                })
            }
        }
        Err(err) => Err(map_resolve_error(artifact_ref_arg, err)),
    }
}

pub fn render_replay_artifact_list(list: &ReplayArtifactList) -> String {
    let mut out = String::new();
    out.push_str(&format!("run_id: {}\n", list.run_id));
    out.push_str(&format!("artifact_dir: {}\n", list.artifact_dir));
    out.push_str(&format!("artifacts: {}\n", list.artifacts.len()));
    for artifact in &list.artifacts {
        out.push_str(&format!(
            "- tool_call_id={} tool={} label={} size_bytes={} sha16={} sha256={} ref={} path={}\n",
            artifact.tool_call_id,
            artifact.tool_name,
            artifact.label,
            artifact
                .size_bytes
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
            artifact.sha16.as_deref().unwrap_or("-"),
            artifact.sha256.as_deref().unwrap_or("-"),
            artifact.ref_value,
            artifact.resolved_path
        ));
    }
    out
}

pub fn render_replay_artifact_inspection(inspection: &ReplayArtifactInspection) -> String {
    let artifact = &inspection.summary;
    let mut out = String::new();
    out.push_str("artifact:\n");
    out.push_str(&format!("  tool_call_id: {}\n", artifact.tool_call_id));
    out.push_str(&format!("  tool: {}\n", artifact.tool_name));
    out.push_str(&format!("  label: {}\n", artifact.label));
    out.push_str(&format!("  ref: {}\n", artifact.ref_value));
    out.push_str(&format!(
        "  size_bytes: {}\n",
        artifact
            .size_bytes
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string())
    ));
    out.push_str(&format!(
        "  sha16: {}\n",
        artifact.sha16.as_deref().unwrap_or("-")
    ));
    out.push_str(&format!(
        "  sha256: {}\n",
        artifact.sha256.as_deref().unwrap_or("-")
    ));
    out.push_str(&format!("  path: {}\n", artifact.resolved_path));
    out.push_str("content:\n");
    out.push_str(&inspection.content);
    if !inspection.content.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[derive(Debug, Clone)]
struct ExtractedArtifactRef {
    tool_call_id: String,
    tool_name: String,
    label: String,
    artifact_ref: ToolResultContentRef,
}

fn extract_observation_artifact_refs(record: &RunRecord) -> Vec<ExtractedArtifactRef> {
    let mut out = Vec::new();
    for message in &record.transcript {
        if !matches!(message.role, Role::Tool) {
            continue;
        }
        let Some(content) = message.content.as_deref() else {
            continue;
        };
        let Ok(envelope) = serde_json::from_str::<Value>(content) else {
            continue;
        };
        let Some(observation) = envelope.get("observation").and_then(Value::as_object) else {
            continue;
        };
        if observation.get("schema_version").and_then(Value::as_str)
            != Some(OBSERVATION_SCHEMA_VERSION)
        {
            continue;
        }
        let tool_call_id = message
            .tool_call_id
            .clone()
            .or_else(|| {
                envelope
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| "unknown".to_string());
        let tool_name = message
            .tool_name
            .clone()
            .or_else(|| {
                observation
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .or_else(|| {
                envelope
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| "unknown".to_string());
        for (label, key) in [
            ("raw_result", "raw_result_ref"),
            ("stdout", "stdout_ref"),
            ("stderr", "stderr_ref"),
        ] {
            let Some(value) = observation.get(key) else {
                continue;
            };
            if let Ok(artifact_ref) = serde_json::from_value::<ToolResultContentRef>(value.clone())
            {
                out.push(ExtractedArtifactRef {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    label: label.to_string(),
                    artifact_ref,
                });
            }
        }
    }
    out
}

fn parse_ref_object(
    raw: &str,
) -> Result<Option<ToolResultContentRef>, ReplayArtifactInspectionError> {
    if !raw.trim_start().starts_with('{') {
        return Ok(None);
    }
    serde_json::from_str::<ToolResultContentRef>(raw)
        .map(Some)
        .map_err(|err| ReplayArtifactInspectionError::MalformedRef {
            artifact_ref: raw.to_string(),
            error: err.to_string(),
        })
}

fn summary_for_explicit_ref(
    state_dir: &Path,
    record: &RunRecord,
    artifact_dir: &Path,
    artifact_ref: &ToolResultContentRef,
) -> Result<ReplayArtifactRefSummary, ReplayArtifactInspectionError> {
    let resolved = resolve_ref_for_replay(artifact_dir, artifact_ref)
        .map_err(|err| map_resolve_error(&artifact_ref.path, err))?;
    ensure_artifact_file_exists(&resolved)?;
    let list = list_replay_artifacts(state_dir, record)?;
    list.artifacts
        .into_iter()
        .find(|summary| summary.artifact_ref == *artifact_ref)
        .ok_or_else(|| ReplayArtifactInspectionError::UnknownRef {
            artifact_ref: artifact_ref.path.clone(),
        })
}

fn artifact_matches_arg(artifact: &ReplayArtifactRefSummary, raw: &str) -> bool {
    artifact.ref_value == raw
        || artifact.relative_path.as_deref() == Some(raw)
        || artifact.sha16.as_deref() == Some(raw)
        || artifact.sha256.as_deref() == Some(raw)
}

fn resolve_ref_for_replay(
    artifact_dir: &Path,
    artifact_ref: &ToolResultContentRef,
) -> Result<PathBuf, crate::tools::ToolResultArtifactResolveError> {
    resolve_tool_result_artifact_ref(artifact_dir, artifact_ref)
}

fn ensure_artifact_file_exists(path: &Path) -> Result<(), ReplayArtifactInspectionError> {
    if path.is_file() {
        Ok(())
    } else {
        Err(ReplayArtifactInspectionError::MissingArtifactFile {
            path: path.display().to_string(),
        })
    }
}

fn read_artifact_content(path: &Path) -> Result<String, ReplayArtifactInspectionError> {
    ensure_artifact_file_exists(path)?;
    std::fs::read_to_string(path).map_err(|err| ReplayArtifactInspectionError::Io {
        path: path.display().to_string(),
        error: err.to_string(),
    })
}

fn map_resolve_error(
    artifact_ref: &str,
    err: crate::tools::ToolResultArtifactResolveError,
) -> ReplayArtifactInspectionError {
    match err {
        crate::tools::ToolResultArtifactResolveError::InvalidRefKind(kind) => {
            ReplayArtifactInspectionError::InvalidRefKind { kind }
        }
        crate::tools::ToolResultArtifactResolveError::PathTraversal(path) => {
            ReplayArtifactInspectionError::UnsafeRef {
                artifact_ref: path,
                reason: "path traversal is not allowed".to_string(),
            }
        }
        crate::tools::ToolResultArtifactResolveError::UnknownRef => {
            ReplayArtifactInspectionError::UnknownRef {
                artifact_ref: artifact_ref.to_string(),
            }
        }
        crate::tools::ToolResultArtifactResolveError::InvalidManifestPath(path) => {
            ReplayArtifactInspectionError::UnsafeRef {
                artifact_ref: path,
                reason: "manifest path is outside the artifact directory".to_string(),
            }
        }
        crate::tools::ToolResultArtifactResolveError::Io(error)
        | crate::tools::ToolResultArtifactResolveError::Parse(error) => {
            ReplayArtifactInspectionError::Io {
                path: artifact_ref.to_string(),
                error,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use tempfile::tempdir;

    use super::{
        inspect_replay_artifact, list_replay_artifacts, render_replay_artifact_inspection,
        render_replay_artifact_list, ReplayArtifactInspectionError,
    };
    use crate::agent::{AgentExitReason, AgentOutcome};
    use crate::compaction::{CompactionMode, CompactionSettings, ToolResultPersist};
    use crate::store::{
        load_run_record, resolve_state_paths, write_run_record, PolicyRecordInfo, RunCliConfig,
    };
    use crate::tools::{
        envelope_to_message, to_model_observation_message, to_tool_result_envelope_with_error,
        ToolErrorCode, ToolErrorDetail, ToolResultMeta,
    };
    use crate::types::{Message, Role, SideEffects, ToolCall};

    fn test_cli_config() -> RunCliConfig {
        RunCliConfig {
            mode: "single".to_string(),
            agent_mode: "build".to_string(),
            output_mode: "human".to_string(),
            provider: "mock".to_string(),
            base_url: "mock://local".to_string(),
            model: "mock-model".to_string(),
            temperature: None,
            top_p: None,
            max_tokens: None,
            seed: None,
            planner_model: None,
            worker_model: None,
            planner_max_steps: None,
            planner_output: None,
            planner_strict: None,
            enforce_plan_tools: "off".to_string(),
            mcp_pin_enforcement: "hard".to_string(),
            trust_mode: "off".to_string(),
            allow_shell: false,
            allow_write: false,
            enable_write_tools: false,
            exec_target: "host".to_string(),
            docker_image: None,
            docker_workdir: None,
            docker_network: None,
            docker_user: None,
            docker_config_summary: None,
            max_tool_output_bytes: 0,
            max_read_bytes: 0,
            max_wall_time_ms: 0,
            max_total_tool_calls: 0,
            max_mcp_calls: 0,
            max_filesystem_read_calls: 0,
            max_filesystem_write_calls: 0,
            max_shell_calls: 0,
            max_network_calls: 0,
            max_browser_calls: 0,
            tool_exec_timeout_ms: 0,
            post_write_verify_timeout_ms: 0,
            approval_mode: "interrupt".to_string(),
            auto_approve_scope: "run".to_string(),
            approval_key: "v1".to_string(),
            unsafe_mode: false,
            no_limits: false,
            unsafe_bypass_allow_flags: false,
            stream: false,
            events_path: None,
            max_context_chars: 0,
            compaction_mode: "off".to_string(),
            compaction_keep_last: 0,
            tool_result_persist: "digest".to_string(),
            hooks_mode: "off".to_string(),
            caps_mode: "off".to_string(),
            hooks_config_path: String::new(),
            hooks_strict: false,
            hooks_timeout_ms: 0,
            hooks_max_stdout_bytes: 0,
            tool_args_strict: "on".to_string(),
            taint: "off".to_string(),
            taint_mode: "propagate".to_string(),
            taint_digest_bytes: 4096,
            repro: "off".to_string(),
            repro_env: "safe".to_string(),
            repro_out: None,
            use_session_settings: false,
            resolved_settings_source: BTreeMap::new(),
            tui_enabled: false,
            tui_refresh_ms: 0,
            tui_max_log_lines: 0,
            http_max_retries: 0,
            http_timeout_ms: 0,
            http_connect_timeout_ms: 0,
            http_stream_idle_timeout_ms: 0,
            http_max_response_bytes: 0,
            http_max_line_bytes: 0,
            tool_catalog: Vec::new(),
            mcp_tool_snapshot: Vec::new(),
            mcp_tool_catalog_hash_hex: None,
            mcp_servers: Vec::new(),
            mcp_config_path: None,
            policy_version: None,
            includes_resolved: Vec::new(),
            mcp_allowlist: None,
            instructions_config_path: None,
            instructions_config_hash_hex: None,
            instruction_model_profile: None,
            instruction_task_profile: None,
            instruction_task_profile_task_kind: None,
            instruction_message_count: 0,
            project_guidance_hash_hex: None,
            project_guidance_sources: Vec::new(),
            project_guidance_truncated: false,
            project_guidance_bytes_loaded: 0,
            project_guidance_bytes_kept: 0,
            repo_map_hash_hex: None,
            repo_map_format: None,
            repo_map_truncated: false,
            repo_map_truncated_reason: None,
            repo_map_bytes_scanned: 0,
            repo_map_bytes_kept: 0,
            repo_map_file_count_included: 0,
            repo_map_injected: false,
            repo_map_likely_target_files_count: 0,
            lsp_context_provider: None,
            lsp_context_schema_version: None,
            lsp_context_truncated: false,
            lsp_context_truncation_reason: None,
            lsp_context_bytes_kept: 0,
            lsp_context_diagnostics_included: 0,
            lsp_context_symbol_query: None,
            lsp_context_symbols_included: 0,
            lsp_context_definitions_included: 0,
            lsp_context_references_included: 0,
            lsp_context_injected: false,
            lsp_context_likely_target_files_count: 0,
            active_profile: None,
            profile_source: None,
            profile_hash_hex: None,
            activated_packs: Vec::new(),
        }
    }

    fn write_record_with_messages(
        run_id: &str,
        messages: Vec<Message>,
    ) -> (tempfile::TempDir, crate::store::StatePaths) {
        let tmp = tempdir().expect("tempdir");
        let paths = resolve_state_paths(tmp.path(), None, None, None, None);
        let outcome = AgentOutcome {
            run_id: run_id.to_string(),
            started_at: "2026-01-01T00:00:00Z".to_string(),
            finished_at: "2026-01-01T00:00:01Z".to_string(),
            exit_reason: AgentExitReason::Ok,
            final_output: "done".to_string(),
            error: None,
            messages,
            tool_calls: Vec::new(),
            tool_decisions: Vec::new(),
            compaction_settings: CompactionSettings {
                max_context_chars: 0,
                mode: CompactionMode::Off,
                keep_last: 20,
                tool_result_persist: ToolResultPersist::Digest,
            },
            final_prompt_size_chars: 0,
            compaction_report: None,
            hook_invocations: Vec::new(),
            provider_retry_count: 0,
            provider_error_count: 0,
            token_usage: None,
            taint: None,
        };
        write_run_record(
            &paths,
            test_cli_config(),
            PolicyRecordInfo {
                source: "none".to_string(),
                hash_hex: None,
                version: None,
                includes_resolved: Vec::new(),
                mcp_allowlist: None,
            },
            "config".to_string(),
            &outcome,
            crate::planner::RunMode::Single,
            None,
            None,
            BTreeMap::new(),
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
            Vec::new(),
            None,
        )
        .expect("write run record");
        (tmp, paths)
    }

    fn persisted_record_with_artifacts() -> (tempfile::TempDir, crate::store::StatePaths, String) {
        let tmp = tempdir().expect("tempdir");
        let paths = resolve_state_paths(tmp.path(), None, None, None, None);
        let run_id = "run_artifacts";
        let tc = ToolCall {
            id: "tc_shell".to_string(),
            name: "shell".to_string(),
            arguments: json!({"command":"cargo test"}),
        };
        let raw_inner = json!({
            "status": 1,
            "stdout": format!("{}\nstdout final", "stdout noise\n".repeat(500)),
            "stderr": format!("{}\nstderr final", "stderr noise\n".repeat(500)),
            "stdout_truncated": false,
            "stderr_truncated": false
        })
        .to_string();
        let raw_msg = envelope_to_message(to_tool_result_envelope_with_error(
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
                duration_ms: Some(10),
                bytes: None,
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
        let artifact_dir = paths.runs_dir.join(format!("{run_id}.tool-results"));
        let compact =
            to_model_observation_message(&raw_msg, &tc, tmp.path(), Some(artifact_dir.as_path()));
        let outcome = AgentOutcome {
            run_id: run_id.to_string(),
            started_at: "2026-01-01T00:00:00Z".to_string(),
            finished_at: "2026-01-01T00:00:01Z".to_string(),
            exit_reason: AgentExitReason::Ok,
            final_output: "done".to_string(),
            error: None,
            messages: vec![compact],
            tool_calls: vec![tc],
            tool_decisions: Vec::new(),
            compaction_settings: CompactionSettings {
                max_context_chars: 0,
                mode: CompactionMode::Off,
                keep_last: 20,
                tool_result_persist: ToolResultPersist::Digest,
            },
            final_prompt_size_chars: 0,
            compaction_report: None,
            hook_invocations: Vec::new(),
            provider_retry_count: 0,
            provider_error_count: 0,
            token_usage: None,
            taint: None,
        };
        write_run_record(
            &paths,
            test_cli_config(),
            PolicyRecordInfo {
                source: "none".to_string(),
                hash_hex: None,
                version: None,
                includes_resolved: Vec::new(),
                mcp_allowlist: None,
            },
            "config".to_string(),
            &outcome,
            crate::planner::RunMode::Single,
            None,
            None,
            BTreeMap::new(),
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
            Vec::new(),
            None,
        )
        .expect("write run record");
        (tmp, paths, run_id.to_string())
    }

    #[test]
    fn replay_artifacts_discovers_refs_from_persisted_run_record() {
        let (_tmp, paths, run_id) = persisted_record_with_artifacts();
        let record = load_run_record(&paths.state_dir, &run_id).expect("load record");
        let list = list_replay_artifacts(&paths.state_dir, &record).expect("list");
        assert_eq!(list.artifacts.len(), 3);
        let labels = list
            .artifacts
            .iter()
            .map(|a| a.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["raw_result", "stdout", "stderr"]);
        let text = render_replay_artifact_list(&list);
        assert!(text.contains("tool_call_id=tc_shell"));
        assert!(text.contains("label=raw_result"));
        assert!(text.contains("size_bytes="));
        assert!(text.contains("sha16="));
        assert!(text.contains("sha256="));
        assert!(text.contains("ref="));
        assert!(serde_json::to_string_pretty(&list)
            .expect("json")
            .contains("\"label\": \"stdout\""));
    }

    #[test]
    fn replay_artifact_resolves_valid_ref_and_reads_content() {
        let (_tmp, paths, run_id) = persisted_record_with_artifacts();
        let record = load_run_record(&paths.state_dir, &run_id).expect("load record");
        let list = list_replay_artifacts(&paths.state_dir, &record).expect("list");
        let raw = list
            .artifacts
            .iter()
            .find(|a| a.label == "raw_result")
            .expect("raw");
        let inspection =
            inspect_replay_artifact(&paths.state_dir, &record, &raw.ref_value).expect("inspect");
        assert_eq!(inspection.summary.label, "raw_result");
        assert!(inspection.content.contains("openagent.tool_result.v1"));
        assert_eq!(
            inspection.summary.resolved_path,
            std::fs::canonicalize(&inspection.summary.resolved_path)
                .expect("canonical")
                .display()
                .to_string()
        );
        let rendered = render_replay_artifact_inspection(&inspection);
        assert!(rendered.contains("content:\n"));
        assert!(rendered.contains("label: raw_result"));
    }

    #[test]
    fn replay_artifact_path_only_value_is_resolved_safe_path() {
        let (_tmp, paths, run_id) = persisted_record_with_artifacts();
        let record = load_run_record(&paths.state_dir, &run_id).expect("load record");
        let list = list_replay_artifacts(&paths.state_dir, &record).expect("list");
        let stdout = list
            .artifacts
            .iter()
            .find(|a| a.label == "stdout")
            .expect("stdout");
        let inspection =
            inspect_replay_artifact(&paths.state_dir, &record, &stdout.ref_value).expect("inspect");
        assert_eq!(inspection.summary.resolved_path, stdout.resolved_path);
        assert!(inspection.summary.resolved_path.ends_with(".txt"));
    }

    #[test]
    fn replay_artifact_unknown_and_traversal_refs_fail_clearly() {
        let (_tmp, paths, run_id) = persisted_record_with_artifacts();
        let record = load_run_record(&paths.state_dir, &run_id).expect("load record");
        let unknown =
            inspect_replay_artifact(&paths.state_dir, &record, "not-a-known-ref").unwrap_err();
        assert!(matches!(
            unknown,
            ReplayArtifactInspectionError::UnknownRef { .. }
        ));

        let traversal = inspect_replay_artifact(
            &paths.state_dir,
            &record,
            r#"{"kind":"run_artifact_path","path":"../outside.json","sha256":"","bytes":0}"#,
        )
        .unwrap_err();
        assert!(matches!(
            traversal,
            ReplayArtifactInspectionError::UnsafeRef { .. }
        ));
    }

    #[test]
    fn replay_artifacts_missing_manifest_fails_clearly() {
        let (_tmp, paths, run_id) = persisted_record_with_artifacts();
        let manifest = paths
            .runs_dir
            .join(format!("{run_id}.tool-results"))
            .join("manifest.json");
        std::fs::remove_file(&manifest).expect("remove manifest");
        let record = load_run_record(&paths.state_dir, &run_id).expect("load record");
        let err = list_replay_artifacts(&paths.state_dir, &record).unwrap_err();
        assert!(matches!(
            err,
            ReplayArtifactInspectionError::MissingManifest { .. }
        ));
    }

    #[test]
    fn replay_artifacts_old_run_without_observations_lists_zero() {
        let (_tmp, paths) = write_record_with_messages(
            "old_run",
            vec![Message {
                role: Role::Tool,
                content: Some("plain old tool result".to_string()),
                tool_call_id: Some("tc_old".to_string()),
                tool_name: Some("shell".to_string()),
                tool_calls: None,
            }],
        );
        let record = load_run_record(&paths.state_dir, "old_run").expect("load record");
        let list = list_replay_artifacts(&paths.state_dir, &record).expect("list");
        assert!(list.artifacts.is_empty());
    }
}
