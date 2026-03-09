use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Context};
use serde::Serialize;
use serde_json::Value;

use crate::gate::ProviderKind;
use crate::providers::ModelProvider;
use crate::store::{provider_to_string, sha256_hex, write_json_atomic};
use crate::types::{self, GenerateRequest, Message, Role};

static ORCHESTRATOR_QUAL_CACHE: OnceLock<Mutex<std::collections::BTreeMap<String, bool>>> =
    OnceLock::new();

const QUALIFICATION_PROBE_PROMPT: &str =
    "Emit exactly one native tool call and no prose:\nname=list_dir\narguments={\"path\":\".\"}";

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn probe_response_to_tool_call(
    resp: &types::GenerateResponse,
) -> Option<types::ToolCall> {
    analyze_probe_response(resp).tool_call
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

#[derive(Debug)]
struct ProbeResponseAnalysis {
    tool_call: Option<types::ToolCall>,
    source: Option<&'static str>,
    failure_reason: Option<&'static str>,
}

enum TextualProbeParseResult {
    Match(types::ToolCall),
    Malformed(&'static str),
    NoMatch,
}

fn parse_named_arguments_probe_tool_call(content: &str) -> TextualProbeParseResult {
    let lines = content.lines().collect::<Vec<_>>();
    let mut candidates = Vec::new();

    for (idx, raw_line) in lines.iter().enumerate() {
        let line = raw_line.trim();
        if !line.starts_with("name=") {
            continue;
        }

        let Some(next_line) = lines
            .iter()
            .skip(idx + 1)
            .map(|line| line.trim())
            .find(|line| !line.is_empty())
        else {
            return TextualProbeParseResult::Malformed("textual_tool_call_malformed");
        };

        if !next_line.starts_with("arguments=") {
            return TextualProbeParseResult::Malformed("textual_tool_call_malformed");
        }

        candidates.push((
            line.trim_start_matches("name=").trim().to_string(),
            next_line
                .trim_start_matches("arguments=")
                .trim()
                .to_string(),
        ));
    }

    match candidates.len() {
        0 => TextualProbeParseResult::NoMatch,
        1 => {
            let (name, args_raw) = candidates.into_iter().next().expect("candidate");
            let Ok(arguments) = serde_json::from_str::<Value>(&args_raw) else {
                return TextualProbeParseResult::Malformed("textual_tool_call_malformed");
            };
            if !arguments.is_object() {
                return TextualProbeParseResult::Malformed("textual_tool_call_malformed");
            }
            TextualProbeParseResult::Match(types::ToolCall {
                id: "named_arguments_probe_tool_call".to_string(),
                name,
                arguments,
            })
        }
        _ => TextualProbeParseResult::Malformed("textual_tool_call_ambiguous"),
    }
}

fn analyze_probe_response(resp: &types::GenerateResponse) -> ProbeResponseAnalysis {
    if let Some(tc) = resp.tool_calls.first() {
        return ProbeResponseAnalysis {
            tool_call: Some(tc.clone()),
            source: Some("native_tool_calls"),
            failure_reason: None,
        };
    }
    let Some(content) = resp.assistant.content.as_deref() else {
        return ProbeResponseAnalysis {
            tool_call: None,
            source: None,
            failure_reason: None,
        };
    };
    if let Some(tc) = parse_wrapped_tool_call_from_content(content) {
        return ProbeResponseAnalysis {
            tool_call: Some(tc),
            source: Some("wrapped_content"),
            failure_reason: None,
        };
    }
    if let Some(tc) = parse_inline_tool_call_from_content(content) {
        return ProbeResponseAnalysis {
            tool_call: Some(tc),
            source: Some("inline_content"),
            failure_reason: None,
        };
    }
    match parse_named_arguments_probe_tool_call(content) {
        TextualProbeParseResult::Match(tc) => ProbeResponseAnalysis {
            tool_call: Some(tc),
            source: Some("named_arguments_content"),
            failure_reason: None,
        },
        TextualProbeParseResult::Malformed(reason) => ProbeResponseAnalysis {
            tool_call: None,
            source: Some("named_arguments_content"),
            failure_reason: Some(reason),
        },
        TextualProbeParseResult::NoMatch => ProbeResponseAnalysis {
            tool_call: None,
            source: None,
            failure_reason: None,
        },
    }
}

#[derive(Debug, Serialize)]
struct QualificationTraceRequest {
    qualification_cache_key: String,
    provider: String,
    base_url: String,
    model: String,
    stream: bool,
    temperature: Option<f32>,
    top_p: Option<f32>,
    max_tokens: Option<u32>,
    seed: Option<u64>,
    stop_sequences: Option<Vec<String>>,
    response_format: Option<String>,
    provider_generate_mode: String,
    tool_catalog: Vec<QualificationTraceTool>,
    request: GenerateRequest,
}

#[derive(Debug, Serialize)]
struct QualificationTraceTool {
    name: String,
    side_effects: String,
    parameters: Value,
}

#[derive(Debug, Serialize)]
struct QualificationTraceResponseRaw {
    raw_response: Option<Value>,
    note: Option<String>,
}

#[derive(Debug, Serialize)]
struct QualificationTraceResponseParsed {
    assistant_content: Option<String>,
    assistant_content_bytes: usize,
    tool_calls: Vec<types::ToolCall>,
    usage: Option<types::TokenUsage>,
    finish_reason: Option<String>,
    inferred_content_empty: bool,
    inferred_tool_call_count: usize,
}

#[derive(Debug, Serialize)]
struct QualificationAttemptVerdict {
    attempt: usize,
    probe_tool_call_present: bool,
    probe_tool_call_source: Option<String>,
    tool_name_matches_expected: Option<bool>,
    tool_path_matches_expected: Option<bool>,
    verdict: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct QualificationTraceVerdict {
    qualification_cache_key: String,
    provider: String,
    base_url: String,
    model: String,
    stream: bool,
    cache_hit: bool,
    cached_value: Option<bool>,
    cache_written: bool,
    cache_write_value: Option<bool>,
    cache_write_reason: Option<String>,
    final_verdict: String,
    final_reason: String,
    attempt_count: usize,
}

#[derive(Debug, Serialize)]
struct QualificationTraceSummary {
    provider: String,
    model: String,
    base_url: String,
    stream: bool,
    qualification_cache_key: String,
    request_prompt_hash: String,
    response_text_length: Option<usize>,
    finish_reason: Option<String>,
    verdict: String,
    cache_outcome: String,
    artifact_files: QualificationTraceArtifactFiles,
}

#[derive(Debug, Default, Serialize)]
struct QualificationTraceArtifactFiles {
    attempts: Vec<String>,
    verdict: String,
    summary: String,
}

struct QualificationTraceRecorder {
    root_dir: Option<PathBuf>,
    provider: String,
    base_url: String,
    model: String,
    stream: bool,
    qualification_cache_key: String,
    request_prompt_hash: String,
    artifact_files: QualificationTraceArtifactFiles,
    attempt_count: usize,
    response_text_length: Option<usize>,
    finish_reason: Option<String>,
    cache_hit: bool,
    cached_value: Option<bool>,
    cache_written: bool,
    cache_write_value: Option<bool>,
    cache_write_reason: Option<String>,
    final_verdict: Option<String>,
    final_reason: Option<String>,
}

impl QualificationTraceRecorder {
    fn new(
        provider_kind: ProviderKind,
        base_url: &str,
        model: &str,
        stream: bool,
        qualification_cache_key: &str,
    ) -> Self {
        let provider = provider_to_string(provider_kind);
        let root_dir = resolve_qualification_trace_root(
            &provider,
            base_url,
            model,
            stream,
            qualification_cache_key,
        );
        Self {
            root_dir,
            provider,
            base_url: base_url.to_string(),
            model: model.to_string(),
            stream,
            qualification_cache_key: qualification_cache_key.to_string(),
            request_prompt_hash: sha256_hex(QUALIFICATION_PROBE_PROMPT.as_bytes()),
            artifact_files: QualificationTraceArtifactFiles {
                verdict: "verdict.json".to_string(),
                summary: "summary.json".to_string(),
                ..QualificationTraceArtifactFiles::default()
            },
            attempt_count: 0,
            response_text_length: None,
            finish_reason: None,
            cache_hit: false,
            cached_value: None,
            cache_written: false,
            cache_write_value: None,
            cache_write_reason: None,
            final_verdict: None,
            final_reason: None,
        }
    }

    fn record_cache_hit(&mut self, value: bool) {
        self.cache_hit = true;
        self.cached_value = Some(value);
    }

    fn record_cache_write(&mut self, value: bool, reason: &str) {
        self.cache_written = true;
        self.cache_write_value = Some(value);
        self.cache_write_reason = Some(reason.to_string());
    }

    fn write_attempt(
        &mut self,
        req: &GenerateRequest,
        resp: &types::GenerateResponse,
        analysis: &ProbeResponseAnalysis,
        verdict: &QualificationAttemptVerdict,
    ) {
        let Some(root_dir) = &self.root_dir else {
            return;
        };
        self.attempt_count = self.attempt_count.saturating_add(1);
        let attempt_dir = root_dir.join(format!("attempt-{:02}", self.attempt_count));
        let request = QualificationTraceRequest {
            qualification_cache_key: self.qualification_cache_key.clone(),
            provider: self.provider.clone(),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            stream: self.stream,
            temperature: req.temperature,
            top_p: req.top_p,
            max_tokens: req.max_tokens,
            seed: req.seed,
            stop_sequences: None,
            response_format: None,
            provider_generate_mode: "generate".to_string(),
            tool_catalog: req
                .tools
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|tool| QualificationTraceTool {
                    name: tool.name,
                    side_effects: format!("{:?}", tool.side_effects).to_lowercase(),
                    parameters: tool.parameters,
                })
                .collect(),
            request: req.clone(),
        };
        let raw = QualificationTraceResponseRaw {
            raw_response: None,
            note: Some(
                "provider.generate() returns a parsed GenerateResponse in qualification; raw provider body is unavailable at this boundary"
                    .to_string(),
            ),
        };
        let parsed = QualificationTraceResponseParsed {
            assistant_content: resp.assistant.content.clone(),
            assistant_content_bytes: resp.assistant.content.as_deref().map(str::len).unwrap_or(0),
            tool_calls: resp.tool_calls.clone(),
            usage: resp.usage.clone(),
            finish_reason: None,
            inferred_content_empty: resp
                .assistant
                .content
                .as_deref()
                .unwrap_or_default()
                .is_empty(),
            inferred_tool_call_count: resp.tool_calls.len(),
        };
        self.response_text_length = Some(parsed.assistant_content_bytes);
        self.finish_reason = None;
        let _ = write_json_atomic(&attempt_dir.join("request.json"), &request);
        let _ = write_json_atomic(&attempt_dir.join("response.raw.json"), &raw);
        let _ = write_json_atomic(&attempt_dir.join("response.parsed.json"), &parsed);
        let _ = write_json_atomic(&attempt_dir.join("verdict.json"), verdict);
        let attempt_summary = QualificationTraceSummary {
            provider: self.provider.clone(),
            model: self.model.clone(),
            base_url: self.base_url.clone(),
            stream: self.stream,
            qualification_cache_key: self.qualification_cache_key.clone(),
            request_prompt_hash: self.request_prompt_hash.clone(),
            response_text_length: self.response_text_length,
            finish_reason: self.finish_reason.clone(),
            verdict: verdict.verdict.clone(),
            cache_outcome: if self.cache_hit {
                "hit".to_string()
            } else if self.cache_written {
                "write".to_string()
            } else {
                "miss".to_string()
            },
            artifact_files: QualificationTraceArtifactFiles {
                attempts: vec![format!("attempt-{:02}/request.json", self.attempt_count)],
                verdict: format!("attempt-{:02}/verdict.json", self.attempt_count),
                summary: format!("attempt-{:02}/summary.json", self.attempt_count),
            },
        };
        let _ = write_json_atomic(&attempt_dir.join("summary.json"), &attempt_summary);
        self.artifact_files
            .attempts
            .push(format!("attempt-{:02}", self.attempt_count));
        let _ = analysis;
    }

    fn finalize(&mut self, verdict: &str, reason: &str) {
        self.final_verdict = Some(verdict.to_string());
        self.final_reason = Some(reason.to_string());
        let Some(root_dir) = &self.root_dir else {
            return;
        };
        let verdict_json = QualificationTraceVerdict {
            qualification_cache_key: self.qualification_cache_key.clone(),
            provider: self.provider.clone(),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            stream: self.stream,
            cache_hit: self.cache_hit,
            cached_value: self.cached_value,
            cache_written: self.cache_written,
            cache_write_value: self.cache_write_value,
            cache_write_reason: self.cache_write_reason.clone(),
            final_verdict: verdict.to_string(),
            final_reason: reason.to_string(),
            attempt_count: self.attempt_count,
        };
        let summary_json = QualificationTraceSummary {
            provider: self.provider.clone(),
            model: self.model.clone(),
            base_url: self.base_url.clone(),
            stream: self.stream,
            qualification_cache_key: self.qualification_cache_key.clone(),
            request_prompt_hash: self.request_prompt_hash.clone(),
            response_text_length: self.response_text_length,
            finish_reason: self.finish_reason.clone(),
            verdict: verdict.to_string(),
            cache_outcome: if self.cache_hit {
                format!(
                    "hit:{}",
                    self.cached_value
                        .map(|v| if v { "true" } else { "false" })
                        .unwrap_or("null")
                )
            } else if self.cache_written {
                format!(
                    "write:{}",
                    self.cache_write_value
                        .map(|v| if v { "true" } else { "false" })
                        .unwrap_or("null")
                )
            } else {
                "miss".to_string()
            },
            artifact_files: QualificationTraceArtifactFiles {
                attempts: self.artifact_files.attempts.clone(),
                verdict: self.artifact_files.verdict.clone(),
                summary: self.artifact_files.summary.clone(),
            },
        };
        let _ = write_json_atomic(&root_dir.join("verdict.json"), &verdict_json);
        let _ = write_json_atomic(&root_dir.join("summary.json"), &summary_json);
    }
}

fn resolve_qualification_trace_root(
    provider: &str,
    base_url: &str,
    model: &str,
    stream: bool,
    qualification_cache_key: &str,
) -> Option<PathBuf> {
    let trace_dir = std::env::var_os("LOCALAGENT_QUAL_TRACE_DIR")?;
    let timestamp = crate::trust::now_rfc3339()
        .replace(':', "-")
        .replace('.', "_");
    let key_hash = sha256_hex(qualification_cache_key.as_bytes());
    let mode = if stream { "stream-on" } else { "stream-off" };
    let model_slug = slugify_for_path(model);
    let provider_slug = slugify_for_path(provider);
    let base_slug = slugify_for_path(base_url);
    Some(Path::new(&trace_dir).join(format!(
        "{timestamp}-{provider_slug}-{model_slug}-{mode}-{base_slug}-{}",
        &key_hash[..12]
    )))
}

fn slugify_for_path(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) async fn ensure_orchestrator_qualified<P: ModelProvider>(
    provider: &P,
    provider_kind: ProviderKind,
    base_url: &str,
    model: &str,
    stream: bool,
    tools: &[types::ToolDef],
    cache_path: &std::path::Path,
) -> anyhow::Result<()> {
    let key = format!(
        "{}|{}|{}",
        provider_to_string(provider_kind),
        base_url,
        model
    );
    let mut trace = QualificationTraceRecorder::new(provider_kind, base_url, model, stream, &key);
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
        trace.record_cache_hit(passed);
        if passed {
            trace.finalize("ok", "cache_hit_pass");
            return Ok(());
        }
        trace.finalize("error", "cache_hit_fail");
        return Err(anyhow!(
            "orchestrator qualification failed previously for this model/session: {key}"
        ));
    }
    let Some(list_dir_tool) = tools.iter().find(|t| t.name == "list_dir").cloned() else {
        if let Ok(mut m) = cache.lock() {
            m.insert(key, false);
            persist_orchestrator_qual_cache(cache_path, &m);
        }
        trace.record_cache_write(false, "list_dir_tool_missing");
        trace.finalize("error", "list_dir_tool_missing");
        return Err(anyhow!(
            "orchestrator qualification failed: list_dir tool is not available"
        ));
    };
    let mut last_failure_reason = "no_tool_call_returned";
    for attempt in 0..3 {
        let req = GenerateRequest {
            model: model.to_string(),
            messages: vec![Message {
                role: Role::User,
                content: Some(QUALIFICATION_PROBE_PROMPT.to_string()),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            }],
            tools: Some(vec![list_dir_tool.clone()]),
            temperature: None,
            top_p: None,
            max_tokens: None,
            seed: None,
        };
        let resp = provider
            .generate(req.clone())
            .await
            .with_context(|| "orchestrator qualification provider call failed")?;
        let analysis = analyze_probe_response(&resp);
        let Some(tc) = analysis.tool_call.clone() else {
            let reason = analysis.failure_reason.unwrap_or("no_tool_call_returned");
            last_failure_reason = reason;
            let verdict = QualificationAttemptVerdict {
                attempt: attempt + 1,
                probe_tool_call_present: false,
                probe_tool_call_source: analysis.source.map(str::to_string),
                tool_name_matches_expected: None,
                tool_path_matches_expected: None,
                verdict: "error".to_string(),
                reason: reason.to_string(),
            };
            trace.write_attempt(&req, &resp, &analysis, &verdict);
            continue;
        };
        let path_ok = tc
            .arguments
            .get("path")
            .and_then(|v| v.as_str())
            .map(|p| p == ".")
            .unwrap_or(false);
        let tool_name_ok = tc.name == "list_dir";
        let verdict = QualificationAttemptVerdict {
            attempt: attempt + 1,
            probe_tool_call_present: true,
            probe_tool_call_source: analysis.source.map(str::to_string),
            tool_name_matches_expected: Some(tool_name_ok),
            tool_path_matches_expected: Some(path_ok),
            verdict: if tool_name_ok && path_ok {
                "ok".to_string()
            } else {
                "error".to_string()
            },
            reason: if tool_name_ok && path_ok {
                if analysis.source == Some("native_tool_calls") {
                    "probe_passed_native_tool_call".to_string()
                } else {
                    "probe_passed_textual_tool_call_fallback".to_string()
                }
            } else {
                "unexpected_tool_call".to_string()
            },
        };
        trace.write_attempt(&req, &resp, &analysis, &verdict);
        if tc.name != "list_dir" || !path_ok {
            last_failure_reason = "unexpected_tool_call";
            continue;
        }
        if let Ok(mut m) = cache.lock() {
            m.insert(key, true);
            persist_orchestrator_qual_cache(cache_path, &m);
        }
        trace.record_cache_write(true, "probe_passed");
        trace.finalize("ok", "probe_passed");
        return Ok(());
    }
    if let Ok(mut m) = cache.lock() {
        m.insert(key, false);
        persist_orchestrator_qual_cache(cache_path, &m);
    }
    trace.record_cache_write(false, last_failure_reason);
    trace.finalize("error", last_failure_reason);
    let msg = match last_failure_reason {
        "textual_tool_call_malformed" => {
            "orchestrator qualification failed: textual probe tool call was malformed"
        }
        "textual_tool_call_ambiguous" => {
            "orchestrator qualification failed: textual probe tool call was ambiguous"
        }
        "unexpected_tool_call" => {
            "orchestrator qualification failed: expected list_dir {\"path\":\".\"}"
        }
        _ => "orchestrator qualification failed: no tool call returned by probe",
    };
    Err(anyhow!(msg))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn qualify_or_enable_readonly_fallback<P: ModelProvider>(
    provider: &P,
    provider_kind: ProviderKind,
    base_url: &str,
    worker_model: &str,
    stream: bool,
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
        stream,
        all_tools,
        cache_path,
    )
    .await
    {
        Ok(()) => Ok(None),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("orchestrator qualification failed")
                || msg.contains("orchestrator qualification provider call failed")
            {
                all_tools.retain(|t| t.side_effects != types::SideEffects::FilesystemWrite);
                return Ok(Some(format!(
                    "orchestrator qualification failed for model '{worker_model}'; continuing in read-only fallback (write tools disabled): {msg}"
                )));
            }
            Err(e)
        }
    }
}
