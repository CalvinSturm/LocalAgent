use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::diagnostics::{Diagnostic, Severity, DIAGNOSTIC_SCHEMA_VERSION};
use crate::lsp_context::{
    build_diagnostics_snapshot_for_provider, build_symbol_context_for_provider,
    DiagnosticsSnapshot, LspContextLimits, LspContextProvider, SymbolContext, SymbolLocation,
};

const TYPESCRIPT_LSP_PROVIDER_NAME: &str = "typescript_language_server";
const DEFAULT_TYPESCRIPT_LSP_COMMAND: &str = "typescript-language-server";
const DEFAULT_TYPESCRIPT_TSC_COMMAND: &str = "tsc";
const INITIALIZE_TIMEOUT_MS: u64 = 2_000;
const DIAGNOSTICS_TOTAL_TIMEOUT_MS: u64 = 3_000;
const DIAGNOSTICS_IDLE_AFTER_FIRST_MS: u64 = 500;
const MAX_TYPESCRIPT_OPEN_FILES: usize = 12;

#[derive(Debug, Clone)]
pub struct TypescriptLspContextProvider {
    command: PathBuf,
}

impl TypescriptLspContextProvider {
    pub fn new(command_override: Option<PathBuf>) -> Self {
        Self {
            command: command_override
                .unwrap_or_else(|| PathBuf::from(DEFAULT_TYPESCRIPT_LSP_COMMAND)),
        }
    }

    fn collect_diagnostics(
        &self,
        workdir: &Path,
        limits: LspContextLimits,
    ) -> Result<Option<DiagnosticsSnapshot>> {
        let files = discover_typescript_files(workdir, MAX_TYPESCRIPT_OPEN_FILES)?;
        if files.is_empty() {
            return Ok(None);
        }

        let mut child = Command::new(resolve_spawn_command(&self.command))
            .arg("--stdio")
            .current_dir(workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed spawning {}", self.command.display()))?;

        let mut stdin = child
            .stdin
            .take()
            .context("typescript language server stdin missing")?;
        let stdout = child
            .stdout
            .take()
            .context("typescript language server stdout missing")?;

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

        let root_uri = path_to_file_uri(workdir);
        let initialize = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {}
            }
        });
        write_lsp_message(&mut stdin, &initialize)?;
        wait_for_initialize_response(&rx)?;

        let initialized = json!({
            "jsonrpc": "2.0",
            "method": "initialized",
            "params": {}
        });
        write_lsp_message(&mut stdin, &initialized)?;

        for file in &files {
            if let Some(language_id) = language_id_for_path(file) {
                let text = fs::read_to_string(file).unwrap_or_default();
                let uri = path_to_file_uri(file);
                let did_open = json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/didOpen",
                    "params": {
                        "textDocument": {
                            "uri": uri.clone(),
                            "languageId": language_id,
                            "version": 1,
                            "text": text
                        }
                    }
                });
                write_lsp_message(&mut stdin, &did_open)?;
                let did_save = json!({
                    "jsonrpc": "2.0",
                    "method": "textDocument/didSave",
                    "params": {
                        "textDocument": {
                            "uri": uri
                        }
                    }
                });
                write_lsp_message(&mut stdin, &did_save)?;
            }
        }

        let mut mapped = Vec::new();
        let started = std::time::Instant::now();
        let mut last_diagnostics_at: Option<std::time::Instant> = None;
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

            match rx.recv_timeout(timeout) {
                Ok(Ok(value)) => {
                    if let Some(diags) = maybe_map_publish_diagnostics(&value) {
                        if !diags.is_empty() {
                            last_diagnostics_at = Some(std::time::Instant::now());
                        }
                        mapped.extend(diags);
                    }
                }
                Ok(Err(_)) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        let shutdown = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown",
            "params": null
        });
        let _ = write_lsp_message(&mut stdin, &shutdown);
        let exit = json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        });
        let _ = write_lsp_message(&mut stdin, &exit);
        let _ = child.kill();
        let _ = child.wait();

        if mapped.is_empty() {
            mapped = collect_tsc_diagnostics(workdir, &files).unwrap_or_default();
        }

        if mapped.is_empty() {
            return Ok(None);
        }

        Ok(Some(build_diagnostics_snapshot_for_provider(
            workdir.to_path_buf(),
            Some("typescript".to_string()),
            mapped,
            limits,
        )))
    }

    fn collect_symbol_context(
        &self,
        workdir: &Path,
        limits: LspContextLimits,
    ) -> Result<Option<SymbolContext>> {
        let files = discover_typescript_files(workdir, MAX_TYPESCRIPT_OPEN_FILES)?;
        let primary_file = match files.first() {
            Some(file) => file.clone(),
            None => return Ok(None),
        };

        let mut child = Command::new(resolve_spawn_command(&self.command))
            .arg("--stdio")
            .current_dir(workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed spawning {}", self.command.display()))?;

        let mut stdin = child
            .stdin
            .take()
            .context("typescript language server stdin missing")?;
        let stdout = child
            .stdout
            .take()
            .context("typescript language server stdout missing")?;

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

        let root_uri = path_to_file_uri(workdir);
        write_lsp_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "processId": null,
                    "rootUri": root_uri,
                    "capabilities": {}
                }
            }),
        )?;
        wait_for_initialize_response(&rx)?;
        write_lsp_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "method": "initialized",
                "params": {}
            }),
        )?;

        for file in &files {
            if let Some(language_id) = language_id_for_path(file) {
                let text = fs::read_to_string(file).unwrap_or_default();
                write_lsp_message(
                    &mut stdin,
                    &json!({
                        "jsonrpc": "2.0",
                        "method": "textDocument/didOpen",
                        "params": {
                            "textDocument": {
                                "uri": path_to_file_uri(file),
                                "languageId": language_id,
                                "version": 1,
                                "text": text
                            }
                        }
                    }),
                )?;
            }
        }

        write_lsp_message(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "textDocument/documentSymbol",
                "params": {
                    "textDocument": {
                        "uri": path_to_file_uri(&primary_file)
                    }
                }
            }),
        )?;
        let symbol_response = wait_for_response(&rx, 3)?;
        let symbols = map_document_symbols(&primary_file, &symbol_response);
        let query = symbols.first().map(|s| s.label.clone()).unwrap_or_else(|| {
            primary_file
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("typescript_symbol_context")
                .to_string()
        });

        let mut definitions = Vec::new();
        let mut references = Vec::new();
        if let Some(anchor) = symbols.first() {
            let line = anchor.line.unwrap_or(1).saturating_sub(1);
            let col = anchor.col.unwrap_or(1).saturating_sub(1);
            let uri = path_to_file_uri(&anchor.path);

            write_lsp_message(
                &mut stdin,
                &json!({
                    "jsonrpc": "2.0",
                    "id": 4,
                    "method": "textDocument/definition",
                    "params": {
                        "textDocument": { "uri": uri.clone() },
                        "position": { "line": line, "character": col }
                    }
                }),
            )?;
            let definition_response = wait_for_response(&rx, 4)?;
            definitions = map_location_response(&definition_response);

            write_lsp_message(
                &mut stdin,
                &json!({
                    "jsonrpc": "2.0",
                    "id": 5,
                    "method": "textDocument/references",
                    "params": {
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": col },
                        "context": { "includeDeclaration": true }
                    }
                }),
            )?;
            let references_response = wait_for_response(&rx, 5)?;
            references = map_location_response(&references_response);
        }

        let shutdown = json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "shutdown",
            "params": null
        });
        let _ = write_lsp_message(&mut stdin, &shutdown);
        let exit = json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        });
        let _ = write_lsp_message(&mut stdin, &exit);
        let _ = child.kill();
        let _ = child.wait();

        if symbols.is_empty() && definitions.is_empty() && references.is_empty() {
            return Ok(None);
        }

        Ok(Some(build_symbol_context_for_provider(
            workdir.to_path_buf(),
            query,
            symbols,
            definitions,
            references,
            limits,
        )))
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
                let candidate_cmd = dir.join(command).with_extension("cmd");
                if candidate_cmd.exists() {
                    return candidate_cmd;
                }
                let candidate_exe = dir.join(command).with_extension("exe");
                if candidate_exe.exists() {
                    return candidate_exe;
                }
                let candidate_bat = dir.join(command).with_extension("bat");
                if candidate_bat.exists() {
                    return candidate_bat;
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

fn collect_tsc_diagnostics(workdir: &Path, files: &[PathBuf]) -> Result<Vec<Diagnostic>> {
    let command = resolve_spawn_command(Path::new(DEFAULT_TYPESCRIPT_TSC_COMMAND));
    let mut cmd = Command::new(command);
    cmd.current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let project = resolve_typescript_project_config(workdir);
    if let Some(config) = project {
        cmd.arg("--noEmit")
            .arg("--pretty")
            .arg("false")
            .arg("--project")
            .arg(config);
    } else {
        cmd.arg("--noEmit")
            .arg("--pretty")
            .arg("false")
            .arg("--allowJs")
            .arg("--checkJs");
        for file in files {
            cmd.arg(file);
        }
    }

    let output = cmd
        .output()
        .context("failed running tsc diagnostics fallback")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut mapped = Vec::new();
    for line in stdout.lines().chain(stderr.lines()) {
        if let Some(diag) = parse_tsc_diagnostic_line(line) {
            mapped.push(diag);
        }
    }
    Ok(mapped)
}

fn resolve_typescript_project_config(workdir: &Path) -> Option<PathBuf> {
    for name in ["tsconfig.json", "jsconfig.json"] {
        let candidate = workdir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

impl LspContextProvider for TypescriptLspContextProvider {
    fn provider_name(&self) -> &'static str {
        TYPESCRIPT_LSP_PROVIDER_NAME
    }

    fn diagnostics_snapshot(
        &self,
        workdir: &Path,
        limits: LspContextLimits,
    ) -> Result<Option<DiagnosticsSnapshot>> {
        self.collect_diagnostics(workdir, limits)
    }

    fn symbol_context(
        &self,
        workdir: &Path,
        limits: LspContextLimits,
    ) -> Result<Option<SymbolContext>> {
        self.collect_symbol_context(workdir, limits)
    }
}

fn discover_typescript_files(root: &Path, max_files: usize) -> Result<Vec<PathBuf>> {
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
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if matches!(
                    name.as_ref(),
                    ".git" | ".localagent" | "node_modules" | "target"
                ) {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if language_id_for_path(&path).is_some() {
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

fn language_id_for_path(path: &Path) -> Option<&'static str> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("ts") => Some("typescript"),
        Some("tsx") => Some("typescriptreact"),
        Some("js") => Some("javascript"),
        Some("jsx") => Some("javascriptreact"),
        Some("mjs") | Some("cjs") => Some("javascript"),
        _ => None,
    }
}

fn wait_for_initialize_response(rx: &mpsc::Receiver<Result<Value>>) -> Result<()> {
    let deadline = Duration::from_millis(INITIALIZE_TIMEOUT_MS);
    let started = std::time::Instant::now();
    loop {
        let remaining = deadline
            .checked_sub(started.elapsed())
            .unwrap_or(Duration::from_millis(0));
        match rx.recv_timeout(remaining) {
            Ok(Ok(value)) => {
                if value.get("id").and_then(|id| id.as_i64()) == Some(1) {
                    if let Some(error) = value.get("error") {
                        return Err(anyhow!("initialize response returned error: {}", error));
                    }
                    return Ok(());
                }
            }
            Ok(Err(err)) => return Err(err),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(anyhow!("timed out waiting for initialize response"))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow!(
                    "typescript language server exited before initialize response"
                ))
            }
        }
    }
}

fn wait_for_response(rx: &mpsc::Receiver<Result<Value>>, expected_id: i64) -> Result<Value> {
    let deadline = Duration::from_millis(INITIALIZE_TIMEOUT_MS);
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
                return Err(anyhow!(
                    "timed out waiting for lsp response {}",
                    expected_id
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow!("typescript language server exited before response"))
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
            let len = value
                .trim()
                .parse::<usize>()
                .context("invalid content length")?;
            content_length = Some(len);
        }
    }
    let len = content_length.context("missing Content-Length header")?;
    let mut buf = vec![0_u8; len];
    reader
        .read_exact(&mut buf)
        .context("failed reading lsp body")?;
    let value = serde_json::from_slice(&buf).context("invalid lsp json body")?;
    Ok(Some(value))
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

fn parse_tsc_diagnostic_line(line: &str) -> Option<Diagnostic> {
    let (left, right) = line.split_once("): error TS")?;
    let open = left.rfind('(')?;
    let path = &left[..open];
    let coords = &left[open + 1..];
    let (line_str, col_str) = coords.split_once(',')?;
    let (code, message) = right.split_once(": ")?;
    Some(Diagnostic {
        schema_version: DIAGNOSTIC_SCHEMA_VERSION.to_string(),
        code: format!("TS{}", code),
        severity: Severity::Error,
        message: message.to_string(),
        path: Some(PathBuf::from(path)),
        line: line_str.parse::<u32>().ok(),
        col: col_str.parse::<u32>().ok(),
        hint: Some("tsc".to_string()),
        details: None,
    })
}

fn diagnostic_code_to_string(code: Option<Value>) -> String {
    match code {
        Some(Value::String(s)) => s,
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => other.to_string(),
        None => "typescript_diagnostic".to_string(),
    }
}

fn map_severity(severity: Option<u32>) -> Severity {
    match severity.unwrap_or(1) {
        1 => Severity::Error,
        2 => Severity::Warning,
        _ => Severity::Info,
    }
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

fn map_document_symbols(path: &Path, result: &Value) -> Vec<SymbolLocation> {
    let mut out = Vec::new();
    match result {
        Value::Array(items) => {
            for item in items {
                if item.get("selectionRange").is_some() {
                    collect_document_symbol_tree(path, item, &mut out);
                } else if let Some(symbol) = map_symbol_information(item) {
                    out.push(symbol);
                }
            }
        }
        Value::Null => {}
        _ => {}
    }
    out.sort_by(|a, b| {
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
    match result {
        Value::Array(items) => items.iter().filter_map(map_location_like).collect(),
        Value::Object(_) => map_location_like(result).into_iter().collect(),
        _ => Vec::new(),
    }
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

#[cfg(test)]
pub(crate) fn parse_lsp_publish_diagnostics(value: &Value) -> Option<Vec<Diagnostic>> {
    maybe_map_publish_diagnostics(value)
}

#[cfg(test)]
pub(crate) fn resolve_typescript_file_discovery(
    root: &Path,
    max_files: usize,
) -> Result<Vec<PathBuf>> {
    discover_typescript_files(root, max_files)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::{
        parse_lsp_publish_diagnostics, parse_tsc_diagnostic_line, path_to_file_uri,
        resolve_typescript_file_discovery,
    };

    #[test]
    fn maps_publish_diagnostics_into_localagent_schema() {
        let value = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": "file:///repo/src/index.ts",
                "diagnostics": [
                    {
                        "range": {
                            "start": { "line": 2, "character": 4 },
                            "end": { "line": 2, "character": 10 }
                        },
                        "severity": 1,
                        "code": 2322,
                        "source": "ts",
                        "message": "Type 'string' is not assignable to type 'number'."
                    }
                ]
            }
        });
        let mapped = parse_lsp_publish_diagnostics(&value).expect("mapped");
        assert_eq!(mapped.len(), 1);
        let first = &mapped[0];
        assert_eq!(first.code, "2322");
        assert_eq!(first.line, Some(3));
        assert_eq!(first.col, Some(5));
        assert_eq!(first.hint.as_deref(), Some("ts"));
    }

    #[test]
    fn discovers_typescript_and_javascript_files_deterministically() {
        let tmp = tempdir().expect("tmp");
        fs::create_dir_all(tmp.path().join("src")).expect("mkdir src");
        fs::create_dir_all(tmp.path().join("node_modules")).expect("mkdir node_modules");
        fs::write(tmp.path().join("src").join("b.ts"), "const b = 1;").expect("write b");
        fs::write(tmp.path().join("src").join("a.js"), "const a = 1;").expect("write a");
        fs::write(
            tmp.path().join("node_modules").join("ignored.ts"),
            "const ignored = 1;",
        )
        .expect("write ignored");
        let found = resolve_typescript_file_discovery(tmp.path(), 10).expect("discover");
        let rendered = found
            .iter()
            .map(|p| p.strip_prefix(tmp.path()).expect("relative").to_path_buf())
            .collect::<Vec<_>>();
        assert_eq!(
            rendered,
            vec![PathBuf::from("src/a.js"), PathBuf::from("src/b.ts")]
        );
    }

    #[test]
    fn file_uri_rendering_avoids_windows_verbatim_prefix() {
        let uri = path_to_file_uri(PathBuf::from(".").as_path());
        assert!(uri.starts_with("file:///"));
        assert!(!uri.contains("file:////?/"));
    }

    #[test]
    fn parses_tsc_diagnostic_lines() {
        let diag = parse_tsc_diagnostic_line(
            r"C:\repo\src\parse.js(8,3): error TS2322: Type 'string' is not assignable to type 'number'.",
        )
        .expect("diag");
        assert_eq!(diag.code, "TS2322");
        assert_eq!(diag.line, Some(8));
        assert_eq!(diag.col, Some(3));
        assert_eq!(diag.hint.as_deref(), Some("tsc"));
    }
}
