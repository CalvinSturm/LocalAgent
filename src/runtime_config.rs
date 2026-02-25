use crate::compaction::{CompactionMode, ToolResultPersist};
use crate::RunArgs;

pub(crate) fn apply_chat_mode(run: &mut RunArgs, mode: &str) -> Option<()> {
    match mode.to_ascii_lowercase().as_str() {
        "safe" => {
            run.enable_write_tools = false;
            run.allow_write = false;
            run.allow_shell = false;
            run.mcp.retain(|m| m != "playwright");
            Some(())
        }
        "coding" | "code" => {
            run.enable_write_tools = true;
            run.allow_write = true;
            run.allow_shell = true;
            run.mcp.retain(|m| m != "playwright");
            Some(())
        }
        "web" => {
            run.enable_write_tools = false;
            run.allow_write = false;
            run.allow_shell = false;
            if !run.mcp.iter().any(|m| m == "playwright") {
                run.mcp.push("playwright".to_string());
            }
            Some(())
        }
        "custom" => {
            run.enable_write_tools = true;
            run.allow_write = true;
            run.allow_shell = true;
            if !run.mcp.iter().any(|m| m == "playwright") {
                run.mcp.push("playwright".to_string());
            }
            Some(())
        }
        _ => None,
    }
}

pub(crate) fn timeout_settings_summary(run: &RunArgs) -> String {
    let request = if run.http_timeout_ms == 0 {
        "off".to_string()
    } else {
        format!("{}s", run.http_timeout_ms / 1000)
    };
    let stream_idle = if run.http_stream_idle_timeout_ms == 0 {
        "off".to_string()
    } else {
        format!("{}s", run.http_stream_idle_timeout_ms / 1000)
    };
    format!(
        "timeouts: request={}, stream-idle={}, connect={}s",
        request,
        stream_idle,
        run.http_connect_timeout_ms / 1000
    )
}

pub(crate) fn is_timeout_error_text(msg: &str) -> bool {
    let lowered = msg.to_ascii_lowercase();
    lowered.contains("timeout")
        || lowered.contains("timed out")
        || lowered.contains("stream idle")
        || lowered.contains("attempt")
}

pub(crate) fn timeout_notice_text(run: &RunArgs) -> String {
    format!(
        "[timeout-notice] provider timeout detected; try /timeout to increase duration ({}) ; use /dismiss to hide this notice",
        timeout_settings_summary(run)
    )
}

pub(crate) fn protocol_remediation_hint(msg: &str) -> Option<String> {
    let m = msg.to_ascii_lowercase();
    if m.contains("repeated invalid patch format") || m.contains("invalid patch format") {
        return Some(
            "[protocol-hint] patch format rejected: use apply_patch with a valid unified diff (headers + @@ hunks), or use write_file only when creating a new file.".to_string(),
        );
    }
    if m.contains("repeated malformed tool calls")
        || m.contains("empty or malformed [tool_call] envelope")
        || m.contains("no tool call returned by probe")
    {
        return Some(
            "[protocol-hint] tool-call formatting issue: emit exactly one native tool call JSON object with {\"name\",\"arguments\"}; avoid wrappers, markdown fences, and prose.".to_string(),
        );
    }
    if m.contains("tool-only phase") || m.contains("repeated prose output during tool-only phase") {
        return Some(
            "[protocol-hint] tool-only violation: emit tool calls only until write/verify is complete; return prose summary only after final read_file verification.".to_string(),
        );
    }
    None
}

pub(crate) fn apply_timeout_input(run: &mut RunArgs, input: &str) -> Result<String, String> {
    let value = input.trim();
    if value.is_empty() {
        return Err("timeout value is empty".to_string());
    }
    if matches!(
        value.to_ascii_lowercase().as_str(),
        "off" | "none" | "disable" | "disabled"
    ) {
        run.http_timeout_ms = 0;
        run.http_stream_idle_timeout_ms = 0;
        return Ok(format!(
            "updated {} (request+stream-idle timeout disabled; connect remains {}s)",
            timeout_settings_summary(run),
            run.http_connect_timeout_ms / 1000
        ));
    }
    let parse_seconds = |s: &str| -> Result<i64, String> {
        s.parse::<i64>()
            .map_err(|_| format!("invalid timeout value: {s}"))
    };
    let current = (run.http_timeout_ms / 1000) as i64;
    let next_seconds = if let Some(delta) = value.strip_prefix('+') {
        current + parse_seconds(delta)?
    } else if let Some(delta) = value.strip_prefix('-') {
        current - parse_seconds(delta)?
    } else {
        parse_seconds(value)?
    };
    if next_seconds <= 0 {
        return Err("timeout must be at least 1 second".to_string());
    }
    let next_ms = (next_seconds as u64) * 1000;
    run.http_timeout_ms = next_ms;
    run.http_stream_idle_timeout_ms = next_ms;
    Ok(format!(
        "updated {} (request+stream-idle now {}s; connect remains {}s)",
        timeout_settings_summary(run),
        next_seconds,
        run.http_connect_timeout_ms / 1000
    ))
}

pub(crate) fn params_settings_summary(run: &RunArgs) -> String {
    format!(
        "params: max_steps={} max_context_chars={} compaction_mode={:?} compaction_keep_last={} tool_result_persist={:?} max_tool_output_bytes={} max_read_bytes={} stream={} allow_shell={} allow_write={} enable_write_tools={} allow_shell_in_workdir={}",
        run.max_steps,
        run.max_context_chars,
        run.compaction_mode,
        run.compaction_keep_last,
        run.tool_result_persist,
        run.max_tool_output_bytes,
        run.max_read_bytes,
        run.stream,
        run.allow_shell,
        run.allow_write,
        run.enable_write_tools,
        run.allow_shell_in_workdir
    )
}

fn parse_toggle(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "true" | "1" | "yes" => Some(true),
        "off" | "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

pub(crate) fn apply_params_input(run: &mut RunArgs, input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("params input is empty".to_string());
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let key = parts
        .next()
        .ok_or_else(|| "missing params key".to_string())?
        .to_ascii_lowercase();
    let value = parts
        .next()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "missing params value".to_string())?;
    match key.as_str() {
        "max_steps" | "steps" => {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| format!("invalid usize for {key}: {value}"))?;
            if parsed == 0 {
                return Err("max_steps must be at least 1".to_string());
            }
            run.max_steps = parsed;
        }
        "max_context_chars" | "max_context" | "context" => {
            run.max_context_chars = value
                .parse::<usize>()
                .map_err(|_| format!("invalid usize for {key}: {value}"))?;
        }
        "compaction_mode" | "compaction" => match value.to_ascii_lowercase().as_str() {
            "off" => run.compaction_mode = CompactionMode::Off,
            "summary" => run.compaction_mode = CompactionMode::Summary,
            _ => {
                return Err(format!(
                    "invalid compaction_mode: {value} (expected off|summary)"
                ))
            }
        },
        "compaction_keep_last" | "keep_last" => {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| format!("invalid usize for {key}: {value}"))?;
            if parsed == 0 {
                return Err("compaction_keep_last must be at least 1".to_string());
            }
            run.compaction_keep_last = parsed;
        }
        "tool_result_persist" | "tool_persist" | "persist" => {
            run.tool_result_persist = match value.to_ascii_lowercase().as_str() {
                "all" => ToolResultPersist::All,
                "digest" => ToolResultPersist::Digest,
                "none" => ToolResultPersist::None,
                _ => {
                    return Err(format!(
                        "invalid tool_result_persist: {value} (expected all|digest|none)"
                    ));
                }
            };
        }
        "max_tool_output_bytes" | "tool_output" => {
            run.max_tool_output_bytes = value
                .parse::<usize>()
                .map_err(|_| format!("invalid usize for {key}: {value}"))?;
        }
        "max_read_bytes" | "read_bytes" => {
            run.max_read_bytes = value
                .parse::<usize>()
                .map_err(|_| format!("invalid usize for {key}: {value}"))?;
        }
        "stream" => {
            run.stream = parse_toggle(value)
                .ok_or_else(|| format!("invalid toggle for stream: {value} (use on|off)"))?;
        }
        "allow_shell" => {
            run.allow_shell = parse_toggle(value)
                .ok_or_else(|| format!("invalid toggle for allow_shell: {value} (use on|off)"))?;
        }
        "allow_write" => {
            run.allow_write = parse_toggle(value)
                .ok_or_else(|| format!("invalid toggle for allow_write: {value} (use on|off)"))?;
        }
        "enable_write_tools" | "write_tools" => {
            run.enable_write_tools = parse_toggle(value).ok_or_else(|| {
                format!("invalid toggle for enable_write_tools: {value} (use on|off)")
            })?;
        }
        "allow_shell_in_workdir" | "shell_in_workdir" => {
            run.allow_shell_in_workdir = parse_toggle(value).ok_or_else(|| {
                format!("invalid toggle for allow_shell_in_workdir: {value} (use on|off)")
            })?;
        }
        _ => {
            return Err(format!(
                "unknown params key: {key}. try: max_steps, max_context_chars, compaction_mode, compaction_keep_last, tool_result_persist, max_tool_output_bytes, max_read_bytes, stream, allow_shell, allow_write, enable_write_tools, allow_shell_in_workdir"
            ));
        }
    }

    Ok(params_settings_summary(run))
}
