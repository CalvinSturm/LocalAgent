use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use localagent::agent::{
    Agent, AgentExitReason, McpPinEnforcementMode, PlanStepConstraint, PlanToolEnforcementMode,
    ToolCallBudget,
};
use localagent::compaction::{CompactionMode, CompactionSettings, ToolResultPersist};
use localagent::eval::metrics::derive_tool_retry_metrics;
use localagent::events::{Event, EventKind, EventSink};
use localagent::gate::{
    compute_policy_hash_hex, ApprovalKeyVersion, ApprovalMode, AutoApproveScope, GateContext,
    NoGate, ProviderKind, ToolGate, TrustGate, TrustMode,
};
use localagent::hooks::config::HooksMode;
use localagent::hooks::runner::{HookManager, HookRuntimeConfig};
use localagent::providers::ModelProvider;
use localagent::taint::{TaintLevel, TaintMode, TaintToggle};
use localagent::target::{ExecTargetKind, HostTarget};
use localagent::tools::{builtin_tools_enabled, ToolArgsStrict, ToolRuntime};
use localagent::trust::approvals::ApprovalsStore;
use localagent::trust::audit::AuditLog;
use localagent::trust::policy::Policy;
use localagent::types::{GenerateRequest, GenerateResponse, Message, Role, ToolCall, ToolDef};
use serde_json::Value;
use tempfile::tempdir;

struct EventCaptureSink {
    events: Arc<Mutex<Vec<Event>>>,
}

impl EventSink for EventCaptureSink {
    fn emit(&mut self, event: Event) -> anyhow::Result<()> {
        self.events.lock().expect("event lock").push(event);
        Ok(())
    }
}

#[derive(Clone)]
enum ScriptStep {
    Tool {
        id: &'static str,
        name: &'static str,
        arguments: Value,
    },
    Final(&'static str),
}

struct ScriptedProvider {
    steps: Vec<ScriptStep>,
    next: AtomicUsize,
}

#[async_trait]
impl ModelProvider for ScriptedProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        let idx = self.next.fetch_add(1, Ordering::SeqCst);
        let step = self
            .steps
            .get(idx)
            .cloned()
            .unwrap_or(ScriptStep::Final("done"));
        Ok(match step {
            ScriptStep::Tool {
                id,
                name,
                arguments,
            } => GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some(String::new()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: vec![ToolCall {
                    id: id.to_string(),
                    name: name.to_string(),
                    arguments,
                }],
                usage: None,
            },
            ScriptStep::Final(text) => GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some(text.to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: Vec::new(),
                usage: None,
            },
        })
    }
}

fn build_agent<P: ModelProvider + 'static>(
    provider: P,
    workdir: &Path,
    tools: Vec<ToolDef>,
    gate: Box<dyn ToolGate>,
    allow_shell: bool,
    allow_write: bool,
    max_steps: usize,
    events: Arc<Mutex<Vec<Event>>>,
) -> Agent<P> {
    Agent {
        provider,
        model: "mock-model".to_string(),
        temperature: None,
        tools,
        max_steps,
        tool_rt: ToolRuntime {
            workdir: workdir.to_path_buf(),
            allow_shell,
            allow_shell_in_workdir_only: false,
            allow_write,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            unsafe_bypass_allow_flags: false,
            tool_args_strict: ToolArgsStrict::On,
            exec_target_kind: ExecTargetKind::Host,
            exec_target: Arc::new(HostTarget),
        },
        gate,
        gate_ctx: GateContext {
            workdir: workdir.to_path_buf(),
            allow_shell,
            allow_write,
            approval_mode: ApprovalMode::Interrupt,
            auto_approve_scope: AutoApproveScope::Run,
            unsafe_mode: false,
            unsafe_bypass_allow_flags: false,
            run_id: None,
            enable_write_tools: false,
            max_tool_output_bytes: 200_000,
            max_read_bytes: 200_000,
            provider: ProviderKind::Mock,
            model: "mock-model".to_string(),
            exec_target: ExecTargetKind::Host,
            approval_key_version: ApprovalKeyVersion::V1,
            tool_schema_hashes: BTreeMap::new(),
            hooks_config_hash_hex: None,
            planner_hash_hex: None,
            taint_enabled: false,
            taint_mode: TaintMode::Propagate,
            taint_overall: TaintLevel::Clean,
            taint_sources: Vec::new(),
        },
        mcp_registry: None,
        stream: false,
        event_sink: Some(Box::new(EventCaptureSink { events })),
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
            timeout_ms: 1_000,
            max_stdout_bytes: 200_000,
        })
        .expect("hooks"),
        policy_loaded: None,
        policy_for_taint: None,
        taint_toggle: TaintToggle::Off,
        taint_mode: TaintMode::Propagate,
        taint_digest_bytes: 4096,
        run_id_override: None,
        omit_tools_field_when_empty: false,
        plan_tool_enforcement: PlanToolEnforcementMode::Off,
        mcp_pin_enforcement: McpPinEnforcementMode::Hard,
        plan_step_constraints: Vec::<PlanStepConstraint>::new(),
        tool_call_budget: ToolCallBudget::default(),
        mcp_runtime_trace: Vec::new(),
        operator_queue: localagent::operator_queue::PendingMessageQueue::default(),
        operator_queue_limits: localagent::operator_queue::QueueLimits::default(),
        operator_queue_rx: None,
    }
}

fn tool_message_error_codes(messages: &[Message]) -> Vec<String> {
    messages
        .iter()
        .filter(|m| matches!(m.role, Role::Tool))
        .filter_map(|m| m.content.as_deref())
        .filter_map(|c| serde_json::from_str::<Value>(c).ok())
        .filter_map(|v| {
            v.get("error")
                .and_then(|e| e.get("code"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect()
}

fn tool_ok_count(messages: &[Message]) -> usize {
    messages
        .iter()
        .filter(|m| matches!(m.role, Role::Tool))
        .filter_map(|m| m.content.as_deref())
        .filter_map(|c| serde_json::from_str::<Value>(c).ok())
        .filter(|v| v.get("ok").and_then(Value::as_bool).unwrap_or(false))
        .count()
}

#[tokio::test]
async fn ci_unknown_tool_self_corrects_or_fails_clear() {
    let tmp = tempdir().expect("tempdir");
    let events = Arc::new(Mutex::new(Vec::<Event>::new()));
    let provider = ScriptedProvider {
        steps: vec![
            ScriptStep::Tool {
                id: "tc0",
                name: "unknown_tool",
                arguments: serde_json::json!({"path":"."}),
            },
            ScriptStep::Tool {
                id: "tc1",
                name: "list_dir",
                arguments: serde_json::json!({"path":"."}),
            },
            ScriptStep::Final("done"),
        ],
        next: AtomicUsize::new(0),
    };
    let mut agent = build_agent(
        provider,
        tmp.path(),
        builtin_tools_enabled(false, false),
        Box::new(NoGate::new()),
        false,
        false,
        6,
        events.clone(),
    );
    let outcome = agent.run("test", Vec::new(), Vec::new()).await;

    assert!(matches!(outcome.exit_reason, AgentExitReason::Ok));
    let codes = tool_message_error_codes(&outcome.messages);
    assert!(
        codes.iter().any(|c| c == "tool_unknown"),
        "expected tool_unknown in {:?}",
        codes
    );
    let called = outcome
        .tool_calls
        .iter()
        .map(|tc| tc.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(called, vec!["unknown_tool", "list_dir"]);
    let (_retries, _failure_classes) = derive_tool_retry_metrics(&events.lock().expect("lock"));
}

#[tokio::test]
async fn ci_invalid_args_repairs_within_bound() {
    let tmp = tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("input.txt"), "hello").expect("seed file");
    let events = Arc::new(Mutex::new(Vec::<Event>::new()));
    let provider = ScriptedProvider {
        steps: vec![
            ScriptStep::Tool {
                id: "tc0",
                name: "read_file",
                arguments: serde_json::json!({"oops":1}),
            },
            ScriptStep::Tool {
                id: "tc1",
                name: "read_file",
                arguments: serde_json::json!({"path":"input.txt"}),
            },
            ScriptStep::Final("done"),
        ],
        next: AtomicUsize::new(0),
    };
    let mut agent = build_agent(
        provider,
        tmp.path(),
        builtin_tools_enabled(false, false),
        Box::new(NoGate::new()),
        false,
        false,
        6,
        events.clone(),
    );
    let outcome = agent.run("test", Vec::new(), Vec::new()).await;

    assert!(matches!(outcome.exit_reason, AgentExitReason::Ok));
    let codes = tool_message_error_codes(&outcome.messages);
    assert!(
        codes.iter().any(|c| c == "tool_args_invalid"),
        "expected tool_args_invalid in {:?}",
        codes
    );
    assert!(tool_ok_count(&outcome.messages) >= 1);
    let (_retries, failure_classes) = derive_tool_retry_metrics(&events.lock().expect("lock"));
    assert!(failure_classes.contains_key("E_SCHEMA"));
}

#[tokio::test]
async fn ci_multi_step_tool_chain_completes() {
    let tmp = tempdir().expect("tempdir");
    let events = Arc::new(Mutex::new(Vec::<Event>::new()));
    let provider = ScriptedProvider {
        steps: vec![
            ScriptStep::Tool {
                id: "tc0",
                name: "list_dir",
                arguments: serde_json::json!({"path":"."}),
            },
            ScriptStep::Tool {
                id: "tc1",
                name: "read_file",
                arguments: serde_json::json!({"path":"Cargo.toml"}),
            },
            ScriptStep::Final("final answer"),
        ],
        next: AtomicUsize::new(0),
    };
    let mut agent = build_agent(
        provider,
        tmp.path(),
        builtin_tools_enabled(false, false),
        Box::new(NoGate::new()),
        false,
        false,
        6,
        events,
    );
    let outcome = agent.run("test", Vec::new(), Vec::new()).await;

    assert!(matches!(outcome.exit_reason, AgentExitReason::Ok));
    let names = outcome
        .tool_calls
        .iter()
        .map(|tc| tc.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["list_dir", "read_file"]);
}

#[tokio::test]
async fn ci_repeat_guard_blocks_repeated_failed_calls() {
    let tmp = tempdir().expect("tempdir");
    let events = Arc::new(Mutex::new(Vec::<Event>::new()));
    let provider = ScriptedProvider {
        steps: vec![
            ScriptStep::Tool {
                id: "tc0",
                name: "unknown_tool",
                arguments: serde_json::json!({"x":1}),
            },
            ScriptStep::Tool {
                id: "tc1",
                name: "unknown_tool",
                arguments: serde_json::json!({"x":1}),
            },
            ScriptStep::Tool {
                id: "tc2",
                name: "unknown_tool",
                arguments: serde_json::json!({"x":1}),
            },
            ScriptStep::Tool {
                id: "tc3",
                name: "unknown_tool",
                arguments: serde_json::json!({"x":1}),
            },
            ScriptStep::Tool {
                id: "tc4",
                name: "unknown_tool",
                arguments: serde_json::json!({"x":1}),
            },
        ],
        next: AtomicUsize::new(0),
    };
    let mut agent = build_agent(
        provider,
        tmp.path(),
        builtin_tools_enabled(false, false),
        Box::new(NoGate::new()),
        false,
        false,
        8,
        events.clone(),
    );
    let outcome = agent.run("test", Vec::new(), Vec::new()).await;

    assert!(matches!(outcome.exit_reason, AgentExitReason::PlannerError));
    let err = outcome.error.unwrap_or_default();
    assert!(err.contains("TOOL_REPEAT_BLOCKED"), "error={err}");

    let saw_repeat_blocked_step = events.lock().expect("lock").iter().any(|ev| {
        matches!(ev.kind, EventKind::StepBlocked)
            && ev.data.get("code").and_then(Value::as_str) == Some("TOOL_REPEAT_BLOCKED")
    });
    assert!(saw_repeat_blocked_step);
}

#[tokio::test]
async fn ci_trust_gate_blocks_shell_without_allow_shell() {
    let tmp = tempdir().expect("tempdir");
    let events = Arc::new(Mutex::new(Vec::<Event>::new()));
    let provider = ScriptedProvider {
        steps: vec![ScriptStep::Tool {
            id: "tc0",
            name: "shell",
            arguments: serde_json::json!({"cmd":"echo","args":["hi"]}),
        }],
        next: AtomicUsize::new(0),
    };
    let policy = Policy::safe_default();
    let policy_hash_hex = compute_policy_hash_hex(b"tool-call-accuracy-ci");
    let approvals = ApprovalsStore::new(tmp.path().join("approvals.json"));
    let audit = AuditLog::new(tmp.path().join("audit.jsonl"));
    let gate = TrustGate::new(policy, approvals, audit, TrustMode::On, policy_hash_hex);
    let mut agent = build_agent(
        provider,
        tmp.path(),
        builtin_tools_enabled(false, true),
        Box::new(gate),
        false,
        false,
        4,
        events.clone(),
    );
    let outcome = agent.run("test", Vec::new(), Vec::new()).await;

    assert!(matches!(outcome.exit_reason, AgentExitReason::Denied));
    assert!(outcome.final_output.contains("denied"));

    let snapshot = events.lock().expect("lock");
    let saw_tool_decision_deny = snapshot.iter().any(|ev| {
        matches!(ev.kind, EventKind::ToolDecision)
            && ev.data.get("decision").and_then(Value::as_str) == Some("deny")
    });
    let saw_tool_exec_start = snapshot
        .iter()
        .any(|ev| matches!(ev.kind, EventKind::ToolExecStart));
    assert!(saw_tool_decision_deny);
    assert!(!saw_tool_exec_start);
}
