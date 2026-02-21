use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use serde_json::{json, Value};

fn main() {
    let call_count_path = std::env::args().nth(1);
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            break;
        };
        let parsed: Result<Value, _> = serde_json::from_str(&line);
        let Ok(msg) = parsed else {
            continue;
        };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let method = msg
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let response = match method.as_str() {
            "initialize" => json!({
                "jsonrpc":"2.0",
                "id": id,
                "result": {
                    "protocolVersion":"2024-11-05",
                    "capabilities":{}
                }
            }),
            "tools/list" => json!({
                "jsonrpc":"2.0",
                "id": id,
                "result": {
                    "tools": [{
                        "name":"echo",
                        "description":"Echo arguments",
                        "inputSchema":{"type":"object","properties":{"msg":{"type":"string"}},"required":["msg"],"additionalProperties":false}
                    }]
                }
            }),
            "tools/call" => {
                if let Some(path) = &call_count_path {
                    let p = PathBuf::from(path);
                    let next = std::fs::read_to_string(&p)
                        .ok()
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .unwrap_or(0)
                        + 1;
                    let _ = std::fs::write(p, next.to_string());
                }
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                let args = params.get("arguments").cloned().unwrap_or(Value::Null);
                json!({
                    "jsonrpc":"2.0",
                    "id": id,
                    "result": {
                        "echo": args
                    }
                })
            }
            _ => json!({
                "jsonrpc":"2.0",
                "id": id,
                "error": { "code": -32601, "message":"Method not found" }
            }),
        };
        let _ = writeln!(stdout, "{}", response);
        let _ = stdout.flush();
    }
}
