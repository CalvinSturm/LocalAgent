use std::path::Path;
use std::time::{Duration, Instant};

use crate::trust::approvals::{ApprovalsStore, StoredStatus};

use super::{ApprovalRow, ToolRow, UiState};

impl UiState {
    pub(super) fn bump_usage(&mut self, side_effects: &str) {
        self.total_tool_execs = self.total_tool_execs.saturating_add(1);
        match side_effects {
            "filesystem_read" => {
                self.filesystem_read_execs = self.filesystem_read_execs.saturating_add(1)
            }
            "filesystem_write" => {
                self.filesystem_write_execs = self.filesystem_write_execs.saturating_add(1)
            }
            "shell_exec" => self.shell_execs = self.shell_execs.saturating_add(1),
            "network" => self.network_execs = self.network_execs.saturating_add(1),
            "browser" => self.browser_execs = self.browser_execs.saturating_add(1),
            _ => {}
        }
    }

    pub(super) fn close_running_tools(&mut self, status: &str, reason_token: &str) {
        for row in &mut self.tool_calls {
            if row.status == "running" || row.status == "STALL" {
                row.status = status.to_string();
                row.reason_token = reason_token.to_string();
                row.running_since = None;
                row.running_for_ms = 0;
                if row.ok.is_none() {
                    row.ok = Some(false);
                }
                if row.short_result.trim().is_empty() {
                    row.short_result = "closed on protocol error".to_string();
                }
            }
        }
    }

    pub fn refresh_approvals(&mut self, path: &Path) -> anyhow::Result<()> {
        let store = ApprovalsStore::new(path.to_path_buf());
        let data = store.list()?;
        let mut rows = data
            .requests
            .into_iter()
            .map(|(id, req)| ApprovalRow {
                id,
                tool: req.tool,
                status: match req.status {
                    StoredStatus::Pending => "pending",
                    StoredStatus::Approved => "approved",
                    StoredStatus::Denied => "denied",
                }
                .to_string(),
                created_at: req.created_at,
            })
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        self.pending_approvals = rows;
        Ok(())
    }

    pub fn pending_approval_count(&self) -> usize {
        self.pending_approvals
            .iter()
            .filter(|r| r.status == "pending")
            .count()
    }

    pub fn push_log(&mut self, line: String) {
        self.logs.push(line);
        if self.logs.len() > self.max_log_lines {
            let drain = self.logs.len() - self.max_log_lines;
            self.logs.drain(0..drain);
        }
    }

    pub(super) fn upsert_tool(
        &mut self,
        tool_call_id: String,
        tool_name: String,
        side_effects: String,
        status: &str,
    ) -> &mut ToolRow {
        if let Some(idx) = self
            .tool_calls
            .iter()
            .position(|t| t.tool_call_id == tool_call_id)
        {
            let row = &mut self.tool_calls[idx];
            row.status = status.to_string();
            if status == "running" && row.running_since.is_none() {
                row.running_since = Some(Instant::now());
                row.running_for_ms = 0;
            }
            if !tool_name.is_empty() {
                row.tool_name = tool_name;
            }
            if !side_effects.is_empty() {
                row.side_effects = side_effects;
            }
            return row;
        }
        self.tool_calls.push(ToolRow {
            tool_call_id,
            tool_name,
            side_effects,
            decision: None,
            decision_source: None,
            reason_token: "-".to_string(),
            decision_reason: None,
            status: status.to_string(),
            running_since: if status == "running" {
                Some(Instant::now())
            } else {
                None
            },
            running_for_ms: 0,
            ok: None,
            short_result: String::new(),
        });
        self.tool_calls.last_mut().expect("tool row")
    }

    pub fn step_allowed_tools_compact(&self) -> String {
        if self.current_step_allowed_tools.is_empty() {
            "-".to_string()
        } else {
            self.current_step_allowed_tools.join(",")
        }
    }

    pub fn last_tool_summary(&self) -> String {
        if let Some(last) = self.tool_calls.last() {
            let outcome = last.decision.clone().unwrap_or_else(|| last.status.clone());
            let reason = last.decision_reason.clone().unwrap_or_default();
            if reason.is_empty() {
                format!("{} {}", last.tool_name, outcome)
            } else {
                format!(
                    "{} {} {}",
                    last.tool_name,
                    outcome,
                    truncate_chars(&reason, 60)
                )
            }
        } else {
            "-".to_string()
        }
    }

    pub fn policy_hash_short(&self) -> String {
        short_hash(&self.policy_hash)
    }

    pub fn mcp_hash_short(&self) -> String {
        short_hash(&self.mcp_catalog_hash)
    }

    pub fn mcp_status_compact(&self) -> String {
        if self.mcp_catalog_hash.is_empty() {
            "-".to_string()
        } else if self.mcp_running_for_ms > 0 {
            format!(
                "{}:{}:{}s",
                self.mcp_hash_short(),
                self.mcp_lifecycle,
                self.mcp_running_for_ms / 1000
            )
        } else {
            format!("{}:{}", self.mcp_hash_short(), self.mcp_lifecycle)
        }
    }

    pub fn mark_cancel_requested(&mut self) {
        self.cancel_lifecycle = "REQUESTED".to_string();
        self.next_hint = "cancel_requested".to_string();
        self.push_log(
            "cancel requested; waiting for run to terminate (press q again to force quit)"
                .to_string(),
        );
    }

    pub fn cancel_requested(&self) -> bool {
        self.cancel_lifecycle == "REQUESTED"
    }

    pub fn on_tick(&mut self, now: Instant) {
        const MCP_STALL_THRESHOLD: Duration = Duration::from_secs(10);
        let mut has_mcp_running = false;
        let mut max_mcp_elapsed_ms = 0u64;
        let mut stall_notice: Option<String> = None;
        for row in &mut self.tool_calls {
            if row.status != "running" && row.status != "STALL" {
                continue;
            }
            let Some(since) = row.running_since else {
                continue;
            };
            let elapsed = now.duration_since(since);
            row.running_for_ms = elapsed.as_millis() as u64;
            if is_mcp_tool(&row.tool_name) {
                has_mcp_running = true;
                max_mcp_elapsed_ms = max_mcp_elapsed_ms.max(row.running_for_ms);
                if elapsed >= MCP_STALL_THRESHOLD {
                    row.status = "STALL".to_string();
                    row.reason_token = "net".to_string();
                    if !self.mcp_stall_notice_emitted {
                        stall_notice = Some(format!(
                            "mcp_stall: tool={} running_for={}s",
                            row.tool_name,
                            row.running_for_ms / 1000
                        ));
                        self.mcp_stall_notice_emitted = true;
                    }
                }
            }
        }
        if let Some(line) = stall_notice {
            self.push_log(line);
        }
        if has_mcp_running {
            self.mcp_running_for_ms = max_mcp_elapsed_ms;
            if max_mcp_elapsed_ms >= MCP_STALL_THRESHOLD.as_millis() as u64 {
                self.mcp_lifecycle = "STALL".to_string();
                self.mcp_stalled = true;
            } else {
                self.mcp_lifecycle = "RUNNING".to_string();
                self.mcp_stalled = false;
            }
        }
    }
}

pub(super) fn reason_token(source: &str, reason: Option<&str>) -> &'static str {
    match source {
        "plan_step_constraint" => "plan",
        "runtime_budget" => "budget",
        "policy" => "policy",
        "approval_store" => "approval",
        _ => {
            let lower = reason.unwrap_or_default().to_ascii_lowercase();
            if lower.contains("invalid tool arguments")
                || lower.contains("missing required field")
                || lower.contains("schema")
            {
                "schema"
            } else if lower.contains("timeout")
                || lower.contains("timed out")
                || lower.contains("connection refused")
                || lower.contains("network")
            {
                "net"
            } else if lower.contains("user denied")
                || lower.contains("cancelled")
                || lower.contains("canceled")
            {
                "user"
            } else if lower.is_empty() {
                "other"
            } else {
                "tool"
            }
        }
    }
}

pub(super) fn class_to_reason_token(class: &str) -> &'static str {
    match class {
        "E_PROTOCOL" => "protocol",
        "E_SCHEMA" => "schema",
        "E_POLICY" => "policy",
        "E_TIMEOUT_TRANSIENT" | "E_NETWORK_TRANSIENT" => "net",
        "E_SELECTOR_AMBIGUOUS" | "E_NON_IDEMPOTENT" | "E_OTHER" => "tool",
        _ => "other",
    }
}

fn short_hash(s: &str) -> String {
    if s.is_empty() {
        "-".to_string()
    } else {
        s.chars().take(8).collect()
    }
}

pub(super) fn is_mcp_tool(name: &str) -> bool {
    name.starts_with("mcp.")
}

pub(super) fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    input.chars().take(max_chars).collect()
}

pub(super) fn is_protocol_violation_text(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("model_tool_protocol_violation")
        || t.contains("repeated malformed tool calls")
        || t.contains("repeated invalid patch format")
        || t.contains("tool-only phase")
        || t.contains("no tool call returned by probe")
}
