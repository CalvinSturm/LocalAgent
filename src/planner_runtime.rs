use crate::events::{EventKind, EventSink};
use crate::planner;
use crate::providers;
use crate::providers::ModelProvider;
use crate::runtime_events;
use crate::types::{GenerateRequest, Message, Role};

#[derive(Debug, Clone)]
pub(crate) struct PlannerPhaseOutput {
    pub(crate) plan_json: serde_json::Value,
    pub(crate) plan_hash_hex: String,
    pub(crate) raw_output: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) ok: bool,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_planner_phase<P: ModelProvider>(
    provider: &P,
    run_id: &str,
    planner_model: &str,
    prompt: &str,
    planner_max_steps: u32,
    planner_output: planner::PlannerOutput,
    planner_strict: bool,
    sink: &mut Option<Box<dyn EventSink>>,
) -> anyhow::Result<PlannerPhaseOutput> {
    let mut messages = vec![
        Message {
            role: Role::System,
            content: Some(
                "You are the planner. Do not call tools. Produce only JSON matching openagent.plan.v1 with fields: schema_version, goal, assumptions[], steps[] where each step includes summary, intended_tools[], done_criteria[], verifier_checks[], plus risks[] and success_criteria[]."
                    .to_string(),
            ),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        },
        Message {
            role: Role::User,
            content: Some(prompt.to_string()),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        },
    ];

    let max_steps = planner_max_steps.max(1);
    let mut last_output = String::new();
    for step in 0..max_steps {
        runtime_events::emit_event(
            sink,
            run_id,
            step,
            EventKind::ModelRequestStart,
            serde_json::json!({
                "model": planner_model,
                "tool_count": 0,
                "stream": false,
                "phase": "planner"
            }),
        );
        let req = GenerateRequest {
            model: planner_model.to_string(),
            messages: messages.clone(),
            tools: None,
        };
        let resp = match provider.generate(req).await {
            Ok(resp) => resp,
            Err(e) => {
                if let Some(pe) = e.downcast_ref::<providers::http::ProviderError>() {
                    for r in &pe.retries {
                        runtime_events::emit_event(
                            sink,
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
                    runtime_events::emit_event(
                        sink,
                        run_id,
                        step,
                        EventKind::ProviderError,
                        serde_json::json!({
                            "kind": pe.kind,
                            "status": pe.http_status,
                            "retryable": pe.retryable,
                            "attempt": pe.attempt,
                            "max_attempts": pe.max_attempts,
                            "message_short": providers::http::message_short(&pe.message)
                        }),
                    );
                }
                return Err(e);
            }
        };

        let output = resp.assistant.content.clone().unwrap_or_default();
        runtime_events::emit_event(
            sink,
            run_id,
            step,
            EventKind::ModelResponseEnd,
            serde_json::json!({
                "content": output,
                "tool_calls": resp.tool_calls.len(),
                "phase": "planner"
            }),
        );
        if !resp.tool_calls.is_empty() {
            let wrapped =
                planner::normalize_planner_output(&output, prompt, planner_output, false)?;
            return Ok(PlannerPhaseOutput {
                plan_json: wrapped.plan_json,
                plan_hash_hex: wrapped.plan_hash_hex,
                raw_output: wrapped.raw_output,
                error: Some(format!(
                    "planner emitted tool calls while tools are disabled (count={})",
                    resp.tool_calls.len()
                )),
                ok: false,
            });
        }
        messages.push(resp.assistant);
        last_output = output;
        if !last_output.trim().is_empty() {
            break;
        }
    }

    match planner::normalize_planner_output(&last_output, prompt, planner_output, planner_strict) {
        Ok(normalized) => Ok(PlannerPhaseOutput {
            plan_json: normalized.plan_json,
            plan_hash_hex: normalized.plan_hash_hex,
            raw_output: normalized.raw_output,
            error: normalized.error,
            ok: !normalized.used_wrapper,
        }),
        Err(e) => {
            let wrapped = planner::wrap_text_plan(prompt, &last_output);
            let hash = planner::hash_canonical_json(&wrapped)?;
            Ok(PlannerPhaseOutput {
                plan_json: wrapped,
                plan_hash_hex: hash,
                raw_output: Some(last_output),
                error: Some(e.to_string()),
                ok: false,
            })
        }
    }
}
