use std::sync::mpsc::Sender;

use anyhow::Context;

use crate::events::{Event, EventSink, JsonlFileSink, MultiSink, StdoutSink};
use crate::gate::{compute_policy_hash_hex, NoGate, ToolGate, TrustGate, TrustMode};
use crate::store;
use crate::trust;
use crate::trust::approvals::ApprovalsStore;
use crate::trust::audit::AuditLog;
use crate::trust::policy::{McpAllowSummary, Policy};
use crate::RunArgs;

pub(crate) fn build_event_sink(
    stream: bool,
    events_path: Option<&std::path::Path>,
    tui_enabled: bool,
    ui_tx: Option<Sender<Event>>,
    suppress_stdout: bool,
) -> anyhow::Result<Option<Box<dyn EventSink>>> {
    let mut multi = MultiSink::new();
    if stream && !tui_enabled && !suppress_stdout {
        multi.push(Box::new(StdoutSink::new()));
    }
    if let Some(tx) = ui_tx {
        multi.push(Box::new(crate::tui::UiSink::new(tx)));
    }
    if let Some(path) = events_path {
        multi.push(Box::new(JsonlFileSink::new(path)?));
    }
    if multi.is_empty() {
        Ok(None)
    } else {
        Ok(Some(Box::new(multi)))
    }
}

pub(crate) struct GateBuild {
    pub(crate) gate: Box<dyn ToolGate>,
    pub(crate) policy_hash_hex: Option<String>,
    pub(crate) policy_source: &'static str,
    pub(crate) policy_for_exposure: Option<Policy>,
    pub(crate) policy_version: Option<u32>,
    pub(crate) includes_resolved: Vec<String>,
    pub(crate) mcp_allowlist: Option<McpAllowSummary>,
}

pub(crate) fn build_gate(args: &RunArgs, paths: &store::StatePaths) -> anyhow::Result<GateBuild> {
    match args.trust {
        TrustMode::Off => Ok(GateBuild {
            gate: Box::new(NoGate::new()),
            policy_hash_hex: None,
            policy_source: "none",
            policy_for_exposure: None,
            policy_version: None,
            includes_resolved: Vec::new(),
            mcp_allowlist: None,
        }),
        TrustMode::Auto => {
            if !paths.policy_path.exists() {
                return Ok(GateBuild {
                    gate: Box::new(NoGate::new()),
                    policy_hash_hex: None,
                    policy_source: "none",
                    policy_for_exposure: None,
                    policy_version: None,
                    includes_resolved: Vec::new(),
                    mcp_allowlist: None,
                });
            }
            let policy_bytes = std::fs::read(&paths.policy_path).with_context(|| {
                format!(
                    "failed reading policy file: {}",
                    paths.policy_path.display()
                )
            })?;
            let policy = Policy::from_path(&paths.policy_path).with_context(|| {
                format!(
                    "failed parsing policy file: {}",
                    paths.policy_path.display()
                )
            })?;
            let policy_hash_hex = compute_policy_hash_hex(&policy_bytes);
            let policy_version = policy.version();
            let includes_resolved = policy.includes_resolved().to_vec();
            let mcp_allowlist = policy.mcp_allowlist_summary();
            Ok(GateBuild {
                gate: Box::new(TrustGate::new(
                    policy.clone(),
                    ApprovalsStore::new(paths.approvals_path.clone()),
                    AuditLog::new(paths.audit_path.clone()),
                    TrustMode::Auto,
                    policy_hash_hex.clone(),
                )),
                policy_hash_hex: Some(policy_hash_hex),
                policy_source: "file",
                policy_for_exposure: Some(policy),
                policy_version: Some(policy_version),
                includes_resolved,
                mcp_allowlist,
            })
        }
        TrustMode::On => {
            let (policy, policy_hash_hex, policy_source) = if paths.policy_path.exists() {
                let policy_bytes = std::fs::read(&paths.policy_path).with_context(|| {
                    format!(
                        "failed reading policy file: {}",
                        paths.policy_path.display()
                    )
                })?;
                let policy = Policy::from_path(&paths.policy_path).with_context(|| {
                    format!(
                        "failed parsing policy file: {}",
                        paths.policy_path.display()
                    )
                })?;
                (policy, compute_policy_hash_hex(&policy_bytes), "file")
            } else {
                let repr = trust::policy::safe_default_policy_repr();
                (
                    Policy::safe_default(),
                    compute_policy_hash_hex(repr.as_bytes()),
                    "default",
                )
            };
            let policy_version = policy.version();
            let includes_resolved = policy.includes_resolved().to_vec();
            let mcp_allowlist = policy.mcp_allowlist_summary();
            Ok(GateBuild {
                gate: Box::new(TrustGate::new(
                    policy.clone(),
                    ApprovalsStore::new(paths.approvals_path.clone()),
                    AuditLog::new(paths.audit_path.clone()),
                    TrustMode::On,
                    policy_hash_hex.clone(),
                )),
                policy_hash_hex: Some(policy_hash_hex),
                policy_source,
                policy_for_exposure: Some(policy),
                policy_version: Some(policy_version),
                includes_resolved,
                mcp_allowlist,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use clap::Parser;
    use serde_json::json;
    use tempfile::tempdir;

    use super::build_gate;
    use crate::gate::{
        ApprovalKeyVersion, ApprovalMode, AutoApproveScope, GateContext, GateDecision,
        ProviderKind, TrustMode,
    };
    use crate::store;
    use crate::taint::{TaintLevel, TaintMode};
    use crate::target::ExecTargetKind;
    use crate::types::ToolCall;

    fn base_args() -> crate::RunArgs {
        crate::RunArgs::parse_from(["localagent"])
    }

    fn gate_ctx(workdir: &Path) -> GateContext {
        GateContext {
            workdir: workdir.to_path_buf(),
            allow_shell: true,
            allow_write: false,
            approval_mode: ApprovalMode::Interrupt,
            auto_approve_scope: AutoApproveScope::Run,
            unsafe_mode: false,
            unsafe_bypass_allow_flags: false,
            run_id: Some("runtime-wiring-test".to_string()),
            enable_write_tools: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            provider: ProviderKind::Lmstudio,
            model: "test-model".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: ApprovalKeyVersion::V1,
            tool_schema_hashes: BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: None,
            taint_enabled: false,
            taint_mode: TaintMode::Propagate,
            taint_overall: TaintLevel::Clean,
            taint_sources: Vec::new(),
        }
    }

    fn shell_policy_yaml() -> &'static str {
        r#"
version: 2
default: deny
rules:
  - tool: "shell"
    decision: deny
    when:
      - arg: cmd
        op: equals
        value: "rm"
    reason: "dangerous command denied"
  - tool: "shell"
    decision: require_approval
    reason: "shell requires approval"
"#
    }

    #[test]
    fn build_gate_happy_path_allows_when_trust_off() {
        let tmp = tempdir().expect("tempdir");
        let paths = store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut args = base_args();
        args.trust = TrustMode::Off;

        let mut gate = build_gate(&args, &paths).expect("build gate").gate;
        let call = ToolCall {
            id: "tc_0".to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path":"Cargo.toml"}),
        };
        let decision = gate.decide(&gate_ctx(tmp.path()), &call);

        assert!(matches!(decision, GateDecision::Allow { .. }));
    }

    #[test]
    fn build_gate_deny_path_uses_policy() {
        let tmp = tempdir().expect("tempdir");
        let paths = store::resolve_state_paths(tmp.path(), None, None, None, None);
        std::fs::create_dir_all(&paths.state_dir).expect("state dir");
        std::fs::write(&paths.policy_path, shell_policy_yaml()).expect("write policy");

        let mut args = base_args();
        args.trust = TrustMode::On;
        args.approval_mode = ApprovalMode::Interrupt;

        let mut gate = build_gate(&args, &paths).expect("build gate").gate;
        let call = ToolCall {
            id: "tc_1".to_string(),
            name: "shell".to_string(),
            arguments: json!({"cmd":"rm","args":["-rf","/tmp/x"]}),
        };
        let decision = gate.decide(&gate_ctx(tmp.path()), &call);

        assert!(matches!(decision, GateDecision::Deny { .. }));
    }

    #[test]
    fn build_gate_approval_required_path_uses_policy() {
        let tmp = tempdir().expect("tempdir");
        let paths = store::resolve_state_paths(tmp.path(), None, None, None, None);
        std::fs::create_dir_all(&paths.state_dir).expect("state dir");
        std::fs::write(&paths.policy_path, shell_policy_yaml()).expect("write policy");

        let mut args = base_args();
        args.trust = TrustMode::On;
        args.approval_mode = ApprovalMode::Interrupt;

        let mut gate = build_gate(&args, &paths).expect("build gate").gate;
        let call = ToolCall {
            id: "tc_2".to_string(),
            name: "shell".to_string(),
            arguments: json!({"cmd":"echo","args":["hi"]}),
        };
        let decision = gate.decide(&gate_ctx(tmp.path()), &call);

        assert!(matches!(decision, GateDecision::RequireApproval { .. }));
    }
}
