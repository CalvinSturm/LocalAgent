use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Context};

use crate::gate::ProviderKind;
use crate::providers::ModelProvider;
use crate::store::provider_to_string;
use crate::types::{self, GenerateRequest, Message, Role};

static ORCHESTRATOR_QUAL_CACHE: OnceLock<Mutex<std::collections::BTreeMap<String, bool>>> =
    OnceLock::new();

pub(crate) fn probe_response_to_tool_call(
    resp: &types::GenerateResponse,
) -> Option<types::ToolCall> {
    if let Some(tc) = resp.tool_calls.first() {
        return Some(tc.clone());
    }
    let content = resp.assistant.content.as_deref()?;
    parse_wrapped_tool_call_from_content(content)
        .or_else(|| parse_inline_tool_call_from_content(content))
}

fn parse_wrapped_tool_call_from_content(content: &str) -> Option<types::ToolCall> {
    let upper = content.to_ascii_uppercase();
    let start_tag = "[TOOL_CALL]";
    let end_tag = "[END_TOOL_CALL]";
    let start = upper.find(start_tag)? + start_tag.len();
    let end = upper[start..].find(end_tag)? + start;
    let body = content[start..end].trim();
    if body.is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    tool_call_from_json_value(&value, "wrapped_probe_tool_call")
}

fn parse_inline_tool_call_from_content(content: &str) -> Option<types::ToolCall> {
    let trimmed = content.trim();
    let candidate = if trimmed.starts_with("```") {
        let mut lines = trimmed.lines();
        let first = lines.next().unwrap_or_default();
        if !first.starts_with("```") {
            return None;
        }
        let rest = lines.collect::<Vec<_>>().join("\n");
        let fence_end = rest.rfind("```")?;
        rest[..fence_end].trim().to_string()
    } else {
        trimmed.to_string()
    };
    let value: serde_json::Value = serde_json::from_str(&candidate).ok()?;
    tool_call_from_json_value(&value, "inline_probe_tool_call")
}

fn tool_call_from_json_value(value: &serde_json::Value, id: &str) -> Option<types::ToolCall> {
    let name = value.get("name").and_then(|v| v.as_str())?;
    let arguments = value
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    Some(types::ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments,
    })
}

fn load_orchestrator_qual_cache(
    path: &std::path::Path,
) -> std::collections::BTreeMap<String, bool> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return std::collections::BTreeMap::new();
    };
    serde_json::from_str::<std::collections::BTreeMap<String, bool>>(&raw).unwrap_or_default()
}

fn persist_orchestrator_qual_cache(
    path: &std::path::Path,
    map: &std::collections::BTreeMap<String, bool>,
) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(payload) = serde_json::to_string_pretty(map) {
        let _ = std::fs::write(path, payload);
    }
}

pub(crate) async fn ensure_orchestrator_qualified<P: ModelProvider>(
    provider: &P,
    provider_kind: ProviderKind,
    base_url: &str,
    model: &str,
    tools: &[types::ToolDef],
    cache_path: &std::path::Path,
) -> anyhow::Result<()> {
    let key = format!(
        "{}|{}|{}",
        provider_to_string(provider_kind),
        base_url,
        model
    );
    let cache =
        ORCHESTRATOR_QUAL_CACHE.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()));
    if let Ok(mut m) = cache.lock() {
        if !m.contains_key(&key) {
            let disk = load_orchestrator_qual_cache(cache_path);
            for (k, v) in disk {
                m.entry(k).or_insert(v);
            }
        }
    }
    if let Some(passed) = cache.lock().ok().and_then(|m| m.get(&key).copied()) {
        if passed {
            return Ok(());
        }
        return Err(anyhow!(
            "orchestrator qualification failed previously for this model/session: {key}"
        ));
    }
    let Some(list_dir_tool) = tools.iter().find(|t| t.name == "list_dir").cloned() else {
        if let Ok(mut m) = cache.lock() {
            m.insert(key, false);
            persist_orchestrator_qual_cache(cache_path, &m);
        }
        return Err(anyhow!(
            "orchestrator qualification failed: list_dir tool is not available"
        ));
    };
    let probe_prompt =
        "Emit exactly one native tool call and no prose:\nname=list_dir\narguments={\"path\":\".\"}";
    for _ in 0..3 {
        let req = GenerateRequest {
            model: model.to_string(),
            messages: vec![Message {
                role: Role::User,
                content: Some(probe_prompt.to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            }],
            tools: Some(vec![list_dir_tool.clone()]),
        };
        let resp = provider
            .generate(req)
            .await
            .with_context(|| "orchestrator qualification provider call failed")?;
        let Some(tc) = probe_response_to_tool_call(&resp) else {
            if let Ok(mut m) = cache.lock() {
                m.insert(key.clone(), false);
                persist_orchestrator_qual_cache(cache_path, &m);
            }
            return Err(anyhow!(
                "orchestrator qualification failed: no tool call returned by probe"
            ));
        };
        let path_ok = tc
            .arguments
            .get("path")
            .and_then(|v| v.as_str())
            .map(|p| p == ".")
            .unwrap_or(false);
        if tc.name != "list_dir" || !path_ok {
            if let Ok(mut m) = cache.lock() {
                m.insert(key.clone(), false);
                persist_orchestrator_qual_cache(cache_path, &m);
            }
            return Err(anyhow!(
                "orchestrator qualification failed: expected list_dir {{\"path\":\".\"}}"
            ));
        }
    }
    if let Ok(mut m) = cache.lock() {
        m.insert(key, true);
        persist_orchestrator_qual_cache(cache_path, &m);
    }
    Ok(())
}

pub(crate) async fn qualify_or_enable_readonly_fallback<P: ModelProvider>(
    provider: &P,
    provider_kind: ProviderKind,
    base_url: &str,
    worker_model: &str,
    write_requested: bool,
    all_tools: &mut Vec<types::ToolDef>,
    cache_path: &std::path::Path,
) -> anyhow::Result<Option<String>> {
    if !write_requested {
        return Ok(None);
    }
    match ensure_orchestrator_qualified(
        provider,
        provider_kind,
        base_url,
        worker_model,
        all_tools,
        cache_path,
    )
    .await
    {
        Ok(()) => Ok(None),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("orchestrator qualification failed") {
                all_tools.retain(|t| t.side_effects != types::SideEffects::FilesystemWrite);
                return Ok(Some(format!(
                    "orchestrator qualification failed for model '{worker_model}'; continuing in read-only fallback (write tools disabled): {msg}"
                )));
            }
            Err(e)
        }
    }
}
