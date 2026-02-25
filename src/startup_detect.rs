use std::time::Duration;

use anyhow::anyhow;
use reqwest::Client;
use serde_json::Value;

use crate::gate::ProviderKind;
use crate::provider_runtime;
use crate::providers::http::HttpConfig;

#[derive(Debug, Clone)]
pub(crate) struct StartupDetection {
    pub provider: Option<ProviderKind>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub status_line: String,
}

pub(crate) async fn detect_startup_provider(http: HttpConfig) -> StartupDetection {
    match discover_local_default(http).await {
        Ok((provider, model, base_url)) => StartupDetection {
            provider: Some(provider),
            model: Some(model),
            base_url: Some(base_url),
            status_line: "Auto-detected local provider and model.".to_string(),
        },
        Err(_) => StartupDetection {
            provider: None,
            model: None,
            base_url: None,
            status_line:
                "No local provider detected. Start LM Studio, Ollama, or llama.cpp and press R."
                    .to_string(),
        },
    }
}

async fn discover_local_default(
    http: HttpConfig,
) -> anyhow::Result<(ProviderKind, String, String)> {
    let candidates = [
        (
            ProviderKind::Lmstudio,
            provider_runtime::default_base_url(ProviderKind::Lmstudio).to_string(),
        ),
        (
            ProviderKind::Ollama,
            provider_runtime::default_base_url(ProviderKind::Ollama).to_string(),
        ),
        (
            ProviderKind::Llamacpp,
            provider_runtime::default_base_url(ProviderKind::Llamacpp).to_string(),
        ),
    ];
    for (provider, base_url) in candidates {
        if let Some(model) = discover_model_for_provider(provider, &base_url, &http).await {
            return Ok((provider, model, base_url));
        }
    }
    Err(anyhow!(
        "No local provider detected. Start LM Studio ({}), Ollama ({}), or llama.cpp server ({}) then rerun.",
        provider_runtime::default_base_url(ProviderKind::Lmstudio),
        provider_runtime::default_base_url(ProviderKind::Ollama),
        provider_runtime::default_base_url(ProviderKind::Llamacpp)
    ))
}

async fn discover_model_for_provider(
    provider: ProviderKind,
    base_url: &str,
    http: &HttpConfig,
) -> Option<String> {
    match provider {
        ProviderKind::Ollama => discover_ollama_model(base_url, http).await,
        ProviderKind::Lmstudio | ProviderKind::Llamacpp => {
            discover_openai_compat_model(base_url, http).await
        }
        ProviderKind::Mock => Some("mock-model".to_string()),
    }
}

async fn discover_openai_compat_model(base_url: &str, http: &HttpConfig) -> Option<String> {
    let mut builder =
        Client::builder().connect_timeout(Duration::from_millis(http.connect_timeout_ms));
    if http.request_timeout_ms > 0 {
        builder = builder.timeout(Duration::from_millis(http.request_timeout_ms));
    }
    let client = builder.build().ok()?;
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    let data = v.get("data")?.as_array()?;
    for item in data {
        if let Some(id) = item.get("id").and_then(|x| x.as_str()) {
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    None
}

async fn discover_ollama_model(base_url: &str, http: &HttpConfig) -> Option<String> {
    let mut builder =
        Client::builder().connect_timeout(Duration::from_millis(http.connect_timeout_ms));
    if http.request_timeout_ms > 0 {
        builder = builder.timeout(Duration::from_millis(http.request_timeout_ms));
    }
    let client = builder.build().ok()?;
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    let models = v.get("models")?.as_array()?;
    for item in models {
        if let Some(name) = item.get("name").and_then(|x| x.as_str()) {
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}
