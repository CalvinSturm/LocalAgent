use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, OnceLock,
};

use async_trait::async_trait;
use clap::Parser;

use tempfile::tempdir;
use tokio::sync::Mutex as AsyncMutex;

use crate::providers::{ModelProvider, StreamDelta};

use crate::target::ExecTargetKind;

use crate::taskgraph::{TaskCompaction, TaskFlags, TaskLimits};

use crate::types::{GenerateRequest, GenerateResponse, Message, Role};

use super::{DockerNetwork, ProviderKind};

use crate::{ops_helpers, provider_runtime};
use crate::{Cli, Commands};

struct CaptureSink {
    events: Arc<Mutex<Vec<crate::events::Event>>>,
}

impl crate::events::EventSink for CaptureSink {
    fn emit(&mut self, event: crate::events::Event) -> anyhow::Result<()> {
        self.events.lock().expect("lock").push(event);

        Ok(())
    }
}

struct PlannerTestProvider {
    seen_tools_none: Arc<Mutex<Vec<bool>>>,
}

#[async_trait]

impl ModelProvider for PlannerTestProvider {
    async fn generate(&self, req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        self.seen_tools_none
            .lock()
            .expect("lock")
            .push(req.tools.is_none());

        Ok(GenerateResponse {
            assistant: Message {
                role: Role::Assistant,

                content: Some("not-json".to_string()),

                tool_call_id: None,

                tool_name: None,

                tool_calls: None,
            },

            tool_calls: Vec::new(),

            usage: None,
        })
    }

    async fn generate_streaming(
        &self,

        req: GenerateRequest,

        _on_delta: &mut (dyn FnMut(StreamDelta) + Send),
    ) -> anyhow::Result<GenerateResponse> {
        self.generate(req).await
    }
}

enum QualificationProbeMode {
    NativePass,

    InlinePass,

    FailNoTool,
}

struct QualificationTestProvider {
    calls: Arc<AtomicUsize>,

    mode: QualificationProbeMode,
}

#[async_trait]

impl ModelProvider for QualificationTestProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);

        let (assistant_content, tool_calls) = match self.mode {
            QualificationProbeMode::NativePass => (
                Some(String::new()),
                vec![crate::types::ToolCall {
                    id: "q1".to_string(),

                    name: "list_dir".to_string(),

                    arguments: serde_json::json!({"path":"."}),
                }],
            ),

            QualificationProbeMode::InlinePass => (
                Some("{\"name\":\"list_dir\",\"arguments\":{\"path\":\".\"}}".to_string()),
                Vec::new(),
            ),

            QualificationProbeMode::FailNoTool => (Some("no tool".to_string()), Vec::new()),
        };

        Ok(GenerateResponse {
            assistant: Message {
                role: Role::Assistant,

                content: assistant_content,

                tool_call_id: None,

                tool_name: None,

                tool_calls: None,
            },

            tool_calls,

            usage: None,
        })
    }

    async fn generate_streaming(
        &self,

        req: GenerateRequest,

        _on_delta: &mut (dyn FnMut(StreamDelta) + Send),
    ) -> anyhow::Result<GenerateResponse> {
        self.generate(req).await
    }
}

struct SequencedQualificationProvider {
    calls: Arc<AtomicUsize>,
    responses: Vec<GenerateResponse>,
}

#[async_trait]
impl ModelProvider for SequencedQualificationProvider {
    async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        let idx = self.calls.fetch_add(1, Ordering::SeqCst);
        self.responses
            .get(idx)
            .cloned()
            .or_else(|| self.responses.last().cloned())
            .ok_or_else(|| anyhow::anyhow!("no qualification responses configured"))
    }

    async fn generate_streaming(
        &self,
        req: GenerateRequest,
        _on_delta: &mut (dyn FnMut(StreamDelta) + Send),
    ) -> anyhow::Result<GenerateResponse> {
        self.generate(req).await
    }
}

fn qualification_trace_env_lock() -> &'static AsyncMutex<()> {
    static LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| AsyncMutex::new(()))
}

fn qualification_trace_dir_for_model(root: &Path, model: &str) -> PathBuf {
    let mut dirs = std::fs::read_dir(root)
        .expect("read trace root")
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .filter(|path| path.is_dir() && path.join("summary.json").exists())
        .collect::<Vec<_>>();
    dirs.sort();
    dirs.into_iter()
        .find(|path| {
            let summary = std::fs::read(path.join("summary.json")).expect("summary");
            let json: serde_json::Value = serde_json::from_slice(&summary).expect("summary json");
            json["model"] == model
        })
        .expect("trace dir for model")
}

#[test]
fn run_command_defaults_to_no_session_and_derived_state_dir() {
    let mut cli = Cli::parse_from([
        "localagent",
        "--provider",
        "mock",
        "--model",
        "mock-model",
        "run",
    ]);
    let argv = vec![
        std::ffi::OsString::from("localagent"),
        std::ffi::OsString::from("--provider"),
        std::ffi::OsString::from("mock"),
        std::ffi::OsString::from("--model"),
        std::ffi::OsString::from("mock-model"),
        std::ffi::OsString::from("run"),
    ];
    let tmp = tempdir().expect("tempdir");
    let workdir = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let _ = crate::cli_dispatch::apply_run_command_defaults(&mut cli, &argv, &workdir);
    assert!(cli.run.no_session);
    let state_dir = cli.run.state_dir.expect("state dir");
    assert!(!state_dir.starts_with(&workdir));
}

#[test]
fn run_command_respects_explicit_state_dir_override() {
    let mut cli = Cli::parse_from([
        "localagent",
        "--provider",
        "mock",
        "--model",
        "mock-model",
        "--state-dir",
        "custom_state",
        "run",
    ]);
    let argv = vec![
        std::ffi::OsString::from("localagent"),
        std::ffi::OsString::from("--provider"),
        std::ffi::OsString::from("mock"),
        std::ffi::OsString::from("--model"),
        std::ffi::OsString::from("mock-model"),
        std::ffi::OsString::from("--state-dir"),
        std::ffi::OsString::from("custom_state"),
        std::ffi::OsString::from("run"),
    ];
    let tmp = tempdir().expect("tempdir");
    let workdir = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let _ = crate::cli_dispatch::apply_run_command_defaults(&mut cli, &argv, &workdir);
    assert!(!cli.run.no_session);
    assert_eq!(
        cli.run
            .state_dir
            .as_ref()
            .expect("state dir")
            .to_string_lossy(),
        "custom_state"
    );
}

#[test]
fn non_run_command_does_not_force_no_session_or_state_dir() {
    let mut cli = Cli::parse_from([
        "localagent",
        "--provider",
        "mock",
        "--model",
        "mock-model",
        "chat",
    ]);
    let argv = vec![
        std::ffi::OsString::from("localagent"),
        std::ffi::OsString::from("--provider"),
        std::ffi::OsString::from("mock"),
        std::ffi::OsString::from("--model"),
        std::ffi::OsString::from("mock-model"),
        std::ffi::OsString::from("chat"),
    ];
    let tmp = tempdir().expect("tempdir");
    let workdir = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let _ = crate::cli_dispatch::apply_run_command_defaults(&mut cli, &argv, &workdir);
    assert!(!cli.run.no_session);
    assert!(cli.run.state_dir.is_none());
}

#[test]
fn bare_startup_bootstrap_detection_matches_no_flags_invocation() {
    let cli = Cli::parse_from(["localagent"]);
    assert!(crate::cli_dispatch::is_bare_startup_bootstrap_invocation(
        &cli
    ));

    let with_prompt = Cli::parse_from([
        "localagent",
        "--provider",
        "mock",
        "--model",
        "mock-model",
        "--prompt",
        "hi",
    ]);
    assert!(!crate::cli_dispatch::is_bare_startup_bootstrap_invocation(
        &with_prompt
    ));
}

#[test]
fn bare_localagent_invocation_defaults_to_sessionless_and_ephemeral_state() {
    let mut cli = Cli::parse_from([
        "localagent",
        "--provider",
        "mock",
        "--model",
        "mock-model",
        "--prompt",
        "hi",
    ]);
    let argv = vec![
        std::ffi::OsString::from("localagent"),
        std::ffi::OsString::from("--provider"),
        std::ffi::OsString::from("mock"),
        std::ffi::OsString::from("--model"),
        std::ffi::OsString::from("mock-model"),
        std::ffi::OsString::from("--prompt"),
        std::ffi::OsString::from("hi"),
    ];
    let tmp = tempdir().expect("tempdir");
    let workdir = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let _ = crate::cli_dispatch::apply_run_command_defaults(&mut cli, &argv, &workdir);
    assert!(cli.run.no_session);
    let state_dir = cli.run.state_dir.expect("state dir");
    assert!(!state_dir.starts_with(&workdir));
}

#[test]

fn doctor_url_construction_openai_compat() {
    let urls =
        provider_runtime::doctor_probe_urls(ProviderKind::Lmstudio, "http://localhost:1234/v1/");

    assert_eq!(urls[0], "http://localhost:1234/v1/models");

    assert_eq!(urls[1], "http://localhost:1234/v1");
}

#[test]

fn policy_doctor_helper_works() {
    let tmp = tempdir().expect("tmp");

    let p = tmp.path().join("policy.yaml");

    std::fs::write(
        &p,
        r#"



version: 2



default: deny



rules:



  - tool: "read_file"



    decision: allow



"#,
    )
    .expect("write");

    let out = ops_helpers::policy_doctor_output(&p).expect("doctor");

    assert!(out.contains("version=2"));

    assert!(out.contains("rules=1"));
}

#[test]

fn policy_effective_helper_json_contains_rules() {
    let tmp = tempdir().expect("tmp");

    let p = tmp.path().join("policy.yaml");

    std::fs::write(
        &p,
        r#"



version: 2



default: deny



rules:



  - tool: "read_file"



    decision: allow



"#,
    )
    .expect("write");

    let out = ops_helpers::policy_effective_output(&p, true).expect("print");

    assert!(out.contains("\"rules\""));

    assert!(out.contains("read_file"));
}

#[test]

fn probe_parser_accepts_inline_json_tool_call() {
    let resp = GenerateResponse {
        assistant: Message {
            role: Role::Assistant,

            content: Some("{\"name\":\"list_dir\",\"arguments\":{\"path\":\".\"}}".to_string()),

            tool_call_id: None,

            tool_name: None,

            tool_calls: None,
        },

        tool_calls: Vec::new(),

        usage: None,
    };

    let tc = super::qualification::probe_response_to_tool_call(&resp).expect("tool call");

    assert_eq!(tc.name, "list_dir");

    assert_eq!(tc.arguments, serde_json::json!({"path":"."}));
}

#[test]

fn probe_parser_accepts_fenced_json_tool_call() {
    let resp = GenerateResponse {
        assistant: Message {
            role: Role::Assistant,

            content: Some(
                "```json\n{\"name\":\"list_dir\",\"arguments\":{\"path\":\".\"}}\n```".to_string(),
            ),

            tool_call_id: None,

            tool_name: None,

            tool_calls: None,
        },

        tool_calls: Vec::new(),

        usage: None,
    };

    let tc = super::qualification::probe_response_to_tool_call(&resp).expect("tool call");

    assert_eq!(tc.name, "list_dir");

    assert_eq!(tc.arguments, serde_json::json!({"path":"."}));
}

#[test]
fn probe_parser_accepts_named_arguments_textual_tool_call() {
    let resp = GenerateResponse {
        assistant: Message {
            role: Role::Assistant,

            content: Some(
                "<think>Need to emit the requested tool shape.</think>\n\nname=list_dir\narguments={\"path\":\".\"}"
                    .to_string(),
            ),

            tool_call_id: None,

            tool_name: None,

            tool_calls: None,
        },

        tool_calls: Vec::new(),

        usage: None,
    };

    let tc = super::qualification::probe_response_to_tool_call(&resp).expect("tool call");

    assert_eq!(tc.name, "list_dir");

    assert_eq!(tc.arguments, serde_json::json!({"path":"."}));
}

#[test]
fn probe_parser_rejects_ambiguous_named_arguments_textual_tool_call() {
    let resp = GenerateResponse {
        assistant: Message {
            role: Role::Assistant,

            content: Some(
                "name=list_dir\narguments={\"path\":\".\"}\n\nname=list_dir\narguments={\"path\":\"src\"}"
                    .to_string(),
            ),

            tool_call_id: None,

            tool_name: None,

            tool_calls: None,
        },

        tool_calls: Vec::new(),

        usage: None,
    };

    assert!(super::qualification::probe_response_to_tool_call(&resp).is_none());
}

#[tokio::test]

async fn planner_phase_omits_tools_and_emits_tool_count_zero() {
    let seen = Arc::new(Mutex::new(Vec::<bool>::new()));

    let provider = PlannerTestProvider {
        seen_tools_none: seen.clone(),
    };

    let events = Arc::new(Mutex::new(Vec::<crate::events::Event>::new()));

    let mut sink: Option<Box<dyn crate::events::EventSink>> = Some(Box::new(CaptureSink {
        events: events.clone(),
    }));

    let out = super::planner_runtime::run_planner_phase(
        &provider,
        "run_test",
        "m",
        "do thing",
        1,
        crate::planner::PlannerOutput::Json,
        false,
        &mut sink,
    )
    .await
    .expect("planner");

    assert!(out.plan_json.get("schema_version").is_some());

    assert_eq!(seen.lock().expect("lock").as_slice(), &[true]);

    let model_start = events
        .lock()
        .expect("lock")
        .iter()
        .find(|e| matches!(e.kind, crate::events::EventKind::ModelRequestStart))
        .cloned()
        .expect("model request event");

    assert_eq!(
        model_start.data.get("tool_count").and_then(|v| v.as_u64()),
        Some(0)
    );
}

#[test]

fn task_settings_merge_defaults_then_overrides() {
    let mut args = default_run_args();

    let defaults = crate::taskgraph::TaskDefaults {
        mode: Some("planner_worker".to_string()),

        provider: Some("ollama".to_string()),

        base_url: Some("http://localhost:11434".to_string()),

        model: Some("m1".to_string()),

        planner_model: Some("pm".to_string()),

        worker_model: Some("wm".to_string()),

        trust: Some("on".to_string()),

        approval_mode: Some("auto".to_string()),

        auto_approve_scope: Some("run".to_string()),

        caps: Some("strict".to_string()),

        hooks: Some("auto".to_string()),

        compaction: TaskCompaction {
            max_context_chars: Some(111),

            mode: Some("summary".to_string()),

            keep_last: Some(7),

            tool_result_persist: Some("digest".to_string()),
        },

        limits: TaskLimits {
            max_read_bytes: Some(123),

            max_tool_output_bytes: Some(456),
        },

        flags: TaskFlags {
            enable_write_tools: Some(true),

            allow_write: Some(true),

            allow_shell: Some(false),

            stream: Some(false),
        },

        mcp: vec!["playwright".to_string()],
    };

    super::task_apply::apply_task_defaults(&mut args, &defaults).expect("defaults");

    let override_s = crate::taskgraph::TaskNodeSettings {
        model: Some("m2".to_string()),

        flags: TaskFlags {
            allow_shell: Some(true),

            ..TaskFlags::default()
        },

        ..crate::taskgraph::TaskNodeSettings::default()
    };

    super::task_apply::apply_node_overrides(&mut args, &override_s).expect("overrides");

    assert_eq!(args.model.as_deref(), Some("m2"));

    assert!(args.allow_shell);

    assert!(matches!(args.mode, crate::planner::RunMode::PlannerWorker));

    assert_eq!(args.mcp, vec!["playwright".to_string()]);
}

#[test]

fn node_summary_line_is_deterministic() {
    let a = super::runtime_events::node_summary_line("N1", "ok", "hello\nworld");

    let b = super::runtime_events::node_summary_line("N1", "ok", "hello\nworld");

    assert_eq!(a, b);

    assert!(a.contains("output_sha256="));
}

#[test]

fn planner_worker_defaults_plan_enforcement_to_hard_when_not_explicit() {
    let resolved = super::runtime_flags::resolve_plan_tool_enforcement(
        crate::planner::RunMode::PlannerWorker,
        crate::agent::PlanToolEnforcementMode::Off,
        false,
    );

    assert!(matches!(
        resolved,
        crate::agent::PlanToolEnforcementMode::Hard
    ));
}

#[test]

fn planner_worker_respects_explicit_off_override() {
    let resolved = super::runtime_flags::resolve_plan_tool_enforcement(
        crate::planner::RunMode::PlannerWorker,
        crate::agent::PlanToolEnforcementMode::Off,
        true,
    );

    assert!(matches!(
        resolved,
        crate::agent::PlanToolEnforcementMode::Off
    ));
}

#[test]

fn planner_worker_respects_explicit_soft_override() {
    let resolved = super::runtime_flags::resolve_plan_tool_enforcement(
        crate::planner::RunMode::PlannerWorker,
        crate::agent::PlanToolEnforcementMode::Soft,
        true,
    );

    assert!(matches!(
        resolved,
        crate::agent::PlanToolEnforcementMode::Soft
    ));
}

#[test]

fn timeout_command_off_disables_request_and_stream_idle() {
    let mut args = default_run_args();

    let msg = super::runtime_config::apply_timeout_input(&mut args, "off").expect("timeout off");

    assert_eq!(args.http_timeout_ms, 0);

    assert_eq!(args.http_stream_idle_timeout_ms, 0);

    assert!(msg.contains("disabled"));

    assert!(super::runtime_config::timeout_settings_summary(&args).contains("request=off"));

    assert!(super::runtime_config::timeout_settings_summary(&args).contains("stream-idle=off"));
}

#[tokio::test]

async fn qualification_failure_is_cached_and_short_circuits_future_attempts() {
    let tmp = tempdir().expect("tmp");

    let cache = tmp.path().join("qual_cache.json");

    let tools = crate::tools::builtin_tools_enabled(true, false);

    let model = format!(
        "qual_model_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    );

    let first_calls = Arc::new(AtomicUsize::new(0));

    let first = QualificationTestProvider {
        calls: first_calls.clone(),

        mode: QualificationProbeMode::FailNoTool,
    };

    let err = super::qualification::ensure_orchestrator_qualified(
        &first,
        ProviderKind::Lmstudio,
        "http://localhost:1234/v1",
        &model,
        false,
        &tools,
        &cache,
    )
    .await
    .expect_err("expected fail");

    assert!(err.to_string().contains("no tool call returned"));

    assert!(first_calls.load(Ordering::SeqCst) >= 1);

    let second_calls = Arc::new(AtomicUsize::new(0));

    let second = QualificationTestProvider {
        calls: second_calls,

        mode: QualificationProbeMode::NativePass,
    };

    let err2 = super::qualification::ensure_orchestrator_qualified(
        &second,
        ProviderKind::Lmstudio,
        "http://localhost:1234/v1",
        &model,
        false,
        &tools,
        &cache,
    )
    .await
    .expect_err("cache should fail fast");

    assert!(err2
        .to_string()
        .contains("failed previously for this model/session"));
}

#[tokio::test]

async fn qualification_fallback_disables_write_tools_and_continues() {
    let tmp = tempdir().expect("tmp");

    let cache = tmp.path().join("qual_cache.json");

    let mut tools = crate::tools::builtin_tools_enabled(true, false);

    assert!(tools
        .iter()
        .any(|t| t.side_effects == crate::types::SideEffects::FilesystemWrite));

    let calls = Arc::new(AtomicUsize::new(0));

    let provider = QualificationTestProvider {
        calls,

        mode: QualificationProbeMode::FailNoTool,
    };

    let mut args = default_run_args();

    args.enable_write_tools = true;

    args.allow_write = true;

    let note = super::qualification::qualify_or_enable_readonly_fallback(
        &provider,
        ProviderKind::Lmstudio,
        "http://localhost:1234/v1",
        "fallback-model",
        false,
        args.enable_write_tools || args.allow_write,
        &mut tools,
        &cache,
    )
    .await
    .expect("fallback should not error")
    .expect("fallback note");

    assert!(note.contains("read-only fallback"));

    assert!(!tools
        .iter()
        .any(|t| t.side_effects == crate::types::SideEffects::FilesystemWrite));
}

#[tokio::test]

async fn qualification_fallback_keeps_write_tools_when_probe_passes() {
    let tmp = tempdir().expect("tmp");

    let cache = tmp.path().join("qual_cache.json");

    let mut tools = crate::tools::builtin_tools_enabled(true, false);

    let calls = Arc::new(AtomicUsize::new(0));

    let provider = QualificationTestProvider {
        calls,

        mode: QualificationProbeMode::InlinePass,
    };

    let mut args = default_run_args();

    args.enable_write_tools = true;

    args.allow_write = true;

    let note = super::qualification::qualify_or_enable_readonly_fallback(
        &provider,
        ProviderKind::Lmstudio,
        "http://localhost:1234/v1",
        "pass-model",
        false,
        args.enable_write_tools || args.allow_write,
        &mut tools,
        &cache,
    )
    .await
    .expect("qualification ok");

    assert!(note.is_none());

    assert!(tools
        .iter()
        .any(|t| t.side_effects == crate::types::SideEffects::FilesystemWrite));
}

#[tokio::test]
async fn qualification_accepts_named_arguments_textual_probe_fallback() {
    let tmp = tempdir().expect("tmp");
    let cache = tmp.path().join("qual_cache.json");
    let tools = crate::tools::builtin_tools_enabled(true, false);
    struct TextualQualificationProvider {
        calls: Arc<AtomicUsize>,
        content: String,
    }

    #[async_trait]
    impl ModelProvider for TextualQualificationProvider {
        async fn generate(&self, _req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
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

        async fn generate_streaming(
            &self,
            req: GenerateRequest,
            _on_delta: &mut (dyn FnMut(StreamDelta) + Send),
        ) -> anyhow::Result<GenerateResponse> {
            self.generate(req).await
        }
    }

    let provider = TextualQualificationProvider {
        calls: Arc::new(AtomicUsize::new(0)),
        content:
            "<think>Need the requested shape.</think>\n\nname=list_dir\narguments={\"path\":\".\"}"
                .to_string(),
    };

    super::qualification::ensure_orchestrator_qualified(
        &provider,
        ProviderKind::Lmstudio,
        "http://localhost:1234/v1",
        "textual-pass-model",
        false,
        &tools,
        &cache,
    )
    .await
    .expect("textual fallback should qualify");
}

#[tokio::test]
async fn qualification_rejects_malformed_named_arguments_textual_probe() {
    let tmp = tempdir().expect("tmp");
    let cache = tmp.path().join("qual_cache.json");
    let tools = crate::tools::builtin_tools_enabled(true, false);

    struct TextualQualificationProvider {
        content: String,
    }

    #[async_trait]
    impl ModelProvider for TextualQualificationProvider {
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

        async fn generate_streaming(
            &self,
            req: GenerateRequest,
            _on_delta: &mut (dyn FnMut(StreamDelta) + Send),
        ) -> anyhow::Result<GenerateResponse> {
            self.generate(req).await
        }
    }

    let provider = TextualQualificationProvider {
        content: "name=list_dir\narguments={\"path\":".to_string(),
    };

    let err = super::qualification::ensure_orchestrator_qualified(
        &provider,
        ProviderKind::Lmstudio,
        "http://localhost:1234/v1",
        "textual-malformed-model",
        false,
        &tools,
        &cache,
    )
    .await
    .expect_err("malformed textual fallback should fail");

    assert!(err
        .to_string()
        .contains("textual probe tool call was malformed"));
}

#[tokio::test]
async fn qualification_keeps_success_when_later_attempts_would_be_ambiguous() {
    let tmp = tempdir().expect("tmp");
    let cache = tmp.path().join("qual_cache.json");
    let tools = crate::tools::builtin_tools_enabled(true, false);
    let provider = SequencedQualificationProvider {
        calls: Arc::new(AtomicUsize::new(0)),
        responses: vec![
            GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some("".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: vec![crate::types::ToolCall {
                    id: "q1".to_string(),
                    name: "list_dir".to_string(),
                    arguments: serde_json::json!({"path":"."}),
                }],
                usage: None,
            },
            GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some(
                        "name=list_dir\narguments={\"path\":\".\"}\n\nname=list_dir\narguments={\"path\":\"src\"}"
                            .to_string(),
                    ),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: Vec::new(),
                usage: None,
            },
        ],
    };

    super::qualification::ensure_orchestrator_qualified(
        &provider,
        ProviderKind::Lmstudio,
        "http://localhost:1234/v1",
        "sticky-success-model",
        false,
        &tools,
        &cache,
    )
    .await
    .expect("first proven success should qualify");

    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn qualification_accepts_later_success_after_prior_soft_fail() {
    let tmp = tempdir().expect("tmp");
    let cache = tmp.path().join("qual_cache.json");
    let tools = crate::tools::builtin_tools_enabled(true, false);
    let provider = SequencedQualificationProvider {
        calls: Arc::new(AtomicUsize::new(0)),
        responses: vec![
            GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some("no tool".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: Vec::new(),
                usage: None,
            },
            GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some("".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: vec![crate::types::ToolCall {
                    id: "q2".to_string(),
                    name: "list_dir".to_string(),
                    arguments: serde_json::json!({"path":"."}),
                }],
                usage: None,
            },
        ],
    };

    super::qualification::ensure_orchestrator_qualified(
        &provider,
        ProviderKind::Lmstudio,
        "http://localhost:1234/v1",
        "later-success-model",
        false,
        &tools,
        &cache,
    )
    .await
    .expect("later proven success should qualify");

    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn qualification_trace_writes_attempt_bundle_when_enabled() {
    let _guard = qualification_trace_env_lock().lock().await;
    let tmp = tempdir().expect("tmp");
    let trace_root = tmp.path().join("qualification-traces");
    let cache = tmp.path().join("qual_cache.json");
    let tools = crate::tools::builtin_tools_enabled(true, false);
    let model = format!(
        "qual_trace_model_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    );
    let provider = QualificationTestProvider {
        calls: Arc::new(AtomicUsize::new(0)),
        mode: QualificationProbeMode::InlinePass,
    };

    unsafe {
        std::env::set_var("LOCALAGENT_QUAL_TRACE_DIR", &trace_root);
    }
    let result = super::qualification::ensure_orchestrator_qualified(
        &provider,
        ProviderKind::Lmstudio,
        "http://localhost:1234/v1",
        &model,
        true,
        &tools,
        &cache,
    )
    .await;
    unsafe {
        std::env::remove_var("LOCALAGENT_QUAL_TRACE_DIR");
    }

    result.expect("qualification should pass");

    let trace_dir = qualification_trace_dir_for_model(&trace_root, &model);

    let request: serde_json::Value = serde_json::from_slice(
        &std::fs::read(trace_dir.join("attempt-01").join("request.json")).expect("request"),
    )
    .expect("request json");
    assert_eq!(request["model"], model);
    assert_eq!(request["stream"], true);
    assert_eq!(
        request["qualification_cache_key"],
        format!("lmstudio|http://localhost:1234/v1|{model}")
    );
    assert_eq!(
        request["request"]["messages"][0]["content"],
        "Emit exactly one native tool call and no prose:\nname=list_dir\narguments={\"path\":\".\"}"
    );

    let parsed: serde_json::Value = serde_json::from_slice(
        &std::fs::read(trace_dir.join("attempt-01").join("response.parsed.json"))
            .expect("parsed response"),
    )
    .expect("parsed json");
    assert_eq!(
        parsed["assistant_content"],
        "{\"name\":\"list_dir\",\"arguments\":{\"path\":\".\"}}"
    );
    assert_eq!(parsed["inferred_tool_call_count"], 0);
    assert_eq!(parsed["finish_reason"], serde_json::Value::Null);

    let verdict: serde_json::Value =
        serde_json::from_slice(&std::fs::read(trace_dir.join("verdict.json")).expect("verdict"))
            .expect("verdict json");
    assert_eq!(verdict["stream"], true);
    assert_eq!(verdict["cache_hit"], false);
    assert_eq!(verdict["cache_write_value"], true);
    assert_eq!(verdict["final_verdict"], "ok");
    assert_eq!(verdict["final_reason"], "probe_passed");

    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(trace_dir.join("summary.json")).expect("summary"))
            .expect("summary json");
    assert_eq!(summary["stream"], true);
    assert_eq!(summary["verdict"], "ok");
    assert_eq!(summary["cache_outcome"], "write:true");
    assert_eq!(summary["artifact_files"]["verdict"], "verdict.json");
    assert_eq!(summary["artifact_files"]["summary"], "summary.json");
}

#[tokio::test]
async fn qualification_trace_records_cache_hit_without_attempt_bundle() {
    let _guard = qualification_trace_env_lock().lock().await;
    let tmp = tempdir().expect("tmp");
    let trace_root = tmp.path().join("qualification-traces");
    let cache = tmp.path().join("qual_cache.json");
    let tools = crate::tools::builtin_tools_enabled(true, false);
    let model = format!(
        "qual_trace_cached_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    );
    std::fs::write(
        &cache,
        serde_json::to_vec_pretty(&serde_json::json!({
            format!("lmstudio|http://localhost:1234/v1|{model}"): true
        }))
        .expect("cache json"),
    )
    .expect("cache write");
    let provider = QualificationTestProvider {
        calls: Arc::new(AtomicUsize::new(0)),
        mode: QualificationProbeMode::FailNoTool,
    };

    unsafe {
        std::env::set_var("LOCALAGENT_QUAL_TRACE_DIR", &trace_root);
    }
    let result = super::qualification::ensure_orchestrator_qualified(
        &provider,
        ProviderKind::Lmstudio,
        "http://localhost:1234/v1",
        &model,
        false,
        &tools,
        &cache,
    )
    .await;
    unsafe {
        std::env::remove_var("LOCALAGENT_QUAL_TRACE_DIR");
    }

    result.expect("cache hit should pass");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);

    let trace_dir = qualification_trace_dir_for_model(&trace_root, &model);
    assert!(!trace_dir.join("attempt-01").exists());

    let verdict: serde_json::Value =
        serde_json::from_slice(&std::fs::read(trace_dir.join("verdict.json")).expect("verdict"))
            .expect("verdict json");
    assert_eq!(verdict["stream"], false);
    assert_eq!(verdict["cache_hit"], true);
    assert_eq!(verdict["cached_value"], true);
    assert_eq!(verdict["cache_written"], false);
    assert_eq!(verdict["final_reason"], "cache_hit_pass");

    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(trace_dir.join("summary.json")).expect("summary"))
            .expect("summary json");
    assert_eq!(summary["stream"], false);
    assert_eq!(summary["verdict"], "ok");
    assert_eq!(summary["cache_outcome"], "hit:true");
    assert_eq!(summary["artifact_files"]["attempts"], serde_json::json!([]));
}

#[test]

fn protocol_hint_detects_tool_call_format_issues() {
    let hint = super::runtime_config::protocol_remediation_hint(



            "MODEL_TOOL_PROTOCOL_VIOLATION: repeated malformed tool calls (tool='list_dir', error='...')",



        )



        .expect("hint");

    assert!(hint.contains("native tool call JSON"));
}

#[test]

fn protocol_hint_detects_invalid_patch_format() {
    let hint = super::runtime_config::protocol_remediation_hint(
        "MODEL_TOOL_PROTOCOL_VIOLATION: repeated invalid patch format for apply_patch",
    )
    .expect("hint");

    assert!(hint.contains("valid unified diff"));
}

#[test]

fn protocol_hint_ignores_non_protocol_errors() {
    assert!(super::runtime_config::protocol_remediation_hint("provider timeout").is_none());
}

#[test]
fn sampling_validation_rejects_invalid_top_p() {
    let mut run = default_run_args();
    run.top_p = Some(0.0);
    let err = super::cli_dispatch::validate_sampling_args(&run).expect_err("must fail");
    assert!(err.to_string().contains("--top-p"));
}

#[test]
fn sampling_validation_rejects_zero_max_tokens() {
    let mut run = default_run_args();
    run.max_tokens = Some(0);
    let err = super::cli_dispatch::validate_sampling_args(&run).expect_err("must fail");
    assert!(err.to_string().contains("--max-tokens"));
}

#[test]
fn sampling_validation_accepts_valid_values() {
    let mut run = default_run_args();
    run.top_p = Some(0.9);
    run.max_tokens = Some(256);
    run.seed = Some(123);
    super::cli_dispatch::validate_sampling_args(&run).expect("valid");
}

#[test]
fn agent_mode_defaults_to_build() {
    let cli = Cli::parse_from(["localagent"]);
    assert!(matches!(cli.run.agent_mode, crate::AgentMode::Build));
}

#[test]
fn output_mode_defaults_to_human() {
    let cli = Cli::parse_from(["localagent"]);
    assert!(matches!(cli.run.output, crate::RunOutputMode::Human));
}

#[test]
fn output_mode_json_parses() {
    let cli = Cli::parse_from(["localagent", "--output", "json"]);
    assert!(matches!(cli.run.output, crate::RunOutputMode::Json));
}

#[test]
fn parser_derived_run_defaults_match_cli_reference() {
    let cli = Cli::parse_from(["localagent"]);
    assert_eq!(cli.run.http_timeout_ms, 120_000);
    assert_eq!(cli.run.http_stream_idle_timeout_ms, 30_000);
    assert_eq!(cli.run.http_connect_timeout_ms, 2_000);
    assert_eq!(cli.run.http_max_retries, 2);
}

#[test]
fn parser_derived_eval_defaults_match_cli_surface() {
    let cli = Cli::parse_from(["localagent", "eval"]);
    let Some(Commands::Eval(eval)) = cli.command else {
        panic!("expected eval command");
    };
    assert!(matches!(eval.run.provider, crate::ProviderKind::Ollama));
    assert!(matches!(eval.run.trust, crate::gate::TrustMode::On));
    assert!(matches!(
        eval.run.approval_mode,
        crate::gate::ApprovalMode::Auto
    ));
    assert!(eval.run.no_session);
    assert_eq!(eval.run.http_timeout_ms, 120_000);
    assert_eq!(eval.run.http_stream_idle_timeout_ms, 30_000);
}

#[test]
fn canonical_cli_reference_run_example_parses_with_global_flags_before_run() {
    let cli = Cli::parse_from([
        "localagent",
        "--provider",
        "lmstudio",
        "--model",
        "example-model",
        "--prompt",
        "hello",
        "run",
    ]);
    assert!(matches!(cli.command, Some(Commands::Run)));
    assert!(matches!(
        cli.run.provider,
        Some(crate::ProviderKind::Lmstudio)
    ));
    assert_eq!(cli.run.model.as_deref(), Some("example-model"));
    assert_eq!(cli.run.prompt.as_deref(), Some("hello"));
}

#[test]
fn llm_setup_guide_chat_tui_example_parses_correctly() {
    let cli = Cli::parse_from([
        "localagent",
        "--provider",
        "lmstudio",
        "--model",
        "example-model",
        "chat",
        "--tui",
    ]);
    assert!(matches!(cli.command, Some(Commands::Chat(_))));
    assert!(matches!(
        cli.run.provider,
        Some(crate::ProviderKind::Lmstudio)
    ));
    assert_eq!(cli.run.model.as_deref(), Some("example-model"));
}

#[test]
fn serve_command_parses_bind_and_port() {
    let cli = Cli::parse_from([
        "localagent",
        "serve",
        "--bind",
        "127.0.0.1",
        "--port",
        "8080",
    ]);
    let Some(Commands::Serve(args)) = cli.command else {
        panic!("expected serve command");
    };
    assert_eq!(args.bind, "127.0.0.1");
    assert_eq!(args.port, 8080);
}

#[test]
fn lsp_provider_command_parses_for_typescript() {
    let cli = super::Cli::parse_from([
        "localagent",
        "--lsp-provider",
        "typescript",
        "--lsp-command",
        "C:\\tools\\tslsp.cmd",
        "run",
    ]);
    assert!(matches!(
        cli.run.lsp_provider,
        Some(crate::cli_args::LspProviderKind::Typescript)
    ));
    assert_eq!(
        cli.run.lsp_command,
        Some(std::path::PathBuf::from("C:\\tools\\tslsp.cmd"))
    );
}

#[test]
fn attach_command_parses_server_url_and_session_id() {
    let cli = Cli::parse_from([
        "localagent",
        "attach",
        "--server-url",
        "http://127.0.0.1:7070",
        "--session-id",
        "s_123",
    ]);
    let Some(Commands::Attach(args)) = cli.command else {
        panic!("expected attach command");
    };
    assert_eq!(args.server_url, "http://127.0.0.1:7070");
    assert_eq!(args.session_id, "s_123");
}

#[test]
fn planner_mode_and_agent_mode_can_be_set_together() {
    let cli = Cli::parse_from([
        "localagent",
        "--mode",
        "planner-worker",
        "--agent-mode",
        "plan",
    ]);
    assert!(matches!(
        cli.run.mode,
        crate::planner::RunMode::PlannerWorker
    ));
    assert!(matches!(cli.run.agent_mode, crate::AgentMode::Plan));
}

#[test]
fn agent_mode_plan_disables_shell_and_write_by_default() {
    let mut args = default_run_args();
    args.agent_mode = crate::AgentMode::Plan;
    super::runtime_flags::apply_agent_mode_capability_baseline(
        &mut args,
        super::runtime_flags::CapabilityExplicitFlags::default(),
    );
    assert!(!args.allow_shell);
    assert!(!args.allow_shell_in_workdir);
    assert!(!args.allow_write);
    assert!(!args.enable_write_tools);
}

#[test]
fn agent_mode_plan_respects_explicit_overrides() {
    let mut args = default_run_args();
    args.agent_mode = crate::AgentMode::Plan;
    args.allow_shell = true;
    args.allow_shell_in_workdir = true;
    args.allow_write = true;
    args.enable_write_tools = true;
    super::runtime_flags::apply_agent_mode_capability_baseline(
        &mut args,
        super::runtime_flags::CapabilityExplicitFlags {
            allow_shell: true,
            allow_shell_in_workdir: true,
            allow_write: true,
            enable_write_tools: true,
        },
    );
    assert!(args.allow_shell);
    assert!(args.allow_shell_in_workdir);
    assert!(args.allow_write);
    assert!(args.enable_write_tools);
}

#[test]
fn agent_mode_does_not_mutate_provider_or_model_resolution() {
    let mut args = default_run_args();
    args.provider = Some(crate::ProviderKind::Lmstudio);
    args.model = Some("m".to_string());
    args.base_url = Some("http://localhost:1234/v1".to_string());
    args.agent_mode = crate::AgentMode::Plan;
    super::runtime_flags::apply_agent_mode_capability_baseline(
        &mut args,
        super::runtime_flags::CapabilityExplicitFlags::default(),
    );
    assert!(matches!(args.provider, Some(crate::ProviderKind::Lmstudio)));
    assert_eq!(args.model.as_deref(), Some("m"));
    assert_eq!(args.base_url.as_deref(), Some("http://localhost:1234/v1"));
}

#[test]
fn output_mode_json_rejects_tui_with_clear_error() {
    let mut args = default_run_args();
    args.output = crate::RunOutputMode::Json;
    args.tui = true;
    let err = super::cli_dispatch::validate_run_output_mode(&args).expect_err("must fail");
    assert!(err
        .to_string()
        .contains("--output json is incompatible with --tui"));
}

#[test]
fn run_cli_config_persists_agent_mode() {
    let args = default_run_args();
    let resolved = crate::session::RunSettingResolution {
        max_context_chars: 0,
        compaction_mode: crate::compaction::CompactionMode::Off,
        compaction_keep_last: 20,
        tool_result_persist: crate::compaction::ToolResultPersist::Digest,
        tool_args_strict: crate::tools::ToolArgsStrict::On,
        caps_mode: crate::session::CapsMode::Off,
        hooks_mode: crate::hooks::config::HooksMode::Off,
        sources: std::collections::BTreeMap::new(),
    };
    let cli = crate::runtime_paths::build_run_cli_config(crate::runtime_paths::RunCliConfigInput {
        provider_kind: crate::ProviderKind::Mock,
        base_url: "http://localhost:1",
        model: "mock-model",
        args: &args,
        resolved_settings: &resolved,
        hooks_config_path: std::path::Path::new("hooks.yaml"),
        mcp_config_path: std::path::Path::new("mcp_servers.json"),
        tool_catalog: Vec::new(),
        mcp_tool_snapshot: Vec::new(),
        mcp_tool_catalog_hash_hex: None,
        policy_version: None,
        includes_resolved: Vec::new(),
        mcp_allowlist: None,
        mode: crate::planner::RunMode::Single,
        planner_model: None,
        worker_model: None,
        planner_max_steps: None,
        planner_output: None,
        planner_strict: None,
        enforce_plan_tools: None,
        instructions: &crate::instructions::InstructionResolution::empty(),
        project_guidance: None,
        repo_map: None,
        lsp_context: None,
        activated_packs: &[],
    });
    assert_eq!(cli.agent_mode, "build");
    assert_eq!(cli.output_mode, "human");
}

#[test]
fn run_cli_config_includes_lsp_context_metadata_when_present() {
    let args = default_run_args();
    let resolved = crate::session::RunSettingResolution {
        max_context_chars: 0,
        compaction_mode: crate::compaction::CompactionMode::Off,
        compaction_keep_last: 20,
        tool_result_persist: crate::compaction::ToolResultPersist::Digest,
        tool_args_strict: crate::tools::ToolArgsStrict::On,
        caps_mode: crate::session::CapsMode::Off,
        hooks_mode: crate::hooks::config::HooksMode::Off,
        sources: std::collections::BTreeMap::new(),
    };
    let diag = crate::diagnostics::Diagnostic {
        schema_version: crate::diagnostics::DIAGNOSTIC_SCHEMA_VERSION.to_string(),
        code: "E001".to_string(),
        severity: crate::diagnostics::Severity::Error,
        message: "bad thing".to_string(),
        path: Some(std::path::PathBuf::from("src/main.rs")),
        line: Some(7),
        col: Some(1),
        hint: None,
        details: None,
    };
    let lsp_context = crate::lsp_context::ResolvedLspContext {
        schema_version: crate::lsp_context::LSP_CONTEXT_SCHEMA_VERSION.to_string(),
        provider: "mock_lsp".to_string(),
        generated_at: "2026-03-08T00:00:00Z".to_string(),
        workdir: std::path::PathBuf::from("."),
        diagnostics_snapshot: Some(crate::lsp_context::DiagnosticsSnapshot {
            schema_version: crate::lsp_context::LSP_DIAGNOSTICS_SCHEMA_VERSION.to_string(),
            source: "lsp".to_string(),
            workspace_root: std::path::PathBuf::from("."),
            language: Some("rust".to_string()),
            items: vec![diag],
            total_count: 1,
            included_count: 1,
            truncated: false,
            truncation_reason: None,
        }),
        symbol_context: Some(crate::lsp_context::SymbolContext {
            schema_version: crate::lsp_context::LSP_SYMBOL_CONTEXT_SCHEMA_VERSION.to_string(),
            source: "lsp".to_string(),
            workspace_root: std::path::PathBuf::from("."),
            query: "parse_count".to_string(),
            symbols: vec![crate::lsp_context::SymbolLocation {
                path: std::path::PathBuf::from("src/lib.rs"),
                line: Some(3),
                col: Some(1),
                label: "fn parse_count".to_string(),
            }],
            definitions: vec![crate::lsp_context::SymbolLocation {
                path: std::path::PathBuf::from("src/lib.rs"),
                line: Some(3),
                col: Some(1),
                label: "fn parse_count".to_string(),
            }],
            references: vec![crate::lsp_context::SymbolLocation {
                path: std::path::PathBuf::from("tests/lib.rs"),
                line: Some(8),
                col: Some(1),
                label: "parse_count(\"7\")".to_string(),
            }],
            symbol_count_total: 1,
            definition_count_total: 1,
            reference_count_total: 1,
            truncated: false,
            truncation_reason: None,
        }),
        truncated: false,
        truncation_reason: None,
        bytes_kept: 123,
    };
    let cli = crate::runtime_paths::build_run_cli_config(crate::runtime_paths::RunCliConfigInput {
        provider_kind: crate::gate::ProviderKind::Mock,
        base_url: "http://localhost:11434",
        model: "model",
        args: &args,
        resolved_settings: &resolved,
        hooks_config_path: std::path::Path::new("hooks.yaml"),
        mcp_config_path: std::path::Path::new("mcp_servers.json"),
        tool_catalog: Vec::new(),
        mcp_tool_snapshot: Vec::new(),
        mcp_tool_catalog_hash_hex: None,
        policy_version: None,
        includes_resolved: Vec::new(),
        mcp_allowlist: None,
        mode: crate::planner::RunMode::Single,
        planner_model: None,
        worker_model: None,
        planner_max_steps: None,
        planner_output: None,
        planner_strict: None,
        enforce_plan_tools: None,
        instructions: &crate::instructions::InstructionResolution::empty(),
        project_guidance: None,
        repo_map: None,
        lsp_context: Some(&lsp_context),
        activated_packs: &[],
    });
    assert_eq!(cli.lsp_context_provider.as_deref(), Some("mock_lsp"));
    assert_eq!(
        cli.lsp_context_schema_version.as_deref(),
        Some(crate::lsp_context::LSP_CONTEXT_SCHEMA_VERSION)
    );
    assert_eq!(cli.lsp_context_bytes_kept, 123);
    assert_eq!(cli.lsp_context_diagnostics_included, 1);
    assert_eq!(cli.lsp_context_symbol_query.as_deref(), Some("parse_count"));
    assert_eq!(cli.lsp_context_symbols_included, 1);
    assert_eq!(cli.lsp_context_definitions_included, 1);
    assert_eq!(cli.lsp_context_references_included, 1);
    assert!(cli.lsp_context_injected);
}

fn default_run_args() -> super::RunArgs {
    super::RunArgs {
        provider: None,

        model: None,

        temperature: None,
        top_p: None,
        max_tokens: None,
        seed: None,

        base_url: None,

        api_key: None,

        prompt: None,

        max_steps: 20,

        max_wall_time_ms: 0,

        max_total_tool_calls: 0,

        max_mcp_calls: 0,

        max_filesystem_read_calls: 0,

        max_filesystem_write_calls: 0,

        max_shell_calls: 0,

        max_network_calls: 0,

        max_browser_calls: 0,

        tool_exec_timeout_ms: 30_000,

        post_write_verify_timeout_ms: 5_000,

        workdir: std::path::PathBuf::from("."),

        state_dir: None,

        mcp: Vec::new(),
        packs: Vec::new(),

        mcp_config: None,

        allow_shell: false,

        allow_shell_in_workdir: false,

        allow_write: false,

        enable_write_tools: false,

        agent_mode: crate::AgentMode::Build,

        exec_target: ExecTargetKind::Host,

        docker_image: "ubuntu:24.04".to_string(),

        docker_workdir: "/work".to_string(),

        docker_network: DockerNetwork::None,

        docker_user: None,

        max_tool_output_bytes: 200_000,

        max_read_bytes: 200_000,

        trust: crate::gate::TrustMode::Off,

        approval_mode: crate::gate::ApprovalMode::Interrupt,

        auto_approve_scope: crate::gate::AutoApproveScope::Run,

        approval_key: crate::gate::ApprovalKeyVersion::V1,

        unsafe_mode: false,

        no_limits: false,

        unsafe_bypass_allow_flags: false,

        policy: None,

        approvals: None,

        audit: None,

        session: "default".to_string(),

        no_session: false,

        reset_session: false,

        max_session_messages: 40,

        use_session_settings: false,

        max_context_chars: 0,

        use_repomap: false,

        repomap_max_bytes: 32 * 1024,
        lsp_provider: None,
        lsp_command: None,
        reliability_profile: None,

        compaction_mode: crate::compaction::CompactionMode::Off,

        compaction_keep_last: 20,

        tool_result_persist: crate::compaction::ToolResultPersist::Digest,

        hooks: crate::hooks::config::HooksMode::Off,

        hooks_config: None,

        hooks_strict: false,

        hooks_timeout_ms: 2000,

        hooks_max_stdout_bytes: 200_000,

        tool_args_strict: crate::tools::ToolArgsStrict::On,

        instructions_config: None,

        instruction_model_profile: None,

        instruction_task_profile: None,

        task_kind: None,

        disable_implementation_guard: false,

        taint: crate::taint::TaintToggle::Off,

        taint_mode: crate::taint::TaintMode::Propagate,

        taint_digest_bytes: 4096,

        repro: crate::repro::ReproMode::Off,

        repro_out: None,

        repro_env: crate::repro::ReproEnvMode::Safe,

        caps: crate::session::CapsMode::Off,

        stream: false,

        output: crate::RunOutputMode::Human,

        events: None,

        http_max_retries: 2,

        http_timeout_ms: 0,

        http_connect_timeout_ms: 2_000,

        http_stream_idle_timeout_ms: 0,

        http_max_response_bytes: 10_000_000,

        http_max_line_bytes: 200_000,

        tui: false,

        tui_refresh_ms: 50,

        tui_max_log_lines: 200,

        mode: crate::planner::RunMode::Single,

        planner_model: None,

        worker_model: None,

        planner_max_steps: 2,

        planner_output: crate::planner::PlannerOutput::Json,

        enforce_plan_tools: crate::agent::PlanToolEnforcementMode::Off,

        mcp_pin_enforcement: crate::agent::McpPinEnforcementMode::Hard,

        planner_strict: true,

        no_planner_strict: false,
        resolved_reliability_profile_source: None,
        resolved_reliability_profile_hash_hex: None,
    }
}

#[test]
fn cli_parse_version_and_chat_with_windows_style_argv0() {
    let argv0 = r"C:\Users\Calvin\Software Projects\LocalAgent\target\debug\localagent.exe";
    let cli_version = Cli::parse_from([argv0, "version"]);
    assert!(matches!(cli_version.command, Some(Commands::Version(_))));

    let cli_chat = Cli::parse_from([argv0, "chat"]);
    assert!(matches!(cli_chat.command, Some(Commands::Chat(_))));
}

#[test]
fn cli_parse_version_with_os_strings() {
    let argv = vec![
        std::ffi::OsString::from(
            r"C:\Users\Calvin\Software Projects\LocalAgent\target\debug\localagent.exe",
        ),
        std::ffi::OsString::from("version"),
    ];
    let cli = Cli::parse_from(argv);
    assert!(matches!(cli.command, Some(Commands::Version(_))));
}
