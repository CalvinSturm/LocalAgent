use anyhow::{anyhow, Context};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

use crate::gate::ProviderKind;
use crate::providers::common::{
    build_http_client, build_tool_envelopes, format_http_error_body, map_token_usage_triplet,
    provider_payload_too_large_error, provider_stream_incomplete_error,
    provider_stream_payload_too_large_error, record_retry_and_sleep, truncate_error_display,
    truncate_for_error, ProviderRetryStepInput, ToolEnvelope as SharedToolEnvelope,
};
use crate::providers::http::{
    classify_reqwest_error, classify_status, HttpConfig, ProviderError, ProviderErrorKind,
    RetryRecord,
};
use crate::providers::{ModelProvider, StreamDelta, ToolCallFragment};
use crate::types::{GenerateRequest, GenerateResponse, Message, Role, ToolCall};

#[derive(Debug, Clone)]
pub struct OpenAiCompatProvider {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    http: HttpConfig,
    compatibility: OpenAiCompatMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenAiCompatMode {
    Standard,
    Lmstudio,
}

#[derive(Debug, Serialize)]
struct OpenAiCompatTrace {
    schema_version: String,
    provider: String,
    base_url: String,
    streaming: bool,
    request: Value,
    events: Vec<OpenAiCompatTraceEvent>,
    result: OpenAiCompatTraceResult,
}

#[derive(Debug, Serialize)]
struct OpenAiCompatTraceEvent {
    phase: String,
    detail: Value,
}

#[derive(Debug, Serialize)]
struct OpenAiCompatTraceResult {
    outcome: String,
    saw_done: bool,
    emitted_any: bool,
    content_bytes: usize,
    partial_tool_calls: usize,
    trailing_buffer_bytes: usize,
    message: Option<String>,
}

struct OpenAiCompatTraceRecorder {
    path: Option<PathBuf>,
    trace: OpenAiCompatTrace,
}

impl OpenAiCompatTraceRecorder {
    fn new(
        compatibility: OpenAiCompatMode,
        base_url: &str,
        streaming: bool,
        request: &OpenAiRequest,
    ) -> Self {
        let request_json = serde_json::to_value(request).unwrap_or(Value::Null);
        Self {
            path: resolve_openai_trace_path(),
            trace: OpenAiCompatTrace {
                schema_version: "openagent.openai_compat_trace.v1".to_string(),
                provider: match compatibility {
                    OpenAiCompatMode::Standard => "openai_compat".to_string(),
                    OpenAiCompatMode::Lmstudio => "lmstudio".to_string(),
                },
                base_url: base_url.to_string(),
                streaming,
                request: request_json,
                events: Vec::new(),
                result: OpenAiCompatTraceResult {
                    outcome: "in_progress".to_string(),
                    saw_done: false,
                    emitted_any: false,
                    content_bytes: 0,
                    partial_tool_calls: 0,
                    trailing_buffer_bytes: 0,
                    message: None,
                },
            },
        }
    }

    fn push_event(&mut self, phase: &str, detail: Value) {
        if self.path.is_none() {
            return;
        }
        self.trace.events.push(OpenAiCompatTraceEvent {
            phase: phase.to_string(),
            detail,
        });
    }

    fn finish(&mut self, result: OpenAiCompatTraceResult) {
        self.trace.result = result;
    }

    fn write(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(rendered) = serde_json::to_string_pretty(&self.trace) {
            let _ = std::fs::write(path, rendered);
        }
    }
}

impl Drop for OpenAiCompatTraceRecorder {
    fn drop(&mut self) {
        self.write();
    }
}

impl OpenAiCompatProvider {
    pub fn new(
        provider_kind: ProviderKind,
        base_url: String,
        api_key: Option<String>,
        http: HttpConfig,
    ) -> anyhow::Result<Self> {
        let client = build_http_client(http, "failed to build OpenAI-compatible HTTP client")?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            http,
            compatibility: match provider_kind {
                ProviderKind::Lmstudio => OpenAiCompatMode::Lmstudio,
                ProviderKind::Llamacpp | ProviderKind::Ollama | ProviderKind::Mock => {
                    OpenAiCompatMode::Standard
                }
            },
        })
    }
}

type OpenAiToolEnvelope = SharedToolEnvelope;

#[derive(Debug, Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiToolEnvelope>>,
    tool_choice: String,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<u64>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    #[serde(default)]
    message: OpenAiMessage,
    #[serde(default)]
    delta: OpenAiMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    total_tokens: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenAiMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCall {
    #[serde(default)]
    index: Option<usize>,
    #[serde(default)]
    id: String,
    #[serde(default)]
    function: OpenAiFunctionCall,
}

#[derive(Debug, Default, Deserialize)]
struct OpenAiFunctionCall {
    #[serde(default)]
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[async_trait]
impl ModelProvider for OpenAiCompatProvider {
    async fn generate(&self, req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        let payload = to_request(req, false, self.compatibility);
        let mut trace =
            OpenAiCompatTraceRecorder::new(self.compatibility, &self.base_url, false, &payload);
        let max_attempts = self.http.http_max_retries + 1;
        let mut retries = Vec::<RetryRecord>::new();
        for attempt in 1..=max_attempts {
            trace.push_event(
                "request_attempt",
                serde_json::json!({
                    "attempt": attempt,
                    "max_attempts": max_attempts,
                }),
            );
            let mut request = self.client.post(&url).json(&payload);
            if let Some(key) = &self.api_key {
                request = request.bearer_auth(key);
            }
            let sent = request.send().await;
            let response = match sent {
                Ok(r) => r,
                Err(e) => {
                    let cls = classify_reqwest_error(&e);
                    if cls.retryable && attempt < max_attempts {
                        record_retry_and_sleep(ProviderRetryStepInput {
                            http: self.http,
                            retry_index: attempt - 1,
                            attempt,
                            max_attempts,
                            kind: cls.kind,
                            status: cls.status,
                            retries: &mut retries,
                        })
                        .await;
                        continue;
                    }
                    trace.finish(OpenAiCompatTraceResult {
                        outcome: "request_error".to_string(),
                        saw_done: false,
                        emitted_any: false,
                        content_bytes: 0,
                        partial_tool_calls: 0,
                        trailing_buffer_bytes: 0,
                        message: Some(format!("failed to call OpenAI-compatible endpoint: {e}")),
                    });
                    return Err(anyhow!(ProviderError {
                        kind: cls.kind,
                        http_status: cls.status,
                        retryable: cls.retryable,
                        attempt,
                        max_attempts,
                        message: format!("failed to call OpenAI-compatible endpoint: {e}"),
                        retries,
                    }));
                }
            };
            let status = response.status();
            if !status.is_success() {
                let cls = classify_status(status.as_u16());
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<body unavailable>".to_string());
                if cls.retryable && attempt < max_attempts {
                    record_retry_and_sleep(ProviderRetryStepInput {
                        http: self.http,
                        retry_index: attempt - 1,
                        attempt,
                        max_attempts,
                        kind: cls.kind,
                        status: Some(status.as_u16()),
                        retries: &mut retries,
                    })
                    .await;
                    continue;
                }
                trace.finish(OpenAiCompatTraceResult {
                    outcome: "http_error".to_string(),
                    saw_done: false,
                    emitted_any: false,
                    content_bytes: 0,
                    partial_tool_calls: 0,
                    trailing_buffer_bytes: 0,
                    message: Some(format!(
                        "OpenAI-compatible endpoint returned HTTP {}: {}",
                        status.as_u16(),
                        format_http_error_body(&body)
                    )),
                });
                return Err(anyhow!(ProviderError {
                    kind: cls.kind,
                    http_status: Some(status.as_u16()),
                    retryable: cls.retryable,
                    attempt,
                    max_attempts,
                    message: format!(
                        "OpenAI-compatible endpoint returned HTTP {}: {}",
                        status.as_u16(),
                        format_http_error_body(&body)
                    ),
                    retries,
                }));
            }
            let bytes = response
                .bytes()
                .await
                .context("failed to read OpenAI-compatible response body")?;
            trace.push_event(
                "response_body",
                serde_json::json!({
                    "status": status.as_u16(),
                    "bytes": bytes.len(),
                    "body_preview": truncate_for_error(&String::from_utf8_lossy(&bytes), 4000),
                }),
            );
            if bytes.len() > self.http.max_response_bytes {
                trace.finish(OpenAiCompatTraceResult {
                    outcome: "payload_too_large".to_string(),
                    saw_done: false,
                    emitted_any: false,
                    content_bytes: 0,
                    partial_tool_calls: 0,
                    trailing_buffer_bytes: 0,
                    message: Some(format!(
                        "response payload exceeded max bytes: {} > {}",
                        bytes.len(),
                        self.http.max_response_bytes
                    )),
                });
                return Err(anyhow!(provider_payload_too_large_error(
                    status.as_u16(),
                    attempt,
                    max_attempts,
                    bytes.len(),
                    self.http.max_response_bytes,
                    retries,
                )));
            }
            let resp: OpenAiResponse = serde_json::from_slice(&bytes)
                .context("failed to parse OpenAI-compatible JSON response")?;
            trace.push_event("response_parsed", summarize_openai_response(&resp));
            return map_openai_response(resp)
                .inspect(|mapped| {
                    trace.push_event("response_mapped", summarize_generate_response(mapped));
                    trace.finish(OpenAiCompatTraceResult {
                        outcome: "success".to_string(),
                        saw_done: true,
                        emitted_any: mapped
                            .assistant
                            .content
                            .as_deref()
                            .is_some_and(|s| !s.trim().is_empty())
                            || !mapped.tool_calls.is_empty(),
                        content_bytes: mapped
                            .assistant
                            .content
                            .as_deref()
                            .map(str::len)
                            .unwrap_or(0),
                        partial_tool_calls: mapped.tool_calls.len(),
                        trailing_buffer_bytes: 0,
                        message: None,
                    });
                })
                .map_err(|e| {
                    trace.finish(OpenAiCompatTraceResult {
                        outcome: "response_map_error".to_string(),
                        saw_done: true,
                        emitted_any: false,
                        content_bytes: 0,
                        partial_tool_calls: 0,
                        trailing_buffer_bytes: 0,
                        message: Some(e.to_string()),
                    });
                    anyhow!(ProviderError {
                        kind: ProviderErrorKind::Parse,
                        http_status: Some(status.as_u16()),
                        retryable: false,
                        attempt,
                        max_attempts,
                        message: e.to_string(),
                        retries,
                    })
                });
        }
        trace.finish(OpenAiCompatTraceResult {
            outcome: "retry_loop_terminated".to_string(),
            saw_done: false,
            emitted_any: false,
            content_bytes: 0,
            partial_tool_calls: 0,
            trailing_buffer_bytes: 0,
            message: Some("unexpected retry loop termination".to_string()),
        });
        Err(anyhow!("unexpected retry loop termination"))
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn generate_streaming(
        &self,
        req: GenerateRequest,
        on_delta: &mut (dyn FnMut(StreamDelta) + Send),
    ) -> anyhow::Result<GenerateResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        let payload = to_request(req, true, self.compatibility);
        let mut trace =
            OpenAiCompatTraceRecorder::new(self.compatibility, &self.base_url, true, &payload);
        let max_attempts = self.http.http_max_retries + 1;
        let mut retries = Vec::<RetryRecord>::new();

        for attempt in 1..=max_attempts {
            trace.push_event(
                "request_attempt",
                serde_json::json!({
                    "attempt": attempt,
                    "max_attempts": max_attempts,
                }),
            );
            let mut request = self.client.post(&url).json(&payload);
            if let Some(key) = &self.api_key {
                request = request.bearer_auth(key);
            }
            let sent = request.send().await;
            let response = match sent {
                Ok(r) => r,
                Err(e) => {
                    let cls = classify_reqwest_error(&e);
                    if cls.retryable && attempt < max_attempts {
                        record_retry_and_sleep(ProviderRetryStepInput {
                            http: self.http,
                            retry_index: attempt - 1,
                            attempt,
                            max_attempts,
                            kind: cls.kind,
                            status: cls.status,
                            retries: &mut retries,
                        })
                        .await;
                        continue;
                    }
                    trace.finish(OpenAiCompatTraceResult {
                        outcome: "request_error".to_string(),
                        saw_done: false,
                        emitted_any: false,
                        content_bytes: 0,
                        partial_tool_calls: 0,
                        trailing_buffer_bytes: 0,
                        message: Some(format!("failed to call OpenAI-compatible endpoint: {e}")),
                    });
                    return Err(anyhow!(ProviderError {
                        kind: cls.kind,
                        http_status: cls.status,
                        retryable: cls.retryable,
                        attempt,
                        max_attempts,
                        message: format!("failed to call OpenAI-compatible endpoint: {e}"),
                        retries,
                    }));
                }
            };

            let status = response.status();
            if !status.is_success() {
                let cls = classify_status(status.as_u16());
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<body unavailable>".to_string());
                if cls.retryable && attempt < max_attempts {
                    record_retry_and_sleep(ProviderRetryStepInput {
                        http: self.http,
                        retry_index: attempt - 1,
                        attempt,
                        max_attempts,
                        kind: cls.kind,
                        status: Some(status.as_u16()),
                        retries: &mut retries,
                    })
                    .await;
                    continue;
                }
                trace.finish(OpenAiCompatTraceResult {
                    outcome: "http_error".to_string(),
                    saw_done: false,
                    emitted_any: false,
                    content_bytes: 0,
                    partial_tool_calls: 0,
                    trailing_buffer_bytes: 0,
                    message: Some(format!(
                        "OpenAI-compatible endpoint returned HTTP {}: {}",
                        status.as_u16(),
                        format_http_error_body(&body)
                    )),
                });
                return Err(anyhow!(ProviderError {
                    kind: cls.kind,
                    http_status: Some(status.as_u16()),
                    retryable: cls.retryable,
                    attempt,
                    max_attempts,
                    message: format!(
                        "OpenAI-compatible endpoint returned HTTP {}: {}",
                        status.as_u16(),
                        format_http_error_body(&body)
                    ),
                    retries,
                }));
            }

            let mut stream = response.bytes_stream();
            let mut text_buf = String::new();
            let mut content_accum = String::new();
            let mut partials: Vec<PartialToolCall> = Vec::new();
            let mut total_bytes: usize = 0;
            let mut emitted_any = false;
            let mut saw_done = false;

            loop {
                let maybe_chunk = if let Some(idle) = self.http.idle_timeout_opt() {
                    let next = tokio::time::timeout(idle, stream.next()).await;
                    match next {
                        Ok(v) => v,
                        Err(_) => {
                            if !emitted_any && attempt < max_attempts {
                                record_retry_and_sleep(ProviderRetryStepInput {
                                    http: self.http,
                                    retry_index: attempt - 1,
                                    attempt,
                                    max_attempts,
                                    kind: ProviderErrorKind::Timeout,
                                    status: Some(status.as_u16()),
                                    retries: &mut retries,
                                })
                                .await;
                                break;
                            }
                            trace.finish(OpenAiCompatTraceResult {
                                outcome: "stream_idle_timeout".to_string(),
                                saw_done,
                                emitted_any,
                                content_bytes: content_accum.len(),
                                partial_tool_calls: partials.len(),
                                trailing_buffer_bytes: text_buf.len(),
                                message: Some("stream idle timeout exceeded".to_string()),
                            });
                            return Err(anyhow!(ProviderError {
                                kind: ProviderErrorKind::Timeout,
                                http_status: Some(status.as_u16()),
                                retryable: !emitted_any,
                                attempt,
                                max_attempts,
                                message: "stream idle timeout exceeded".to_string(),
                                retries,
                            }));
                        }
                    }
                } else {
                    stream.next().await
                };

                let Some(chunk_res) = maybe_chunk else {
                    break;
                };

                let chunk = match chunk_res {
                    Ok(c) => c,
                    Err(e) => {
                        let cls = classify_reqwest_error(&e);
                        if cls.retryable && !emitted_any && attempt < max_attempts {
                            record_retry_and_sleep(ProviderRetryStepInput {
                                http: self.http,
                                retry_index: attempt - 1,
                                attempt,
                                max_attempts,
                                kind: cls.kind,
                                status: cls.status.or(Some(status.as_u16())),
                                retries: &mut retries,
                            })
                            .await;
                            break;
                        }
                        trace.finish(OpenAiCompatTraceResult {
                            outcome: "stream_read_error".to_string(),
                            saw_done,
                            emitted_any,
                            content_bytes: content_accum.len(),
                            partial_tool_calls: partials.len(),
                            trailing_buffer_bytes: text_buf.len(),
                            message: Some(format!("failed reading stream chunk: {e}")),
                        });
                        return Err(anyhow!(ProviderError {
                            kind: cls.kind,
                            http_status: cls.status.or(Some(status.as_u16())),
                            retryable: cls.retryable && !emitted_any,
                            attempt,
                            max_attempts,
                            message: format!("failed reading stream chunk: {e}"),
                            retries,
                        }));
                    }
                };

                total_bytes = total_bytes.saturating_add(chunk.len());
                if total_bytes > self.http.max_response_bytes {
                    return Err(anyhow!(provider_stream_payload_too_large_error(
                        status.as_u16(),
                        attempt,
                        max_attempts,
                        total_bytes,
                        self.http.max_response_bytes,
                        retries,
                    )));
                }

                let mut chunk_text = String::from_utf8_lossy(&chunk).to_string();
                chunk_text = chunk_text.replace("\r\n", "\n").replace('\r', "\n");
                trace.push_event(
                    "stream_chunk",
                    serde_json::json!({
                        "bytes": chunk.len(),
                        "chunk_preview": truncate_for_error(&chunk_text, 2000),
                    }),
                );
                text_buf.push_str(&chunk_text);

                for raw_event in drain_sse_events(&mut text_buf) {
                    if raw_event.len() > self.http.max_line_bytes {
                        return Err(anyhow!(ProviderError {
                            kind: ProviderErrorKind::PayloadTooLarge,
                            http_status: Some(status.as_u16()),
                            retryable: false,
                            attempt,
                            max_attempts,
                            message: format!(
                                "sse event exceeded max bytes: {} > {}",
                                raw_event.len(),
                                self.http.max_line_bytes
                            ),
                            retries,
                        }));
                    }
                    match parse_sse_event_payload(&raw_event) {
                        Ok(Some(payload_text)) => {
                            if payload_text == "[DONE]" {
                                saw_done = true;
                                trace.push_event("stream_done", serde_json::json!({}));
                                continue;
                            }
                            match handle_openai_stream_json(
                                &payload_text,
                                on_delta,
                                &mut content_accum,
                                &mut partials,
                            ) {
                                Ok(summary) => {
                                    trace.push_event(
                                        "stream_event",
                                        serde_json::json!({
                                            "payload_preview": truncate_for_error(&payload_text, 2000),
                                            "content_delta_preview": summary.content_delta_preview,
                                            "tool_fragments": summary.tool_fragments,
                                            "finish_reason": summary.finish_reason,
                                            "content_bytes_total": content_accum.len(),
                                            "partial_tool_calls": partials.len(),
                                        }),
                                    );
                                }
                                Err(e) => {
                                    trace.finish(OpenAiCompatTraceResult {
                                        outcome: "stream_parse_error".to_string(),
                                        saw_done,
                                        emitted_any,
                                        content_bytes: content_accum.len(),
                                        partial_tool_calls: partials.len(),
                                        trailing_buffer_bytes: text_buf.len(),
                                        message: Some(format!(
                                            "malformed OpenAI-compatible stream event: {}",
                                            truncate_error_display(&e, 200)
                                        )),
                                    });
                                    return Err(anyhow!(ProviderError {
                                        kind: ProviderErrorKind::Parse,
                                        http_status: Some(status.as_u16()),
                                        retryable: false,
                                        attempt,
                                        max_attempts,
                                        message: format!(
                                            "malformed OpenAI-compatible stream event: {}",
                                            truncate_error_display(&e, 200)
                                        ),
                                        retries,
                                    }));
                                }
                            }
                            emitted_any = true;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            trace.finish(OpenAiCompatTraceResult {
                                outcome: "invalid_sse_event".to_string(),
                                saw_done,
                                emitted_any,
                                content_bytes: content_accum.len(),
                                partial_tool_calls: partials.len(),
                                trailing_buffer_bytes: text_buf.len(),
                                message: Some(format!(
                                    "invalid SSE event: {}",
                                    truncate_error_display(&e, 200)
                                )),
                            });
                            return Err(anyhow!(ProviderError {
                                kind: ProviderErrorKind::Parse,
                                http_status: Some(status.as_u16()),
                                retryable: false,
                                attempt,
                                max_attempts,
                                message: format!(
                                    "invalid SSE event: {}",
                                    truncate_error_display(&e, 200)
                                ),
                                retries,
                            }));
                        }
                    }
                }
            }

            if attempt < max_attempts
                && !saw_done
                && !content_accum.is_empty()
                && partials.is_empty()
            {
                // stream ended unexpectedly after partial content; do not retry
            }

            if attempt < max_attempts && !saw_done && !emitted_any {
                continue;
            }

            let tool_calls = finalize_tool_calls(partials);
            let content = if content_accum.is_empty() {
                None
            } else {
                Some(content_accum)
            };
            trace.finish(OpenAiCompatTraceResult {
                outcome: "success".to_string(),
                saw_done,
                emitted_any,
                content_bytes: content.as_deref().map(str::len).unwrap_or(0),
                partial_tool_calls: tool_calls.len(),
                trailing_buffer_bytes: text_buf.len(),
                message: None,
            });
            return Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content,
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls,
                usage: None,
            });
        }

        Err(anyhow!(provider_stream_incomplete_error(self.http)))
    }
}

fn drain_sse_events(buf: &mut String) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(pos) = buf.find("\n\n") {
        out.push(buf[..pos].to_string());
        *buf = buf[pos + 2..].to_string();
    }
    out
}

fn to_request(
    req: GenerateRequest,
    stream: bool,
    compatibility: OpenAiCompatMode,
) -> OpenAiRequest {
    let tools = build_tool_envelopes(req.tools);
    let messages = normalize_messages(req.messages, tools.is_some(), compatibility);
    OpenAiRequest {
        model: req.model,
        messages,
        tools,
        tool_choice: "auto".to_string(),
        temperature: req.temperature.unwrap_or(0.2),
        top_p: req.top_p,
        max_tokens: req.max_tokens,
        seed: req.seed,
        stream,
    }
}

fn normalize_messages(
    messages: Vec<Message>,
    has_tools: bool,
    compatibility: OpenAiCompatMode,
) -> Vec<Message> {
    if compatibility == OpenAiCompatMode::Standard {
        return messages
            .into_iter()
            .filter(|message| !is_semantically_empty_assistant_message(message))
            .collect();
    }

    let mapped = messages
        .into_iter()
        .map(|mut message| {
            if matches!(message.role, Role::Developer) {
                message.role = Role::System;
            }
            message
        })
        .collect::<Vec<_>>();

    if !has_tools {
        return mapped;
    }

    collapse_pre_user_instruction_messages(mapped)
}

fn is_semantically_empty_assistant_message(message: &Message) -> bool {
    if !matches!(message.role, Role::Assistant) {
        return false;
    }

    let has_content = message
        .content
        .as_deref()
        .map(|content| !content.trim().is_empty())
        .unwrap_or(false);
    let has_tool_calls = message
        .tool_calls
        .as_ref()
        .map(|calls| !calls.is_empty())
        .unwrap_or(false);

    !has_content && !has_tool_calls && message.tool_call_id.is_none() && message.tool_name.is_none()
}

fn collapse_pre_user_instruction_messages(messages: Vec<Message>) -> Vec<Message> {
    let first_user_index = messages
        .iter()
        .position(|message| matches!(message.role, Role::User));
    let Some(first_user_index) = first_user_index else {
        return messages;
    };

    let mut pre_user_chunks = Vec::new();
    let mut normalized = Vec::with_capacity(messages.len());

    for (index, message) in messages.into_iter().enumerate() {
        if index < first_user_index && matches!(message.role, Role::System) {
            if let Some(content) = message.content {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    pre_user_chunks.push(trimmed.to_string());
                }
            }
            continue;
        }

        normalized.push(message);
    }

    if !pre_user_chunks.is_empty() {
        normalized.insert(
            0,
            Message {
                role: Role::System,
                content: Some(pre_user_chunks.join("\n\n")),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
        );
    }

    normalized
}

fn map_openai_response(resp: OpenAiResponse) -> anyhow::Result<GenerateResponse> {
    let usage = resp
        .usage
        .as_ref()
        .map(|u| map_token_usage_triplet(u.prompt_tokens, u.completion_tokens, u.total_tokens));
    let first = resp
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("missing choices[0] in OpenAI-compatible response"))?;
    let mut tool_calls = Vec::new();
    if let Some(tcalls) = first.message.tool_calls {
        for tc in tcalls {
            let arguments = match tc.function.arguments {
                Value::String(s) => match serde_json::from_str::<Value>(&s) {
                    Ok(v) => v,
                    Err(_) => Value::String(s),
                },
                other => other,
            };
            tool_calls.push(ToolCall {
                id: tc.id,
                name: tc.function.name,
                arguments,
            });
        }
    }
    Ok(GenerateResponse {
        assistant: Message {
            role: Role::Assistant,
            content: first.message.content,
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        },
        tool_calls,
        usage,
    })
}

#[derive(Debug, Default, Clone)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

fn parse_sse_event_payload(raw_event: &str) -> anyhow::Result<Option<String>> {
    let mut data_lines = Vec::new();
    for line in raw_event.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(':') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }
    if data_lines.is_empty() {
        return Ok(None);
    }
    Ok(Some(data_lines.join("\n")))
}

#[derive(Debug, Default)]
struct OpenAiStreamEventSummary {
    content_delta_preview: Option<String>,
    tool_fragments: Vec<Value>,
    finish_reason: Option<String>,
}

fn handle_openai_stream_json(
    payload: &str,
    on_delta: &mut (dyn FnMut(StreamDelta) + Send),
    content_accum: &mut String,
    partials: &mut Vec<PartialToolCall>,
) -> anyhow::Result<OpenAiStreamEventSummary> {
    let item: OpenAiResponse =
        serde_json::from_str(payload).context("failed parsing OpenAI-compatible stream event")?;
    let mut summary = OpenAiStreamEventSummary::default();
    if let Some(choice) = item.choices.into_iter().next() {
        summary.finish_reason = choice.finish_reason.clone();
        if let Some(content) = choice.delta.content {
            if !content.is_empty() {
                content_accum.push_str(&content);
                summary.content_delta_preview = Some(truncate_for_error(&content, 256));
                on_delta(StreamDelta::Content(content));
            }
        }
        if let Some(tool_calls) = choice.delta.tool_calls {
            for tc in tool_calls {
                let idx = tc.index.unwrap_or(partials.len());
                ensure_partial_len(partials, idx + 1);
                let p = &mut partials[idx];
                if !tc.id.is_empty() {
                    p.id = tc.id.clone();
                }
                if !tc.function.name.is_empty() {
                    p.name = tc.function.name.clone();
                }
                if let Some(fragment) = value_to_string_fragment(&tc.function.arguments) {
                    p.arguments.push_str(&fragment);
                    summary.tool_fragments.push(serde_json::json!({
                        "index": idx,
                        "id": if p.id.is_empty() { Value::Null } else { Value::String(p.id.clone()) },
                        "name": if p.name.is_empty() { Value::Null } else { Value::String(p.name.clone()) },
                        "arguments_fragment_preview": truncate_for_error(&fragment, 256),
                        "complete": choice.finish_reason.as_deref() == Some("tool_calls"),
                    }));
                    on_delta(StreamDelta::ToolCallFragment(ToolCallFragment {
                        index: idx,
                        id: if p.id.is_empty() {
                            None
                        } else {
                            Some(p.id.clone())
                        },
                        name: if p.name.is_empty() {
                            None
                        } else {
                            Some(p.name.clone())
                        },
                        arguments_fragment: Some(fragment),
                        complete: choice.finish_reason.as_deref() == Some("tool_calls"),
                    }));
                }
            }
        }
    }
    Ok(summary)
}

fn ensure_partial_len(partials: &mut Vec<PartialToolCall>, len: usize) {
    while partials.len() < len {
        partials.push(PartialToolCall::default());
    }
}

fn value_to_string_fragment(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

fn finalize_tool_calls(partials: Vec<PartialToolCall>) -> Vec<ToolCall> {
    partials
        .into_iter()
        .enumerate()
        .filter(|(_, p)| !p.name.is_empty())
        .map(|(i, p)| ToolCall {
            id: if p.id.is_empty() {
                format!("openai_tc_{i}")
            } else {
                p.id
            },
            name: p.name,
            arguments: match serde_json::from_str::<Value>(&p.arguments) {
                Ok(v) => v,
                Err(_) => Value::String(p.arguments),
            },
        })
        .collect()
}

fn resolve_openai_trace_path() -> Option<PathBuf> {
    let dir = std::env::var_os("LOCALAGENT_OPENAI_TRACE_DIR")?;
    let base = Path::new(&dir);
    let stamp = crate::trust::now_rfc3339()
        .replace(':', "-")
        .replace('.', "_");
    Some(base.join(format!("openai-compat-trace-{stamp}.json")))
}

fn summarize_openai_response(resp: &OpenAiResponse) -> Value {
    let choices = resp
        .choices
        .iter()
        .map(|choice| {
            serde_json::json!({
                "finish_reason": choice.finish_reason,
                "content_preview": choice
                    .message
                    .content
                    .as_deref()
                    .map(|s| truncate_for_error(s, 500)),
                "tool_calls": choice
                    .message
                    .tool_calls
                    .as_ref()
                    .map(|calls| {
                        calls.iter()
                            .map(|call| {
                                serde_json::json!({
                                    "id": call.id,
                                    "index": call.index,
                                    "name": call.function.name,
                                    "arguments_preview": truncate_for_error(
                                        &call.function.arguments.to_string(),
                                        500
                                    ),
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "choice_count": resp.choices.len(),
        "choices": choices,
        "usage": resp.usage.as_ref().map(summarize_openai_usage),
    })
}

fn summarize_generate_response(resp: &GenerateResponse) -> Value {
    serde_json::json!({
        "assistant_content_preview": resp
            .assistant
            .content
            .as_deref()
            .map(|s| truncate_for_error(s, 500)),
        "tool_calls": resp
            .tool_calls
            .iter()
            .map(|call| {
                serde_json::json!({
                    "id": call.id,
                    "name": call.name,
                    "arguments_preview": truncate_for_error(&call.arguments.to_string(), 500),
                })
            })
            .collect::<Vec<_>>(),
        "usage": resp.usage,
    })
}

fn summarize_openai_usage(usage: &OpenAiUsage) -> Value {
    serde_json::json!({
        "prompt_tokens": usage.prompt_tokens,
        "completion_tokens": usage.completion_tokens,
        "total_tokens": usage.total_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        collapse_pre_user_instruction_messages, drain_sse_events, finalize_tool_calls,
        handle_openai_stream_json, map_openai_response, normalize_messages,
        parse_sse_event_payload, summarize_generate_response, summarize_openai_response,
        to_request, OpenAiCompatMode, OpenAiResponse, PartialToolCall,
    };
    use crate::providers::StreamDelta;
    use crate::types::{GenerateRequest, GenerateResponse, Message, Role, ToolCall};

    #[test]
    fn parses_openai_stream_content_and_tool() {
        let mut deltas = Vec::new();
        let mut content = String::new();
        let mut partials = Vec::<PartialToolCall>::new();
        handle_openai_stream_json(
            r#"{"choices":[{"delta":{"content":"hel"}}]}"#,
            &mut |d| deltas.push(d),
            &mut content,
            &mut partials,
        )
        .expect("parse1");
        handle_openai_stream_json(
            r#"{"choices":[{"delta":{"content":"lo","tool_calls":[{"index":0,"id":"c1","function":{"name":"list_dir","arguments":"{\"path\":\".\"}"}}]},"finish_reason":"tool_calls"}]}"#,
            &mut |d| deltas.push(d),
            &mut content,
            &mut partials,
        )
        .expect("parse2");
        let tc = finalize_tool_calls(partials);
        assert_eq!(content, "hello");
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].name, "list_dir");
        assert!(matches!(deltas[0], StreamDelta::Content(_)));
    }

    #[test]
    fn parses_sse_data_block() {
        let event = "data: {\"x\":1}\n\n";
        let p = parse_sse_event_payload(event).expect("parse");
        assert_eq!(p.as_deref(), Some("{\"x\":1}"));
    }

    #[test]
    fn drains_sse_events_across_chunk_boundaries() {
        let mut buf = "data: {\"a\":1}\n".to_string();
        assert!(drain_sse_events(&mut buf).is_empty());
        buf.push('\n');
        buf.push_str("data: {\"b\":2}\n\n");
        let ev = drain_sse_events(&mut buf);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0], "data: {\"a\":1}");
        assert_eq!(ev[1], "data: {\"b\":2}");
    }

    #[test]
    fn parse_done_payload() {
        let p = parse_sse_event_payload("data: [DONE]\n\n").expect("parse");
        assert_eq!(p.as_deref(), Some("[DONE]"));
    }

    #[test]
    fn malformed_stream_json_returns_error() {
        let mut deltas = Vec::new();
        let mut content = String::new();
        let mut partials = Vec::<PartialToolCall>::new();
        let err = handle_openai_stream_json(
            "{\"choices\":[{\"delta\":{\"content\":\u{fffd}}}]}",
            &mut |d| deltas.push(d),
            &mut content,
            &mut partials,
        )
        .expect_err("expected parse error");
        assert!(err
            .to_string()
            .contains("failed parsing OpenAI-compatible stream event"));
    }

    #[test]
    fn maps_usage_tokens_when_present() {
        let resp: OpenAiResponse = serde_json::from_str(
            r#"{
                "choices":[{"message":{"content":"ok"}}],
                "usage":{"prompt_tokens":12,"completion_tokens":5,"total_tokens":17}
            }"#,
        )
        .expect("parse");
        let mapped = map_openai_response(resp).expect("map");
        let usage = mapped.usage.expect("usage");
        assert_eq!(usage.prompt_tokens, Some(12));
        assert_eq!(usage.completion_tokens, Some(5));
        assert_eq!(usage.total_tokens, Some(17));
    }

    #[test]
    fn summarize_openai_response_preserves_finish_reason_and_tool_preview() {
        let resp: OpenAiResponse = serde_json::from_str(
            r#"{
                "choices":[{
                    "message":{
                        "content":"done",
                        "tool_calls":[{"index":0,"id":"c1","function":{"name":"list_dir","arguments":"{\"path\":\".\"}"}}]
                    },
                    "finish_reason":"tool_calls"
                }]
            }"#,
        )
        .expect("parse");
        let summary = summarize_openai_response(&resp);
        assert_eq!(summary["choice_count"], 1);
        assert_eq!(summary["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(summary["choices"][0]["tool_calls"][0]["name"], "list_dir");
    }

    #[test]
    fn summarize_generate_response_preserves_assistant_and_tool_preview() {
        let resp = GenerateResponse {
            assistant: Message {
                role: Role::Assistant,
                content: Some("verified=yes".to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
            tool_calls: vec![ToolCall {
                id: "c1".to_string(),
                name: "shell".to_string(),
                arguments: serde_json::json!({"command":"node --test"}),
            }],
            usage: None,
        };
        let summary = summarize_generate_response(&resp);
        assert_eq!(summary["assistant_content_preview"], "verified=yes");
        assert_eq!(summary["tool_calls"][0]["name"], "shell");
    }

    #[test]
    fn to_request_uses_temperature_override_when_present() {
        let payload = to_request(
            GenerateRequest {
                model: "m".to_string(),
                messages: Vec::new(),
                tools: None,
                temperature: Some(0.55),
                top_p: None,
                max_tokens: None,
                seed: None,
            },
            false,
            OpenAiCompatMode::Standard,
        );
        assert!((payload.temperature - 0.55).abs() < f32::EPSILON);
    }

    #[test]
    fn to_request_defaults_temperature_to_point_two_when_unset() {
        let payload = to_request(
            GenerateRequest {
                model: "m".to_string(),
                messages: Vec::new(),
                tools: None,
                temperature: None,
                top_p: None,
                max_tokens: None,
                seed: None,
            },
            false,
            OpenAiCompatMode::Standard,
        );
        assert!((payload.temperature - 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn to_request_passes_through_sampling_controls_when_present() {
        let payload = to_request(
            GenerateRequest {
                model: "m".to_string(),
                messages: Vec::new(),
                tools: None,
                temperature: Some(0.3),
                top_p: Some(0.8),
                max_tokens: Some(256),
                seed: Some(42),
            },
            false,
            OpenAiCompatMode::Standard,
        );
        assert_eq!(payload.top_p, Some(0.8));
        assert_eq!(payload.max_tokens, Some(256));
        assert_eq!(payload.seed, Some(42));
    }

    #[test]
    fn normalize_messages_for_lmstudio_maps_developer_to_system() {
        let normalized = normalize_messages(
            vec![
                Message {
                    role: Role::Developer,
                    content: Some("repair instruction".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::User,
                    content: Some("hi".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
            ],
            false,
            OpenAiCompatMode::Lmstudio,
        );

        assert!(matches!(normalized[0].role, Role::System));
        assert_eq!(normalized[0].content.as_deref(), Some("repair instruction"));
    }

    #[test]
    fn normalize_messages_for_standard_drops_semantically_empty_assistant_turns() {
        let normalized = normalize_messages(
            vec![
                Message {
                    role: Role::User,
                    content: Some("task".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::Assistant,
                    content: Some(String::new()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::Tool,
                    content: Some("tool result".to_string()),
                    tool_call_id: Some("tc1".to_string()),
                    tool_name: Some("read_file".to_string()),
                    tool_calls: None,
                },
            ],
            true,
            OpenAiCompatMode::Standard,
        );

        assert_eq!(normalized.len(), 2);
        assert!(matches!(normalized[0].role, Role::User));
        assert!(matches!(normalized[1].role, Role::Tool));
    }

    #[test]
    fn normalize_messages_for_standard_keeps_non_empty_assistant_turns() {
        let normalized = normalize_messages(
            vec![
                Message {
                    role: Role::User,
                    content: Some("task".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                Message {
                    role: Role::Assistant,
                    content: Some("done".to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
            ],
            false,
            OpenAiCompatMode::Standard,
        );

        assert_eq!(normalized.len(), 2);
        assert!(matches!(normalized[1].role, Role::Assistant));
        assert_eq!(normalized[1].content.as_deref(), Some("done"));
    }

    #[test]
    fn collapse_pre_user_instruction_messages_merges_leading_system_blocks() {
        let normalized = collapse_pre_user_instruction_messages(vec![
            Message {
                role: Role::System,
                content: Some("base".to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
            Message {
                role: Role::System,
                content: Some("project".to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
            Message {
                role: Role::User,
                content: Some("task".to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            },
        ]);

        assert_eq!(normalized.len(), 2);
        assert!(matches!(normalized[0].role, Role::System));
        assert_eq!(normalized[0].content.as_deref(), Some("base\n\nproject"));
        assert!(matches!(normalized[1].role, Role::User));
    }

    #[test]
    fn to_request_collapses_lmstudio_pre_user_instructions_when_tools_present() {
        let payload = to_request(
            GenerateRequest {
                model: "m".to_string(),
                messages: vec![
                    Message {
                        role: Role::System,
                        content: Some("base".to_string()),
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                    },
                    Message {
                        role: Role::Developer,
                        content: Some("project".to_string()),
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                    },
                    Message {
                        role: Role::User,
                        content: Some("task".to_string()),
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                    },
                ],
                tools: Some(vec![crate::types::ToolDef {
                    name: "read_file".to_string(),
                    description: "read".to_string(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {}
                    }),
                    side_effects: crate::types::SideEffects::FilesystemRead,
                }]),
                temperature: None,
                top_p: None,
                max_tokens: None,
                seed: None,
            },
            false,
            OpenAiCompatMode::Lmstudio,
        );

        assert_eq!(payload.messages.len(), 2);
        assert!(matches!(payload.messages[0].role, Role::System));
        assert_eq!(
            payload.messages[0].content.as_deref(),
            Some("base\n\nproject")
        );
        assert!(matches!(payload.messages[1].role, Role::User));
    }
}
