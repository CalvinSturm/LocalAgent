use crate::compaction::{maybe_compact, CompactionOutcome};
use crate::events::EventKind;
use crate::providers::http::{message_short, ProviderError};
use crate::providers::ModelProvider;
use crate::types::Message;

use super::Agent;

impl<P: ModelProvider> Agent<P> {
    pub(super) fn check_wall_time_budget_exceeded(
        &mut self,
        run_id: &str,
        step: u32,
        run_started: &std::time::Instant,
    ) -> Option<String> {
        if self.tool_call_budget.max_wall_time_ms == 0 {
            return None;
        }
        let elapsed_ms = run_started.elapsed().as_millis() as u64;
        if elapsed_ms <= self.tool_call_budget.max_wall_time_ms {
            return None;
        }
        let reason = format!(
            "runtime budget exceeded: wall time {}ms > limit {}ms",
            elapsed_ms, self.tool_call_budget.max_wall_time_ms
        );
        self.emit_event(
            run_id,
            step,
            EventKind::Error,
            serde_json::json!({
                "error": reason,
                "source": "runtime_budget",
                "elapsed_ms": elapsed_ms,
                "max_wall_time_ms": self.tool_call_budget.max_wall_time_ms
            }),
        );
        self.emit_event(
            run_id,
            step,
            EventKind::RunEnd,
            serde_json::json!({"exit_reason":"budget_exceeded"}),
        );
        Some(reason)
    }

    pub(super) fn compact_messages_for_step(
        &mut self,
        run_id: &str,
        step: u32,
        messages: &[Message],
        provider_retry_count: &mut u32,
        provider_error_count: &mut u32,
    ) -> Result<CompactionOutcome, String> {
        match maybe_compact(messages, &self.compaction_settings) {
            Ok(c) => Ok(c),
            Err(e) => {
                if let Some(pe) = e.downcast_ref::<ProviderError>() {
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
                let err_text = format!("compaction failed: {e}");
                self.emit_event(
                    run_id,
                    step,
                    EventKind::Error,
                    serde_json::json!({"error": err_text}),
                );
                self.emit_event(
                    run_id,
                    step,
                    EventKind::RunEnd,
                    serde_json::json!({"exit_reason":"provider_error"}),
                );
                Err(err_text)
            }
        }
    }
}
