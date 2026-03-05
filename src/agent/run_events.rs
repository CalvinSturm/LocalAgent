use anyhow::Error;

use crate::agent_utils::add_opt_u32;
use crate::events::EventKind;
use crate::providers::http::{message_short, ProviderError};
use crate::providers::ModelProvider;
use crate::types::TokenUsage;

use super::Agent;

pub(super) fn apply_usage_totals(
    usage: &TokenUsage,
    saw_token_usage: &mut bool,
    total_token_usage: &mut TokenUsage,
) {
    *saw_token_usage = true;
    total_token_usage.prompt_tokens = add_opt_u32(total_token_usage.prompt_tokens, usage.prompt_tokens);
    total_token_usage.completion_tokens =
        add_opt_u32(total_token_usage.completion_tokens, usage.completion_tokens);
    total_token_usage.total_tokens = add_opt_u32(total_token_usage.total_tokens, usage.total_tokens);
}

impl<P: ModelProvider> Agent<P> {
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
