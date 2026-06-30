use reqwest::{Client, StatusCode};
use serde_json::Value;

use crate::gate::ProviderKind;
use crate::providers::http::HttpConfig;
use crate::{DoctorArgs, EvalArgs, RunArgs};

pub(crate) async fn doctor_check(args: &DoctorArgs) -> Result<String, String> {
    let provider = args
        .provider
        .ok_or_else(|| "--provider is required unless --docker is used".to_string())?;
    let base_url = args
        .base_url
        .clone()
        .unwrap_or_else(|| default_base_url(provider).to_string());
    let report = diagnose_provider_readiness(
        provider,
        &base_url,
        args.api_key.as_deref(),
        doctor_http_config(),
    )
    .await;
    let rendered = report.render_doctor();
    if report.is_ready() {
        Ok(rendered)
    } else {
        Err(rendered)
    }
}

async fn get_with_optional_bearer(
    client: &Client,
    url: &str,
    api_key: Option<&str>,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut req = client.get(url);
    if let Some(key) = api_key {
        req = req.bearer_auth(key);
    }
    req.send().await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderReadinessState {
    Ready,
    ProviderNotRunning,
    WrongBaseUrl,
    NoModelLoaded,
    ModelListEndpointUnavailable,
    RequestTimeout,
    UnsupportedResponseShape,
}

impl ProviderReadinessState {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::ProviderNotRunning => "provider not running",
            Self::WrongBaseUrl => "wrong base URL",
            Self::NoModelLoaded => "provider online but no model loaded",
            Self::ModelListEndpointUnavailable => "model list endpoint unavailable",
            Self::RequestTimeout => "request timeout",
            Self::UnsupportedResponseShape => "unsupported response shape",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderReadinessReport {
    pub(crate) provider: ProviderKind,
    pub(crate) base_url: String,
    pub(crate) models_url: String,
    pub(crate) state: ProviderReadinessState,
    pub(crate) model: Option<String>,
    pub(crate) model_count: usize,
    pub(crate) detail: String,
    pub(crate) next_action: String,
}

impl ProviderReadinessReport {
    pub(crate) fn is_ready(&self) -> bool {
        self.state == ProviderReadinessState::Ready
    }

    pub(crate) fn model_status(&self) -> String {
        match self.state {
            ProviderReadinessState::Ready => match (&self.model, self.model_count) {
                (Some(model), 1) => format!("ready ({model}; 1 model available)"),
                (Some(model), n) => format!("ready ({model}; {n} models available)"),
                (None, _) => "ready".to_string(),
            },
            ProviderReadinessState::NoModelLoaded => "no model loaded".to_string(),
            other => format!("unknown ({})", other.label()),
        }
    }

    pub(crate) fn streaming_support(&self) -> String {
        match self.state {
            ProviderReadinessState::Ready => {
                "supported by provider; not verified by doctor".to_string()
            }
            ProviderReadinessState::NoModelLoaded => {
                "supported by provider; blocked until a model is loaded".to_string()
            }
            ProviderReadinessState::ProviderNotRunning
            | ProviderReadinessState::WrongBaseUrl
            | ProviderReadinessState::ModelListEndpointUnavailable
            | ProviderReadinessState::RequestTimeout
            | ProviderReadinessState::UnsupportedResponseShape => {
                "unknown until provider readiness passes".to_string()
            }
        }
    }

    pub(crate) fn startup_status_line(&self) -> String {
        if self.is_ready() {
            "Auto-detected local provider and model.".to_string()
        } else {
            format!("{}: {}", self.state.label(), self.next_action)
        }
    }

    pub(crate) fn render_doctor(&self) -> String {
        let mut lines = Vec::new();
        lines.push(if self.is_ready() {
            "OK: provider readiness passed".to_string()
        } else {
            format!("FAIL: {}", self.state.label())
        });
        lines.push(format!(
            "Provider tested: {}",
            provider_cli_name(self.provider)
        ));
        lines.push(format!("Base URL tested: {}", self.base_url));
        lines.push(format!("Model list endpoint: {}", self.models_url));
        lines.push(format!("Model status: {}", self.model_status()));
        lines.push(format!("Streaming support: {}", self.streaming_support()));
        if !self.detail.is_empty() {
            lines.push(format!("Detail: {}", self.detail));
        }
        lines.push(format!("Next action: {}", self.next_action));
        lines.join("\n")
    }
}

pub(crate) async fn diagnose_provider_readiness(
    provider: ProviderKind,
    base_url: &str,
    api_key: Option<&str>,
    http: HttpConfig,
) -> ProviderReadinessReport {
    if provider == ProviderKind::Mock {
        return ProviderReadinessReport {
            provider,
            base_url: base_url.to_string(),
            models_url: base_url.to_string(),
            state: ProviderReadinessState::Ready,
            model: Some("mock".to_string()),
            model_count: 1,
            detail: "mock provider does not require a local server or model download".to_string(),
            next_action: "Use `--provider mock --model mock` for deterministic local smoke runs."
                .to_string(),
        };
    }

    let models_url = doctor_probe_urls(provider, base_url)
        .into_iter()
        .next()
        .unwrap_or_else(|| base_url.to_string());

    let client = match build_probe_client(http) {
        Ok(client) => client,
        Err(e) => {
            return report_for_state(
                provider,
                base_url,
                &models_url,
                ProviderReadinessState::WrongBaseUrl,
                format!("failed to build HTTP client: {e}"),
                None,
                0,
            );
        }
    };

    let response = get_with_optional_bearer(&client, &models_url, api_key).await;
    match response {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                return diagnose_non_success_models_endpoint(
                    provider,
                    base_url,
                    &models_url,
                    &client,
                    api_key,
                    status,
                )
                .await;
            }
            let body = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("{{\"localagent_body_read_error\":\"{e}\"}}"));
            let probe = classify_models_response(provider, status, &body);
            report_for_state(
                provider,
                base_url,
                &models_url,
                probe.state,
                probe.detail,
                probe.model,
                probe.model_count,
            )
        }
        Err(err) => {
            let (state, detail) = classify_probe_error(&err, &models_url);
            report_for_state(provider, base_url, &models_url, state, detail, None, 0)
        }
    }
}

async fn diagnose_non_success_models_endpoint(
    provider: ProviderKind,
    base_url: &str,
    models_url: &str,
    client: &Client,
    api_key: Option<&str>,
    status: StatusCode,
) -> ProviderReadinessReport {
    if status == StatusCode::NOT_FOUND {
        let base_probe =
            get_with_optional_bearer(client, base_url.trim_end_matches('/'), api_key).await;
        if base_probe
            .as_ref()
            .is_ok_and(|resp| resp.status().is_success())
        {
            return report_for_state(
                provider,
                base_url,
                models_url,
                ProviderReadinessState::WrongBaseUrl,
                format!(
                    "{} returned HTTP 404, but the base URL answered",
                    models_url
                ),
                None,
                0,
            );
        }
    }

    report_for_state(
        provider,
        base_url,
        models_url,
        ProviderReadinessState::ModelListEndpointUnavailable,
        format!("{models_url} returned HTTP {}", status.as_u16()),
        None,
        0,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelProbe {
    state: ProviderReadinessState,
    model: Option<String>,
    model_count: usize,
    detail: String,
}

fn classify_models_response(provider: ProviderKind, status: StatusCode, body: &str) -> ModelProbe {
    if !status.is_success() {
        return ModelProbe {
            state: ProviderReadinessState::ModelListEndpointUnavailable,
            model: None,
            model_count: 0,
            detail: format!("model list endpoint returned HTTP {}", status.as_u16()),
        };
    }

    let parsed: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(e) => {
            return ModelProbe {
                state: ProviderReadinessState::UnsupportedResponseShape,
                model: None,
                model_count: 0,
                detail: format!("model list endpoint did not return valid JSON: {e}"),
            };
        }
    };

    let names = match provider {
        ProviderKind::Ollama => extract_string_field_array(&parsed, "models", "name"),
        ProviderKind::Lmstudio | ProviderKind::Llamacpp => {
            extract_string_field_array(&parsed, "data", "id")
        }
        ProviderKind::Mock => Some(vec!["mock".to_string()]),
    };

    let Some(names) = names else {
        return ModelProbe {
            state: ProviderReadinessState::UnsupportedResponseShape,
            model: None,
            model_count: 0,
            detail: expected_models_shape(provider).to_string(),
        };
    };

    if names.is_empty() {
        return ModelProbe {
            state: ProviderReadinessState::NoModelLoaded,
            model: None,
            model_count: 0,
            detail: "model list endpoint returned no usable model identifiers".to_string(),
        };
    }

    ModelProbe {
        state: ProviderReadinessState::Ready,
        model: names.first().cloned(),
        model_count: names.len(),
        detail: "model list endpoint returned at least one model".to_string(),
    }
}

fn extract_string_field_array(
    value: &Value,
    array_key: &str,
    field_key: &str,
) -> Option<Vec<String>> {
    let items = value.get(array_key)?.as_array()?;
    let mut out = Vec::new();
    for item in items {
        let name = item.get(field_key).and_then(|x| x.as_str())?;
        if !name.trim().is_empty() {
            out.push(name.to_string());
        }
    }
    Some(out)
}

fn expected_models_shape(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Ollama => "expected Ollama JSON shape: {\"models\":[{\"name\":\"...\"}]}",
        ProviderKind::Lmstudio | ProviderKind::Llamacpp => {
            "expected OpenAI-compatible JSON shape: {\"data\":[{\"id\":\"...\"}]}"
        }
        ProviderKind::Mock => "mock provider does not use a model list endpoint",
    }
}

fn classify_probe_error(err: &reqwest::Error, url: &str) -> (ProviderReadinessState, String) {
    if err.is_connect() {
        return (
            ProviderReadinessState::ProviderNotRunning,
            format!("could not connect to {url}: {err}"),
        );
    }
    if err.is_timeout() {
        return (
            ProviderReadinessState::RequestTimeout,
            format!("timed out while requesting {url}: {err}"),
        );
    }
    if err.is_builder() {
        return (
            ProviderReadinessState::WrongBaseUrl,
            format!("invalid provider URL {url}: {err}"),
        );
    }
    (
        ProviderReadinessState::ModelListEndpointUnavailable,
        format!("failed to request {url}: {err}"),
    )
}

fn report_for_state(
    provider: ProviderKind,
    base_url: &str,
    models_url: &str,
    state: ProviderReadinessState,
    detail: String,
    model: Option<String>,
    model_count: usize,
) -> ProviderReadinessReport {
    let next_action = next_action(provider, base_url, state, model.as_deref());
    ProviderReadinessReport {
        provider,
        base_url: base_url.to_string(),
        models_url: models_url.to_string(),
        state,
        model,
        model_count,
        detail,
        next_action,
    }
}

fn next_action(
    provider: ProviderKind,
    base_url: &str,
    state: ProviderReadinessState,
    model: Option<&str>,
) -> String {
    match state {
        ProviderReadinessState::Ready => format!(
            "Run `localagent --provider {} --model {} chat --tui`.",
            provider_cli_name(provider),
            model.unwrap_or("<model>")
        ),
        ProviderReadinessState::ProviderNotRunning => match provider {
            ProviderKind::Lmstudio => format!(
                "Open LM Studio, start the local server, confirm it listens at {base_url}, then rerun doctor."
            ),
            ProviderKind::Llamacpp => {
                "Start `llama-server` with `--host 127.0.0.1 --port 8080 --jinja`, then rerun doctor.".to_string()
            }
            ProviderKind::Ollama => {
                "Start Ollama with the desktop app or `ollama serve`, then rerun doctor."
                    .to_string()
            }
            ProviderKind::Mock => {
                "Use `--provider mock --model mock`; no local server is required.".to_string()
            }
        },
        ProviderReadinessState::WrongBaseUrl => match provider {
            ProviderKind::Lmstudio | ProviderKind::Llamacpp => {
                "Confirm this is the provider's OpenAI-compatible API base URL and that the /models endpoint is enabled.".to_string()
            }
            ProviderKind::Ollama => format!(
                "Use the Ollama base URL `{}`; do not include `/v1`.",
                default_base_url(provider)
            ),
            ProviderKind::Mock => {
                "Use `--provider mock --model mock`; no HTTP base URL is required.".to_string()
            }
        },
        ProviderReadinessState::NoModelLoaded => match provider {
            ProviderKind::Lmstudio => {
                "Load a model in LM Studio local server mode, then press R or rerun doctor."
                    .to_string()
            }
            ProviderKind::Llamacpp => {
                "Restart `llama-server` with a GGUF model loaded, then press R or rerun doctor."
                    .to_string()
            }
            ProviderKind::Ollama => {
                "Pull a model such as `ollama pull qwen3:8b`, then press R or rerun doctor."
                    .to_string()
            }
            ProviderKind::Mock => {
                "Use `--provider mock --model mock`; no model download is required.".to_string()
            }
        },
        ProviderReadinessState::ModelListEndpointUnavailable => match provider {
            ProviderKind::Lmstudio | ProviderKind::Llamacpp => {
                "Confirm the server exposes `GET /v1/models` and retry with the correct `--base-url`.".to_string()
            }
            ProviderKind::Ollama => {
                "Confirm Ollama exposes `GET /api/tags` at the tested base URL, then rerun doctor."
                    .to_string()
            }
            ProviderKind::Mock => {
                "Use `--provider mock --model mock`; no model list endpoint is required."
                    .to_string()
            }
        },
        ProviderReadinessState::RequestTimeout => {
            "Wait for the provider to finish loading or increase the HTTP timeout, then rerun doctor."
                .to_string()
        }
        ProviderReadinessState::UnsupportedResponseShape => {
            "Confirm the base URL points to the selected provider and returns the expected model-list JSON shape."
                .to_string()
        }
    }
}

fn build_probe_client(http: HttpConfig) -> Result<Client, reqwest::Error> {
    let mut builder = Client::builder().connect_timeout(http.connect_timeout());
    if let Some(timeout) = http.request_timeout_opt() {
        builder = builder.timeout(timeout);
    }
    builder.build()
}

fn doctor_http_config() -> HttpConfig {
    HttpConfig {
        request_timeout_ms: 3_000,
        http_max_retries: 0,
        ..HttpConfig::default()
    }
}

pub(crate) fn default_base_url(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Lmstudio => "http://localhost:1234/v1",
        ProviderKind::Llamacpp => "http://localhost:8080/v1",
        ProviderKind::Ollama => "http://localhost:11434",
        ProviderKind::Mock => "mock://local",
    }
}

pub(crate) fn provider_cli_name(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Lmstudio => "lmstudio",
        ProviderKind::Llamacpp => "llamacpp",
        ProviderKind::Ollama => "ollama",
        ProviderKind::Mock => "mock",
    }
}

pub(crate) fn doctor_probe_urls(provider: ProviderKind, base_url: &str) -> Vec<String> {
    let trimmed = base_url.trim_end_matches('/').to_string();
    match provider {
        ProviderKind::Lmstudio | ProviderKind::Llamacpp => {
            vec![format!("{trimmed}/models"), trimmed]
        }
        ProviderKind::Ollama => vec![format!("{trimmed}/api/tags")],
        ProviderKind::Mock => vec![trimmed],
    }
}

pub(crate) fn http_config_from_run_args(args: &RunArgs) -> HttpConfig {
    HttpConfig {
        connect_timeout_ms: args.http_connect_timeout_ms,
        request_timeout_ms: args.http_timeout_ms,
        stream_idle_timeout_ms: args.http_stream_idle_timeout_ms,
        max_response_bytes: args.http_max_response_bytes,
        max_line_bytes: args.http_max_line_bytes,
        http_max_retries: args.http_max_retries,
        ..HttpConfig::default()
    }
}

pub(crate) fn http_config_from_eval_args(args: &EvalArgs) -> HttpConfig {
    HttpConfig {
        connect_timeout_ms: args.http_connect_timeout_ms,
        request_timeout_ms: args.http_timeout_ms,
        stream_idle_timeout_ms: args.http_stream_idle_timeout_ms,
        max_response_bytes: args.http_max_response_bytes,
        max_line_bytes: args.http_max_line_bytes,
        http_max_retries: args.http_max_retries,
        ..HttpConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_models_response, report_for_state, ProviderReadinessState};
    use crate::gate::ProviderKind;
    use reqwest::StatusCode;

    #[test]
    fn classifies_openai_compatible_ready_models() {
        let probe = classify_models_response(
            ProviderKind::Lmstudio,
            StatusCode::OK,
            r#"{"data":[{"id":"local-model"}]}"#,
        );
        assert_eq!(probe.state, ProviderReadinessState::Ready);
        assert_eq!(probe.model.as_deref(), Some("local-model"));
        assert_eq!(probe.model_count, 1);
    }

    #[test]
    fn classifies_openai_compatible_no_model_loaded() {
        let probe =
            classify_models_response(ProviderKind::Llamacpp, StatusCode::OK, r#"{"data":[]}"#);
        assert_eq!(probe.state, ProviderReadinessState::NoModelLoaded);
        assert_eq!(probe.model, None);
    }

    #[test]
    fn classifies_ollama_ready_models() {
        let probe = classify_models_response(
            ProviderKind::Ollama,
            StatusCode::OK,
            r#"{"models":[{"name":"qwen3:8b"}]}"#,
        );
        assert_eq!(probe.state, ProviderReadinessState::Ready);
        assert_eq!(probe.model.as_deref(), Some("qwen3:8b"));
    }

    #[test]
    fn classifies_ollama_no_model_loaded() {
        let probe =
            classify_models_response(ProviderKind::Ollama, StatusCode::OK, r#"{"models":[]}"#);
        assert_eq!(probe.state, ProviderReadinessState::NoModelLoaded);
    }

    #[test]
    fn classifies_unsupported_response_shape() {
        let probe =
            classify_models_response(ProviderKind::Lmstudio, StatusCode::OK, r#"{"models":[]}"#);
        assert_eq!(
            probe.state,
            ProviderReadinessState::UnsupportedResponseShape
        );
    }

    #[test]
    fn classifies_invalid_json_as_unsupported_shape() {
        let probe = classify_models_response(ProviderKind::Ollama, StatusCode::OK, "not json");
        assert_eq!(
            probe.state,
            ProviderReadinessState::UnsupportedResponseShape
        );
    }

    #[test]
    fn classifies_model_list_endpoint_unavailable() {
        let probe = classify_models_response(
            ProviderKind::Lmstudio,
            StatusCode::SERVICE_UNAVAILABLE,
            "busy",
        );
        assert_eq!(
            probe.state,
            ProviderReadinessState::ModelListEndpointUnavailable
        );
    }

    #[test]
    fn doctor_ready_streaming_support_is_not_a_generation_verification() {
        let report = report_for_state(
            ProviderKind::Lmstudio,
            "http://localhost:1234/v1",
            "http://localhost:1234/v1/models",
            ProviderReadinessState::Ready,
            "model list endpoint returned at least one model".to_string(),
            Some("local-model".to_string()),
            1,
        );
        let rendered = report.render_doctor();

        assert!(
            rendered.contains("Streaming support: supported by provider; not verified by doctor")
        );
        assert!(!rendered.contains("readiness: ready"));
    }

    #[test]
    fn wrong_base_url_next_action_names_openai_compatible_models_endpoint() {
        let report = report_for_state(
            ProviderKind::Lmstudio,
            "http://localhost:1234",
            "http://localhost:1234/models",
            ProviderReadinessState::WrongBaseUrl,
            "model list endpoint returned HTTP 404".to_string(),
            None,
            0,
        );

        assert_eq!(
            report.next_action,
            "Confirm this is the provider's OpenAI-compatible API base URL and that the /models endpoint is enabled."
        );
    }
}
