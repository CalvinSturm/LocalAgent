use crate::agent::{Agent, McpRuntimeTraceEntry};
use crate::events::{Event, EventKind};
use crate::providers::ModelProvider;

impl<P: ModelProvider> Agent<P> {
    pub(crate) fn emit_event(
        &mut self,
        run_id: &str,
        step: u32,
        kind: EventKind,
        data: serde_json::Value,
    ) {
        self.capture_mcp_runtime_trace(step, &kind, &data);
        if let Some(sink) = &mut self.event_sink {
            if let Err(e) = sink.emit(Event::new(run_id.to_string(), step, kind, data)) {
                eprintln!("WARN: failed to emit event: {e}");
            }
        }
    }

    pub(crate) fn capture_mcp_runtime_trace(
        &mut self,
        step: u32,
        kind: &EventKind,
        data: &serde_json::Value,
    ) {
        let mut push = |lifecycle: &str| {
            self.mcp_runtime_trace.push(McpRuntimeTraceEntry {
                step,
                lifecycle: lifecycle.to_string(),
                tool_call_id: data
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                tool_name: data
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                reason: data
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                progress_ticks: data
                    .get("progress_ticks")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32),
                elapsed_ms: data.get("elapsed_ms").and_then(|v| v.as_u64()),
            });
        };
        match kind {
            EventKind::ToolExecStart => {
                if data
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .starts_with("mcp.")
                {
                    push("running");
                }
            }
            EventKind::ToolExecEnd => {
                if data
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .starts_with("mcp.")
                {
                    let ok = data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
                    if ok {
                        push("done");
                    } else {
                        push("fail");
                    }
                }
            }
            EventKind::ToolRetry => {
                if data
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .starts_with("mcp.")
                {
                    let action = data
                        .get("action")
                        .and_then(|v| v.as_str())
                        .unwrap_or("stop");
                    if action == "retry" {
                        push("wait_retry");
                    } else {
                        push("fail");
                    }
                }
            }
            EventKind::McpProgress => push("wait_task"),
            EventKind::McpCancelled => push("cancelled"),
            EventKind::McpPinned => push("pinned"),
            EventKind::McpDrift => push("drift"),
            EventKind::PackActivated => push("pack"),
            EventKind::QueueSubmitted => push("queue_submitted"),
            EventKind::QueueDelivered => push("queue_delivered"),
            EventKind::QueueInterrupt => push("queue_interrupt"),
            _ => {}
        }
    }
}
