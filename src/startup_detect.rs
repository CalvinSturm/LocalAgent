use crate::gate::ProviderKind;
use crate::provider_runtime;
use crate::providers::http::HttpConfig;

#[derive(Debug, Clone)]
pub(crate) struct StartupDetection {
    pub provider: Option<ProviderKind>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub status_line: String,
    pub next_action: Option<String>,
    pub readiness_state: Option<provider_runtime::ProviderReadinessState>,
}

pub(crate) async fn detect_startup_provider(http: HttpConfig) -> StartupDetection {
    let reports = diagnose_local_defaults(http).await;
    if let Some(report) = reports.iter().find(|report| report.is_ready()) {
        return StartupDetection {
            provider: Some(report.provider),
            model: report.model.clone(),
            base_url: Some(report.base_url.clone()),
            status_line: report.startup_status_line(),
            next_action: Some(report.next_action.clone()),
            readiness_state: Some(report.state),
        };
    }

    if let Some(report) = reports
        .iter()
        .min_by_key(|report| readiness_rank(report.state))
    {
        if report.state != provider_runtime::ProviderReadinessState::ProviderNotRunning {
            return StartupDetection {
                provider: Some(report.provider),
                model: None,
                base_url: Some(report.base_url.clone()),
                status_line: report.startup_status_line(),
                next_action: Some(report.next_action.clone()),
                readiness_state: Some(report.state),
            };
        }
    }

    StartupDetection {
        provider: None,
        model: None,
        base_url: None,
        status_line: format!(
            "No local provider detected. Start LM Studio ({}), Ollama ({}), or llama.cpp server ({}) and press R.",
            provider_runtime::default_base_url(ProviderKind::Lmstudio),
            provider_runtime::default_base_url(ProviderKind::Ollama),
            provider_runtime::default_base_url(ProviderKind::Llamacpp)
        ),
        next_action: Some("Start a supported local provider, load a model, then press R.".to_string()),
        readiness_state: Some(provider_runtime::ProviderReadinessState::ProviderNotRunning),
    }
}

async fn diagnose_local_defaults(
    http: HttpConfig,
) -> Vec<provider_runtime::ProviderReadinessReport> {
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

    let mut reports = Vec::new();
    for (provider, base_url) in candidates {
        reports.push(
            provider_runtime::diagnose_provider_readiness(provider, &base_url, None, http).await,
        );
    }
    reports
}

fn readiness_rank(state: provider_runtime::ProviderReadinessState) -> u8 {
    match state {
        provider_runtime::ProviderReadinessState::Ready => 0,
        provider_runtime::ProviderReadinessState::NoModelLoaded => 1,
        provider_runtime::ProviderReadinessState::UnsupportedResponseShape => 2,
        provider_runtime::ProviderReadinessState::WrongBaseUrl => 3,
        provider_runtime::ProviderReadinessState::ModelListEndpointUnavailable => 4,
        provider_runtime::ProviderReadinessState::RequestTimeout => 5,
        provider_runtime::ProviderReadinessState::ProviderNotRunning => 6,
    }
}
