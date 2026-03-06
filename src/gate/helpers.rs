use hex::encode as hex_encode;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::ApprovalKeyVersion;
use crate::target::ExecTargetKind;
use crate::trust::approvals::canonical_json;

pub(super) fn with_exec_target_arg(args: &Value, exec_target: ExecTargetKind) -> Value {
    let mut out = match args {
        Value::Object(map) => Value::Object(map.clone()),
        _ => Value::Object(serde_json::Map::new()),
    };
    if let Value::Object(ref mut map) = out {
        map.insert(
            "__exec_target".to_string(),
            Value::String(
                match exec_target {
                    ExecTargetKind::Host => "host",
                    ExecTargetKind::Docker => "docker",
                }
                .to_string(),
            ),
        );
    }
    out
}

pub fn compute_policy_hash_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(hasher.finalize())
}

pub fn compute_approval_key(
    tool_name: &str,
    arguments: &Value,
    workdir: &std::path::Path,
    policy_hash_hex: &str,
) -> String {
    let canonical_args = canonical_json(arguments).unwrap_or_else(|_| "null".to_string());
    let normalized_workdir = normalize_workdir(workdir);
    let payload = format!(
        "v1\n{}\n{}\n{}\n{}\n",
        tool_name, canonical_args, normalized_workdir, policy_hash_hex
    );
    compute_policy_hash_hex(payload.as_bytes())
}

#[allow(clippy::too_many_arguments)]
pub fn compute_approval_key_with_version(
    version: ApprovalKeyVersion,
    tool_name: &str,
    arguments: &Value,
    workdir: &std::path::Path,
    policy_hash_hex: &str,
    tool_schema_hash_hex: Option<&str>,
    hooks_config_hash_hex: Option<&str>,
    exec_target: ExecTargetKind,
    planner_hash_hex: Option<&str>,
) -> String {
    match version {
        ApprovalKeyVersion::V1 => {
            compute_approval_key(tool_name, arguments, workdir, policy_hash_hex)
        }
        ApprovalKeyVersion::V2 => {
            let canonical_args = canonical_json(arguments).unwrap_or_else(|_| "null".to_string());
            let normalized_workdir = normalize_workdir(workdir);
            let payload = format!(
                "v2|tool={}|args={}|workdir={}|policy={}|schema={}|hooks={}|exec_target={}|planner={}",
                tool_name,
                canonical_args,
                normalized_workdir,
                policy_hash_hex,
                tool_schema_hash_hex.unwrap_or("none"),
                hooks_config_hash_hex.unwrap_or("none"),
                match exec_target {
                    ExecTargetKind::Host => "host",
                    ExecTargetKind::Docker => "docker",
                },
                planner_hash_hex.unwrap_or("none"),
            );
            compute_policy_hash_hex(payload.as_bytes())
        }
    }
}

fn normalize_workdir(path: &std::path::Path) -> String {
    match std::fs::canonicalize(path) {
        Ok(p) => p.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}
