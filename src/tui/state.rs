use std::time::Instant;

use crate::events::{Event, EventKind};

mod events;
mod support;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Default)]
pub struct ToolRow {
    pub tool_call_id: String,
    pub tool_name: String,
    pub side_effects: String,
    pub decision: Option<String>,
    pub decision_source: Option<String>,
    pub reason_token: String,
    pub decision_reason: Option<String>,
    pub status: String,
    pub running_since: Option<Instant>,
    pub running_for_ms: u64,
    pub ok: Option<bool>,
    pub short_result: String,
}

#[derive(Debug, Clone, Default)]
pub struct ApprovalRow {
    pub id: String,
    pub tool: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct UiState {
    pub run_id: String,
    pub step: u32,
    pub provider: String,
    pub model: String,
    pub mode_label: String,
    pub authority_label: String,
    pub mcp_pin_enforcement: String,
    pub caps_source: String,
    pub policy_hash: String,
    pub mcp_catalog_hash: String,
    pub mcp_pin_state: String,
    pub mcp_lifecycle: String,
    pub mcp_running_for_ms: u64,
    pub mcp_stalled: bool,
    mcp_stall_notice_emitted: bool,
    pub cancel_lifecycle: String,
    pub net_status: String,
    pub assistant_text: String,
    pub tool_calls: Vec<ToolRow>,
    pub pending_approvals: Vec<ApprovalRow>,
    pub logs: Vec<String>,
    pub exit_reason: Option<String>,
    pub show_details: bool,
    pub current_step_id: String,
    pub current_step_goal: String,
    pub current_step_allowed_tools: Vec<String>,
    pub next_hint: String,
    pub enforce_plan_tools_effective: String,
    pub schema_repair_seen: bool,
    pub last_failure_class: String,
    pub last_tool_retry_count: u64,
    pub total_tool_execs: u64,
    pub filesystem_read_execs: u64,
    pub filesystem_write_execs: u64,
    pub shell_execs: u64,
    pub network_execs: u64,
    pub browser_execs: u64,
    max_log_lines: usize,
}

impl UiState {
    pub fn new(max_log_lines: usize) -> Self {
        Self {
            run_id: String::new(),
            step: 0,
            provider: String::new(),
            model: String::new(),
            mode_label: "SAFE".to_string(),
            authority_label: "VETO".to_string(),
            mcp_pin_enforcement: "HARD".to_string(),
            caps_source: String::new(),
            policy_hash: String::new(),
            mcp_catalog_hash: String::new(),
            mcp_pin_state: "-".to_string(),
            mcp_lifecycle: "IDLE".to_string(),
            mcp_running_for_ms: 0,
            mcp_stalled: false,
            mcp_stall_notice_emitted: false,
            cancel_lifecycle: "NONE".to_string(),
            net_status: "OK".to_string(),
            assistant_text: String::new(),
            tool_calls: Vec::new(),
            pending_approvals: Vec::new(),
            logs: Vec::new(),
            exit_reason: None,
            show_details: false,
            current_step_id: "-".to_string(),
            current_step_goal: "-".to_string(),
            current_step_allowed_tools: Vec::new(),
            next_hint: "-".to_string(),
            enforce_plan_tools_effective: "-".to_string(),
            schema_repair_seen: false,
            last_failure_class: "-".to_string(),
            last_tool_retry_count: 0,
            total_tool_execs: 0,
            filesystem_read_execs: 0,
            filesystem_write_execs: 0,
            shell_execs: 0,
            network_execs: 0,
            browser_execs: 0,
            max_log_lines,
        }
    }

    pub fn apply_event(&mut self, ev: &Event) {
        self.step = ev.step;
        if self.run_id.is_empty() {
            self.run_id = ev.run_id.clone();
        }
        match ev.kind {
            EventKind::RunStart => self.apply_run_start_event(ev),
            EventKind::RunEnd => self.apply_run_end_event(ev),
            EventKind::ModelDelta => self.apply_model_delta_event(ev),
            EventKind::ModelResponseEnd => self.apply_model_response_end_event(ev),
            EventKind::ToolCallDetected => self.apply_tool_call_detected_event(ev),
            EventKind::ToolDecision => self.apply_tool_decision_event(ev),
            EventKind::ToolExecStart => self.apply_tool_exec_start_event(ev),
            EventKind::ToolExecEnd => self.apply_tool_exec_end_event(ev),
            EventKind::PostWriteVerifyStart => self.apply_post_write_verify_start_event(ev),
            EventKind::PostWriteVerifyEnd => self.apply_post_write_verify_end_event(ev),
            EventKind::PolicyLoaded => self.apply_policy_loaded_event(ev),
            EventKind::PlannerStart | EventKind::WorkerStart => self.apply_plan_lifecycle_event(ev),
            EventKind::ProviderError => self.apply_provider_error_event(ev),
            EventKind::ProviderRetry => self.apply_provider_retry_event(ev),
            EventKind::ToolRetry => self.apply_tool_retry_event(ev),
            EventKind::McpDrift => self.apply_mcp_drift_event(ev),
            EventKind::McpProgress => self.apply_mcp_progress_event(ev),
            EventKind::McpCancelled => self.apply_mcp_cancelled_event(ev),
            EventKind::McpPinned => self.apply_mcp_pinned_event(ev),
            EventKind::PackActivated => self.apply_pack_activated_event(ev),
            EventKind::QueueSubmitted => self.apply_queue_submitted_event(ev),
            EventKind::QueueDelivered => self.apply_queue_delivered_event(ev),
            EventKind::QueueInterrupt => self.apply_queue_interrupt_event(ev),
            EventKind::Error => self.apply_error_event(ev),
            _ => self.apply_misc_log_event(ev),
        }
    }
}
