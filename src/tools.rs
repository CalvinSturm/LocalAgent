use std::path::PathBuf;
use std::sync::Arc;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::target::{DockerMeta, ExecTarget, ExecTargetKind};
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
use exec_support::ToolExecution;
pub use schema::{
    compact_builtin_schema, invalid_args_detail, minimal_builtin_example,
    sorted_builtin_tool_names, validate_builtin_tool_args, validate_schema_args,
};

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
        "str_replace" => exec_write::run_str_replace(rt, &normalized_args).await,
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
mod tests;
