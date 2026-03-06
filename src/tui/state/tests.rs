use std::time::{Duration, Instant};

use tempfile::tempdir;

use crate::events::{Event, EventKind};
use crate::trust::approvals::ApprovalsStore;

use super::{ApprovalRow, UiState};

#[test]
fn apply_event_model_delta_appends() {
    let mut s = UiState::new(10);
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ModelDelta,
        serde_json::json!({"delta":"hello"}),
    ));
    assert_eq!(s.assistant_text, "hello");
}

#[test]
fn apply_event_tool_lifecycle() {
    let mut s = UiState::new(10);
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolCallDetected,
        serde_json::json!({"tool_call_id":"tc1","name":"read_file","side_effects":"filesystem_read"}),
    ));
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolDecision,
        serde_json::json!({"tool_call_id":"tc1","name":"read_file","decision":"allow","reason":"ok"}),
    ));
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolExecEnd,
        serde_json::json!({"tool_call_id":"tc1","name":"read_file","ok":true,"content":"abc"}),
    ));
    assert_eq!(s.tool_calls.len(), 1);
    assert_eq!(s.tool_calls[0].decision.as_deref(), Some("allow"));
    assert_eq!(s.tool_calls[0].ok, Some(true));
    assert_eq!(s.tool_calls[0].short_result, "abc");
    assert_eq!(s.total_tool_execs, 1);
    assert_eq!(s.filesystem_read_execs, 1);
}

#[test]
fn tool_decision_reflects_deny_and_approval_in_tui_state() {
    let mut s = UiState::new(10);
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolCallDetected,
        serde_json::json!({"tool_call_id":"tc_mcp","name":"mcp.stub.echo","side_effects":"network"}),
    ));
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolDecision,
        serde_json::json!({
            "tool_call_id":"tc_mcp",
            "name":"mcp.stub.echo",
            "side_effects":"network",
            "decision":"require_approval",
            "reason":"shell requires approval",
            "source":"policy"
        }),
    ));
    assert_eq!(s.tool_calls.len(), 1);
    assert_eq!(
        s.tool_calls[0].decision.as_deref(),
        Some("require_approval")
    );
    assert_eq!(s.tool_calls[0].status, "PEND:approval");
    assert_eq!(s.next_hint, "pending_approval");
    assert_eq!(s.mcp_lifecycle, "WAIT:APPROVAL");

    s.apply_event(&Event::new(
        "r1".to_string(),
        2,
        EventKind::ToolCallDetected,
        serde_json::json!({"tool_call_id":"tc2","name":"write_file","side_effects":"filesystem_write"}),
    ));
    s.apply_event(&Event::new(
        "r1".to_string(),
        2,
        EventKind::ToolDecision,
        serde_json::json!({
            "tool_call_id":"tc2",
            "name":"write_file",
            "side_effects":"filesystem_write",
            "decision":"deny",
            "reason":"writes denied",
            "source":"policy"
        }),
    ));
    assert_eq!(s.tool_calls.len(), 2);
    assert_eq!(s.tool_calls[1].decision.as_deref(), Some("deny"));
    assert!(s.tool_calls[1].status.starts_with("DENY:"));
    assert_eq!(s.next_hint, "blocked(policy)");
}

#[test]
fn schema_repair_flag_turns_on_from_retry_event() {
    let mut s = UiState::new(10);
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolRetry,
        serde_json::json!({"name":"read_file","attempt":1,"max_retries":1,"failure_class":"E_SCHEMA","action":"repair"}),
    ));
    assert!(s.schema_repair_seen);
    assert_eq!(s.last_failure_class, "E_SCHEMA");
    assert_eq!(s.last_tool_retry_count, 1);
}

#[test]
fn failure_class_and_retry_count_are_captured_on_tool_exec_end() {
    let mut s = UiState::new(10);
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolExecEnd,
        serde_json::json!({
            "tool_call_id":"tc1",
            "name":"read_file",
            "ok":false,
            "content":"error",
            "retry_count":1,
            "failure_class":"E_TIMEOUT_TRANSIENT"
        }),
    ));
    assert_eq!(s.last_failure_class, "E_TIMEOUT_TRANSIENT");
    assert_eq!(s.last_tool_retry_count, 1);
}

#[test]
fn tool_exec_end_protocol_violation_sets_protocol_reason_token() {
    let mut s = UiState::new(10);
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolExecEnd,
        serde_json::json!({
            "tool_call_id":"tc1",
            "name":"apply_patch",
            "ok":false,
            "content":"MODEL_TOOL_PROTOCOL_VIOLATION: repeated invalid patch format for apply_patch",
            "retry_count":1,
            "failure_class":"E_OTHER"
        }),
    ));
    assert_eq!(s.tool_calls.len(), 1);
    assert_eq!(s.tool_calls[0].reason_token, "protocol");
    assert_eq!(s.tool_calls[0].status, "FAIL:protocol");
}

#[test]
fn logs_are_capped() {
    let mut s = UiState::new(2);
    s.push_log("a".to_string());
    s.push_log("b".to_string());
    s.push_log("c".to_string());
    assert_eq!(s.logs, vec!["b".to_string(), "c".to_string()]);
}

#[test]
fn approvals_refresh_and_transition() {
    let tmp = tempdir().expect("tmp");
    let path = tmp.path().join("approvals.json");
    let store = ApprovalsStore::new(path.clone());
    let id = store
        .create_pending("shell", &serde_json::json!({"cmd":"echo"}), None, None)
        .expect("pending");
    let mut s = UiState::new(10);
    s.refresh_approvals(&path).expect("refresh");
    assert_eq!(s.pending_approvals.len(), 1);
    assert_eq!(s.pending_approvals[0].status, "pending");
    store.approve(&id, None, None).expect("approve");
    s.refresh_approvals(&path).expect("refresh2");
    assert_eq!(s.pending_approvals[0].status, "approved");
}

#[test]
fn approvals_queue_multiple_entries_preserve_independent_statuses() {
    let tmp = tempdir().expect("tmp");
    let path = tmp.path().join("approvals.json");
    let store = ApprovalsStore::new(path.clone());
    let id1 = store
        .create_pending("shell", &serde_json::json!({"cmd":"echo one"}), None, None)
        .expect("pending1");
    let id2 = store
        .create_pending(
            "write_file",
            &serde_json::json!({"path":"x","content":"y"}),
            None,
            None,
        )
        .expect("pending2");

    let mut s = UiState::new(10);
    s.refresh_approvals(&path).expect("refresh");
    assert_eq!(s.pending_approvals.len(), 2);
    let rows = s
        .pending_approvals
        .iter()
        .map(|r| (r.id.clone(), r.status.clone(), r.tool.clone()))
        .collect::<Vec<_>>();
    let rows = rows
        .into_iter()
        .map(|(id, status, tool)| (id, (status, tool)))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        rows.get(&id1),
        Some(&("pending".to_string(), "shell".to_string()))
    );
    assert_eq!(
        rows.get(&id2),
        Some(&("pending".to_string(), "write_file".to_string()))
    );

    store.deny(&id1).expect("deny1");
    s.refresh_approvals(&path).expect("refresh2");
    let rows2 = s
        .pending_approvals
        .iter()
        .map(|r| (r.id.clone(), r.status.clone(), r.tool.clone()))
        .collect::<Vec<_>>();
    let rows2 = rows2
        .into_iter()
        .map(|(id, status, tool)| (id, (status, tool)))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        rows2.get(&id1),
        Some(&("denied".to_string(), "shell".to_string()))
    );
    assert_eq!(
        rows2.get(&id2),
        Some(&("pending".to_string(), "write_file".to_string()))
    );
}

#[test]
fn pending_approval_count_counts_pending_only() {
    let mut s = UiState::new(10);
    s.pending_approvals = vec![
        ApprovalRow {
            id: "a1".to_string(),
            tool: "shell".to_string(),
            status: "pending".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        },
        ApprovalRow {
            id: "a2".to_string(),
            tool: "write_file".to_string(),
            status: "approved".to_string(),
            created_at: "2026-01-01T00:00:01Z".to_string(),
        },
        ApprovalRow {
            id: "a3".to_string(),
            tool: "shell".to_string(),
            status: "denied".to_string(),
            created_at: "2026-01-01T00:00:02Z".to_string(),
        },
    ];
    assert_eq!(s.pending_approval_count(), 1);
}

#[test]
fn mcp_lifecycle_running_retry_done() {
    let mut s = UiState::new(10);
    s.mcp_catalog_hash = "abcdef123456".to_string();
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolExecStart,
        serde_json::json!({"tool_call_id":"tc1","name":"mcp.playwright.browser_snapshot","side_effects":"browser"}),
    ));
    assert_eq!(s.mcp_lifecycle, "RUNNING");
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolRetry,
        serde_json::json!({"name":"mcp.playwright.browser_snapshot","attempt":1,"max_retries":1,"failure_class":"E_TIMEOUT_TRANSIENT","action":"retry"}),
    ));
    assert_eq!(s.mcp_lifecycle, "WAIT:RETRY");
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolExecEnd,
        serde_json::json!({"tool_call_id":"tc1","name":"mcp.playwright.browser_snapshot","ok":true,"content":"ok","retry_count":1,"failure_class":null}),
    ));
    assert_eq!(s.mcp_lifecycle, "DONE");
    assert!(s.mcp_status_compact().contains("DONE"));
}

#[test]
fn mcp_running_tool_marked_cancelled_on_run_cancel() {
    let mut s = UiState::new(10);
    s.mark_cancel_requested();
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolExecStart,
        serde_json::json!({"tool_call_id":"tc1","name":"mcp.playwright.browser_snapshot","side_effects":"browser"}),
    ));
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::RunEnd,
        serde_json::json!({"exit_reason":"cancelled"}),
    ));
    assert_eq!(s.mcp_lifecycle, "CANCELLED");
    assert_eq!(s.cancel_lifecycle, "COMPLETE");
    assert_eq!(s.tool_calls[0].status, "CANCEL:user");
    assert_eq!(s.tool_calls[0].reason_token, "user");
}

#[test]
fn non_mcp_running_tool_is_closed_on_run_end() {
    let mut s = UiState::new(10);
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolExecStart,
        serde_json::json!({"tool_call_id":"tc1","name":"write_file","side_effects":"filesystem_write"}),
    ));
    assert_eq!(s.tool_calls[0].status, "running");
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::RunEnd,
        serde_json::json!({"exit_reason":"ok"}),
    ));
    assert_eq!(s.tool_calls[0].status, "DONE:run_end");
    assert_eq!(s.tool_calls[0].reason_token, "run_end");
    assert!(s.tool_calls[0]
        .short_result
        .contains("closed on run_end without tool_exec_end"));
}

#[test]
fn tool_protocol_guard_error_closes_running_tool_row() {
    let mut s = UiState::new(10);
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolExecStart,
        serde_json::json!({"tool_call_id":"tc1","name":"apply_patch","side_effects":"filesystem_write"}),
    ));
    assert_eq!(s.tool_calls[0].status, "running");
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::Error,
        serde_json::json!({"source":"tool_protocol_guard","error":"MODEL_TOOL_PROTOCOL_VIOLATION"}),
    ));
    assert_eq!(s.tool_calls[0].status, "FAIL:protocol");
    assert_eq!(s.tool_calls[0].reason_token, "protocol");
    assert_eq!(s.tool_calls[0].ok, Some(false));
    assert_eq!(s.next_hint, "blocked(protocol)");
}

#[test]
fn cancel_request_transitions_to_complete_on_run_end() {
    let mut s = UiState::new(10);
    s.mark_cancel_requested();
    assert!(s.cancel_requested());
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::RunEnd,
        serde_json::json!({"exit_reason":"ok"}),
    ));
    assert_eq!(s.cancel_lifecycle, "COMPLETE");
}

#[test]
fn on_tick_marks_long_running_mcp_as_stalled() {
    let mut s = UiState::new(10);
    s.mcp_catalog_hash = "abcdef123456".to_string();
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::ToolExecStart,
        serde_json::json!({"tool_call_id":"tc1","name":"mcp.playwright.browser_snapshot","side_effects":"browser"}),
    ));
    s.tool_calls[0].running_since = Some(Instant::now() - Duration::from_secs(12));
    s.on_tick(Instant::now());
    assert_eq!(s.mcp_lifecycle, "STALL");
    assert!(s.mcp_stalled);
    assert!(s.mcp_running_for_ms >= 12_000);
    assert_eq!(s.tool_calls[0].status, "STALL");
}

#[test]
fn mcp_drift_event_sets_drift_lifecycle() {
    let mut s = UiState::new(10);
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::McpDrift,
        serde_json::json!({
            "name":"mcp.playwright.browser_snapshot",
            "expected_hash_hex":"abc",
            "actual_hash_hex":"def"
        }),
    ));
    assert_eq!(s.mcp_lifecycle, "DRIFT");
}

#[test]
fn mcp_docs_drift_event_logs_docs_specific_summary() {
    let mut s = UiState::new(10);
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::McpDrift,
        serde_json::json!({
            "name":"mcp.stub.echo",
            "catalog_hash_expected":"aaa",
            "catalog_hash_live":"aaa",
            "catalog_drift":false,
            "docs_hash_expected":"bbb",
            "docs_hash_live":"ccc",
            "docs_drift":true,
            "primary_code":"MCP_DOCS_DRIFT"
        }),
    ));
    assert_eq!(s.mcp_lifecycle, "DRIFT");
    let last = s.logs.last().cloned().unwrap_or_default();
    assert!(last.contains("MCP_DOCS_DRIFT"));
    assert!(last.contains("docs"));
    assert!(last.contains("mcp.stub.echo"));
}

#[test]
fn mcp_pinned_event_sets_pin_state() {
    let mut s = UiState::new(10);
    s.apply_event(&Event::new(
        "r".to_string(),
        1,
        EventKind::McpPinned,
        serde_json::json!({
            "enforcement":"warn",
            "configured_hash_hex":"abc",
            "startup_live_hash_hex":"abc",
            "pinned":true
        }),
    ));
    assert_eq!(s.mcp_pin_state, "PINNED");
    assert_eq!(s.mcp_pin_enforcement, "WARN");
}

#[test]
fn mcp_progress_event_updates_lifecycle() {
    let mut s = UiState::new(10);
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::McpProgress,
        serde_json::json!({
            "name":"mcp.playwright.browser_snapshot",
            "progress_ticks":2,
            "elapsed_ms":1500
        }),
    ));
    assert_eq!(s.mcp_lifecycle, "WAIT:TASK");
    assert_eq!(s.mcp_running_for_ms, 1500);
}

#[test]
fn mcp_cancelled_event_sets_cancelled_lifecycle() {
    let mut s = UiState::new(10);
    s.apply_event(&Event::new(
        "r1".to_string(),
        1,
        EventKind::McpCancelled,
        serde_json::json!({
            "name":"mcp.playwright.browser_snapshot",
            "reason":"timeout"
        }),
    ));
    assert_eq!(s.mcp_lifecycle, "CANCELLED");
    assert_eq!(s.next_hint, "cancelled");
}

#[test]
fn queue_events_log_stable_summaries() {
    let mut s = UiState::new(50);
    s.apply_event(&Event::new(
        "r".to_string(),
        1,
        EventKind::QueueSubmitted,
        serde_json::json!({
            "queue_id":"q7",
            "kind":"steer",
            "truncated": false,
            "bytes_kept": 12,
            "next_delivery":"after current tool finishes"
        }),
    ));
    s.apply_event(&Event::new(
        "r".to_string(),
        1,
        EventKind::QueueDelivered,
        serde_json::json!({
            "queue_id":"q7",
            "kind":"steer",
            "delivery_boundary":"post_tool"
        }),
    ));
    s.apply_event(&Event::new(
        "r".to_string(),
        1,
        EventKind::QueueInterrupt,
        serde_json::json!({
            "queue_id":"q7",
            "cancelled_remaining_work": true,
            "cancelled_reason":"operator_steer"
        }),
    ));
    let joined = s.logs.join("\n");
    assert!(joined.contains("queue_submitted: id=q7 kind=steer"));
    assert!(joined.contains("queue_delivered: id=q7 kind=steer boundary=post_tool"));
    assert!(joined.contains("queue_interrupt: id=q7 cancelled_remaining_work=true"));
    assert_eq!(s.next_hint, "interrupt_applied");
}
