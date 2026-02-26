use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;

use super::{
    sanitize_user_visible_output, Agent, AgentExitReason, McpPinEnforcementMode,
    PlanStepConstraint, PlanToolEnforcementMode, ToolCallBudget,
};
use crate::compaction::{CompactionMode, CompactionSettings, ToolResultPersist};
use crate::gate::{ApprovalMode, AutoApproveScope, GateContext, NoGate, ProviderKind};
use crate::hooks::config::HooksMode;
use crate::hooks::runner::{HookManager, HookRuntimeConfig};
use crate::operator_queue::{PendingMessageQueue, QueueLimits, QueueMessageKind};
use crate::providers::{ModelProvider, StreamDelta};
use crate::target::{ExecTargetKind, HostTarget};
use crate::tools::{ToolArgsStrict, ToolRuntime};
use crate::types::{GenerateRequest, GenerateResponse, Message, Role};

struct MockProvider {
    generate_calls: Arc<AtomicUsize>,
    stream_calls: Arc<AtomicUsize>,
    seen_messages: Arc<Mutex<Vec<Message>>>,
}

#[test]
fn sanitize_hides_thought_and_think_sections() {
    let s = "<think>internal</think>\nTHOUGHT: hidden\nRESPONSE: visible";
    assert_eq!(sanitize_user_visible_output(s), "visible");
}

#[async_trait]
impl ModelProvider for MockProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        *self.seen_messages.lock().expect("lock") = _req.messages.clone();
        self.generate_calls.fetch_add(1, Ordering::SeqCst);
        Ok(GenerateResponse {
            assistant: Message {
                role: Role::Assistant,
                content: Some("done".to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
            tool_calls: Vec::new(),
            usage: None,
        })
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn generate_streaming(
        &self,
        req: GenerateRequest,
        _on_delta: &mut (dyn FnMut(StreamDelta) + Send),
    ) -> anyhow::Result<GenerateResponse> {
        self.stream_calls.fetch_add(1, Ordering::SeqCst);
        self.generate(req).await
    }
}

#[tokio::test]
async fn non_stream_mode_uses_non_stream_generate() {
    let generate_calls = Arc::new(AtomicUsize::new(0));
    let stream_calls = Arc::new(AtomicUsize::new(0));
    let provider = MockProvider {
        generate_calls: generate_calls.clone(),
        stream_calls: stream_calls.clone(),
        seen_messages: Arc::new(Mutex::new(Vec::new())),
    };
    let mut agent = Agent {
        provider,
        model: "m".to_string(),
        tools: Vec::new(),
        max_steps: 1,
        tool_rt: ToolRuntime {
            workdir: std::env::current_dir().expect("cwd"),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: std::env::current_dir().expect("cwd"),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: None,
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: None,
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Off,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: Vec::new(),
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let out = agent.run("hi", vec![], Vec::new()).await;
    assert_eq!(out.final_output, "done");
    assert_eq!(generate_calls.load(Ordering::SeqCst), 1);
    assert_eq!(stream_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn task_memory_message_is_injected_into_transcript() {
    let seen_messages = Arc::new(Mutex::new(Vec::new()));
    let provider = MockProvider {
        generate_calls: Arc::new(AtomicUsize::new(0)),
        stream_calls: Arc::new(AtomicUsize::new(0)),
        seen_messages: seen_messages.clone(),
    };
    let mut agent = Agent {
        provider,
        model: "m".to_string(),
        tools: Vec::new(),
        max_steps: 1,
        tool_rt: ToolRuntime {
            workdir: std::env::current_dir().expect("cwd"),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: std::env::current_dir().expect("cwd"),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: None,
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: None,
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Off,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: Vec::new(),
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let mem_msg = Message {
        role: Role::Developer,
        content: Some("TASK MEMORY (user-authored, authoritative)\n- [x] T: C".to_string()),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
    };
    let out = agent.run("hi", vec![], vec![mem_msg]).await;
    assert!(out.messages.iter().any(|m| m
        .content
        .as_deref()
        .unwrap_or_default()
        .contains("TASK MEMORY")));
    assert!(seen_messages.lock().expect("lock").iter().any(|m| m
        .content
        .as_deref()
        .unwrap_or_default()
        .contains("TASK MEMORY")));
}

#[test]
fn tool_error_detection() {
    assert!(super::tool_result_has_error(
        &json!({"error":"x"}).to_string()
    ));
    assert!(!super::tool_result_has_error(
        &json!({"ok":true}).to_string()
    ));
}

#[test]
fn implementation_guard_requires_post_write_read_back() {
    let calls = vec![
        crate::types::ToolCall {
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path":"chess.html"}),
        },
        crate::types::ToolCall {
            id: "tc2".to_string(),
            name: "apply_patch".to_string(),
            arguments: json!({"path":"chess.html","patch":"@@ -1 +1 @@\n-a\n+b\n"}),
        },
    ];
    let err = crate::agent_impl_guard::implementation_integrity_violation(
        "improve chess.html file",
        "done",
        &calls,
    )
    .expect("expected guard failure");
    assert!(err.contains("post-write verification missing read_file"));
}

#[test]
fn implementation_guard_accepts_post_write_read_back() {
    let calls = vec![
        crate::types::ToolCall {
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path":"chess.html"}),
        },
        crate::types::ToolCall {
            id: "tc2".to_string(),
            name: "apply_patch".to_string(),
            arguments: json!({"path":"chess.html","patch":"@@ -1 +1 @@\n-a\n+b\n"}),
        },
        crate::types::ToolCall {
            id: "tc3".to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path":"chess.html"}),
        },
    ];
    assert!(crate::agent_impl_guard::implementation_integrity_violation(
        "improve chess.html file",
        "done",
        &calls
    )
    .is_none());
}

#[test]
fn wrapped_tool_call_content_is_parsed() {
    let raw =
        "[TOOL_CALL]\n{\"name\":\"list_dir\",\"arguments\":{\"path\":\".\"}}\n[END_TOOL_CALL]";
    let mut allowed = std::collections::BTreeSet::new();
    allowed.insert("list_dir".to_string());
    let calls = crate::agent_tool_exec::extract_wrapped_tool_calls(raw, 1, &allowed);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "list_dir");
    assert_eq!(calls[0].arguments, serde_json::json!({"path":"."}));
}

#[test]
fn wrapper_marker_detection_works() {
    assert!(super::contains_tool_wrapper_markers(
        "[TOOL_CALL]\n[END_TOOL_CALL]"
    ));
    assert!(!super::contains_tool_wrapper_markers("plain response"));
}

#[test]
fn inline_tool_call_json_is_parsed() {
    let raw = "{\"name\":\"list_dir\",\"arguments\":{\"path\":\".\"}}";
    let mut allowed = std::collections::BTreeSet::new();
    allowed.insert("list_dir".to_string());
    let tc = crate::agent_tool_exec::extract_inline_tool_call(raw, 1, &allowed).expect("tool call");
    assert_eq!(tc.name, "list_dir");
    assert_eq!(tc.arguments, serde_json::json!({"path":"."}));
}

#[test]
fn inline_tool_call_fenced_json_is_parsed() {
    let raw = "```json\n{\"name\":\"list_dir\",\"arguments\":{\"path\":\".\"}}\n```";
    let mut allowed = std::collections::BTreeSet::new();
    allowed.insert("list_dir".to_string());
    let tc = crate::agent_tool_exec::extract_inline_tool_call(raw, 1, &allowed).expect("tool call");
    assert_eq!(tc.name, "list_dir");
}

#[test]
fn tool_failure_classification_schema_and_network() {
    let tc_read = crate::types::ToolCall {
        id: "tc-schema".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"a.txt"}),
    };
    let schema_msg = json!({
        "schema_version":"openagent.tool_result.v1",
        "ok":false,
        "content":"invalid tool arguments: missing required field: path"
    })
    .to_string();
    assert_eq!(
        super::classify_tool_failure(&tc_read, &schema_msg, false).as_str(),
        "E_SCHEMA"
    );

    let tc_mcp = crate::types::ToolCall {
        id: "tc-net".to_string(),
        name: "mcp.playwright.browser_snapshot".to_string(),
        arguments: serde_json::json!({}),
    };
    let net_msg = json!({
        "schema_version":"openagent.tool_result.v1",
        "ok":false,
        "content":"mcp call failed: connection refused"
    })
    .to_string();
    assert_eq!(
        super::classify_tool_failure(&tc_mcp, &net_msg, false).as_str(),
        "E_NETWORK_TRANSIENT"
    );
}

#[test]
fn retry_policy_disables_blind_retries_for_side_effectful_tools() {
    assert_eq!(
        crate::agent_tool_exec::ToolFailureClass::TimeoutTransient
            .retry_limit_for(crate::types::SideEffects::FilesystemRead),
        1
    );
    assert_eq!(
        crate::agent_tool_exec::ToolFailureClass::TimeoutTransient
            .retry_limit_for(crate::types::SideEffects::ShellExec),
        0
    );
    assert_eq!(
        crate::agent_tool_exec::ToolFailureClass::NetworkTransient
            .retry_limit_for(crate::types::SideEffects::Browser),
        0
    );
}

struct EventCaptureSink {
    events: Arc<Mutex<Vec<crate::events::Event>>>,
}

impl crate::events::EventSink for EventCaptureSink {
    fn emit(&mut self, event: crate::events::Event) -> anyhow::Result<()> {
        self.events.lock().expect("lock").push(event);
        Ok(())
    }
}

struct ToolCallProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelProvider for ToolCallProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some(String::new()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: vec![crate::types::ToolCall {
                    id: "tc1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"a.txt"}),
                }],
                usage: None,
            })
        } else {
            Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some("done".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: Vec::new(),
                usage: None,
            })
        }
    }
}

struct NoToolProvider;

#[async_trait]
impl ModelProvider for NoToolProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        Ok(GenerateResponse {
            assistant: Message {
                role: Role::Assistant,
                content: Some("done".to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
            tool_calls: Vec::new(),
            usage: None,
        })
    }
}

struct DualToolProvider;

#[async_trait]
impl ModelProvider for DualToolProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        Ok(GenerateResponse {
            assistant: Message {
                role: Role::Assistant,
                content: Some(String::new()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
            tool_calls: vec![
                crate::types::ToolCall {
                    id: "tc1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"a.txt"}),
                },
                crate::types::ToolCall {
                    id: "tc2".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"a.txt"}),
                },
            ],
            usage: None,
        })
    }
}

struct DualThenDoneProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelProvider for DualThenDoneProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some(String::new()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: vec![
                    crate::types::ToolCall {
                        id: "tc1".to_string(),
                        name: "read_file".to_string(),
                        arguments: serde_json::json!({"path":"a.txt"}),
                    },
                    crate::types::ToolCall {
                        id: "tc2".to_string(),
                        name: "read_file".to_string(),
                        arguments: serde_json::json!({"path":"a.txt"}),
                    },
                ],
                usage: None,
            })
        } else {
            Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some("done".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: Vec::new(),
                usage: None,
            })
        }
    }
}

struct CountingNoToolProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelProvider for CountingNoToolProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(GenerateResponse {
            assistant: Message {
                role: Role::Assistant,
                content: Some("done".to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
            tool_calls: Vec::new(),
            usage: None,
        })
    }
}

struct StaticContentProvider {
    content: String,
}

#[async_trait]
impl ModelProvider for StaticContentProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        Ok(GenerateResponse {
            assistant: Message {
                role: Role::Assistant,
                content: Some(self.content.clone()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
            tool_calls: Vec::new(),
            usage: None,
        })
    }
}

struct InvalidThenValidProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelProvider for InvalidThenValidProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        match n {
            0 => Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some(String::new()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: vec![crate::types::ToolCall {
                    id: "tc_bad".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({}),
                }],
                usage: None,
            }),
            1 => Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some(String::new()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: vec![crate::types::ToolCall {
                    id: "tc_good".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"a.txt"}),
                }],
                usage: None,
            }),
            _ => Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some("done".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: Vec::new(),
                usage: None,
            }),
        }
    }
}

struct AlwaysInvalidArgsProvider;

#[async_trait]
impl ModelProvider for AlwaysInvalidArgsProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        Ok(GenerateResponse {
            assistant: Message {
                role: Role::Assistant,
                content: Some(String::new()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
            tool_calls: vec![crate::types::ToolCall {
                id: "tc_bad".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({}),
            }],
            usage: None,
        })
    }
}

struct AlwaysInvalidPatchProvider;

#[async_trait]
impl ModelProvider for AlwaysInvalidPatchProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        Ok(GenerateResponse {
            assistant: Message {
                role: Role::Assistant,
                content: Some(String::new()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
            tool_calls: vec![crate::types::ToolCall {
                id: "tc_bad_patch".to_string(),
                name: "apply_patch".to_string(),
                arguments: serde_json::json!({
                    "path":"a.txt",
                    "patch":"@@ -1 +1 @@\n-no-match\n+new\n"
                }),
            }],
            usage: None,
        })
    }
}

struct ToolOnlyProseThenToolProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelProvider for ToolOnlyProseThenToolProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        match n {
            0 => Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some("I will help with that.".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: Vec::new(),
                usage: None,
            }),
            1 => Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some(String::new()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: vec![crate::types::ToolCall {
                    id: "tc1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"a.txt"}),
                }],
                usage: None,
            }),
            _ => Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some("done".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: Vec::new(),
                usage: None,
            }),
        }
    }
}

struct ToolOnlyAlwaysProseProvider;

#[async_trait]
impl ModelProvider for ToolOnlyAlwaysProseProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        Ok(GenerateResponse {
            assistant: Message {
                role: Role::Assistant,
                content: Some("prose only".to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
            tool_calls: Vec::new(),
            usage: None,
        })
    }
}

#[tokio::test]
async fn emits_tool_exec_target_before_exec_start() {
    let tmp = tempfile::tempdir().expect("tmp");
    tokio::fs::write(tmp.path().join("a.txt"), "x")
        .await
        .expect("write");
    let events = Arc::new(Mutex::new(Vec::<crate::events::Event>::new()));
    let provider = ToolCallProvider {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let mut agent = Agent {
        provider,
        model: "m".to_string(),
        tools: vec![crate::types::ToolDef {
            name: "read_file".to_string(),
            description: "d".to_string(),
            parameters: serde_json::json!({"type":"object"}),
            side_effects: crate::types::SideEffects::FilesystemRead,
        }],
        max_steps: 3,
        tool_rt: ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: tmp.path().to_path_buf(),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: None,
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: Some(Box::new(EventCaptureSink {
            events: events.clone(),
        })),
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Off,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: Vec::new(),
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let out = agent.run("hi", vec![], Vec::new()).await;
    assert_eq!(out.final_output, "done");
    let evs = events.lock().expect("lock");
    let target_idx = evs
        .iter()
        .position(|e| matches!(e.kind, crate::events::EventKind::ToolExecTarget))
        .expect("target event");
    let start_idx = evs
        .iter()
        .position(|e| matches!(e.kind, crate::events::EventKind::ToolExecStart))
        .expect("start event");
    assert!(target_idx < start_idx);
}

#[tokio::test]
async fn plan_tool_enforcement_hard_denies_disallowed_tool() {
    let provider = ToolCallProvider {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let mut agent = Agent {
        provider,
        model: "m".to_string(),
        tools: vec![crate::types::ToolDef {
            name: "read_file".to_string(),
            description: "d".to_string(),
            parameters: serde_json::json!({"type":"object"}),
            side_effects: crate::types::SideEffects::FilesystemRead,
        }],
        max_steps: 2,
        tool_rt: ToolRuntime {
            workdir: std::env::current_dir().expect("cwd"),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: std::env::current_dir().expect("cwd"),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: Some("plan123".to_string()),
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: None,
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Hard,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: vec![PlanStepConstraint {
            step_id: "S1".to_string(),
            intended_tools: vec!["list_dir".to_string()],
        }],
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let out = agent.run("hi", vec![], Vec::new()).await;
    assert!(matches!(out.exit_reason, AgentExitReason::Denied));
    assert!(out.final_output.contains("is not allowed for plan step S1"));
    assert!(out
        .tool_decisions
        .iter()
        .any(|d| d.source.as_deref() == Some("plan_step_constraint")));
}

#[tokio::test]
async fn operator_interrupt_delivers_post_tool_and_cancels_remaining_turn_work() {
    let tmp = tempfile::tempdir().expect("tmp");
    tokio::fs::write(tmp.path().join("a.txt"), "x")
        .await
        .expect("write");
    let events = Arc::new(Mutex::new(Vec::<crate::events::Event>::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = DualThenDoneProvider {
        calls: calls.clone(),
    };
    let mut agent = Agent {
        provider,
        model: "m".to_string(),
        tools: vec![crate::types::ToolDef {
            name: "read_file".to_string(),
            description: "d".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            side_effects: crate::types::SideEffects::FilesystemRead,
        }],
        max_steps: 4,
        tool_rt: ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: tmp.path().to_path_buf(),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: None,
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: Some(Box::new(EventCaptureSink {
            events: events.clone(),
        })),
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Off,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: Vec::new(),
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let _ = agent.queue_operator_message(QueueMessageKind::Steer, "interrupt now");
    let out = agent.run("hi", vec![], Vec::new()).await;
    assert!(matches!(out.exit_reason, AgentExitReason::Ok));
    assert_eq!(out.final_output, "done");
    // second tool in first response should be skipped due to interrupt-after-post_tool
    assert_eq!(out.tool_calls.iter().filter(|t| t.id == "tc2").count(), 0);
    let evs = events.lock().expect("lock");
    let kinds = evs
        .iter()
        .map(|e| format!("{:?}", e.kind))
        .collect::<Vec<_>>();
    assert!(kinds.iter().any(|k| k == "QueueDelivered"));
    assert!(kinds.iter().any(|k| k == "QueueInterrupt"));
    let delivered = evs
        .iter()
        .find(|e| matches!(e.kind, crate::events::EventKind::QueueDelivered))
        .expect("queue delivered");
    assert_eq!(
        delivered
            .data
            .get("delivery_boundary")
            .and_then(|v| v.as_str())
            .unwrap_or_default(),
        "post_tool"
    );
}

#[tokio::test]
async fn operator_next_delivers_at_turn_idle_without_interrupt() {
    let calls = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(Mutex::new(Vec::<crate::events::Event>::new()));
    let provider = CountingNoToolProvider {
        calls: calls.clone(),
    };
    let mut agent = Agent {
        provider,
        model: "m".to_string(),
        tools: Vec::new(),
        max_steps: 4,
        tool_rt: ToolRuntime {
            workdir: std::env::current_dir().expect("cwd"),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: std::env::current_dir().expect("cwd"),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: None,
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: Some(Box::new(EventCaptureSink {
            events: events.clone(),
        })),
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Off,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: Vec::new(),
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let _ = agent.queue_operator_message(QueueMessageKind::FollowUp, "next message");
    let out = agent.run("hi", vec![], Vec::new()).await;
    assert!(matches!(out.exit_reason, AgentExitReason::Ok));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let evs = events.lock().expect("lock");
    let delivered = evs
        .iter()
        .find(|e| matches!(e.kind, crate::events::EventKind::QueueDelivered))
        .expect("queue delivered");
    assert_eq!(
        delivered
            .data
            .get("delivery_boundary")
            .and_then(|v| v.as_str())
            .unwrap_or_default(),
        "turn_idle"
    );
    assert!(!evs
        .iter()
        .any(|e| matches!(e.kind, crate::events::EventKind::QueueInterrupt)));
}

#[tokio::test]
async fn halting_is_blocked_when_plan_steps_are_pending() {
    let mut agent = Agent {
        provider: NoToolProvider,
        model: "m".to_string(),
        tools: vec![crate::types::ToolDef {
            name: "read_file".to_string(),
            description: "d".to_string(),
            parameters: serde_json::json!({"type":"object"}),
            side_effects: crate::types::SideEffects::FilesystemRead,
        }],
        max_steps: 3,
        tool_rt: ToolRuntime {
            workdir: std::env::current_dir().expect("cwd"),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: std::env::current_dir().expect("cwd"),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: Some("plan123".to_string()),
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: None,
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Hard,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: vec![PlanStepConstraint {
            step_id: "S1".to_string(),
            intended_tools: vec!["read_file".to_string()],
        }],
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let out = agent.run("hi", vec![], Vec::new()).await;
    assert!(matches!(out.exit_reason, AgentExitReason::PlannerError));
    let err = out.error.as_deref().unwrap_or_default();
    assert!(err.contains("halt") || err.contains("control envelope"));
}

#[tokio::test]
async fn emits_step_lifecycle_events_for_pending_plan_halt() {
    let events = Arc::new(Mutex::new(Vec::<crate::events::Event>::new()));
    let mut agent = Agent {
        provider: NoToolProvider,
        model: "m".to_string(),
        tools: vec![crate::types::ToolDef {
            name: "read_file".to_string(),
            description: "d".to_string(),
            parameters: serde_json::json!({"type":"object"}),
            side_effects: crate::types::SideEffects::FilesystemRead,
        }],
        max_steps: 2,
        tool_rt: ToolRuntime {
            workdir: std::env::current_dir().expect("cwd"),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: std::env::current_dir().expect("cwd"),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: Some("plan123".to_string()),
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: Some(Box::new(EventCaptureSink {
            events: events.clone(),
        })),
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Hard,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: vec![PlanStepConstraint {
            step_id: "S1".to_string(),
            intended_tools: vec!["read_file".to_string()],
        }],
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let out = agent.run("hi", vec![], Vec::new()).await;
    assert!(matches!(out.exit_reason, AgentExitReason::PlannerError));
    let evs = events.lock().expect("lock");
    assert!(evs
        .iter()
        .any(|e| matches!(e.kind, crate::events::EventKind::StepStarted)));
    assert!(evs
        .iter()
        .any(|e| matches!(e.kind, crate::events::EventKind::StepBlocked)));
}

#[tokio::test]
async fn tool_budget_exceeded_returns_deterministic_exit() {
    let tmp = tempfile::tempdir().expect("tmp");
    tokio::fs::write(tmp.path().join("a.txt"), "x")
        .await
        .expect("write");
    let mut agent = Agent {
        provider: DualToolProvider,
        model: "m".to_string(),
        tools: vec![crate::types::ToolDef {
            name: "read_file".to_string(),
            description: "d".to_string(),
            parameters: serde_json::json!({"type":"object"}),
            side_effects: crate::types::SideEffects::FilesystemRead,
        }],
        max_steps: 1,
        tool_rt: ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: tmp.path().to_path_buf(),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: None,
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: None,
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Off,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: Vec::new(),
        tool_call_budget: ToolCallBudget {
            max_total_tool_calls: 1,
            ..ToolCallBudget::default()
        },
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let out = agent.run("hi", vec![], Vec::new()).await;
    assert!(matches!(out.exit_reason, AgentExitReason::BudgetExceeded));
    assert!(out
        .tool_decisions
        .iter()
        .any(|d| d.source.as_deref() == Some("runtime_budget")));
}

#[tokio::test]
async fn planner_enforced_final_output_uses_user_output_field() {
    let provider = StaticContentProvider {
            content: r#"{"schema_version":"openagent.step_result.v1","step_id":"S1","status":"done","next_step_id":"final","user_output":"all checks passed"}"#.to_string(),
        };
    let mut agent = Agent {
        provider,
        model: "m".to_string(),
        tools: vec![crate::types::ToolDef {
            name: "read_file".to_string(),
            description: "d".to_string(),
            parameters: serde_json::json!({"type":"object"}),
            side_effects: crate::types::SideEffects::FilesystemRead,
        }],
        max_steps: 2,
        tool_rt: ToolRuntime {
            workdir: std::env::current_dir().expect("cwd"),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: std::env::current_dir().expect("cwd"),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: Some("plan123".to_string()),
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: None,
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Hard,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: vec![PlanStepConstraint {
            step_id: "S1".to_string(),
            intended_tools: vec!["read_file".to_string()],
        }],
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let out = agent.run("hi", vec![], Vec::new()).await;
    assert!(matches!(out.exit_reason, AgentExitReason::Ok));
    assert_eq!(out.final_output, "all checks passed");
}

#[tokio::test]
async fn schema_repair_retry_happens_before_execution() {
    let tmp = tempfile::tempdir().expect("tmp");
    tokio::fs::write(tmp.path().join("a.txt"), "x")
        .await
        .expect("write");
    let events = Arc::new(Mutex::new(Vec::<crate::events::Event>::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = InvalidThenValidProvider {
        calls: calls.clone(),
    };
    let mut agent = Agent {
        provider,
        model: "m".to_string(),
        tools: vec![crate::types::ToolDef {
            name: "read_file".to_string(),
            description: "d".to_string(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{"path":{"type":"string"}},
                "required":["path"]
            }),
            side_effects: crate::types::SideEffects::FilesystemRead,
        }],
        max_steps: 4,
        tool_rt: ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: tmp.path().to_path_buf(),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: None,
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: Some(Box::new(EventCaptureSink {
            events: events.clone(),
        })),
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Off,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: Vec::new(),
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let out = agent.run("hi", vec![], Vec::new()).await;
    assert!(matches!(out.exit_reason, AgentExitReason::Ok));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let evs = events.lock().expect("lock");
    assert!(evs.iter().any(|e| {
        matches!(e.kind, crate::events::EventKind::ToolRetry)
            && e.data
                .get("failure_class")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                == "E_SCHEMA"
            && e.data
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                == "repair"
    }));
}

#[tokio::test]
async fn repeated_malformed_tool_calls_fail_fast_with_protocol_violation() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut agent = Agent {
        provider: AlwaysInvalidArgsProvider,
        model: "m".to_string(),
        tools: vec![crate::types::ToolDef {
            name: "read_file".to_string(),
            description: "d".to_string(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{"path":{"type":"string"}},
                "required":["path"]
            }),
            side_effects: crate::types::SideEffects::FilesystemRead,
        }],
        max_steps: 8,
        tool_rt: ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: tmp.path().to_path_buf(),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: None,
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: None,
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Off,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: Vec::new(),
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let out = agent.run("hi", vec![], Vec::new()).await;
    assert!(
        matches!(out.exit_reason, AgentExitReason::PlannerError),
        "unexpected exit={:?} error={:?}",
        out.exit_reason,
        out.error
    );
    assert!(out
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("MODEL_TOOL_PROTOCOL_VIOLATION"));
}

#[tokio::test]
async fn repeated_invalid_patch_format_fails_fast_with_protocol_violation() {
    let tmp = tempfile::tempdir().expect("tmp");
    tokio::fs::write(tmp.path().join("a.txt"), "hello\n")
        .await
        .expect("write");
    let mut agent = Agent {
        provider: AlwaysInvalidPatchProvider,
        model: "m".to_string(),
        tools: vec![crate::types::ToolDef {
            name: "apply_patch".to_string(),
            description: "d".to_string(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{"path":{"type":"string"},"patch":{"type":"string"}},
                "required":["path","patch"]
            }),
            side_effects: crate::types::SideEffects::FilesystemWrite,
        }],
        max_steps: 8,
        tool_rt: ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: true,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_write: true,
            approval_mode: ApprovalMode::Interrupt,
            auto_approve_scope: AutoApproveScope::Run,
            unsafe_mode: false,
            unsafe_bypass_allow_flags: false,
            run_id: None,
            enable_write_tools: true,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: None,
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: None,
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Off,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: Vec::new(),
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let out = agent.run("hi", vec![], Vec::new()).await;
    assert!(
        matches!(out.exit_reason, AgentExitReason::PlannerError),
        "unexpected exit={:?} error={:?}",
        out.exit_reason,
        out.error
    );
    assert!(out
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("repeated invalid patch format"));
}

#[tokio::test]
async fn tool_only_prompt_repairs_once_then_allows_tool_call() {
    let tmp = tempfile::tempdir().expect("tmp");
    tokio::fs::write(tmp.path().join("a.txt"), "x")
        .await
        .expect("write");
    let calls = Arc::new(AtomicUsize::new(0));
    let mut agent = Agent {
        provider: ToolOnlyProseThenToolProvider {
            calls: calls.clone(),
        },
        model: "m".to_string(),
        tools: vec![crate::types::ToolDef {
            name: "read_file".to_string(),
            description: "d".to_string(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{"path":{"type":"string"}},
                "required":["path"]
            }),
            side_effects: crate::types::SideEffects::FilesystemRead,
        }],
        max_steps: 5,
        tool_rt: ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: tmp.path().to_path_buf(),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: None,
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: None,
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Off,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: Vec::new(),
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let out = agent
        .run(
            "Emit exactly one tool call and no prose.",
            vec![],
            Vec::new(),
        )
        .await;
    assert!(matches!(out.exit_reason, AgentExitReason::Ok));
    assert!(out.tool_calls.iter().any(|t| t.name == "read_file"));
    assert!(calls.load(Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn tool_only_prompt_repeated_prose_fails_fast() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut agent = Agent {
        provider: ToolOnlyAlwaysProseProvider,
        model: "m".to_string(),
        tools: vec![crate::types::ToolDef {
            name: "list_dir".to_string(),
            description: "d".to_string(),
            parameters: serde_json::json!({
                "type":"object",
                "properties":{"path":{"type":"string"}},
                "required":["path"]
            }),
            side_effects: crate::types::SideEffects::FilesystemRead,
        }],
        max_steps: 4,
        tool_rt: ToolRuntime {
            workdir: tmp.path().to_path_buf(),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: tmp.path().to_path_buf(),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: None,
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: None,
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Off,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: Vec::new(),
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let out = agent
        .run(
            "Emit exactly one tool call and no prose.",
            vec![],
            Vec::new(),
        )
        .await;
    assert!(matches!(out.exit_reason, AgentExitReason::PlannerError));
    assert!(out
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("tool-only phase"));
}

#[tokio::test]
async fn invalid_done_transition_fails_with_planner_error() {
    let content = serde_json::json!({
        "schema_version": crate::planner::STEP_RESULT_SCHEMA_VERSION,
        "step_id": "S2",
        "status": "done",
        "evidence": ["ok"]
    })
    .to_string();
    let mut agent = Agent {
        provider: StaticContentProvider { content },
        model: "m".to_string(),
        tools: Vec::new(),
        max_steps: 1,
        tool_rt: ToolRuntime {
            workdir: std::env::current_dir().expect("cwd"),
            allow_shell: false,
            allow_shell_in_workdir_only: false,
            allow_write: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: std::sync::Arc::new(HostTarget),
        },
        gate: Box::new(NoGate::new()),
        gate_ctx: GateContext {
            workdir: std::env::current_dir().expect("cwd"),
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
            provider: ProviderKind::Ollama,
            model: "m".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: crate::gate::ApprovalKeyVersion::V1,
            tool_schema_hashes: std::collections::BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: Some("plan123".to_string()),
            taint_enabled: false,
            taint_mode: crate::taint::TaintMode::Propagate,
            taint_overall: crate::taint::TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: None,
        compaction_settings: CompactionSettings {
            max_context_chars: 0,
            mode: CompactionMode::Off,
            keep_last: 20,
            tool_result_persist: ToolResultPersist::Digest,
        },
        hooks: HookManager::build(HookRuntimeConfig {
            mode: HooksMode::Off,
            config_path: std::env::temp_dir().join("unused_hooks.yaml"),
            strict: false,
            timeout_ms: 1000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: crate::taint::TaintToggle::Off,
        taint_mode: crate::taint::TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Hard,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: vec![
            PlanStepConstraint {
                step_id: "S1".to_string(),
                intended_tools: Vec::new(),
            },
            PlanStepConstraint {
                step_id: "S2".to_string(),
                intended_tools: Vec::new(),
            },
        ],
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let out = agent.run("hi", vec![], Vec::new()).await;
    assert!(matches!(out.exit_reason, AgentExitReason::PlannerError));
    assert!(out
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("invalid step completion transition"));
}

#[test]
fn taint_spans_browser_deterministic() {
    let tc = crate::types::ToolCall {
        id: "tc1".to_string(),
        name: "mcp.playwright.browser_snapshot".to_string(),
        arguments: serde_json::json!({}),
    };
    let content = serde_json::json!({
        "schema_version":"openagent.tool_result.v1",
        "content":"OPENAGENT_FIXTURE_OK"
    })
    .to_string();
    let a = crate::agent_taint_helpers::compute_taint_spans_for_tool(&tc, &content, None, 8);
    let b = crate::agent_taint_helpers::compute_taint_spans_for_tool(&tc, &content, None, 8);
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].source, "browser");
    assert_eq!(a[0].digest, b[0].digest);
}

#[test]
fn taint_file_glob_matches_read_file() {
    let policy = crate::trust::policy::Policy::from_yaml(
        r#"
version: 2
default: deny
taint:
  file_path_globs: ["**/.env"]
"#,
    )
    .expect("policy");
    let tc = crate::types::ToolCall {
        id: "tcf".to_string(),
        name: "read_file".to_string(),
        arguments: serde_json::json!({"path":"repo/.env"}),
    };
    let spans =
        crate::agent_taint_helpers::compute_taint_spans_for_tool(&tc, "secret", Some(&policy), 16);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].source, "file");
    assert!(spans[0].detail.contains("matched taint glob"));
}
