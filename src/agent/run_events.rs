use anyhow::Error;

use crate::agent_utils::add_opt_u32;
use crate::events::EventKind;
use crate::providers::http::{message_short, ProviderError};
use crate::providers::ModelProvider;
use crate::tools::tool_side_effects;
use crate::types::TokenUsage;
use crate::types::ToolCall;

use super::Agent;

pub(super) fn apply_usage_totals(
    usage: &TokenUsage,
    saw_token_usage: &mut bool,
    total_token_usage: &mut TokenUsage,
) {
    *saw_token_usage = true;
    total_token_usage.prompt_tokens =
        add_opt_u32(total_token_usage.prompt_tokens, usage.prompt_tokens);
    total_token_usage.completion_tokens =
        add_opt_u32(total_token_usage.completion_tokens, usage.completion_tokens);
    total_token_usage.total_tokens =
        add_opt_u32(total_token_usage.total_tokens, usage.total_tokens);
}

impl<P: ModelProvider> Agent<P> {
    pub(super) fn emit_tool_retry_event(
        &mut self,
        run_id: &str,
        step: u32,
        tc: &ToolCall,
        event: ToolRetryEvent<'_>,
    ) {
        self.emit_event(
            run_id,
            step,
            EventKind::ToolRetry,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "attempt": event.attempt,
                "max_retries": event.max_retries,
                "failure_class": event.failure_class,
                "action": event.action,
                "error_code": event.error_code
            }),
        );
    }

    pub(super) fn emit_schema_repair_exhausted_event(
        &mut self,
        run_id: &str,
        step: u32,
        tc: &ToolCall,
        attempt: u32,
    ) {
        self.emit_event(
            run_id,
            step,
            EventKind::Error,
            serde_json::json!({
                "error": "schema repair attempts exhausted",
                "source": "schema_repair",
                "code": "TOOL_SCHEMA_REPAIR_EXHAUSTED",
                "tool_call_id": tc.id,
                "name": tc.name,
                "attempt": attempt,
                "max_attempts": super::MAX_SCHEMA_REPAIR_ATTEMPTS
            }),
        );
    }

    pub(super) fn emit_tool_exec_start_events(&mut self, run_id: &str, step: u32, tc: &ToolCall) {
        self.emit_event(
            run_id,
            step,
            EventKind::ToolExecTarget,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "exec_target": if tc.name.starts_with("mcp.") { "host" } else {
                    match self.tool_rt.exec_target_kind {
                        crate::target::ExecTargetKind::Host => "host",
                        crate::target::ExecTargetKind::Docker => "docker",
                    }
                }
            }),
        );
        self.emit_event(
            run_id,
            step,
            EventKind::ToolExecStart,
            serde_json::json!({
                "tool_call_id": tc.id,
                "name": tc.name,
                "side_effects": tool_side_effects(&tc.name)
            }),
        );
    }

    pub(super) fn record_provider_error_events(
        &mut self,
        run_id: &str,
        step: u32,
        err: &Error,
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
    ) {
        if let Some(pe) = err.downcast_ref::<ProviderError>() {
            for r in &pe.retries {
                *provider_retry_count = provider_retry_count.saturating_add(1);
                self.emit_event(
                    run_id,
                    step,
                    EventKind::ProviderRetry,
                    serde_json::json!({
                        "attempt": r.attempt,
                        "max_attempts": r.max_attempts,
                        "kind": r.kind,
                        "status": r.status,
                        "backoff_ms": r.backoff_ms
                    }),
                );
            }
            *provider_error_count = provider_error_count.saturating_add(1);
            self.emit_event(
                run_id,
                step,
                EventKind::ProviderError,
                serde_json::json!({
                    "kind": pe.kind,
                    "status": pe.http_status,
                    "retryable": pe.retryable,
                    "attempt": pe.attempt,
                    "max_attempts": pe.max_attempts,
                    "message_short": message_short(&pe.message)
                }),
            );
        }
    }
}

pub(super) struct ToolRetryEvent<'a> {
    pub(super) attempt: u32,
    pub(super) max_retries: u32,
    pub(super) failure_class: &'a str,
    pub(super) action: &'a str,
    pub(super) error_code: Option<&'a str>,
}
