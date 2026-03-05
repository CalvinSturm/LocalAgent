use crate::compaction::{CompactionReport, CompactionSettings};
use crate::hooks::protocol::HookInvocationReport;
use crate::taint::TaintSpan;
use crate::trust::policy::McpAllowSummary;
use crate::types::{Message, TokenUsage, ToolCall};

#[derive(Debug, Clone, Copy)]
pub enum AgentExitReason {
    Ok,
    ProviderError,
    PlannerError,
    Denied,
    ApprovalRequired,
    HookAborted,
    MaxSteps,
    BudgetExceeded,
    Cancelled,
}

impl AgentExitReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentExitReason::Ok => "ok",
            AgentExitReason::ProviderError => "provider_error",
            AgentExitReason::PlannerError => "planner_error",
            AgentExitReason::Denied => "denied",
            AgentExitReason::ApprovalRequired => "approval_required",
            AgentExitReason::HookAborted => "hook_aborted",
            AgentExitReason::MaxSteps => "max_steps",
            AgentExitReason::BudgetExceeded => "budget_exceeded",
            AgentExitReason::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ToolCallBudget {
    pub max_wall_time_ms: u64,
    pub max_total_tool_calls: usize,
    pub max_mcp_calls: usize,
    pub max_filesystem_read_calls: usize,
    pub max_filesystem_write_calls: usize,
    pub max_shell_calls: usize,
    pub max_network_calls: usize,
    pub max_browser_calls: usize,
    pub tool_exec_timeout_ms: u64,
    pub post_write_verify_timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct AgentOutcome {
    pub run_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub exit_reason: AgentExitReason,
    pub final_output: String,
    pub error: Option<String>,
    pub messages: Vec<Message>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_decisions: Vec<ToolDecisionRecord>,
    pub compaction_settings: CompactionSettings,
    pub final_prompt_size_chars: usize,
    pub compaction_report: Option<CompactionReport>,
    pub hook_invocations: Vec<HookInvocationReport>,
    pub provider_retry_count: u32,
    pub provider_error_count: u32,
    pub token_usage: Option<TokenUsage>,
    pub taint: Option<AgentTaintRecord>,
}

pub(super) struct AgentOutcomeBuilderInput {
    pub(super) run_id: String,
    pub(super) started_at: String,
    pub(super) exit_reason: AgentExitReason,
    pub(super) final_output: String,
    pub(super) error: Option<String>,
    pub(super) messages: Vec<Message>,
    pub(super) tool_calls: Vec<ToolCall>,
    pub(super) tool_decisions: Vec<ToolDecisionRecord>,
    pub(super) final_prompt_size_chars: usize,
    pub(super) compaction_report: Option<CompactionReport>,
    pub(super) hook_invocations: Vec<HookInvocationReport>,
    pub(super) provider_retry_count: u32,
    pub(super) provider_error_count: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentTaintRecord {
    pub enabled: bool,
    pub mode: String,
    pub digest_bytes: usize,
    pub overall: String,
    #[serde(default)]
    pub spans_by_tool_call_id: std::collections::BTreeMap<String, Vec<TaintSpan>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolDecisionRecord {
    pub step: u32,
    pub tool_call_id: String,
    pub tool: String,
    pub decision: String,
    pub reason: Option<String>,
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taint_overall: Option<String>,
    #[serde(default)]
    pub taint_enforced: bool,
    #[serde(default)]
    pub escalated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escalation_reason: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpRuntimeTraceEntry {
    pub step: u32,
    pub lifecycle: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_ticks: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PolicyLoadedInfo {
    pub version: u32,
    pub rules_count: usize,
    pub includes_count: usize,
    pub includes_resolved: Vec<String>,
    pub mcp_allowlist: Option<McpAllowSummary>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum PlanToolEnforcementMode {
    Off,
    Soft,
    Hard,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum McpPinEnforcementMode {
    Off,
    Warn,
    Hard,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlanStepConstraint {
    pub step_id: String,
    pub intended_tools: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkerStepStatus {
    pub(crate) step_id: String,
    pub(crate) status: String,
    pub(crate) next_step_id: Option<String>,
    pub(crate) user_output: Option<String>,
}
