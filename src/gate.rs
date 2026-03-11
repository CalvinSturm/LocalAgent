use std::collections::BTreeMap;
use std::path::PathBuf;

use clap::ValueEnum;
use serde_json::Value;

mod helpers;
#[cfg(test)]
mod tests;

use helpers::with_exec_target_arg;
#[allow(unused_imports)]
pub use helpers::{
    compute_approval_key, compute_approval_key_with_version, compute_policy_hash_hex,
};

use crate::taint::{TaintLevel, TaintMode};
use crate::target::ExecTargetKind;
use crate::trust::approvals::{
    ApprovalDecisionMatch, ApprovalProvenance, ApprovalStatus, ApprovalsStore,
};
use crate::trust::audit::{AuditEvent, AuditLog, AuditResult};
use crate::trust::policy::{Policy, PolicyDecision};
use crate::types::ToolCall;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ProviderKind {
    Lmstudio,
    Llamacpp,
    Ollama,
    Mock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TrustMode {
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ApprovalMode {
    Interrupt,
    Auto,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AutoApproveScope {
    Run,
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ApprovalKeyVersion {
    V1,
    V2,
}

impl ApprovalKeyVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1 => "v1",
            Self::V2 => "v2",
        }
    }
}

#[derive(Debug, Clone)]
pub enum GateDecision {
    Allow {
        approval_id: Option<String>,
        approval_key: Option<String>,
        reason: Option<String>,
        source: Option<String>,
        taint_enforced: bool,
        escalated: bool,
        escalation_reason: Option<String>,
    },
    Deny {
        reason: String,
        approval_key: Option<String>,
        source: Option<String>,
        taint_enforced: bool,
        escalated: bool,
        escalation_reason: Option<String>,
    },
    RequireApproval {
        reason: String,
        approval_id: String,
        approval_key: Option<String>,
        source: Option<String>,
        taint_enforced: bool,
        escalated: bool,
        escalation_reason: Option<String>,
    },
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct GateContext {
    pub workdir: PathBuf,
    pub allow_shell: bool,
    pub allow_write: bool,
    pub approval_mode: ApprovalMode,
    pub auto_approve_scope: AutoApproveScope,
    pub unsafe_mode: bool,
    pub unsafe_bypass_allow_flags: bool,
    pub run_id: Option<String>,
    pub enable_write_tools: bool,
    pub max_tool_output_bytes: usize,
    pub max_read_bytes: usize,
    pub provider: ProviderKind,
    pub model: String,
    pub exec_target: ExecTargetKind,
    pub approval_key_version: ApprovalKeyVersion,
    pub tool_schema_hashes: BTreeMap<String, String>,
    pub hooks_config_hash_hex: Option<String>,
    pub planner_hash_hex: Option<String>,
    pub taint_enabled: bool,
    pub taint_mode: TaintMode,
    pub taint_overall: TaintLevel,
    pub taint_sources: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GateEvent {
    pub run_id: String,
    pub step: u32,
    pub tool_call_id: String,
    pub tool: String,
    pub arguments: Value,
    pub decision: String,
    pub decision_reason: Option<String>,
    pub decision_source: Option<String>,
    pub approval_id: Option<String>,
    pub approval_key: Option<String>,
    pub approval_mode: Option<String>,
    pub auto_approve_scope: Option<String>,
    pub approval_key_version: Option<String>,
    pub tool_schema_hash_hex: Option<String>,
    pub hooks_config_hash_hex: Option<String>,
    pub planner_hash_hex: Option<String>,
    pub exec_target: Option<String>,
    pub taint_overall: Option<String>,
    pub taint_enforced: bool,
    pub escalated: bool,
    pub escalation_reason: Option<String>,
    pub result_ok: bool,
    pub result_content: String,
    pub result_input_digest: Option<String>,
    pub result_output_digest: Option<String>,
    pub result_input_len: Option<usize>,
    pub result_output_len: Option<usize>,
}

pub trait ToolGate: Send {
    fn decide(&mut self, ctx: &GateContext, call: &ToolCall) -> GateDecision;
    fn record(&mut self, event: GateEvent);
}

#[derive(Debug, Clone)]
pub struct NoGate;

impl NoGate {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoGate {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolGate for NoGate {
    fn decide(&mut self, ctx: &GateContext, call: &ToolCall) -> GateDecision {
        if call.name == "shell" && !ctx.allow_shell && !ctx.unsafe_bypass_allow_flags {
            return GateDecision::Deny {
                reason: "shell requires --allow-shell".to_string(),
                approval_key: None,
                source: Some("hard_gate".to_string()),
                taint_enforced: false,
                escalated: false,
                escalation_reason: None,
            };
        }
        if (call.name == "write_file"
            || call.name == "apply_patch"
            || call.name == "edit"
            || call.name == "str_replace")
            && !ctx.allow_write
            && !ctx.unsafe_bypass_allow_flags
        {
            return GateDecision::Deny {
                reason: "writes require --allow-write".to_string(),
                approval_key: None,
                source: Some("hard_gate".to_string()),
                taint_enforced: false,
                escalated: false,
                escalation_reason: None,
            };
        }
        GateDecision::Allow {
            approval_id: None,
            approval_key: None,
            reason: None,
            source: None,
            taint_enforced: false,
            escalated: false,
            escalation_reason: None,
        }
    }

    fn record(&mut self, _event: GateEvent) {}
}

#[derive(Debug, Clone)]
pub struct TrustGate {
    pub policy: Policy,
    pub approvals: ApprovalsStore,
    pub audit: AuditLog,
    #[allow(dead_code)]
    pub trust_mode: TrustMode,
    pub policy_hash_hex: String,
}

impl TrustGate {
    pub fn new(
        policy: Policy,
        approvals: ApprovalsStore,
        audit: AuditLog,
        trust_mode: TrustMode,
        policy_hash_hex: String,
    ) -> Self {
        Self {
            policy,
            approvals,
            audit,
            trust_mode,
            policy_hash_hex,
        }
    }
}

impl ToolGate for TrustGate {
    fn decide(&mut self, ctx: &GateContext, call: &ToolCall) -> GateDecision {
        let tool_schema_hash_hex = ctx.tool_schema_hashes.get(&call.name).cloned();
        let approval_key = compute_approval_key_with_version(
            ctx.approval_key_version,
            &call.name,
            &call.arguments,
            &ctx.workdir,
            &self.policy_hash_hex,
            tool_schema_hash_hex.as_deref(),
            ctx.hooks_config_hash_hex.as_deref(),
            ctx.exec_target,
            ctx.planner_hash_hex.as_deref(),
        );
        let approval_provenance = ApprovalProvenance {
            approval_key_version: ctx.approval_key_version.as_str().to_string(),
            tool_schema_hash_hex,
            hooks_config_hash_hex: ctx.hooks_config_hash_hex.clone(),
            exec_target: Some(
                match ctx.exec_target {
                    ExecTargetKind::Host => "host",
                    ExecTargetKind::Docker => "docker",
                }
                .to_string(),
            ),
            planner_hash_hex: ctx.planner_hash_hex.clone(),
        };
        let args_with_target = with_exec_target_arg(&call.arguments, ctx.exec_target);

        if call.name == "shell" && !ctx.allow_shell && !ctx.unsafe_bypass_allow_flags {
            return GateDecision::Deny {
                reason: "shell requires --allow-shell".to_string(),
                approval_key: Some(approval_key),
                source: Some("hard_gate".to_string()),
                taint_enforced: false,
                escalated: false,
                escalation_reason: None,
            };
        }
        if (call.name == "write_file"
            || call.name == "apply_patch"
            || call.name == "edit"
            || call.name == "str_replace")
            && !ctx.allow_write
            && !ctx.unsafe_bypass_allow_flags
        {
            return GateDecision::Deny {
                reason: "writes require --allow-write".to_string(),
                approval_key: Some(approval_key),
                source: Some("hard_gate".to_string()),
                taint_enforced: false,
                escalated: false,
                escalation_reason: None,
            };
        }

        if let Err(reason) = self.policy.mcp_tool_allowed(&call.name) {
            return GateDecision::Deny {
                reason,
                approval_key: Some(approval_key),
                source: Some("mcp_allowlist".to_string()),
                taint_enforced: false,
                escalated: false,
                escalation_reason: None,
            };
        }

        let eval = self.policy.evaluate(&call.name, &args_with_target);
        let side_effects = crate::tools::tool_side_effects(&call.name);
        let taint_enforced = ctx.taint_enabled
            && matches!(ctx.taint_mode, TaintMode::PropagateAndEnforce)
            && matches!(ctx.taint_overall, TaintLevel::Tainted);
        let should_escalate = taint_enforced
            && matches!(
                side_effects,
                crate::types::SideEffects::FilesystemWrite
                    | crate::types::SideEffects::ShellExec
                    | crate::types::SideEffects::Network
            );
        let escalation_reason = if should_escalate {
            Some("taint_escalation".to_string())
        } else {
            None
        };
        let mut decision = match eval.decision {
            PolicyDecision::Allow => GateDecision::Allow {
                approval_id: None,
                approval_key: Some(approval_key),
                reason: eval.reason,
                source: eval.source,
                taint_enforced,
                escalated: false,
                escalation_reason: None,
            },
            PolicyDecision::Deny => GateDecision::Deny {
                reason: eval
                    .reason
                    .unwrap_or_else(|| format!("policy denied tool '{}'", call.name)),
                approval_key: Some(approval_key),
                source: eval.source,
                taint_enforced,
                escalated: false,
                escalation_reason: None,
            },
            PolicyDecision::RequireApproval => {
                if matches!(ctx.approval_mode, ApprovalMode::Auto) {
                    return match ctx.auto_approve_scope {
                        AutoApproveScope::Run => GateDecision::Allow {
                            approval_id: Some(format!(
                                "auto:{}:{}",
                                ctx.run_id.clone().unwrap_or_else(|| "run".to_string()),
                                call.id
                            )),
                            approval_key: Some(approval_key),
                            reason: eval.reason.clone(),
                            source: eval.source.clone(),
                            taint_enforced,
                            escalated: false,
                            escalation_reason: None,
                        },
                        AutoApproveScope::Session => {
                            match self.approvals.ensure_approved_for_key(
                                &call.name,
                                &call.arguments,
                                &approval_key,
                                Some(approval_provenance.clone()),
                            ) {
                                Ok(id) => GateDecision::Allow {
                                    approval_id: Some(id),
                                    approval_key: Some(approval_key),
                                    reason: eval.reason.clone(),
                                    source: eval.source.clone(),
                                    taint_enforced,
                                    escalated: false,
                                    escalation_reason: None,
                                },
                                Err(e) => GateDecision::Deny {
                                    reason: format!("failed to auto-approve: {e}"),
                                    approval_key: Some(approval_key),
                                    source: eval.source.clone(),
                                    taint_enforced,
                                    escalated: false,
                                    escalation_reason: None,
                                },
                            }
                        }
                    };
                }

                match self
                    .approvals
                    .consume_matching_approved(&approval_key, ctx.approval_key_version.as_str())
                {
                    Ok(Some(usage)) => GateDecision::Allow {
                        approval_id: Some(usage.id),
                        approval_key: Some(usage.approval_key),
                        reason: eval.reason.clone(),
                        source: eval.source.clone(),
                        taint_enforced,
                        escalated: false,
                        escalation_reason: None,
                    },
                    Ok(None) => match self
                        .approvals
                        .find_matching_decision(&approval_key, ctx.approval_key_version.as_str())
                    {
                        Ok(Some(ApprovalDecisionMatch {
                            id,
                            status: ApprovalStatus::Denied,
                        })) => GateDecision::Deny {
                            reason: format!("approval denied: {id}"),
                            approval_key: Some(approval_key),
                            source: eval.source.clone(),
                            taint_enforced,
                            escalated: false,
                            escalation_reason: None,
                        },
                        Ok(Some(ApprovalDecisionMatch {
                            id,
                            status: ApprovalStatus::Pending,
                        })) => GateDecision::RequireApproval {
                            reason: eval
                                .reason
                                .clone()
                                .unwrap_or_else(|| format!("approval pending: {id}")),
                            approval_id: id,
                            approval_key: Some(approval_key),
                            source: eval.source.clone(),
                            taint_enforced,
                            escalated: false,
                            escalation_reason: None,
                        },
                        Ok(None) => {
                            match self.approvals.create_pending(
                                &call.name,
                                &call.arguments,
                                Some(approval_key.clone()),
                                Some(approval_provenance.clone()),
                            ) {
                                Ok(id) => GateDecision::RequireApproval {
                                    reason: eval.reason.clone().unwrap_or_else(|| {
                                        if matches!(ctx.approval_mode, ApprovalMode::Fail) {
                                            format!("approval required (fail mode): {id}")
                                        } else {
                                            format!("approval required: {id}")
                                        }
                                    }),
                                    approval_id: id,
                                    approval_key: Some(approval_key),
                                    source: eval.source.clone(),
                                    taint_enforced,
                                    escalated: false,
                                    escalation_reason: None,
                                },
                                Err(e) => GateDecision::Deny {
                                    reason: format!("failed to create approval request: {e}"),
                                    approval_key: Some(approval_key),
                                    source: eval.source.clone(),
                                    taint_enforced,
                                    escalated: false,
                                    escalation_reason: None,
                                },
                            }
                        }
                        Err(e) => GateDecision::Deny {
                            reason: format!("failed to read approvals store: {e}"),
                            approval_key: Some(approval_key),
                            source: eval.source.clone(),
                            taint_enforced,
                            escalated: false,
                            escalation_reason: None,
                        },
                    },
                    Err(e) => GateDecision::Deny {
                        reason: format!("failed to read approvals store: {e}"),
                        approval_key: Some(approval_key),
                        source: eval.source,
                        taint_enforced,
                        escalated: false,
                        escalation_reason: None,
                    },
                }
            }
        };

        if should_escalate {
            decision = match decision {
                GateDecision::Deny { .. } => decision,
                GateDecision::RequireApproval {
                    reason,
                    approval_id,
                    approval_key,
                    source,
                    taint_enforced,
                    ..
                } => GateDecision::RequireApproval {
                    reason: if reason.is_empty() {
                        "approval required due to tainted content".to_string()
                    } else {
                        reason
                    },
                    approval_id,
                    approval_key,
                    source,
                    taint_enforced,
                    escalated: true,
                    escalation_reason: escalation_reason.clone(),
                },
                GateDecision::Allow {
                    approval_id: _,
                    approval_key,
                    reason: _,
                    source,
                    taint_enforced,
                    ..
                } => {
                    if matches!(ctx.approval_mode, ApprovalMode::Auto) {
                        let auto_id = match ctx.auto_approve_scope {
                            AutoApproveScope::Run => format!(
                                "auto:{}:{}",
                                ctx.run_id.clone().unwrap_or_else(|| "run".to_string()),
                                call.id
                            ),
                            AutoApproveScope::Session => {
                                let key_for_session = approval_key.clone().unwrap_or_default();
                                self.approvals
                                    .ensure_approved_for_key(
                                        &call.name,
                                        &call.arguments,
                                        &key_for_session,
                                        Some(approval_provenance.clone()),
                                    )
                                    .unwrap_or_else(|_| {
                                        format!(
                                            "auto:{}:{}",
                                            ctx.run_id.clone().unwrap_or_else(|| "run".to_string()),
                                            call.id
                                        )
                                    })
                            }
                        };
                        GateDecision::Allow {
                            approval_id: Some(auto_id),
                            approval_key,
                            reason: Some("taint_escalation".to_string()),
                            source,
                            taint_enforced,
                            escalated: true,
                            escalation_reason: escalation_reason.clone(),
                        }
                    } else {
                        let id = self
                            .approvals
                            .create_pending(
                                &call.name,
                                &call.arguments,
                                approval_key.clone(),
                                Some(approval_provenance.clone()),
                            )
                            .unwrap_or_else(|_| format!("pending:{}:{}", call.name, call.id));
                        GateDecision::RequireApproval {
                            reason: "approval required due to tainted content".to_string(),
                            approval_id: id,
                            approval_key,
                            source,
                            taint_enforced,
                            escalated: true,
                            escalation_reason: escalation_reason.clone(),
                        }
                    }
                }
            };
        }

        decision
    }

    fn record(&mut self, event: GateEvent) {
        let audit = AuditEvent {
            ts: crate::trust::now_rfc3339(),
            run_id: event.run_id,
            step: event.step,
            tool_call_id: event.tool_call_id,
            tool: event.tool,
            arguments: event.arguments,
            decision: event.decision,
            decision_reason: event.decision_reason,
            decision_source: event.decision_source,
            approval_id: event.approval_id,
            approval_key: event.approval_key,
            approval_mode: event.approval_mode,
            auto_approve_scope: event.auto_approve_scope,
            approval_key_version: event.approval_key_version,
            tool_schema_hash_hex: event.tool_schema_hash_hex,
            hooks_config_hash_hex: event.hooks_config_hash_hex,
            planner_hash_hex: event.planner_hash_hex,
            exec_target: event.exec_target,
            taint_overall: event.taint_overall,
            taint_enforced: event.taint_enforced,
            escalated: event.escalated,
            escalation_reason: event.escalation_reason,
            result: AuditResult {
                ok: event.result_ok,
                content: event.result_content,
                input_digest: event.result_input_digest,
                output_digest: event.result_output_digest,
                input_len: event.result_input_len,
                output_len: event.result_output_len,
            },
        };
        if let Err(e) = self.audit.append(&audit) {
            eprintln!("WARN: failed to append audit log: {e}");
        }
    }
}
