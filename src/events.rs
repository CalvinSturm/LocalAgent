use std::io::Write;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    RunStart,
    RunEnd,
    ModelRequestStart,
    ModelDelta,
    ModelResponseEnd,
    ToolCallDetected,
    ToolDecision,
    ToolExecTarget,
    ToolExecStart,
    ToolExecEnd,
    PostWriteVerifyStart,
    PostWriteVerifyEnd,
    ToolRetry,
    TaintUpdated,
    CompactionPerformed,
    PolicyLoaded,
    PlannerStart,
    PlannerEnd,
    WorkerStart,
    StepStarted,
    StepVerified,
    StepBlocked,
    StepReplanned,
    TaskgraphStart,
    TaskgraphNodeStart,
    TaskgraphNodeEnd,
    TaskgraphEnd,
    HookStart,
    HookEnd,
    HookError,
    ProviderRetry,
    ProviderError,
    ReproSnapshot,
    McpServerStart,
    McpServerStop,
    McpProgress,
    McpCancelled,
    McpPinned,
    McpDrift,
    PackActivated,
    QueueSubmitted,
    QueueDelivered,
    QueueInterrupt,
    LearningCaptured,
    LearningPromoted,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub ts: String,
    pub run_id: String,
    pub step: u32,
    pub kind: EventKind,
    pub data: Value,
}

impl Event {
    pub fn new(run_id: String, step: u32, kind: EventKind, data: Value) -> Self {
        Self {
            ts: crate::trust::now_rfc3339(),
            run_id,
            step,
            kind,
            data,
        }
    }
}

pub trait EventSink: Send {
    fn emit(&mut self, event: Event) -> anyhow::Result<()>;
}

pub struct StdoutSink;

impl StdoutSink {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StdoutSink {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSink for StdoutSink {
    fn emit(&mut self, event: Event) -> anyhow::Result<()> {
        if matches!(event.kind, EventKind::ModelDelta) {
            if let Some(delta) = event.data.get("delta").and_then(|v| v.as_str()) {
                print!("{delta}");
                std::io::stdout().flush().ok();
            }
        }
        Ok(())
    }
}

pub struct JsonlFileSink {
    file: std::fs::File,
}

impl JsonlFileSink {
    pub fn new(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("failed to open events file {}", path.display()))?;
        Ok(Self { file })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedRunEventV1 {
    pub schema_version: String,
    pub sequence: u64,
    pub ts: String,
    pub run_id: String,
    pub step: u32,
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: Value,
}

const CONTENT_PREVIEW_MAX_BYTES: usize = 4096;

fn utf8_truncate_with_meta(input: &str, max_bytes: usize) -> (String, bool, usize) {
    let src = input.as_bytes();
    if src.len() <= max_bytes {
        return (input.to_string(), false, src.len());
    }
    let mut end = max_bytes;
    while end > 0 && std::str::from_utf8(&src[..end]).is_err() {
        end -= 1;
    }
    (
        String::from_utf8_lossy(&src[..end]).to_string(),
        true,
        src.len(),
    )
}

fn projected_type(kind: &EventKind) -> Option<&'static str> {
    match kind {
        EventKind::RunStart => Some("run_started"),
        EventKind::StepStarted => Some("step_started"),
        EventKind::ToolCallDetected => Some("tool_call_detected"),
        EventKind::ToolDecision => Some("tool_decision"),
        EventKind::ToolExecStart => Some("tool_exec_started"),
        EventKind::ToolExecEnd => Some("tool_exec_finished"),
        EventKind::ToolRetry => Some("tool_retry"),
        EventKind::StepBlocked => Some("step_blocked"),
        EventKind::ProviderRetry => Some("provider_retry"),
        EventKind::ProviderError => Some("provider_error"),
        EventKind::RunEnd => Some("run_finished"),
        _ => None,
    }
}

fn data_string(data: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(v) = data.get(*k).and_then(Value::as_str) {
            return Some(v.to_string());
        }
    }
    None
}

pub(crate) fn project_event_v1(event: &Event, sequence: u64) -> Option<ProjectedRunEventV1> {
    let event_type = projected_type(&event.kind)?;
    let data = match event_type {
        "run_started" => serde_json::json!({}),
        "step_started" => serde_json::json!({
            "step_id": data_string(&event.data, &["step_id"]),
            "step_index": event.data.get("step_index").cloned().unwrap_or(Value::Null),
            "allowed_tools": event.data.get("allowed_tools").cloned().unwrap_or(Value::Null),
            "enforcement_mode": data_string(&event.data, &["enforcement_mode"]),
        }),
        "tool_call_detected" => serde_json::json!({
            "tool_call_id": data_string(&event.data, &["tool_call_id", "id"]),
            "tool": data_string(&event.data, &["tool", "name"]),
        }),
        "tool_decision" => serde_json::json!({
            "tool_call_id": data_string(&event.data, &["tool_call_id", "id"]),
            "tool": data_string(&event.data, &["tool", "name"]),
            "decision": data_string(&event.data, &["decision"]),
            "reason": data_string(&event.data, &["reason"]),
            "source": data_string(&event.data, &["source"]),
        }),
        "tool_exec_started" => serde_json::json!({
            "tool_call_id": data_string(&event.data, &["tool_call_id", "id"]),
            "tool": data_string(&event.data, &["tool", "name"]),
        }),
        "tool_exec_finished" => {
            let content =
                data_string(&event.data, &["content", "content_preview"]).unwrap_or_default();
            let (preview, truncated, original_bytes) =
                utf8_truncate_with_meta(&content, CONTENT_PREVIEW_MAX_BYTES);
            serde_json::json!({
                "tool_call_id": data_string(&event.data, &["tool_call_id", "id"]),
                "tool": data_string(&event.data, &["tool", "name"]),
                "ok": event.data.get("ok").cloned().unwrap_or(Value::Null),
                "content_preview": preview,
                "truncated": truncated,
                "original_bytes": if truncated { Value::from(original_bytes as u64) } else { Value::Null }
            })
        }
        "tool_retry" => serde_json::json!({
            "tool_call_id": data_string(&event.data, &["tool_call_id", "id"]),
            "tool": data_string(&event.data, &["tool", "name"]),
            "attempt": event.data.get("attempt").cloned().unwrap_or(Value::Null),
            "max_retries": event.data.get("max_retries").cloned().unwrap_or(Value::Null),
            "failure_class": data_string(&event.data, &["failure_class"]),
            "action": data_string(&event.data, &["action"]),
        }),
        "step_blocked" => serde_json::json!({
            "reason": data_string(&event.data, &["reason"]),
            "tool": data_string(&event.data, &["tool", "name"]),
            "step_id": data_string(&event.data, &["step_id"]),
        }),
        "provider_retry" => serde_json::json!({
            "attempt": event.data.get("attempt").cloned().unwrap_or(Value::Null),
            "max_attempts": event.data.get("max_attempts").cloned().unwrap_or(Value::Null),
            "kind": data_string(&event.data, &["kind"]),
            "status": event.data.get("status").cloned().unwrap_or(Value::Null),
            "backoff_ms": event.data.get("backoff_ms").cloned().unwrap_or(Value::Null),
        }),
        "provider_error" => serde_json::json!({
            "kind": data_string(&event.data, &["kind"]),
            "status": event.data.get("status").cloned().unwrap_or(Value::Null),
            "retryable": event.data.get("retryable").cloned().unwrap_or(Value::Null),
            "message_short": data_string(&event.data, &["message_short"]),
        }),
        "run_finished" => serde_json::json!({
            "exit_reason": data_string(&event.data, &["exit_reason"]).unwrap_or_default(),
            "ok": event.data.get("ok").and_then(Value::as_bool).unwrap_or(false),
            "final_output": data_string(&event.data, &["final_output"]).unwrap_or_default(),
            "error": event.data.get("error").cloned().unwrap_or(Value::Null),
        }),
        _ => return None,
    };
    Some(ProjectedRunEventV1 {
        schema_version: "openagent.run_event.v1".to_string(),
        sequence,
        ts: event.ts.clone(),
        run_id: event.run_id.clone(),
        step: event.step,
        event_type: event_type.to_string(),
        data,
    })
}

#[allow(dead_code)]
pub(crate) fn projected_pre_run_failure_v1(message: &str) -> ProjectedRunEventV1 {
    ProjectedRunEventV1 {
        schema_version: "openagent.run_event.v1".to_string(),
        sequence: 1,
        ts: crate::trust::now_rfc3339(),
        run_id: String::new(),
        step: 0,
        event_type: "run_finished".to_string(),
        data: serde_json::json!({
            "exit_reason": "pre_run_error",
            "ok": false,
            "final_output": "",
            "error": message
        }),
    }
}

pub struct JsonStdoutProjectedSink {
    sequence: u64,
}

impl JsonStdoutProjectedSink {
    pub fn new() -> Self {
        Self { sequence: 0 }
    }
}

impl Default for JsonStdoutProjectedSink {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSink for JsonStdoutProjectedSink {
    fn emit(&mut self, event: Event) -> anyhow::Result<()> {
        if projected_type(&event.kind).is_none() {
            return Ok(());
        }
        self.sequence = self.sequence.saturating_add(1);
        if let Some(projected) = project_event_v1(&event, self.sequence) {
            let line = serde_json::to_string(&projected)?;
            println!("{line}");
        }
        Ok(())
    }
}

impl EventSink for JsonlFileSink {
    fn emit(&mut self, event: Event) -> anyhow::Result<()> {
        let line = serde_json::to_string(&event)?;
        writeln!(self.file, "{line}")?;
        Ok(())
    }
}

pub struct MultiSink {
    sinks: Vec<Box<dyn EventSink>>,
}

impl MultiSink {
    pub fn new() -> Self {
        Self { sinks: Vec::new() }
    }

    pub fn push(&mut self, sink: Box<dyn EventSink>) {
        self.sinks.push(sink);
    }

    pub fn is_empty(&self) -> bool {
        self.sinks.is_empty()
    }
}

impl Default for MultiSink {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSink for MultiSink {
    fn emit(&mut self, event: Event) -> anyhow::Result<()> {
        for sink in &mut self.sinks {
            sink.emit(event.clone())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use serde_json::Value;

    use super::{
        project_event_v1, Event, EventKind, EventSink, JsonlFileSink, ProjectedRunEventV1,
    };

    #[test]
    fn event_serializes() {
        let ev = Event::new(
            "run1".to_string(),
            0,
            EventKind::RunStart,
            serde_json::json!({"x":1}),
        );
        let s = serde_json::to_string(&ev).expect("serialize");
        assert!(s.contains("\"run_start\""));
        assert!(s.contains("\"run1\""));
    }

    #[test]
    fn jsonl_appends() {
        let tmp = tempdir().expect("tempdir");
        let path = tmp.path().join("events.jsonl");
        let mut sink = JsonlFileSink::new(&path).expect("sink");
        sink.emit(Event::new(
            "r".to_string(),
            0,
            EventKind::RunStart,
            serde_json::json!({}),
        ))
        .expect("emit1");
        sink.emit(Event::new(
            "r".to_string(),
            1,
            EventKind::RunEnd,
            serde_json::json!({}),
        ))
        .expect("emit2");
        let content = std::fs::read_to_string(path).expect("read");
        assert_eq!(content.lines().count(), 2);
    }

    #[test]
    fn taint_updated_kind_serializes() {
        let ev = Event::new(
            "r".to_string(),
            1,
            EventKind::TaintUpdated,
            serde_json::json!({"overall":"tainted"}),
        );
        let s = serde_json::to_string(&ev).expect("serialize");
        assert!(s.contains("\"taint_updated\""));
    }

    #[test]
    fn repro_snapshot_kind_serializes() {
        let ev = Event::new(
            "r".to_string(),
            1,
            EventKind::ReproSnapshot,
            serde_json::json!({"enabled":true}),
        );
        let s = serde_json::to_string(&ev).expect("serialize");
        assert!(s.contains("\"repro_snapshot\""));
    }

    #[test]
    fn pack_activated_kind_serializes() {
        let ev = Event::new(
            "r".to_string(),
            1,
            EventKind::PackActivated,
            serde_json::json!({"pack_id":"web/playwright"}),
        );
        let s = serde_json::to_string(&ev).expect("serialize");
        assert!(s.contains("\"pack_activated\""));
    }

    #[test]
    fn queue_event_kinds_serialize() {
        for kind in [
            EventKind::QueueSubmitted,
            EventKind::QueueDelivered,
            EventKind::QueueInterrupt,
        ] {
            let ev = Event::new(
                "r".to_string(),
                1,
                kind,
                serde_json::json!({"queue_id":"q1"}),
            );
            let s = serde_json::to_string(&ev).expect("serialize");
            assert!(s.contains("\"queue_"));
        }
    }

    #[test]
    fn learning_captured_kind_serializes() {
        let ev = Event::new(
            "learn".to_string(),
            0,
            EventKind::LearningCaptured,
            serde_json::json!({"learning_id":"01H..."}),
        );
        let s = serde_json::to_string(&ev).expect("serialize");
        assert!(s.contains("\"learning_captured\""));
    }

    #[test]
    fn learning_promoted_kind_serializes() {
        let ev = Event::new(
            "learn".to_string(),
            0,
            EventKind::LearningPromoted,
            serde_json::json!({"learning_id":"01H...","target":"check"}),
        );
        let s = serde_json::to_string(&ev).expect("serialize");
        assert!(s.contains("\"learning_promoted\""));
    }

    #[test]
    fn projection_ignores_unmapped_kind() {
        let ev = Event::new(
            "r".to_string(),
            1,
            EventKind::McpProgress,
            serde_json::json!({"x":1}),
        );
        assert!(project_event_v1(&ev, 1).is_none());
    }

    #[test]
    fn projection_includes_run_finished_required_fields() {
        let ev = Event::new(
            "r".to_string(),
            2,
            EventKind::RunEnd,
            serde_json::json!({"exit_reason":"ok","ok":true,"final_output":"done","error":null}),
        );
        let p = project_event_v1(&ev, 9).expect("projected");
        assert_eq!(p.schema_version, "openagent.run_event.v1");
        assert_eq!(p.sequence, 9);
        assert_eq!(p.event_type, "run_finished");
        assert_eq!(
            p.data.get("exit_reason").and_then(Value::as_str),
            Some("ok")
        );
        assert_eq!(p.data.get("ok").and_then(Value::as_bool), Some(true));
        assert_eq!(
            p.data.get("final_output").and_then(Value::as_str),
            Some("done")
        );
        assert!(p.data.get("error").is_some());
    }

    #[test]
    fn projection_truncates_large_content_preview() {
        let huge = "a".repeat(5000);
        let ev = Event::new(
            "r".to_string(),
            3,
            EventKind::ToolExecEnd,
            serde_json::json!({"name":"read_file","ok":true,"content":huge}),
        );
        let p = project_event_v1(&ev, 10).expect("projected");
        assert_eq!(p.event_type, "tool_exec_finished");
        assert_eq!(p.data.get("truncated").and_then(Value::as_bool), Some(true));
        assert!(p
            .data
            .get("original_bytes")
            .and_then(Value::as_u64)
            .is_some());
        assert!(
            p.data
                .get("content_preview")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .len()
                <= 4096
        );
    }

    #[test]
    fn projection_serializes_parseable_json_line() {
        let ev = Event::new(
            "r".to_string(),
            0,
            EventKind::RunStart,
            serde_json::json!({}),
        );
        let p = project_event_v1(&ev, 1).expect("projected");
        let line = serde_json::to_string(&p).expect("line");
        let parsed: ProjectedRunEventV1 = serde_json::from_str(&line).expect("parse");
        assert_eq!(parsed.event_type, "run_started");
    }
}
