use std::io::{self, BufRead, BufReader, Read, Write};

use anyhow::{Context, Result};
use serde_json::{json, Value};

fn main() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();
    let mut primary_uri = Value::Null;

    while let Some(message) = read_lsp_message(&mut reader)? {
        match message.get("method").and_then(|m| m.as_str()) {
            Some("initialize") => {
                write_lsp_message(
                    &mut writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": message.get("id").cloned().unwrap_or(Value::Null),
                        "result": { "capabilities": {} }
                    }),
                )?;
            }
            Some("textDocument/didOpen") => {
                primary_uri = message
                    .get("params")
                    .and_then(|p| p.get("textDocument"))
                    .and_then(|t| t.get("uri"))
                    .cloned()
                    .unwrap_or(Value::Null);
                write_lsp_message(
                    &mut writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "method": "textDocument/publishDiagnostics",
                        "params": {
                            "uri": primary_uri,
                            "diagnostics": [
                                {
                                    "range": {
                                        "start": { "line": 0, "character": 6 },
                                        "end": { "line": 0, "character": 11 }
                                    },
                                    "severity": 1,
                                    "code": 2322,
                                    "source": "ts",
                                    "message": "Type 'string' is not assignable to type 'number'."
                                }
                            ]
                        }
                    }),
                )?;
            }
            Some("textDocument/documentSymbol") => {
                write_lsp_message(
                    &mut writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": message.get("id").cloned().unwrap_or(Value::Null),
                        "result": [
                            {
                                "name": "value",
                                "kind": 13,
                                "range": {
                                    "start": { "line": 0, "character": 6 },
                                    "end": { "line": 0, "character": 11 }
                                },
                                "selectionRange": {
                                    "start": { "line": 0, "character": 6 },
                                    "end": { "line": 0, "character": 11 }
                                }
                            }
                        ]
                    }),
                )?;
            }
            Some("textDocument/definition") => {
                write_lsp_message(
                    &mut writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": message.get("id").cloned().unwrap_or(Value::Null),
                        "result": [
                            {
                                "uri": primary_uri,
                                "range": {
                                    "start": { "line": 0, "character": 6 },
                                    "end": { "line": 0, "character": 11 }
                                }
                            }
                        ]
                    }),
                )?;
            }
            Some("textDocument/references") => {
                write_lsp_message(
                    &mut writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": message.get("id").cloned().unwrap_or(Value::Null),
                        "result": [
                            {
                                "uri": primary_uri,
                                "range": {
                                    "start": { "line": 0, "character": 6 },
                                    "end": { "line": 0, "character": 11 }
                                }
                            },
                            {
                                "uri": primary_uri,
                                "range": {
                                    "start": { "line": 0, "character": 6 },
                                    "end": { "line": 0, "character": 11 }
                                }
                            }
                        ]
                    }),
                )?;
            }
            Some("shutdown") => {
                write_lsp_message(
                    &mut writer,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": message.get("id").cloned().unwrap_or(Value::Null),
                        "result": Value::Null
                    }),
                )?;
            }
            Some("exit") => break,
            _ => {}
        }
    }

    Ok(())
}

fn write_lsp_message<W: Write>(writer: &mut W, value: &Value) -> Result<()> {
    let body = serde_json::to_vec(value).context("serialize stub message")?;
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .context("write stub header")?;
    writer.write_all(&body).context("write stub body")?;
    writer.flush().context("flush stub writer")?;
    Ok(())
}

fn read_lsp_message<R: BufRead + Read>(reader: &mut R) -> Result<Option<Value>> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .context("read stub header line")?;
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
                    .context("parse stub content length")?,
            );
        }
    }
    let len = content_length.context("missing stub Content-Length")?;
    let mut buf = vec![0_u8; len];
    reader.read_exact(&mut buf).context("read stub body")?;
    let value = serde_json::from_slice(&buf).context("parse stub body json")?;
    Ok(Some(value))
}
