use crate::events::{Event, EventKind};

use super::support::{
    class_to_reason_token, is_mcp_tool, is_protocol_violation_text, reason_token, truncate_chars,
};
use super::UiState;

impl UiState {
    pub(super) fn apply_tool_call_detected_event(&mut self, ev: &Event) {
        let id = ev
            .data
            .get("tool_call_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let name = ev
            .data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let side = ev
            .data
            .get("side_effects")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        self.upsert_tool(id, name, side, "detected");
    }

    pub(super) fn apply_run_start_event(&mut self, ev: &Event) {
        self.model = ev
            .data
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if let Some(mode) = ev
            .data
            .get("enforce_plan_tools_effective")
            .and_then(|v| v.as_str())
        {
            self.enforce_plan_tools_effective = mode.to_string();
        }
        self.net_status = "OK".to_string();
        self.mcp_lifecycle = "IDLE".to_string();
        self.mcp_pin_state = "-".to_string();
        self.mcp_running_for_ms = 0;
        self.mcp_stalled = false;
        self.mcp_stall_notice_emitted = false;
        self.cancel_lifecycle = "NONE".to_string();
    }

    pub(super) fn apply_run_end_event(&mut self, ev: &Event) {
        let exit_reason = ev
            .data
            .get("exit_reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        self.exit_reason = exit_reason.clone();
        if self.cancel_lifecycle == "REQUESTED" || exit_reason.as_deref() == Some("cancelled") {
            self.cancel_lifecycle = "COMPLETE".to_string();
        }
        if exit_reason.as_deref() == Some("cancelled") {
            if self.tool_calls.iter().any(|t| {
                is_mcp_tool(&t.tool_name) && (t.status == "running" || t.status == "STALL")
            }) {
                self.mcp_lifecycle = "CANCELLED".to_string();
            }
            for row in &mut self.tool_calls {
                if is_mcp_tool(&row.tool_name) && (row.status == "running" || row.status == "STALL")
                {
                    row.status = "CANCEL:user".to_string();
                    row.reason_token = "user".to_string();
                    row.running_since = None;
                    row.running_for_ms = 0;
                    row.ok = Some(false);
                }
            }
            self.mcp_running_for_ms = 0;
            self.mcp_stalled = false;
            self.mcp_stall_notice_emitted = false;
        }
        let close_status = if exit_reason.as_deref() == Some("cancelled") {
            "CANCEL:run_end"
        } else {
            "DONE:run_end"
        };
        for row in &mut self.tool_calls {
            if row.status == "running" || row.status == "STALL" {
                row.status = close_status.to_string();
                row.reason_token = "run_end".to_string();
                row.running_since = None;
                row.running_for_ms = 0;
                if row.short_result.trim().is_empty() {
                    row.short_result = "closed on run_end without tool_exec_end".to_string();
                }
            }
        }
        self.next_hint = "done".to_string();
    }

    pub(super) fn apply_model_delta_event(&mut self, ev: &Event) {
        if let Some(delta) = ev.data.get("delta").and_then(|v| v.as_str()) {
            self.assistant_text.push_str(delta);
        }
    }

    pub(super) fn apply_model_response_end_event(&mut self, ev: &Event) {
        if self.assistant_text.is_empty() {
            if let Some(content) = ev.data.get("content").and_then(|v| v.as_str()) {
                self.assistant_text.push_str(content);
            }
        }
    }

    pub(super) fn apply_policy_loaded_event(&mut self, ev: &Event) {
        if let Some(hash) = ev.data.get("policy_hash_hex").and_then(|v| v.as_str()) {
            self.policy_hash = hash.to_string();
        }
    }

    pub(super) fn apply_plan_lifecycle_event(&mut self, ev: &Event) {
        if let Some(mode) = ev
            .data
            .get("enforce_plan_tools_effective")
            .and_then(|v| v.as_str())
        {
            self.enforce_plan_tools_effective = mode.to_string();
        }
        if let Some(step_id) = ev.data.get("plan_step_id").and_then(|v| v.as_str()) {
            self.current_step_id = step_id.to_string();
        }
    }

    pub(super) fn apply_tool_decision_event(&mut self, ev: &Event) {
        let id = ev
            .data
            .get("tool_call_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let name = ev
            .data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let side = ev
            .data
            .get("side_effects")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let decision = ev
            .data
            .get("decision")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let reason = ev
            .data
            .get("reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let source = ev
            .data
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let is_mcp = is_mcp_tool(&name);
        let is_deny = decision == "deny";
        let is_pending = decision == "require_approval";
        {
            let row = self.upsert_tool(id, name, side, "decided");
            row.decision = Some(decision);
            row.decision_source = Some(source.clone());
            row.reason_token = reason_token(&source, reason.as_deref()).to_string();
            row.decision_reason = reason;
            if is_deny {
                row.status = format!("DENY:{}", row.reason_token);
            } else if is_pending {
                row.status = "PEND:approval".to_string();
            }
        }
        if let Some(step_id) = ev.data.get("plan_step_id").and_then(|v| v.as_str()) {
            self.current_step_id = step_id.to_string();
        }
        if let Some(allowed) = ev.data.get("plan_allowed_tools").and_then(|v| v.as_array()) {
            self.current_step_allowed_tools = allowed
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
        if is_deny {
            self.next_hint = format!("blocked({})", reason_token(&source, None));
        } else if is_pending {
            self.next_hint = "pending_approval".to_string();
        }
        if is_mcp {
            if is_pending {
                self.mcp_lifecycle = "WAIT:APPROVAL".to_string();
            } else if is_deny {
                self.mcp_lifecycle = "DENY".to_string();
            }
        }
    }

    pub(super) fn apply_tool_exec_start_event(&mut self, ev: &Event) {
        let id = ev
            .data
            .get("tool_call_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let name = ev
            .data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let side = ev
            .data
            .get("side_effects")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let _ = self.upsert_tool(id, name, side, "running");
        if is_mcp_tool(
            ev.data
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
        ) {
            self.mcp_lifecycle = "RUNNING".to_string();
            self.mcp_running_for_ms = 0;
            self.mcp_stalled = false;
            self.mcp_stall_notice_emitted = false;
        }
    }

    pub(super) fn apply_tool_exec_end_event(&mut self, ev: &Event) {
        let id = ev
            .data
            .get("tool_call_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let name = ev
            .data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let ok = ev.data.get("ok").and_then(|v| v.as_bool());
        let result = ev
            .data
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let failure_class = ev
            .data
            .get("failure_class")
            .and_then(|v| v.as_str())
            .unwrap_or("E_OTHER");
        let side_effects = {
            let row = self.upsert_tool(id, name, String::new(), "done");
            row.ok = ok;
            row.short_result = truncate_chars(result, 200);
            row.running_since = None;
            row.running_for_ms = 0;
            if matches!(ok, Some(false)) {
                let mut token = class_to_reason_token(failure_class).to_string();
                if is_protocol_violation_text(result) {
                    token = "protocol".to_string();
                }
                row.status = format!("FAIL:{token}");
                row.reason_token = token;
            }
            row.side_effects.clone()
        };
        if matches!(ok, Some(true)) {
            self.bump_usage(&side_effects);
            self.next_hint = "continue".to_string();
            if is_mcp_tool(
                ev.data
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
            ) {
                self.mcp_lifecycle = "DONE".to_string();
                self.mcp_running_for_ms = 0;
                self.mcp_stalled = false;
                self.mcp_stall_notice_emitted = false;
            }
        } else if is_mcp_tool(
            ev.data
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
        ) {
            self.mcp_lifecycle = "FAIL".to_string();
            self.mcp_running_for_ms = 0;
            self.mcp_stalled = false;
            self.mcp_stall_notice_emitted = false;
        }
        self.last_tool_retry_count = ev
            .data
            .get("retry_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        self.last_failure_class = ev
            .data
            .get("failure_class")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("-")
            .to_string();
    }

    pub(super) fn apply_post_write_verify_start_event(&mut self, ev: &Event) {
        let path = ev.data.get("path").and_then(|v| v.as_str()).unwrap_or("-");
        let timeout_ms = ev
            .data
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        self.next_hint = "runtime_verify".to_string();
        self.push_log(format!(
            "post_write_verify_start: path={} timeout_ms={}",
            path, timeout_ms
        ));
    }

    pub(super) fn apply_post_write_verify_end_event(&mut self, ev: &Event) {
        let path = ev.data.get("path").and_then(|v| v.as_str()).unwrap_or("-");
        let ok = ev.data.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        let status = ev
            .data
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or(if ok { "ok" } else { "failed" });
        let elapsed_ms = ev
            .data
            .get("elapsed_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        self.push_log(format!(
            "post_write_verify_end: path={} ok={} status={} elapsed_ms={}",
            path, ok, status, elapsed_ms
        ));
        self.next_hint = if ok {
            "continue".to_string()
        } else {
            "blocked(runtime_verify)".to_string()
        };
    }

    pub(super) fn apply_mcp_drift_event(&mut self, ev: &Event) {
        let expected = ev
            .data
            .get("catalog_hash_expected")
            .or_else(|| ev.data.get("expected_hash_hex"))
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let actual = ev
            .data
            .get("catalog_hash_live")
            .or_else(|| ev.data.get("actual_hash_hex"))
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let docs_expected = ev
            .data
            .get("docs_hash_expected")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let docs_actual = ev
            .data
            .get("docs_hash_live")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let catalog_drift = ev
            .data
            .get("catalog_drift")
            .and_then(|v| v.as_bool())
            .unwrap_or(actual != expected && actual != "-");
        let docs_drift = ev
            .data
            .get("docs_drift")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let primary_code = ev
            .data
            .get("primary_code")
            .and_then(|v| v.as_str())
            .unwrap_or("MCP_DRIFT");
        self.mcp_lifecycle = "DRIFT".to_string();
        self.mcp_pin_state = "DRIFT".to_string();
        self.mcp_stalled = false;
        self.mcp_running_for_ms = 0;
        let tool = ev
            .data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("mcp.tool");
        let summary = match (catalog_drift, docs_drift) {
            (true, true) => format!(
                "mcp_drift[{primary_code}]: catalog {}->{} docs {}->{} tool={tool}",
                truncate_chars(expected, 12),
                truncate_chars(actual, 12),
                truncate_chars(docs_expected, 12),
                truncate_chars(docs_actual, 12),
            ),
            (true, false) => format!(
                "mcp_drift[{primary_code}]: catalog {}->{} tool={tool}",
                truncate_chars(expected, 12),
                truncate_chars(actual, 12),
            ),
            (false, true) => format!(
                "mcp_drift[{primary_code}]: docs {}->{} tool={tool}",
                truncate_chars(docs_expected, 12),
                truncate_chars(docs_actual, 12),
            ),
            (false, false) => format!("mcp_drift[{primary_code}]: tool={tool}"),
        };
        self.push_log(summary);
    }

    pub(super) fn apply_mcp_progress_event(&mut self, ev: &Event) {
        let ticks = ev
            .data
            .get("progress_ticks")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let elapsed_ms = ev
            .data
            .get("elapsed_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        self.mcp_lifecycle = "WAIT:TASK".to_string();
        self.mcp_running_for_ms = elapsed_ms;
        self.mcp_stalled = false;
        self.mcp_stall_notice_emitted = false;
        self.push_log(format!(
            "mcp_progress: tool={} ticks={} elapsed_ms={}",
            ev.data
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("mcp.tool"),
            ticks,
            elapsed_ms
        ));
    }

    pub(super) fn apply_mcp_cancelled_event(&mut self, ev: &Event) {
        let reason = ev
            .data
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("cancelled");
        self.mcp_lifecycle = "CANCELLED".to_string();
        self.mcp_running_for_ms = 0;
        self.mcp_stalled = false;
        self.mcp_stall_notice_emitted = false;
        self.next_hint = "cancelled".to_string();
        self.push_log(format!(
            "mcp_cancelled: tool={} reason={}",
            ev.data
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("mcp.tool"),
            reason
        ));
    }

    pub(super) fn apply_mcp_pinned_event(&mut self, ev: &Event) {
        if let Some(enforcement) = ev.data.get("enforcement").and_then(|v| v.as_str()) {
            self.mcp_pin_enforcement = enforcement.to_ascii_uppercase();
        }
        let pinned = ev
            .data
            .get("pinned")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        self.mcp_pin_state = if pinned { "PINNED" } else { "UNPINNED" }.to_string();
        self.push_log(format!(
            "mcp_pinned: configured={} startup_live={} pinned={}",
            ev.data
                .get("configured_hash_hex")
                .and_then(|v| v.as_str())
                .map(|s| truncate_chars(s, 12))
                .unwrap_or_else(|| "-".to_string()),
            ev.data
                .get("startup_live_hash_hex")
                .and_then(|v| v.as_str())
                .map(|s| truncate_chars(s, 12))
                .unwrap_or_else(|| "-".to_string()),
            pinned
        ));
    }

    pub(super) fn apply_pack_activated_event(&mut self, ev: &Event) {
        let pack_id = ev
            .data
            .get("pack_id")
            .and_then(|v| v.as_str())
            .unwrap_or("pack");
        let truncated = ev
            .data
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let bytes_kept = ev
            .data
            .get("bytes_kept")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        self.push_log(format!(
            "pack_activated: id={} truncated={} bytes_kept={}",
            pack_id, truncated, bytes_kept
        ));
    }

    pub(super) fn apply_queue_submitted_event(&mut self, ev: &Event) {
        let queue_id = ev
            .data
            .get("queue_id")
            .and_then(|v| v.as_str())
            .unwrap_or("q?");
        let kind = ev
            .data
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let truncated = ev
            .data
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let bytes_kept = ev
            .data
            .get("bytes_kept")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let boundary_phrase = ev
            .data
            .get("next_delivery")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        self.push_log(format!(
            "queue_submitted: id={} kind={} truncated={} bytes_kept={} next={}",
            queue_id, kind, truncated, bytes_kept, boundary_phrase
        ));
    }

    pub(super) fn apply_queue_delivered_event(&mut self, ev: &Event) {
        let queue_id = ev
            .data
            .get("queue_id")
            .and_then(|v| v.as_str())
            .unwrap_or("q?");
        let kind = ev
            .data
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let boundary = ev
            .data
            .get("delivery_boundary")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        self.push_log(format!(
            "queue_delivered: id={} kind={} boundary={}",
            queue_id, kind, boundary
        ));
    }

    pub(super) fn apply_queue_interrupt_event(&mut self, ev: &Event) {
        let queue_id = ev
            .data
            .get("queue_id")
            .and_then(|v| v.as_str())
            .unwrap_or("q?");
        let reason = ev
            .data
            .get("cancelled_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("operator_steer");
        let cancelled_remaining_work = ev
            .data
            .get("cancelled_remaining_work")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        self.push_log(format!(
            "queue_interrupt: id={} cancelled_remaining_work={} reason={}",
            queue_id, cancelled_remaining_work, reason
        ));
        if cancelled_remaining_work {
            self.next_hint = "interrupt_applied".to_string();
        }
    }

    pub(super) fn apply_provider_error_event(&mut self, ev: &Event) {
        let msg = ev
            .data
            .get("message_short")
            .and_then(|v| v.as_str())
            .unwrap_or("provider error");
        self.push_log(format!("provider_error: {msg}"));
        self.net_status = "DISC".to_string();
    }

    pub(super) fn apply_provider_retry_event(&mut self, ev: &Event) {
        let attempt = ev.data.get("attempt").and_then(|v| v.as_u64()).unwrap_or(0);
        let max_attempts = ev
            .data
            .get("max_attempts")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let kind = ev
            .data
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("other");
        let backoff_ms = ev
            .data
            .get("backoff_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        self.push_log(format!(
            "provider_retry: attempt {attempt}/{max_attempts} kind={kind} backoff_ms={backoff_ms}"
        ));
        self.net_status = "SLOW".to_string();
    }

    pub(super) fn apply_tool_retry_event(&mut self, ev: &Event) {
        let tool = ev
            .data
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("tool");
        let attempt = ev.data.get("attempt").and_then(|v| v.as_u64()).unwrap_or(0);
        let max_retries = ev
            .data
            .get("max_retries")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let class = ev
            .data
            .get("failure_class")
            .and_then(|v| v.as_str())
            .unwrap_or("E_OTHER");
        let action = ev
            .data
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("stop");
        if class == "E_SCHEMA" && action == "repair" {
            self.schema_repair_seen = true;
        }
        if is_mcp_tool(tool) {
            if action == "retry" {
                self.mcp_lifecycle = "WAIT:RETRY".to_string();
            } else if action == "stop" {
                self.mcp_lifecycle = "FAIL".to_string();
            }
        }
        self.last_failure_class = class.to_string();
        self.last_tool_retry_count = attempt;
        self.push_log(format!(
            "tool_retry: {tool} class={class} attempt={attempt}/{max_retries} action={action}"
        ));
    }

    pub(super) fn apply_error_event(&mut self, ev: &Event) {
        let msg = ev
            .data
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        let source = ev
            .data
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        self.push_log(format!("error: {msg}"));
        if source == "plan_halt_guard" {
            self.next_hint = "blocked(plan)".to_string();
        }
        if source == "tool_protocol_guard" {
            self.close_running_tools("FAIL:protocol", "protocol");
            self.next_hint = "blocked(protocol)".to_string();
        }
    }

    pub(super) fn apply_misc_log_event(&mut self, ev: &Event) {
        if matches!(ev.kind, EventKind::CompactionPerformed) {
            self.push_log("compaction performed".to_string());
        }
    }
}
