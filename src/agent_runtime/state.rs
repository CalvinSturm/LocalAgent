use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunPhase {
    Setup,
    Planning,
    Executing,
    WaitingForApproval,
    WaitingForOperatorInput,
    VerifyingChanges,
    Validating,
    CollectingFinalAnswer,
    Finalizing,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTier {
    NoSideEffects,
    ReadOnlyHost,
    ScopedHostWrite,
    ScopedHostShell,
    DockerIsolated,
    McpOnly,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetryState {
    #[serde(default)]
    pub required_validation_retry_count: u32,
    #[serde(default)]
    pub post_write_follow_on_turn_count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidationState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_command: Option<String>,
    #[serde(default)]
    pub satisfied: bool,
    #[serde(default)]
    pub repair_mode: bool,
    #[serde(default)]
    pub collecting_final_answer: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub awaiting_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PhaseSummaryEntryV1 {
    pub phase: RunPhase,
    pub entered_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exited_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InterruptKindV1 {
    ApprovalRequired,
    OperatorInterrupt,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterruptHistoryEntryV1 {
    pub kind: InterruptKindV1,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletionDecisionRecordV1 {
    pub kind: String,
    pub allowed: bool,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_phase: Option<RunPhase>,
    pub reason: String,
    #[serde(default)]
    pub unmet_requirements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunCheckpointV1 {
    pub schema_version: String,
    pub phase: RunPhase,
    pub step_index: u32,
    pub execution_tier: ExecutionTier,
    pub terminal_boundary: bool,
    pub retry_state: RetryState,
    pub validation_state: ValidationState,
    pub approval_state: ApprovalState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_plan_step_id: Option<String>,
    #[serde(default)]
    pub last_tool_fact_envelopes: Vec<crate::agent::ToolFactEnvelopeV1>,
}
