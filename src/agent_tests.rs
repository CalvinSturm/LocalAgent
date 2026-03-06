use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::time::{sleep, Duration};

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
use crate::target::{
    ExecTarget, ExecTargetKind, HostTarget, ListReq, PatchReq, ReadReq, ShellReq, TargetDescribe,
    TargetResult, WriteReq,
};
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

#[test]
fn split_user_visible_and_thinking_extracts_think_blocks() {
    let s = "<think>first idea</think>\nVisible answer\n<think>second idea</think>";
    let (visible, thinking) = crate::agent_output_sanitize::split_user_visible_and_thinking(s);
    assert_eq!(visible, "Visible answer");
    assert_eq!(thinking.as_deref(), Some("first idea\n\nsecond idea"));
}

#[test]
fn split_streaming_extracts_unclosed_think_block() {
    let s = "<think>working plan step 1\nstep 2";
    let (visible, thinking) =
        crate::agent_output_sanitize::split_user_visible_and_thinking_streaming(s);
    assert_eq!(visible, "");
    assert_eq!(thinking.as_deref(), Some("working plan step 1\nstep 2"));
}

#[test]
fn split_streaming_shows_only_post_think_visible_text() {
    let s = "<think>internal plan</think>\nFinal answer";
    let (visible, thinking) =
        crate::agent_output_sanitize::split_user_visible_and_thinking_streaming(s);
    assert_eq!(visible, "Final answer");
    assert_eq!(thinking.as_deref(), Some("internal plan"));
}

#[test]
fn runtime_completion_decision_executes_tools_when_tool_calls_present() {
    let d = super::runtime_completion_decision(&super::RuntimeCompletionInputs {
        has_tool_calls: true,
        plan_tool_enforcement: PlanToolEnforcementMode::Off,
        active_plan_step_idx: 0,
        plan_step_constraints_len: 0,
        tool_only_phase_active: false,
        enforce_implementation_integrity_guard: false,
        observed_tool_calls_len: 0,
        blocked_attempt_count_next: 1,
    });
    assert!(matches!(d, super::RuntimeCompletionDecision::ExecuteTools));
}

#[test]
fn runtime_completion_decision_no_tool_call_but_not_complete_returns_continue() {
    let d = super::runtime_completion_decision(&super::RuntimeCompletionInputs {
        has_tool_calls: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Hard,
        active_plan_step_idx: 0,
        plan_step_constraints_len: 1,
        tool_only_phase_active: false,
        enforce_implementation_integrity_guard: false,
        observed_tool_calls_len: 0,
        blocked_attempt_count_next: 1,
    });
    assert!(matches!(
        d,
        super::RuntimeCompletionDecision::Continue { .. }
    ));
}

#[test]
fn runtime_completion_decision_runtime_complete_returns_finalize_ok() {
    let d = super::runtime_completion_decision(&super::RuntimeCompletionInputs {
        has_tool_calls: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Off,
        active_plan_step_idx: 0,
        plan_step_constraints_len: 0,
        tool_only_phase_active: false,
        enforce_implementation_integrity_guard: false,
        observed_tool_calls_len: 1,
        blocked_attempt_count_next: 1,
    });
    assert!(matches!(d, super::RuntimeCompletionDecision::FinalizeOk));
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
async fn compaction_failure_emits_run_end_provider_error() {
    let tmp = tempfile::tempdir().expect("tmp");
    let events = Arc::new(Mutex::new(Vec::<crate::events::Event>::new()));
    let mut agent = Agent {
        provider: NoToolProvider,
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
        tools: Vec::new(),
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
        event_sink: Some(Box::new(EventCaptureSink {
            events: events.clone(),
        })),
        compaction_settings: CompactionSettings {
            max_context_chars: 1024,
            mode: CompactionMode::Summary,
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
            "hi",
            vec![Message {
                role: Role::System,
                content: Some("__FORCE_COMPACTION_ERROR__".to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            }],
            Vec::new(),
        )
        .await;
    assert!(matches!(out.exit_reason, AgentExitReason::ProviderError));
    assert!(out
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("forced compaction error"));
    let evs = events.lock().expect("lock");
    assert!(evs.iter().any(|e| {
        matches!(e.kind, crate::events::EventKind::RunEnd)
            && e.data
                .get("exit_reason")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s == "provider_error")
    }));
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
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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

#[tokio::test]
async fn build_initial_messages_contains_tool_contract_version_marker() {
    let seen_messages = Arc::new(Mutex::new(Vec::new()));
    let provider = MockProvider {
        generate_calls: Arc::new(AtomicUsize::new(0)),
        stream_calls: Arc::new(AtomicUsize::new(0)),
        seen_messages: seen_messages.clone(),
    };
    let mut agent = Agent {
        provider,
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
    let out = agent.run("hello", vec![], Vec::new()).await;
    let sys = out
        .messages
        .iter()
        .find(|m| matches!(m.role, Role::System))
        .and_then(|m| m.content.as_deref())
        .unwrap_or_default()
        .to_string();
    assert!(sys.contains("TOOL_CONTRACT_VERSION: v1"));
    assert!(sys.contains("Emit at most one tool call per assistant step."));
    assert!(sys.contains("[TOOL_CALL]"));
    assert!(sys.contains("[END_TOOL_CALL]"));
}

#[test]
fn tool_error_detection() {
    assert!(super::tool_result_has_error(
        &json!({"error":"x"}).to_string()
    ));
    assert!(!super::tool_result_has_error(
        &json!({"ok":true}).to_string()
    ));
    assert_eq!(
        crate::agent_tool_exec::tool_result_error_code(
            &json!({"ok":false,"error":{"code":"tool_unknown"}}).to_string()
        )
        .map(|c| c.as_str()),
        Some("tool_unknown")
    );
    assert_eq!(
        crate::agent_tool_exec::tool_result_error_code(
            &json!({"ok":false,"error":{"code":"shell_gate_deny"}}).to_string()
        )
        .map(|c| c.as_str()),
        Some("shell_gate_deny")
    );
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
fn implementation_guard_requires_post_write_read_back_without_bypass() {
    let calls = vec![
        crate::types::ToolCall {
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path":"main.rs"}),
        },
        crate::types::ToolCall {
            id: "tc2".to_string(),
            name: "apply_patch".to_string(),
            arguments: json!({"path":"main.rs","patch":"@@ -1 +1 @@\n-a\n+b\n"}),
        },
    ];
    let executions = vec![
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "read_file".to_string(),
            path: Some("main.rs".to_string()),
            ok: true,
            changed: None,
        },
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "apply_patch".to_string(),
            path: Some("main.rs".to_string()),
            ok: true,
            changed: None,
        },
    ];
    let err = crate::agent_impl_guard::implementation_integrity_violation_with_tool_executions(
        "Edit main.rs using apply_patch and confirm done.",
        "done",
        &calls,
        &executions,
        true,
    )
    .expect("expected guard failure");
    assert!(err.contains("post-write verification missing read_file on 'main.rs'"));
}

#[test]
fn implementation_guard_requires_successful_read_before_apply_patch() {
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
    let executions = vec![
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "read_file".to_string(),
            path: Some("chess.html".to_string()),
            ok: false,
            changed: None,
        },
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "apply_patch".to_string(),
            path: Some("chess.html".to_string()),
            ok: true,
            changed: None,
        },
    ];
    let err = crate::agent_impl_guard::implementation_integrity_violation_with_tool_executions(
        "improve chess.html file",
        "done",
        &calls,
        &executions,
        true,
    )
    .expect("expected guard failure");
    assert!(err.contains("apply_patch on 'chess.html' requires prior read_file"));
}

#[test]
fn implementation_guard_requires_successful_post_write_read_back() {
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
    let executions = vec![
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "read_file".to_string(),
            path: Some("chess.html".to_string()),
            ok: true,
            changed: None,
        },
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "apply_patch".to_string(),
            path: Some("chess.html".to_string()),
            ok: true,
            changed: None,
        },
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "read_file".to_string(),
            path: Some("chess.html".to_string()),
            ok: false,
            changed: None,
        },
    ];
    let err = crate::agent_impl_guard::implementation_integrity_violation_with_tool_executions(
        "improve chess.html file",
        "done",
        &calls,
        &executions,
        true,
    )
    .expect("expected guard failure");
    assert!(err.contains("post-write verification missing read_file on 'chess.html'"));
}

#[test]
fn implementation_guard_rejects_noop_apply_patch_even_with_read_back() {
    let calls = vec![
        crate::types::ToolCall {
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path":"main.rs"}),
        },
        crate::types::ToolCall {
            id: "tc2".to_string(),
            name: "apply_patch".to_string(),
            arguments: json!({"path":"main.rs","patch":"@@ -1,3 +1,3 @@\n fn answer() -> i32 {\n-    return 1;\n+    return 1;\n }\n"}),
        },
        crate::types::ToolCall {
            id: "tc3".to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path":"main.rs"}),
        },
    ];
    let executions = vec![
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "read_file".to_string(),
            path: Some("main.rs".to_string()),
            ok: true,
            changed: None,
        },
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "apply_patch".to_string(),
            path: Some("main.rs".to_string()),
            ok: true,
            changed: None,
        },
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "read_file".to_string(),
            path: Some("main.rs".to_string()),
            ok: true,
            changed: None,
        },
    ];
    let err = crate::agent_impl_guard::implementation_integrity_violation_with_tool_executions(
        "Edit main.rs using apply_patch and confirm done.",
        "done",
        &calls,
        &executions,
        true,
    );
    assert!(
        err.is_none(),
        "noop apply_patch with successful post-write read-back should be accepted"
    );
}

#[test]
fn implementation_guard_accepts_dot_prefixed_post_write_read_back() {
    let calls = vec![
        crate::types::ToolCall {
            id: "tc1".to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path":"./main.rs"}),
        },
        crate::types::ToolCall {
            id: "tc2".to_string(),
            name: "apply_patch".to_string(),
            arguments: json!({"path":"main.rs","patch":"@@ -1 +1 @@\n-a\n+b\n"}),
        },
        crate::types::ToolCall {
            id: "tc3".to_string(),
            name: "read_file".to_string(),
            arguments: json!({"path":"./main.rs"}),
        },
    ];
    let executions = vec![
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "read_file".to_string(),
            path: Some(crate::agent_impl_guard::normalize_tool_path("./main.rs")),
            ok: true,
            changed: None,
        },
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "apply_patch".to_string(),
            path: Some(crate::agent_impl_guard::normalize_tool_path("main.rs")),
            ok: true,
            changed: None,
        },
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "read_file".to_string(),
            path: Some(crate::agent_impl_guard::normalize_tool_path("./main.rs")),
            ok: true,
            changed: None,
        },
    ];
    let err = crate::agent_impl_guard::implementation_integrity_violation_with_tool_executions(
        "improve main.rs file",
        "done",
        &calls,
        &executions,
        true,
    );
    assert!(
        err.is_none(),
        "dot-prefixed read_file should satisfy verification"
    );
}

#[test]
fn implementation_guard_requires_explicit_enforcement_signal() {
    let calls = vec![crate::types::ToolCall {
        id: "tc1".to_string(),
        name: "apply_patch".to_string(),
        arguments: json!({"path":"main.rs","patch":"@@ -1 +1 @@\n-a\n+b\n"}),
    }];
    let executions = vec![crate::agent_impl_guard::ToolExecutionRecord {
        name: "apply_patch".to_string(),
        path: Some("main.rs".to_string()),
        ok: true,
        changed: None,
    }];
    let err = crate::agent_impl_guard::implementation_integrity_violation_with_tool_executions(
        "improve main.rs",
        "done",
        &calls,
        &executions,
        false,
    );
    assert!(
        err.is_none(),
        "guard must be off without explicit enforcement"
    );
}

#[test]
fn pending_post_write_verification_paths_tracks_only_unverified_writes() {
    let executions = vec![
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "read_file".to_string(),
            path: Some("a.rs".to_string()),
            ok: true,
            changed: None,
        },
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "apply_patch".to_string(),
            path: Some("a.rs".to_string()),
            ok: true,
            changed: None,
        },
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "write_file".to_string(),
            path: Some("b.rs".to_string()),
            ok: true,
            changed: None,
        },
        crate::agent_impl_guard::ToolExecutionRecord {
            name: "read_file".to_string(),
            path: Some("a.rs".to_string()),
            ok: true,
            changed: None,
        },
    ];
    let pending = crate::agent_impl_guard::pending_post_write_verification_paths(&executions);
    assert_eq!(pending.len(), 1);
    assert!(pending.contains("b.rs"));
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

struct AlwaysToolProvider;

#[async_trait]
impl ModelProvider for AlwaysToolProvider {
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
                id: "tc_repeat".to_string(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path":"a.txt"}),
            }],
            usage: None,
        })
    }
}

struct ReadPatchThenDoneProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelProvider for ReadPatchThenDoneProvider {
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
                    id: "tc_read".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"main.rs"}),
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
                    id: "tc_patch".to_string(),
                    name: "apply_patch".to_string(),
                    arguments: serde_json::json!({
                        "path":"main.rs",
                        "patch":"@@ -1,3 +1,3 @@\n fn answer() -> i32 {\n-    return 1;\n+    return 2;\n }\n"
                    }),
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

struct ReadThenDoneProvider {
    calls: Arc<AtomicUsize>,
}

struct ReadNoopPatchThenDoneProvider {
    calls: Arc<AtomicUsize>,
}

struct ReadThenDoneThenPatchThenDoneProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelProvider for ReadNoopPatchThenDoneProvider {
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
                    id: "tc_read".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"main.rs"}),
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
                    id: "tc_patch".to_string(),
                    name: "apply_patch".to_string(),
                    arguments: serde_json::json!({
                        "path":"main.rs",
                        "patch":"@@ -1,3 +1,3 @@\n fn answer() -> i32 {\n-    return 1;\n+    return 1;\n }\n"
                    }),
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

#[async_trait]
impl ModelProvider for ReadThenDoneThenPatchThenDoneProvider {
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
                    id: "tc_read".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path":"main.rs"}),
                }],
                usage: None,
            }),
            1 => Ok(GenerateResponse {
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
            2 => Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some(String::new()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: vec![crate::types::ToolCall {
                    id: "tc_patch".to_string(),
                    name: "apply_patch".to_string(),
                    arguments: serde_json::json!({
                        "path":"main.rs",
                        "patch":"@@ -1,3 +1,3 @@\n fn answer() -> i32 {\n-    return 1;\n+    return 2;\n }\n"
                    }),
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

#[async_trait]
impl ModelProvider for ReadThenDoneProvider {
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
                    id: "tc_read".to_string(),
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

#[derive(Clone)]
struct SlowReadExecTarget {
    host: HostTarget,
    read_calls: Arc<AtomicUsize>,
    hang_on_call: usize,
    delay_ms: u64,
}

#[async_trait]
impl ExecTarget for SlowReadExecTarget {
    fn kind(&self) -> ExecTargetKind {
        ExecTargetKind::Host
    }

    fn describe(&self) -> TargetDescribe {
        self.host.describe()
    }

    async fn exec_shell(&self, req: ShellReq) -> TargetResult {
        self.host.exec_shell(req).await
    }

    async fn read_file(&self, req: ReadReq) -> TargetResult {
        let call_idx = self.read_calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call_idx == self.hang_on_call {
            sleep(Duration::from_millis(self.delay_ms)).await;
        }
        self.host.read_file(req).await
    }

    async fn list_dir(&self, req: ListReq) -> TargetResult {
        self.host.list_dir(req).await
    }

    async fn write_file(&self, req: WriteReq) -> TargetResult {
        self.host.write_file(req).await
    }

    async fn apply_patch(&self, req: PatchReq) -> TargetResult {
        self.host.apply_patch(req).await
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

struct AlwaysUnknownToolProvider;

#[async_trait]
impl ModelProvider for AlwaysUnknownToolProvider {
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
                id: "tc_unknown_loop".to_string(),
                name: "grep_search".to_string(),
                arguments: serde_json::json!({"path":"."}),
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
                    "patch":"@@ -1 +1 @@\n"
                }),
            }],
            usage: None,
        })
    }
}

struct UniqueInvalidPatchProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelProvider for UniqueInvalidPatchProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        let path = format!("p{n}.txt");
        Ok(GenerateResponse {
            assistant: Message {
                role: Role::Assistant,
                content: Some(String::new()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
            tool_calls: vec![crate::types::ToolCall {
                id: format!("tc_bad_patch_{n}"),
                name: "apply_patch".to_string(),
                arguments: serde_json::json!({
                    "path": path,
                    "patch":"@@ -1 +1 @@\n"
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
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
    let provider = ToolCallProvider {
        calls: calls.clone(),
    };
    let mut agent = Agent {
        provider,
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
        tools: vec![crate::types::ToolDef {
            name: "read_file".to_string(),
            description: "d".to_string(),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            side_effects: crate::types::SideEffects::FilesystemRead,
        }],
        max_steps: 2,
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
    assert_eq!(out.tool_calls.iter().filter(|t| t.id == "tc1").count(), 1);
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
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
        provider: AlwaysToolProvider,
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
        tools: vec![crate::types::ToolDef {
            name: "read_file".to_string(),
            description: "d".to_string(),
            parameters: serde_json::json!({"type":"object"}),
            side_effects: crate::types::SideEffects::FilesystemRead,
        }],
        max_steps: 2,
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
async fn multiple_tool_calls_in_single_step_fail_with_protocol_violation() {
    let tmp = tempfile::tempdir().expect("tmp");
    tokio::fs::write(tmp.path().join("a.txt"), "x")
        .await
        .expect("write");
    let mut agent = Agent {
        provider: DualToolProvider,
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
        .contains("multiple tool calls in a single assistant step"));
}

#[tokio::test]
async fn planner_enforced_final_output_uses_user_output_field() {
    let provider = StaticContentProvider {
            content: r#"{"schema_version":"openagent.step_result.v1","step_id":"S1","status":"done","next_step_id":"final","user_output":"all checks passed"}"#.to_string(),
        };
    let mut agent = Agent {
        provider,
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
            && e.data
                .get("error_code")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                == "tool_args_invalid"
    }));
}

#[tokio::test]
async fn repeated_malformed_tool_calls_fail_fast_with_protocol_violation() {
    let tmp = tempfile::tempdir().expect("tmp");
    let mut agent = Agent {
        provider: AlwaysInvalidArgsProvider,
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
async fn repeated_failed_unknown_tool_calls_are_blocked_by_repeat_guard() {
    let tmp = tempfile::tempdir().expect("tmp");
    let events = Arc::new(Mutex::new(Vec::<crate::events::Event>::new()));
    let mut agent = Agent {
        provider: AlwaysUnknownToolProvider,
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
        max_steps: 10,
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
    assert!(matches!(out.exit_reason, AgentExitReason::PlannerError));
    assert!(out.final_output.contains("TOOL_REPEAT_BLOCKED"));
    let evs = events.lock().expect("lock");
    assert!(evs.iter().any(|e| {
        matches!(e.kind, crate::events::EventKind::StepBlocked)
            && e.data
                .get("code")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                == "TOOL_REPEAT_BLOCKED"
    }));
}

#[tokio::test]
async fn repeated_invalid_patch_format_fails_fast_with_protocol_violation() {
    let tmp = tempfile::tempdir().expect("tmp");
    let events = Arc::new(Mutex::new(Vec::<crate::events::Event>::new()));
    tokio::fs::write(tmp.path().join("a.txt"), "hello\n")
        .await
        .expect("write");
    let mut agent = Agent {
        provider: AlwaysInvalidPatchProvider,
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
    let evs = events.lock().expect("lock");
    let starts = evs
        .iter()
        .filter(|e| matches!(e.kind, crate::events::EventKind::ToolExecStart))
        .count();
    let ends = evs
        .iter()
        .filter(|e| matches!(e.kind, crate::events::EventKind::ToolExecEnd))
        .count();
    assert_eq!(starts, ends, "tool exec start/end mismatch");
}

#[tokio::test]
async fn runtime_post_write_verification_allows_finalize_without_model_read_back() {
    let tmp = tempfile::tempdir().expect("tmp");
    tokio::fs::write(
        tmp.path().join("main.rs"),
        "fn answer() -> i32 {\n    return 1;\n}\n",
    )
    .await
    .expect("seed");
    let calls = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(Mutex::new(Vec::<crate::events::Event>::new()));
    let mut agent = Agent {
        provider: ReadPatchThenDoneProvider {
            calls: calls.clone(),
        },
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
        tools: vec![
            crate::types::ToolDef {
                name: "read_file".to_string(),
                description: "d".to_string(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties":{"path":{"type":"string"}},
                    "required":["path"]
                }),
                side_effects: crate::types::SideEffects::FilesystemRead,
            },
            crate::types::ToolDef {
                name: "apply_patch".to_string(),
                description: "d".to_string(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties":{"path":{"type":"string"},"patch":{"type":"string"}},
                    "required":["path","patch"]
                }),
                side_effects: crate::types::SideEffects::FilesystemWrite,
            },
        ],
        max_steps: 6,
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
    let out = agent
        .run(
            "Edit main.rs to return 2.",
            vec![],
            vec![Message {
                role: Role::System,
                content: Some(crate::agent::INTERNAL_ENFORCE_IMPLEMENTATION_GUARD_FLAG.to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            }],
        )
        .await;
    assert!(matches!(out.exit_reason, AgentExitReason::Ok), "{out:?}");
    assert!(out.error.is_none(), "{out:?}");
    let main = tokio::fs::read_to_string(tmp.path().join("main.rs"))
        .await
        .expect("read main");
    assert!(main.contains("return 2;"), "{main}");
    let evs = events.lock().expect("lock");
    let verify_starts = evs
        .iter()
        .filter(|e| matches!(e.kind, crate::events::EventKind::PostWriteVerifyStart))
        .count();
    let verify_ends = evs
        .iter()
        .filter(|e| matches!(e.kind, crate::events::EventKind::PostWriteVerifyEnd))
        .count();
    let model_requests = evs
        .iter()
        .filter(|e| matches!(e.kind, crate::events::EventKind::ModelRequestStart))
        .count();
    assert_eq!(verify_starts, 1, "expected one runtime verify start");
    assert_eq!(verify_ends, 1, "expected one runtime verify end");
    assert_eq!(
        model_requests, 2,
        "runtime should finalize after verified write without another model turn"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "provider should not be called again after successful verified write"
    );
    let end_ok = evs
        .iter()
        .find(|e| matches!(e.kind, crate::events::EventKind::PostWriteVerifyEnd))
        .and_then(|e| e.data.get("ok"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(end_ok, "runtime verify end should be ok");
}

#[tokio::test]
async fn runtime_noop_apply_patch_does_not_finalize_ok() {
    let tmp = tempfile::tempdir().expect("tmp");
    tokio::fs::write(
        tmp.path().join("main.rs"),
        "fn answer() -> i32 {\n    return 1;\n}\n",
    )
    .await
    .expect("seed");
    let calls = Arc::new(AtomicUsize::new(0));
    let mut agent = Agent {
        provider: ReadNoopPatchThenDoneProvider {
            calls: calls.clone(),
        },
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
        tools: vec![
            crate::types::ToolDef {
                name: "read_file".to_string(),
                description: "d".to_string(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties":{"path":{"type":"string"}},
                    "required":["path"]
                }),
                side_effects: crate::types::SideEffects::FilesystemRead,
            },
            crate::types::ToolDef {
                name: "apply_patch".to_string(),
                description: "d".to_string(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties":{"path":{"type":"string"},"patch":{"type":"string"}},
                    "required":["path","patch"]
                }),
                side_effects: crate::types::SideEffects::FilesystemWrite,
            },
        ],
        max_steps: 6,
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
    let out = agent
        .run(
            "Edit main.rs to return 2.",
            vec![],
            vec![Message {
                role: Role::System,
                content: Some(crate::agent::INTERNAL_ENFORCE_IMPLEMENTATION_GUARD_FLAG.to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            }],
        )
        .await;
    assert!(
        matches!(out.exit_reason, AgentExitReason::PlannerError),
        "noop apply_patch (changed:false) should be caught as planner error: {out:?}"
    );
    assert!(
        out.error
            .as_deref()
            .unwrap_or("")
            .contains("without an effective write"),
        "error should mention ineffective write: {out:?}"
    );
    let main = tokio::fs::read_to_string(tmp.path().join("main.rs"))
        .await
        .expect("read main");
    assert!(main.contains("return 1;"), "{main}");
}

#[tokio::test]
async fn runtime_read_then_done_recovers_with_corrective_write_instruction() {
    let tmp = tempfile::tempdir().expect("tmp");
    tokio::fs::write(
        tmp.path().join("main.rs"),
        "fn answer() -> i32 {\n    return 1;\n}\n",
    )
    .await
    .expect("seed");
    let calls = Arc::new(AtomicUsize::new(0));
    let mut agent = Agent {
        provider: ReadThenDoneThenPatchThenDoneProvider {
            calls: calls.clone(),
        },
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
        tools: vec![
            crate::types::ToolDef {
                name: "read_file".to_string(),
                description: "d".to_string(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties":{"path":{"type":"string"}},
                    "required":["path"]
                }),
                side_effects: crate::types::SideEffects::FilesystemRead,
            },
            crate::types::ToolDef {
                name: "apply_patch".to_string(),
                description: "d".to_string(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties":{"path":{"type":"string"},"patch":{"type":"string"}},
                    "required":["path","patch"]
                }),
                side_effects: crate::types::SideEffects::FilesystemWrite,
            },
        ],
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
    let out = agent
        .run(
            "Edit main.rs to return 2.",
            vec![],
            vec![Message {
                role: Role::System,
                content: Some(crate::agent::INTERNAL_ENFORCE_IMPLEMENTATION_GUARD_FLAG.to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            }],
        )
        .await;
    assert!(matches!(out.exit_reason, AgentExitReason::Ok), "{out:?}");
    assert!(out.error.is_none(), "{out:?}");
    let main = tokio::fs::read_to_string(tmp.path().join("main.rs"))
        .await
        .expect("read main");
    assert!(main.contains("return 2;"), "{main}");
    assert!(
        calls.load(Ordering::SeqCst) >= 3,
        "expected corrective continuation before successful write"
    );
}

#[tokio::test]
async fn runtime_post_write_verification_timeout_fails_deterministically() {
    let tmp = tempfile::tempdir().expect("tmp");
    tokio::fs::write(
        tmp.path().join("main.rs"),
        "fn answer() -> i32 {\n    return 1;\n}\n",
    )
    .await
    .expect("seed");
    let calls = Arc::new(AtomicUsize::new(0));
    let read_calls = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(Mutex::new(Vec::<crate::events::Event>::new()));
    let mut agent = Agent {
        provider: ReadPatchThenDoneProvider {
            calls: calls.clone(),
        },
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
        tools: vec![
            crate::types::ToolDef {
                name: "read_file".to_string(),
                description: "d".to_string(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties":{"path":{"type":"string"}},
                    "required":["path"]
                }),
                side_effects: crate::types::SideEffects::FilesystemRead,
            },
            crate::types::ToolDef {
                name: "apply_patch".to_string(),
                description: "d".to_string(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties":{"path":{"type":"string"},"patch":{"type":"string"}},
                    "required":["path","patch"]
                }),
                side_effects: crate::types::SideEffects::FilesystemWrite,
            },
        ],
        max_steps: 6,
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
            exec_target: std::sync::Arc::new(SlowReadExecTarget {
                host: HostTarget,
                read_calls,
                hang_on_call: 2,
                delay_ms: 250,
            }),
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
        tool_call_budget: ToolCallBudget {
            post_write_verify_timeout_ms: 50,
            tool_exec_timeout_ms: 30_000,
            ..ToolCallBudget::default()
        },
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let started = std::time::Instant::now();
    let out = agent
        .run(
            "Edit main.rs to return 2.",
            vec![],
            vec![Message {
                role: Role::System,
                content: Some(crate::agent::INTERNAL_ENFORCE_IMPLEMENTATION_GUARD_FLAG.to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            }],
        )
        .await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "unexpected slow timeout path: {:?}",
        started.elapsed()
    );
    assert!(
        matches!(out.exit_reason, AgentExitReason::PlannerError),
        "{out:?}"
    );
    assert!(out
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("runtime post-write verification timed out"));
    let evs = events.lock().expect("lock");
    let verify_starts = evs
        .iter()
        .filter(|e| matches!(e.kind, crate::events::EventKind::PostWriteVerifyStart))
        .count();
    let verify_ends = evs
        .iter()
        .filter(|e| matches!(e.kind, crate::events::EventKind::PostWriteVerifyEnd))
        .count();
    assert_eq!(verify_starts, 1, "expected one runtime verify start");
    assert_eq!(verify_ends, 1, "expected one runtime verify end");
    let timeout_status = evs
        .iter()
        .find(|e| matches!(e.kind, crate::events::EventKind::PostWriteVerifyEnd))
        .and_then(|e| e.data.get("status"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    assert_eq!(timeout_status, "timeout");
}

#[tokio::test]
async fn runtime_tool_execution_timeout_is_bounded() {
    let tmp = tempfile::tempdir().expect("tmp");
    tokio::fs::write(tmp.path().join("a.txt"), "x")
        .await
        .expect("seed");
    let calls = Arc::new(AtomicUsize::new(0));
    let read_calls = Arc::new(AtomicUsize::new(0));
    let mut agent = Agent {
        provider: ReadThenDoneProvider {
            calls: calls.clone(),
        },
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
            exec_target: std::sync::Arc::new(SlowReadExecTarget {
                host: HostTarget,
                read_calls: read_calls.clone(),
                hang_on_call: 1,
                delay_ms: 250,
            }),
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
            tool_exec_timeout_ms: 50,
            post_write_verify_timeout_ms: 5_000,
            ..ToolCallBudget::default()
        },
        mcp_runtime_trace: Vec::new(),
        operator_queue: PendingMessageQueue::default(),
        operator_queue_limits: QueueLimits::default(),
        operator_queue_rx: None,
    };
    let started = std::time::Instant::now();
    let out = agent
        .run("Read a.txt then finish.", vec![], Vec::new())
        .await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "tool timeout path should be bounded: {:?}",
        started.elapsed()
    );
    assert!(matches!(out.exit_reason, AgentExitReason::Ok), "{out:?}");
    assert!(
        read_calls.load(Ordering::SeqCst) >= 2,
        "expected timeout+retry behavior, read_calls={}",
        read_calls.load(Ordering::SeqCst)
    );
}

#[tokio::test]
async fn invalid_patch_format_attempts_are_scoped_per_tool_key() {
    let tmp = tempfile::tempdir().expect("tmp");
    let calls = Arc::new(AtomicUsize::new(0));
    let mut agent = Agent {
        provider: UniqueInvalidPatchProvider {
            calls: calls.clone(),
        },
        model: "m".to_string(),
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
        max_steps: 4,
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
        !matches!(out.exit_reason, AgentExitReason::PlannerError),
        "unexpected planner_error={:?}",
        out.error
    );
    assert!(
        !out.error
            .as_deref()
            .unwrap_or_default()
            .contains("repeated invalid patch format"),
        "unexpected repeated-invalid error: {:?}",
        out.error
    );
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
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,
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
