use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::diagnostics::{Diagnostic, Severity, DIAGNOSTIC_SCHEMA_VERSION};
use crate::lsp_context::SymbolLocation;
use crate::types::SideEffects;

use super::exec_support::{base_meta, failed_exec, path_is_workdir_scoped, ToolExecution};
use super::{LspToolProvider, ToolErrorCode, ToolErrorDetail, ToolRuntime};

const DEFAULT_TYPESCRIPT_LSP_COMMAND: &str = "typescript-language-server";
const DEFAULT_RUST_LSP_COMMAND: &str = "rust-analyzer";
const INITIALIZE_TIMEOUT_MS: u64 = 2_000;
const RESPONSE_TIMEOUT_MS: u64 = 2_000;
const DIAGNOSTICS_TOTAL_TIMEOUT_MS: u64 = 3_000;
const DIAGNOSTICS_IDLE_AFTER_FIRST_MS: u64 = 500;
const MAX_OPEN_FILES: usize = 12;

pub(super) async fn run_lsp_tool(rt: &ToolRuntime, tool_name: &str, args: &Value) -> ToolExecution {
    let Some(config) = rt.lsp.clone() else {
        return failed_exec(
            rt,
            SideEffects::FilesystemRead,
            "LSP tools are disabled. Re-run with --lsp-provider rust or --lsp-provider typescript."
                .to_string(),
            Some(ToolErrorDetail {
                code: ToolErrorCode::ToolDisabled,
                message: "LSP tools are disabled for this run.".to_string(),
                expected_schema: super::compact_builtin_schema(tool_name),
                received_args: Some(args.clone()),
                minimal_example: super::minimal_builtin_example(tool_name),
                available_tools: None,
            }),
        );
    };

    match run_lsp_tool_inner(rt, &config, tool_name, args) {
        Ok(content) => ok_json(rt, content),
        Err(err) => failed_exec(
            rt,
            SideEffects::FilesystemRead,
            format!("lsp request failed: {err}"),
            Some(ToolErrorDetail {
                code: ToolErrorCode::LspRequestFailed,
                message: err.to_string(),
                expected_schema: super::compact_builtin_schema(tool_name),
                received_args: Some(args.clone()),
                minimal_example: super::minimal_builtin_example(tool_name),
                available_tools: None,
            }),
        ),
    }
}

fn run_lsp_tool_inner(
    rt: &ToolRuntime,
    config: &super::LspToolRuntime,
    tool_name: &str,
    args: &Value,
) -> Result<Value> {
    let mut session = LspSession::start(config, &rt.workdir)?;
    match tool_name {
        "lsp.diagnostics" => {
            let files = if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                vec![scoped_source_file(rt, config.provider, path)?]
            } else {
                discover_source_files(&rt.workdir, config.provider, MAX_OPEN_FILES)?
            };
            for file in &files {
                session.open_document(file, config.provider)?;
            }
            let diagnostics = session.collect_diagnostics()?;
            let max_results = max_results(args, 100);
            let diagnostic_count = diagnostics.len();
            let truncated = diagnostic_count > max_results;
            Ok(json!({
                "provider": provider_name(config.provider),
                "diagnostics": truncate_values(diagnostics, max_results),
                "diagnostic_count": diagnostic_count,
                "truncated": truncated,
                "max_results": max_results
            }))
        }
        "lsp.document_symbols" => {
            let path = required_path(args)?;
            let file = scoped_source_file(rt, config.provider, path)?;
            session.open_document(&file, config.provider)?;
            let symbols = session.document_symbols(&file)?;
            let max_results = max_results(args, 100);
            let symbol_count = symbols.len();
            let truncated = symbol_count > max_results;
            Ok(json!({
                "provider": provider_name(config.provider),
                "path": path,
                "symbols": truncate_values(symbols, max_results),
                "symbol_count": symbol_count,
                "truncated": truncated,
                "max_results": max_results
            }))
        }
        "lsp.goto_definition" => {
            let path = required_path(args)?;
            let file = scoped_source_file(rt, config.provider, path)?;
            session.open_document(&file, config.provider)?;
            let locations = session.location_request(
                "textDocument/definition",
                &file,
                required_zero_based_position(args)?,
                None,
            )?;
            let max_results = max_results(args, 20);
            let definition_count = locations.len();
            let truncated = definition_count > max_results;
            Ok(json!({
                "provider": provider_name(config.provider),
                "path": path,
                "definitions": truncate_values(locations, max_results),
                "definition_count": definition_count,
                "truncated": truncated,
                "max_results": max_results
            }))
        }
        "lsp.find_references" => {
            let path = required_path(args)?;
            let file = scoped_source_file(rt, config.provider, path)?;
            session.open_document(&file, config.provider)?;
            let locations = session.location_request(
                "textDocument/references",
                &file,
                required_zero_based_position(args)?,
                Some(json!({
                    "context": {
                        "includeDeclaration": args
                            .get("include_declaration")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(true)
                    }
                })),
            )?;
            let max_results = max_results(args, 100);
            let reference_count = locations.len();
            let truncated = reference_count > max_results;
            Ok(json!({
                "provider": provider_name(config.provider),
                "path": path,
                "references": truncate_values(locations, max_results),
                "reference_count": reference_count,
                "truncated": truncated,
                "max_results": max_results
            }))
        }
        "lsp.hover" => {
            let path = required_path(args)?;
            let file = scoped_source_file(rt, config.provider, path)?;
            session.open_document(&file, config.provider)?;
            let hover = session.hover(&file, required_zero_based_position(args)?)?;
            Ok(json!({
                "provider": provider_name(config.provider),
                "path": path,
                "hover": hover
            }))
        }
        _ => Err(anyhow!("unknown lsp tool: {tool_name}")),
    }
}

fn ok_json(rt: &ToolRuntime, value: Value) -> ToolExecution {
    let mut content = value.to_string();
    let mut truncated = false;
    if rt.max_tool_output_bytes > 0 && content.len() > rt.max_tool_output_bytes {
        content.truncate(rt.max_tool_output_bytes);
        truncated = true;
    }
    ToolExecution {
        ok: true,
        content,
        truncated,
        error: None,
        meta: base_meta(rt, SideEffects::FilesystemRead),
    }
}

struct LspSession {
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<Result<Value>>,
    next_id: i64,
}

impl LspSession {
    fn start(config: &super::LspToolRuntime, workdir: &Path) -> Result<Self> {
        let command = resolve_spawn_command(
            config
                .command
                .as_deref()
                .unwrap_or_else(|| Path::new(default_command(config.provider))),
        );
        let mut cmd = Command::new(&command);
        if matches!(config.provider, LspToolProvider::Typescript) {
            cmd.arg("--stdio");
        }
        let mut child = cmd
            .current_dir(workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed spawning LSP command {}", command.display()))?;
        let mut stdin = child.stdin.take().context("lsp stdin missing")?;
        let stdout = child.stdout.take().context("lsp stdout missing")?;
        let (tx, rx) = mpsc::channel::<Result<Value>>();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_lsp_message(&mut reader) {
                    Ok(Some(value)) => {
                        if tx.send(Ok(value)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(err) => {
                        let _ = tx.send(Err(err));
                        break;
                    }
                }
            }
        });

        write_lsp_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": null,
                    "rootUri": path_to_file_uri(workdir),
                    "capabilities": {}
                }
            }),
        )?;
        wait_for_response_id(&rx, 1, INITIALIZE_TIMEOUT_MS)?;
        write_lsp_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": "initialized",
                "params": {}
            }),
        )?;

        Ok(Self {
            child,
            stdin,
            rx,
            next_id: 2,
        })
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        write_lsp_message(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params
            }),
        )?;
        wait_for_response_id(&self.rx, id, RESPONSE_TIMEOUT_MS)
    }

    fn open_document(&mut self, path: &Path, provider: LspToolProvider) -> Result<()> {
        let language_id = language_id_for_path(path, provider)
            .ok_or_else(|| anyhow!("unsupported source file for provider: {}", path.display()))?;
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed reading source file {}", path.display()))?;
        write_lsp_message(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": {
                    "textDocument": {
                        "uri": path_to_file_uri(path),
                        "languageId": language_id,
                        "version": 1,
                        "text": text
                    }
                }
            }),
        )
    }

    fn collect_diagnostics(&mut self) -> Result<Vec<Value>> {
        let started = std::time::Instant::now();
        let mut last_diagnostics_at: Option<std::time::Instant> = None;
        let mut mapped = Vec::<Value>::new();
        loop {
            let elapsed = started.elapsed();
            if elapsed >= Duration::from_millis(DIAGNOSTICS_TOTAL_TIMEOUT_MS) {
                break;
            }
            let timeout = if let Some(last_at) = last_diagnostics_at {
                let idle_elapsed = last_at.elapsed();
                if idle_elapsed >= Duration::from_millis(DIAGNOSTICS_IDLE_AFTER_FIRST_MS) {
                    break;
                }
                let idle_remaining = Duration::from_millis(DIAGNOSTICS_IDLE_AFTER_FIRST_MS)
                    .saturating_sub(idle_elapsed);
                let total_remaining =
                    Duration::from_millis(DIAGNOSTICS_TOTAL_TIMEOUT_MS).saturating_sub(elapsed);
                std::cmp::min(idle_remaining, total_remaining)
            } else {
                Duration::from_millis(DIAGNOSTICS_TOTAL_TIMEOUT_MS).saturating_sub(elapsed)
            };
            match self.rx.recv_timeout(timeout) {
                Ok(Ok(value)) => {
                    if let Some(diags) = maybe_map_publish_diagnostics(&value) {
                        if !diags.is_empty() {
                            last_diagnostics_at = Some(std::time::Instant::now());
                        }
                        mapped.extend(
                            diags
                                .into_iter()
                                .filter_map(|d| serde_json::to_value(d).ok()),
                        );
                    }
                }
                Ok(Err(err)) => return Err(err),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        Ok(mapped)
    }

    fn document_symbols(&mut self, path: &Path) -> Result<Vec<Value>> {
        let result = self.request(
            "textDocument/documentSymbol",
            json!({
                "textDocument": {
                    "uri": path_to_file_uri(path)
                }
            }),
        )?;
        Ok(map_document_symbols(path, &result)
            .into_iter()
            .filter_map(|s| serde_json::to_value(s).ok())
            .collect())
    }

    fn location_request(
        &mut self,
        method: &str,
        path: &Path,
        position: (u32, u32),
        extra: Option<Value>,
    ) -> Result<Vec<Value>> {
        let mut params = json!({
            "textDocument": { "uri": path_to_file_uri(path) },
            "position": { "line": position.0, "character": position.1 }
        });
        if let (Some(obj), Some(extra_obj)) = (
            params.as_object_mut(),
            extra.and_then(|v| v.as_object().cloned()),
        ) {
            for (key, value) in extra_obj {
                obj.insert(key, value);
            }
        }
        let result = self.request(method, params)?;
        Ok(map_location_response(&result)
            .into_iter()
            .filter_map(|s| serde_json::to_value(s).ok())
            .collect())
    }

    fn hover(&mut self, path: &Path, position: (u32, u32)) -> Result<Value> {
        let result = self.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": path_to_file_uri(path) },
                "position": { "line": position.0, "character": position.1 }
            }),
        )?;
        Ok(json!({
            "contents": hover_contents_to_text(result.get("contents").unwrap_or(&Value::Null)),
            "raw": result
        }))
    }
}

impl Drop for LspSession {
    fn drop(&mut self) {
        let id = self.next_id;
        let _ = write_lsp_message(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "shutdown",
                "params": null
            }),
        );
        let _ = write_lsp_message(
            &mut self.stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": "exit",
                "params": null
            }),
        );
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn required_path(args: &Value) -> Result<&str> {
    args.get("path")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("path is required"))
}

fn required_zero_based_position(args: &Value) -> Result<(u32, u32)> {
    let line = args
        .get("line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow!("line is required"))?;
    let column = args
        .get("column")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow!("column is required"))?;
    Ok((
        (line as u32).saturating_sub(1),
        (column as u32).saturating_sub(1),
    ))
}

fn max_results(args: &Value, default: usize) -> usize {
    args.get("max_results")
        .and_then(|v| v.as_u64())
        .map(|v| v.clamp(1, 1000) as usize)
        .unwrap_or(default)
}

fn truncate_values(mut values: Vec<Value>, max: usize) -> Vec<Value> {
    if values.len() > max {
        values.truncate(max);
    }
    values
}

fn scoped_source_file(rt: &ToolRuntime, provider: LspToolProvider, path: &str) -> Result<PathBuf> {
    if !path_is_workdir_scoped(path) && !rt.unsafe_bypass_allow_flags {
        return Err(anyhow!(
            "path must stay within workdir (no absolute paths or '..' traversal)"
        ));
    }
    let file = rt.workdir.join(path);
    if !file.exists() {
        return Err(anyhow!("path does not exist: {}", file.display()));
    }
    if language_id_for_path(&file, provider).is_none() {
        return Err(anyhow!(
            "unsupported source file for {} provider: {}",
            provider_name(provider),
            path
        ));
    }
    Ok(file)
}

fn discover_source_files(
    root: &Path,
    provider: LspToolProvider,
    max_files: usize,
) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if found.len() >= max_files {
            break;
        }
        let mut entries = fs::read_dir(&dir)
            .with_context(|| format!("failed reading directory {}", dir.display()))?
            .filter_map(|entry| entry.ok())
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if matches!(
                    name.as_str(),
                    ".git" | ".localagent" | "node_modules" | "target"
                ) {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if language_id_for_path(&path, provider).is_some() {
                found.push(path);
                if found.len() >= max_files {
                    break;
                }
            }
        }
    }
    found.sort();
    Ok(found)
}

fn provider_name(provider: LspToolProvider) -> &'static str {
    match provider {
        LspToolProvider::Rust => "rust_analyzer",
        LspToolProvider::Typescript => "typescript_language_server",
    }
}

fn default_command(provider: LspToolProvider) -> &'static str {
    match provider {
        LspToolProvider::Rust => DEFAULT_RUST_LSP_COMMAND,
        LspToolProvider::Typescript => DEFAULT_TYPESCRIPT_LSP_COMMAND,
    }
}

fn language_id_for_path(path: &Path, provider: LspToolProvider) -> Option<&'static str> {
    match provider {
        LspToolProvider::Rust => match path.extension().and_then(|ext| ext.to_str()) {
            Some("rs") => Some("rust"),
            _ => None,
        },
        LspToolProvider::Typescript => match path.extension().and_then(|ext| ext.to_str()) {
            Some("ts") => Some("typescript"),
            Some("tsx") => Some("typescriptreact"),
            Some("js") | Some("mjs") | Some("cjs") => Some("javascript"),
            Some("jsx") => Some("javascriptreact"),
            _ => None,
        },
    }
}

fn resolve_spawn_command(command: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        if command.extension().is_some() {
            return command.to_path_buf();
        }
        if command.components().count() > 1 {
            let cmd = command.with_extension("cmd");
            if cmd.exists() {
                return cmd;
            }
            return command.to_path_buf();
        }
        if let Some(path_os) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path_os) {
                for ext in ["cmd", "exe", "bat"] {
                    let candidate = dir.join(command).with_extension(ext);
                    if candidate.exists() {
                        return candidate;
                    }
                }
                let candidate = dir.join(command);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }
    command.to_path_buf()
}

fn wait_for_response_id(
    rx: &mpsc::Receiver<Result<Value>>,
    expected_id: i64,
    timeout_ms: u64,
) -> Result<Value> {
    let deadline = Duration::from_millis(timeout_ms);
    let started = std::time::Instant::now();
    loop {
        let remaining = deadline
            .checked_sub(started.elapsed())
            .unwrap_or(Duration::from_millis(0));
        match rx.recv_timeout(remaining) {
            Ok(Ok(value)) => {
                if value.get("id").and_then(|id| id.as_i64()) == Some(expected_id) {
                    if let Some(error) = value.get("error") {
                        return Err(anyhow!("lsp response returned error: {}", error));
                    }
                    return Ok(value.get("result").cloned().unwrap_or(Value::Null));
                }
            }
            Ok(Err(err)) => return Err(err),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(anyhow!("timed out waiting for lsp response {expected_id}"))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow!(
                    "language server exited before response {expected_id}"
                ))
            }
        }
    }
}

fn write_lsp_message<W: Write>(writer: &mut W, value: &Value) -> Result<()> {
    let body = serde_json::to_vec(value).context("failed serializing lsp request")?;
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .context("failed writing lsp header")?;
    writer.write_all(&body).context("failed writing lsp body")?;
    writer.flush().context("failed flushing lsp message")?;
    Ok(())
}

fn read_lsp_message<R: BufRead + Read>(reader: &mut R) -> Result<Option<Value>> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .context("failed reading lsp header line")?;
        if read == 0 {
            return Ok(None);
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .context("invalid content length")?,
            );
        }
    }
    let len = content_length.context("missing Content-Length header")?;
    let mut buf = vec![0_u8; len];
    reader
        .read_exact(&mut buf)
        .context("failed reading lsp body")?;
    Ok(Some(
        serde_json::from_slice(&buf).context("invalid lsp json body")?,
    ))
}

fn path_to_file_uri(path: &Path) -> String {
    let absolute = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let rendered = absolute.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    let rendered = rendered
        .strip_prefix("//?/")
        .unwrap_or(rendered.as_str())
        .to_string();
    if rendered.starts_with('/') {
        format!("file://{rendered}")
    } else {
        format!("file:///{rendered}")
    }
}

fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let trimmed = uri
        .strip_prefix("file:///")
        .or_else(|| uri.strip_prefix("file://"))?;
    let normalized = if trimmed.len() >= 3 && trimmed.as_bytes().get(1) == Some(&b':') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };
    Some(PathBuf::from(
        normalized.replace('/', std::path::MAIN_SEPARATOR_STR),
    ))
}

fn maybe_map_publish_diagnostics(value: &Value) -> Option<Vec<Diagnostic>> {
    let method = value.get("method")?.as_str()?;
    if method != "textDocument/publishDiagnostics" {
        return None;
    }
    let params = value.get("params")?.clone();
    let payload: PublishDiagnosticsParams = serde_json::from_value(params).ok()?;
    let path = file_uri_to_path(&payload.uri);
    let mut mapped = Vec::new();
    for item in payload.diagnostics {
        mapped.push(Diagnostic {
            schema_version: DIAGNOSTIC_SCHEMA_VERSION.to_string(),
            code: diagnostic_code_to_string(item.code),
            severity: map_severity(item.severity),
            message: item.message,
            path: path.clone(),
            line: item.range.start.line.checked_add(1),
            col: item.range.start.character.checked_add(1),
            hint: item.source.clone(),
            details: Some(json!({
                "source": item.source,
                "range": {
                    "start": {
                        "line": item.range.start.line,
                        "character": item.range.start.character
                    },
                    "end": {
                        "line": item.range.end.line,
                        "character": item.range.end.character
                    }
                }
            })),
        });
    }
    Some(mapped)
}

fn diagnostic_code_to_string(code: Option<Value>) -> String {
    match code {
        Some(Value::String(s)) => s,
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => other.to_string(),
        None => "lsp_diagnostic".to_string(),
    }
}

fn map_severity(severity: Option<u32>) -> Severity {
    match severity.unwrap_or(1) {
        1 => Severity::Error,
        2 => Severity::Warning,
        _ => Severity::Info,
    }
}

fn map_document_symbols(path: &Path, result: &Value) -> Vec<SymbolLocation> {
    let mut out = Vec::new();
    if let Value::Array(items) = result {
        for item in items {
            if item.get("selectionRange").is_some() {
                collect_document_symbol_tree(path, item, &mut out);
            } else if let Some(symbol) = map_symbol_information(item) {
                out.push(symbol);
            }
        }
    }
    sort_symbols(&mut out);
    out
}

fn collect_document_symbol_tree(path: &Path, item: &Value, out: &mut Vec<SymbolLocation>) {
    if let Some(symbol) = map_document_symbol(path, item) {
        out.push(symbol);
    }
    if let Some(children) = item.get("children").and_then(|c| c.as_array()) {
        for child in children {
            collect_document_symbol_tree(path, child, out);
        }
    }
}

fn map_document_symbol(path: &Path, item: &Value) -> Option<SymbolLocation> {
    let label = item.get("name")?.as_str()?.to_string();
    let selection_range = item.get("selectionRange").or_else(|| item.get("range"))?;
    let start = selection_range.get("start")?;
    Some(SymbolLocation {
        path: path.to_path_buf(),
        line: start
            .get("line")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32 + 1),
        col: start
            .get("character")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32 + 1),
        label,
    })
}

fn map_symbol_information(item: &Value) -> Option<SymbolLocation> {
    let label = item.get("name")?.as_str()?.to_string();
    let location = item.get("location")?;
    let uri = location.get("uri")?.as_str()?;
    let path = file_uri_to_path(uri)?;
    let start = location.get("range")?.get("start")?;
    Some(SymbolLocation {
        path,
        line: start
            .get("line")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32 + 1),
        col: start
            .get("character")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32 + 1),
        label,
    })
}

fn map_location_response(result: &Value) -> Vec<SymbolLocation> {
    let mut out = match result {
        Value::Array(items) => items.iter().filter_map(map_location_like).collect(),
        Value::Object(_) => map_location_like(result).into_iter().collect(),
        _ => Vec::new(),
    };
    sort_symbols(&mut out);
    out
}

fn map_location_like(value: &Value) -> Option<SymbolLocation> {
    if let Some(uri) = value.get("uri").and_then(|v| v.as_str()) {
        let path = file_uri_to_path(uri)?;
        let start = value.get("range")?.get("start")?;
        return Some(SymbolLocation {
            path,
            line: start
                .get("line")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32 + 1),
            col: start
                .get("character")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32 + 1),
            label: "location".to_string(),
        });
    }
    if let Some(uri) = value.get("targetUri").and_then(|v| v.as_str()) {
        let path = file_uri_to_path(uri)?;
        let start = value.get("targetRange")?.get("start")?;
        return Some(SymbolLocation {
            path,
            line: start
                .get("line")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32 + 1),
            col: start
                .get("character")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32 + 1),
            label: "location".to_string(),
        });
    }
    None
}

fn sort_symbols(items: &mut [SymbolLocation]) {
    items.sort_by(|a, b| {
        (
            a.path.to_string_lossy().to_string(),
            a.line.unwrap_or(0),
            a.col.unwrap_or(0),
            a.label.clone(),
        )
            .cmp(&(
                b.path.to_string_lossy().to_string(),
                b.line.unwrap_or(0),
                b.col.unwrap_or(0),
                b.label.clone(),
            ))
    });
}

fn hover_contents_to_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(hover_contents_to_text)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(obj) => obj
            .get("value")
            .or_else(|| obj.get("contents"))
            .map(hover_contents_to_text)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

#[derive(Debug, Deserialize)]
struct PublishDiagnosticsParams {
    uri: String,
    diagnostics: Vec<LspDiagnostic>,
}

#[derive(Debug, Deserialize)]
struct LspDiagnostic {
    range: LspRange,
    severity: Option<u32>,
    code: Option<Value>,
    source: Option<String>,
    message: String,
}

#[derive(Debug, Deserialize)]
struct LspRange {
    start: LspPosition,
    end: LspPosition,
}

#[derive(Debug, Deserialize)]
struct LspPosition {
    line: u32,
    character: u32,
}
