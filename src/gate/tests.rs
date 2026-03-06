use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::json;
use tempfile::tempdir;

use super::{
    compute_approval_key, compute_approval_key_with_version, compute_policy_hash_hex,
    ApprovalKeyVersion, ApprovalMode, AutoApproveScope, ExecTargetKind, GateContext, GateDecision,
    NoGate, ProviderKind, ToolGate, TrustGate, TrustMode,
};
use crate::trust::approvals::{ApprovalProvenance, ApprovalsStore};
use crate::trust::audit::AuditLog;
use crate::trust::policy::Policy;
use crate::types::ToolCall;

#[test]
fn nogate_always_allows() {
    let mut gate = NoGate::new();
    let ctx = GateContext {
        workdir: PathBuf::from("."),
        allow_shell: false,
        allow_write: false,
        approval_mode: ApprovalMode::Interrupt,
        auto_approve_scope: AutoApproveScope::Run,
        unsafe_mode: false,
        unsafe_bypass_allow_flags: false,
        run_id: None,
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
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_overall: crate::taint::TaintLevel::Clean,
        taint_sources: Vec::new(),
    };
    let call = ToolCall {
        id: "tc_0".to_string(),
        name: "read_file".to_string(),
        arguments: json!({"path":"Cargo.toml"}),
    };
    let decision = gate.decide(&ctx, &call);
    assert!(matches!(decision, GateDecision::Allow { .. }));
}

#[test]
fn approval_key_matching_allows_when_approved() {
    let tmp = tempdir().expect("tempdir");
    let approvals = tmp.path().join("approvals.json");
    let audit = tmp.path().join("audit.jsonl");
    let store = ApprovalsStore::new(approvals);
    let policy = Policy::safe_default();
    let policy_hash = compute_policy_hash_hex(b"default");
    let ctx = GateContext {
        workdir: tmp.path().to_path_buf(),
        allow_shell: true,
        allow_write: false,
        approval_mode: ApprovalMode::Interrupt,
        auto_approve_scope: AutoApproveScope::Run,
        unsafe_mode: false,
        unsafe_bypass_allow_flags: false,
        run_id: Some("r1".to_string()),
        enable_write_tools: false,
        max_tool_output_bytes: 200_000,
        max_read_bytes: 200_000,
        provider: ProviderKind::Lmstudio,
        model: "m".to_string(),
        exec_target: ExecTargetKind::Host,
        approval_key_version: ApprovalKeyVersion::V1,
        tool_schema_hashes: BTreeMap::new(),
        hooks_config_hash_hex: None,
        planner_hash_hex: None,
        taint_enabled: false,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_overall: crate::taint::TaintLevel::Clean,
        taint_sources: Vec::new(),
    };
    let call = ToolCall {
        id: "tc_1".to_string(),
        name: "shell".to_string(),
        arguments: json!({"cmd":"echo","args":["hi"]}),
    };
    let key = compute_approval_key(&call.name, &call.arguments, &ctx.workdir, &policy_hash);
    let id = store
        .create_pending(&call.name, &call.arguments, Some(key.clone()), None)
        .expect("create pending");
    store.approve(&id, None, None).expect("approve");
    let mut gate = TrustGate::new(
        policy,
        store,
        AuditLog::new(audit),
        TrustMode::On,
        policy_hash,
    );
    let decision = gate.decide(&ctx, &call);
    assert!(matches!(decision, GateDecision::Allow { .. }));
}

#[test]
fn approval_ttl_expired_requires_new() {
    let tmp = tempdir().expect("tempdir");
    let approvals = tmp.path().join("approvals.json");
    let audit = tmp.path().join("audit.jsonl");
    let store = ApprovalsStore::new(approvals);
    let policy = Policy::safe_default();
    let policy_hash = compute_policy_hash_hex(b"default");
    let ctx = GateContext {
        workdir: tmp.path().to_path_buf(),
        allow_shell: true,
        allow_write: false,
        approval_mode: ApprovalMode::Interrupt,
        auto_approve_scope: AutoApproveScope::Run,
        unsafe_mode: false,
        unsafe_bypass_allow_flags: false,
        run_id: Some("r1".to_string()),
        enable_write_tools: false,
        max_tool_output_bytes: 200_000,
        max_read_bytes: 200_000,
        provider: ProviderKind::Lmstudio,
        model: "m".to_string(),
        exec_target: ExecTargetKind::Host,
        approval_key_version: ApprovalKeyVersion::V1,
        tool_schema_hashes: BTreeMap::new(),
        hooks_config_hash_hex: None,
        planner_hash_hex: None,
        taint_enabled: false,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_overall: crate::taint::TaintLevel::Clean,
        taint_sources: Vec::new(),
    };
    let call = ToolCall {
        id: "tc_1".to_string(),
        name: "shell".to_string(),
        arguments: json!({"cmd":"echo","args":["hi"]}),
    };
    let key = compute_approval_key(&call.name, &call.arguments, &ctx.workdir, &policy_hash);
    let id = store
        .create_pending(&call.name, &call.arguments, Some(key), None)
        .expect("create pending");
    store.approve(&id, Some(0), None).expect("approve expired");
    let mut gate = TrustGate::new(
        policy,
        store,
        AuditLog::new(audit),
        TrustMode::On,
        policy_hash,
    );
    let decision = gate.decide(&ctx, &call);
    assert!(matches!(decision, GateDecision::RequireApproval { .. }));
}

#[test]
fn approval_max_uses_exhaustion() {
    let tmp = tempdir().expect("tempdir");
    let approvals = tmp.path().join("approvals.json");
    let audit = tmp.path().join("audit.jsonl");
    let store = ApprovalsStore::new(approvals);
    let policy = Policy::safe_default();
    let policy_hash = compute_policy_hash_hex(b"default");
    let ctx = GateContext {
        workdir: tmp.path().to_path_buf(),
        allow_shell: true,
        allow_write: false,
        approval_mode: ApprovalMode::Interrupt,
        auto_approve_scope: AutoApproveScope::Run,
        unsafe_mode: false,
        unsafe_bypass_allow_flags: false,
        run_id: Some("r1".to_string()),
        enable_write_tools: false,
        max_tool_output_bytes: 200_000,
        max_read_bytes: 200_000,
        provider: ProviderKind::Lmstudio,
        model: "m".to_string(),
        exec_target: ExecTargetKind::Host,
        approval_key_version: ApprovalKeyVersion::V1,
        tool_schema_hashes: BTreeMap::new(),
        hooks_config_hash_hex: None,
        planner_hash_hex: None,
        taint_enabled: false,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_overall: crate::taint::TaintLevel::Clean,
        taint_sources: Vec::new(),
    };
    let call = ToolCall {
        id: "tc_1".to_string(),
        name: "shell".to_string(),
        arguments: json!({"cmd":"echo","args":["hi"]}),
    };
    let key = compute_approval_key(&call.name, &call.arguments, &ctx.workdir, &policy_hash);
    let id = store
        .create_pending(&call.name, &call.arguments, Some(key), None)
        .expect("create pending");
    store.approve(&id, None, Some(1)).expect("approve");
    let mut gate = TrustGate::new(
        policy,
        store,
        AuditLog::new(audit),
        TrustMode::On,
        policy_hash,
    );
    assert!(matches!(
        gate.decide(&ctx, &call),
        GateDecision::Allow { .. }
    ));
    assert!(matches!(
        gate.decide(&ctx, &call),
        GateDecision::RequireApproval { .. }
    ));
}

#[test]
fn approval_mode_fail_requires_approval() {
    let tmp = tempdir().expect("tempdir");
    let approvals = tmp.path().join("approvals.json");
    let audit = tmp.path().join("audit.jsonl");
    let store = ApprovalsStore::new(approvals);
    let policy = Policy::safe_default();
    let policy_hash = compute_policy_hash_hex(b"default");
    let ctx = GateContext {
        workdir: tmp.path().to_path_buf(),
        allow_shell: true,
        allow_write: false,
        approval_mode: ApprovalMode::Fail,
        auto_approve_scope: AutoApproveScope::Run,
        unsafe_mode: false,
        unsafe_bypass_allow_flags: false,
        run_id: Some("r1".to_string()),
        enable_write_tools: false,
        max_tool_output_bytes: 200_000,
        max_read_bytes: 200_000,
        provider: ProviderKind::Lmstudio,
        model: "m".to_string(),
        exec_target: ExecTargetKind::Host,
        approval_key_version: ApprovalKeyVersion::V1,
        tool_schema_hashes: BTreeMap::new(),
        hooks_config_hash_hex: None,
        planner_hash_hex: None,
        taint_enabled: false,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_overall: crate::taint::TaintLevel::Clean,
        taint_sources: Vec::new(),
    };
    let call = ToolCall {
        id: "tc_1".to_string(),
        name: "shell".to_string(),
        arguments: json!({"cmd":"echo","args":["hi"]}),
    };
    let mut gate = TrustGate::new(
        policy,
        store,
        AuditLog::new(audit),
        TrustMode::On,
        policy_hash,
    );
    assert!(matches!(
        gate.decide(&ctx, &call),
        GateDecision::RequireApproval { .. }
    ));
}

#[test]
fn approval_mode_auto_run_allows() {
    let tmp = tempdir().expect("tempdir");
    let approvals = tmp.path().join("approvals.json");
    let audit = tmp.path().join("audit.jsonl");
    let store = ApprovalsStore::new(approvals);
    let policy = Policy::safe_default();
    let policy_hash = compute_policy_hash_hex(b"default");
    let ctx = GateContext {
        workdir: tmp.path().to_path_buf(),
        allow_shell: true,
        allow_write: false,
        approval_mode: ApprovalMode::Auto,
        auto_approve_scope: AutoApproveScope::Run,
        unsafe_mode: false,
        unsafe_bypass_allow_flags: false,
        run_id: Some("r99".to_string()),
        enable_write_tools: false,
        max_tool_output_bytes: 200_000,
        max_read_bytes: 200_000,
        provider: ProviderKind::Lmstudio,
        model: "m".to_string(),
        exec_target: ExecTargetKind::Host,
        approval_key_version: ApprovalKeyVersion::V1,
        tool_schema_hashes: BTreeMap::new(),
        hooks_config_hash_hex: None,
        planner_hash_hex: None,
        taint_enabled: false,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_overall: crate::taint::TaintLevel::Clean,
        taint_sources: Vec::new(),
    };
    let call = ToolCall {
        id: "tc_abc".to_string(),
        name: "shell".to_string(),
        arguments: json!({"cmd":"echo","args":["hi"]}),
    };
    let mut gate = TrustGate::new(
        policy,
        store,
        AuditLog::new(audit),
        TrustMode::On,
        policy_hash,
    );
    match gate.decide(&ctx, &call) {
        GateDecision::Allow { approval_id, .. } => {
            let id = approval_id.unwrap_or_default();
            assert!(id.contains("auto:r99:tc_abc"));
        }
        _ => panic!("expected allow"),
    }
}

#[test]
fn policy_can_match_exec_target_condition() {
    let tmp = tempdir().expect("tempdir");
    let approvals = tmp.path().join("approvals.json");
    let audit = tmp.path().join("audit.jsonl");
    let store = ApprovalsStore::new(approvals);
    let policy = Policy::from_yaml(
        r#"
version: 2
default: allow
rules:
  - tool: "read_file"
    decision: deny
    when:
      - arg: "__exec_target"
        op: equals
        value: "docker"
"#,
    )
    .expect("policy");
    let policy_hash = compute_policy_hash_hex(b"custom");
    let mut gate = TrustGate::new(
        policy,
        store,
        AuditLog::new(audit),
        TrustMode::On,
        policy_hash,
    );
    let call = ToolCall {
        id: "tc_x".to_string(),
        name: "read_file".to_string(),
        arguments: json!({"path":"a.txt"}),
    };

    let mut ctx = GateContext {
        workdir: tmp.path().to_path_buf(),
        allow_shell: false,
        allow_write: false,
        approval_mode: ApprovalMode::Interrupt,
        auto_approve_scope: AutoApproveScope::Run,
        unsafe_mode: false,
        unsafe_bypass_allow_flags: false,
        run_id: Some("r".to_string()),
        enable_write_tools: false,
        max_tool_output_bytes: 200_000,
        max_read_bytes: 200_000,
        provider: ProviderKind::Lmstudio,
        model: "m".to_string(),
        exec_target: ExecTargetKind::Host,
        approval_key_version: ApprovalKeyVersion::V1,
        tool_schema_hashes: BTreeMap::new(),
        hooks_config_hash_hex: None,
        planner_hash_hex: None,
        taint_enabled: false,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_overall: crate::taint::TaintLevel::Clean,
        taint_sources: Vec::new(),
    };
    assert!(matches!(
        gate.decide(&ctx, &call),
        GateDecision::Allow { .. }
    ));
    ctx.exec_target = ExecTargetKind::Docker;
    assert!(matches!(
        gate.decide(&ctx, &call),
        GateDecision::Deny { .. }
    ));
}

#[test]
fn approval_key_v2_deterministic_known_hash() {
    let got = compute_approval_key_with_version(
        ApprovalKeyVersion::V2,
        "read_file",
        &json!({"path":"a.txt"}),
        std::path::Path::new("/tmp/w"),
        "abc",
        Some("def"),
        None,
        ExecTargetKind::Host,
        None,
    );
    assert_eq!(
        got,
        "6cec1a4c99be252db98654e874d29f1aa0306692181b4ae494ef42bfbca5aba1"
    );
}

#[test]
fn gate_key_version_matching_v1_vs_v2() {
    let tmp = tempdir().expect("tmp");
    let approvals = tmp.path().join("approvals.json");
    let audit = tmp.path().join("audit.jsonl");
    let store = ApprovalsStore::new(approvals);
    let policy = Policy::safe_default();
    let policy_hash = compute_policy_hash_hex(b"default");
    let call = ToolCall {
        id: "tc_1".to_string(),
        name: "shell".to_string(),
        arguments: json!({"cmd":"echo","args":["hi"]}),
    };
    let key_v2 = compute_approval_key_with_version(
        ApprovalKeyVersion::V2,
        &call.name,
        &call.arguments,
        tmp.path(),
        &policy_hash,
        None,
        None,
        ExecTargetKind::Host,
        None,
    );
    let id = store
        .create_pending(
            &call.name,
            &call.arguments,
            Some(key_v2),
            Some(ApprovalProvenance {
                approval_key_version: "v2".to_string(),
                tool_schema_hash_hex: None,
                hooks_config_hash_hex: None,
                exec_target: Some("host".to_string()),
                planner_hash_hex: None,
            }),
        )
        .expect("pending");
    store.approve(&id, None, None).expect("approve");

    let mut gate = TrustGate::new(
        policy,
        store,
        AuditLog::new(audit),
        TrustMode::On,
        policy_hash,
    );
    let mut ctx = GateContext {
        workdir: tmp.path().to_path_buf(),
        allow_shell: true,
        allow_write: false,
        approval_mode: ApprovalMode::Interrupt,
        auto_approve_scope: AutoApproveScope::Run,
        unsafe_mode: false,
        unsafe_bypass_allow_flags: false,
        run_id: Some("r".to_string()),
        enable_write_tools: false,
        max_tool_output_bytes: 200_000,
        max_read_bytes: 200_000,
        provider: ProviderKind::Lmstudio,
        model: "m".to_string(),
        exec_target: ExecTargetKind::Host,
        approval_key_version: ApprovalKeyVersion::V1,
        tool_schema_hashes: BTreeMap::new(),
        hooks_config_hash_hex: None,
        planner_hash_hex: None,
        taint_enabled: false,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_overall: crate::taint::TaintLevel::Clean,
        taint_sources: Vec::new(),
    };
    assert!(matches!(
        gate.decide(&ctx, &call),
        GateDecision::RequireApproval { .. }
    ));
    ctx.approval_key_version = ApprovalKeyVersion::V2;
    assert!(matches!(
        gate.decide(&ctx, &call),
        GateDecision::Allow { .. }
    ));
}

#[test]
fn taint_enforcement_escalates_shell_to_require_approval() {
    let tmp = tempdir().expect("tmp");
    let approvals = tmp.path().join("approvals.json");
    let audit = tmp.path().join("audit.jsonl");
    let store = ApprovalsStore::new(approvals);
    let policy = Policy::from_yaml(
        r#"
version: 2
default: deny
rules:
  - tool: "shell"
    decision: allow
"#,
    )
    .expect("policy");
    let policy_hash = compute_policy_hash_hex(b"custom");
    let mut gate = TrustGate::new(
        policy,
        store,
        AuditLog::new(audit),
        TrustMode::On,
        policy_hash,
    );
    let ctx = GateContext {
        workdir: tmp.path().to_path_buf(),
        allow_shell: true,
        allow_write: false,
        approval_mode: ApprovalMode::Interrupt,
        auto_approve_scope: AutoApproveScope::Run,
        unsafe_mode: false,
        unsafe_bypass_allow_flags: false,
        run_id: Some("r".to_string()),
        enable_write_tools: false,
        max_tool_output_bytes: 200_000,
        max_read_bytes: 200_000,
        provider: ProviderKind::Lmstudio,
        model: "m".to_string(),
        exec_target: ExecTargetKind::Host,
        approval_key_version: ApprovalKeyVersion::V1,
        tool_schema_hashes: BTreeMap::new(),
        hooks_config_hash_hex: None,
        planner_hash_hex: None,
        taint_enabled: true,
        taint_mode: crate::taint::TaintMode::PropagateAndEnforce,
        taint_overall: crate::taint::TaintLevel::Tainted,
        taint_sources: vec!["browser".to_string()],
    };
    let call = ToolCall {
        id: "tc_taint".to_string(),
        name: "shell".to_string(),
        arguments: json!({"cmd":"echo","args":["hi"]}),
    };
    match gate.decide(&ctx, &call) {
        GateDecision::RequireApproval {
            escalated,
            escalation_reason,
            ..
        } => {
            assert!(escalated);
            assert_eq!(escalation_reason.as_deref(), Some("taint_escalation"));
        }
        _ => panic!("expected require_approval"),
    }
}

#[test]
fn taint_propagate_mode_does_not_escalate() {
    let tmp = tempdir().expect("tmp");
    let approvals = tmp.path().join("approvals.json");
    let audit = tmp.path().join("audit.jsonl");
    let store = ApprovalsStore::new(approvals);
    let policy = Policy::from_yaml(
        r#"
version: 2
default: deny
rules:
  - tool: "shell"
    decision: allow
"#,
    )
    .expect("policy");
    let policy_hash = compute_policy_hash_hex(b"custom");
    let mut gate = TrustGate::new(
        policy,
        store,
        AuditLog::new(audit),
        TrustMode::On,
        policy_hash,
    );
    let ctx = GateContext {
        workdir: tmp.path().to_path_buf(),
        allow_shell: true,
        allow_write: false,
        approval_mode: ApprovalMode::Interrupt,
        auto_approve_scope: AutoApproveScope::Run,
        unsafe_mode: false,
        unsafe_bypass_allow_flags: false,
        run_id: Some("r".to_string()),
        enable_write_tools: false,
        max_tool_output_bytes: 200_000,
        max_read_bytes: 200_000,
        provider: ProviderKind::Lmstudio,
        model: "m".to_string(),
        exec_target: ExecTargetKind::Host,
        approval_key_version: ApprovalKeyVersion::V1,
        tool_schema_hashes: BTreeMap::new(),
        hooks_config_hash_hex: None,
        planner_hash_hex: None,
        taint_enabled: true,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_overall: crate::taint::TaintLevel::Tainted,
        taint_sources: vec!["browser".to_string()],
    };
    let call = ToolCall {
        id: "tc_taint".to_string(),
        name: "shell".to_string(),
        arguments: json!({"cmd":"echo","args":["hi"]}),
    };
    match gate.decide(&ctx, &call) {
        GateDecision::Allow { escalated, .. } => assert!(!escalated),
        _ => panic!("expected allow"),
    }
}
