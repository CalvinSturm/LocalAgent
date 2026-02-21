use uuid::Uuid;

use crate::gate::{ApprovalMode, AutoApproveScope, GateContext, GateDecision, GateEvent, ToolGate};
use crate::providers::ModelProvider;
use crate::tools::{execute_tool, ToolRuntime};
use crate::types::{GenerateRequest, Message, Role, ToolCall, ToolDef};

#[derive(Debug, Clone, Copy)]
pub enum AgentExitReason {
    Ok,
    ProviderError,
    Denied,
    ApprovalRequired,
    MaxSteps,
}

impl AgentExitReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentExitReason::Ok => "ok",
            AgentExitReason::ProviderError => "provider_error",
            AgentExitReason::Denied => "denied",
            AgentExitReason::ApprovalRequired => "approval_required",
            AgentExitReason::MaxSteps => "max_steps",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentOutcome {
    pub run_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub exit_reason: AgentExitReason,
    pub final_output: String,
    pub error: Option<String>,
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ToolCall>,
}

pub struct Agent<P: ModelProvider> {
    pub provider: P,
    pub model: String,
    pub tools: Vec<ToolDef>,
    pub max_steps: usize,
    pub tool_rt: ToolRuntime,
    pub gate: Box<dyn ToolGate>,
    pub gate_ctx: GateContext,
}

impl<P: ModelProvider> Agent<P> {
    pub async fn run(&mut self, user_prompt: &str, session_messages: Vec<Message>) -> AgentOutcome {
        let run_id = Uuid::new_v4().to_string();
        self.gate_ctx.run_id = Some(run_id.clone());
        let started_at = crate::trust::now_rfc3339();
        let mut messages = vec![Message {
            role: Role::System,
            content: Some(
                "You are an agent that may call tools to gather information. Use tools when \
                 needed, then provide a final direct answer when done. If no tools are \
                 needed, answer immediately."
                    .to_string(),
            ),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        }];
        messages.extend(session_messages);
        messages.push(Message {
            role: Role::User,
            content: Some(user_prompt.to_string()),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        });

        let mut observed_tool_calls = Vec::new();
        for step in 0..self.max_steps {
            let mut tools_sorted = self.tools.clone();
            tools_sorted.sort_by(|a, b| a.name.cmp(&b.name));

            let req = GenerateRequest {
                model: self.model.clone(),
                messages: messages.clone(),
                tools: tools_sorted,
            };

            let resp = match self.provider.generate(req).await {
                Ok(r) => r,
                Err(e) => {
                    return AgentOutcome {
                        run_id,
                        started_at,
                        finished_at: crate::trust::now_rfc3339(),
                        exit_reason: AgentExitReason::ProviderError,
                        final_output: String::new(),
                        error: Some(e.to_string()),
                        messages,
                        tool_calls: observed_tool_calls,
                    };
                }
            };
            messages.push(resp.assistant.clone());

            if resp.tool_calls.is_empty() {
                return AgentOutcome {
                    run_id,
                    started_at,
                    finished_at: crate::trust::now_rfc3339(),
                    exit_reason: AgentExitReason::Ok,
                    final_output: resp.assistant.content.unwrap_or_default(),
                    error: None,
                    messages,
                    tool_calls: observed_tool_calls,
                };
            }

            for tc in &resp.tool_calls {
                observed_tool_calls.push(tc.clone());
                let approval_mode_meta =
                    if matches!(self.gate_ctx.approval_mode, ApprovalMode::Auto) {
                        Some("auto".to_string())
                    } else {
                        None
                    };
                let auto_scope_meta = if matches!(self.gate_ctx.approval_mode, ApprovalMode::Auto) {
                    Some(
                        match self.gate_ctx.auto_approve_scope {
                            AutoApproveScope::Run => "run",
                            AutoApproveScope::Session => "session",
                        }
                        .to_string(),
                    )
                } else {
                    None
                };
                match self.gate.decide(&self.gate_ctx, tc) {
                    GateDecision::Allow {
                        approval_id,
                        approval_key,
                    } => {
                        let tool_msg = execute_tool(&self.tool_rt, tc).await;
                        let content = tool_msg.content.clone().unwrap_or_default();
                        self.gate.record(GateEvent {
                            run_id: run_id.clone(),
                            step: step as u32,
                            tool_call_id: tc.id.clone(),
                            tool: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                            decision: "allow".to_string(),
                            approval_id,
                            approval_key,
                            approval_mode: approval_mode_meta.clone(),
                            auto_approve_scope: auto_scope_meta.clone(),
                            result_ok: !tool_result_has_error(&content),
                            result_content: content,
                        });
                        messages.push(tool_msg);
                    }
                    GateDecision::Deny {
                        reason,
                        approval_key,
                    } => {
                        self.gate.record(GateEvent {
                            run_id: run_id.clone(),
                            step: step as u32,
                            tool_call_id: tc.id.clone(),
                            tool: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                            decision: "deny".to_string(),
                            approval_id: None,
                            approval_key,
                            approval_mode: approval_mode_meta.clone(),
                            auto_approve_scope: auto_scope_meta.clone(),
                            result_ok: false,
                            result_content: reason.clone(),
                        });
                        return AgentOutcome {
                            run_id,
                            started_at,
                            finished_at: crate::trust::now_rfc3339(),
                            exit_reason: AgentExitReason::Denied,
                            final_output: format!("Tool call '{}' denied: {}", tc.name, reason),
                            error: None,
                            messages,
                            tool_calls: observed_tool_calls,
                        };
                    }
                    GateDecision::RequireApproval {
                        reason,
                        approval_id,
                        approval_key,
                    } => {
                        self.gate.record(GateEvent {
                            run_id: run_id.clone(),
                            step: step as u32,
                            tool_call_id: tc.id.clone(),
                            tool: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                            decision: "require_approval".to_string(),
                            approval_id: Some(approval_id.clone()),
                            approval_key,
                            approval_mode: approval_mode_meta.clone(),
                            auto_approve_scope: auto_scope_meta.clone(),
                            result_ok: false,
                            result_content: reason,
                        });
                        return AgentOutcome {
                            run_id,
                            started_at,
                            finished_at: crate::trust::now_rfc3339(),
                            exit_reason: AgentExitReason::ApprovalRequired,
                            final_output: format!(
                                "Approval required: {}. Run: openagent approve {} (or deny) then re-run.",
                                approval_id, approval_id
                            ),
                            error: None,
                            messages,
                            tool_calls: observed_tool_calls,
                        };
                    }
                }
            }
        }

        AgentOutcome {
            run_id,
            started_at,
            finished_at: crate::trust::now_rfc3339(),
            exit_reason: AgentExitReason::MaxSteps,
            final_output: "Max steps reached before the model produced a final answer.".to_string(),
            error: None,
            messages,
            tool_calls: observed_tool_calls,
        }
    }
}

fn tool_result_has_error(content: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(v) => v.get("error").is_some(),
        Err(_) => false,
    }
}
