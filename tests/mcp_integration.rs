use std::fs;
use std::time::Duration;

use openagent::mcp::registry::McpRegistry;
use openagent::mcp::types::{McpConfigFile, McpServerConfig};
use openagent::tools::ToolArgsStrict;
use openagent::trust::policy::{Policy, PolicyDecision};
use openagent::types::ToolCall;
use serde_json::json;
use tempfile::tempdir;

fn stub_bin() -> Option<String> {
    std::env::var("CARGO_BIN_EXE_mcp_stub").ok()
}

#[tokio::test]
async fn mcp_tool_naming_and_schema_conversion() {
    let Some(stub) = stub_bin() else {
        eprintln!("skipping: CARGO_BIN_EXE_mcp_stub not set");
        return;
    };
    let tmp = tempdir().expect("tempdir");
    let cfg_path = tmp.path().join("mcp_servers.json");
    let mut servers = std::collections::BTreeMap::new();
    servers.insert(
        "stub".to_string(),
        McpServerConfig {
            command: stub,
            args: vec![],
        },
    );
    let cfg = McpConfigFile {
        schema_version: "openagent.mcp_servers.v1".to_string(),
        servers,
    };
    fs::write(
        &cfg_path,
        serde_json::to_string_pretty(&cfg).expect("serialize"),
    )
    .expect("write config");

    let reg =
        McpRegistry::from_config_path(&cfg_path, &["stub".to_string()], Duration::from_secs(5))
            .await
            .expect("start registry");
    let names = reg
        .tool_defs()
        .into_iter()
        .map(|t| t.name)
        .collect::<Vec<_>>();
    assert!(names.iter().any(|n| n == "mcp.stub.echo"));
}

#[tokio::test]
async fn mcp_call_routing_returns_wrapped_result() {
    let Some(stub) = stub_bin() else {
        eprintln!("skipping: CARGO_BIN_EXE_mcp_stub not set");
        return;
    };
    let tmp = tempdir().expect("tempdir");
    let cfg_path = tmp.path().join("mcp_servers.json");
    let mut servers = std::collections::BTreeMap::new();
    servers.insert(
        "stub".to_string(),
        McpServerConfig {
            command: stub,
            args: vec![],
        },
    );
    let cfg = McpConfigFile {
        schema_version: "openagent.mcp_servers.v1".to_string(),
        servers,
    };
    fs::write(
        &cfg_path,
        serde_json::to_string_pretty(&cfg).expect("serialize"),
    )
    .expect("write config");

    let reg =
        McpRegistry::from_config_path(&cfg_path, &["stub".to_string()], Duration::from_secs(5))
            .await
            .expect("start registry");

    let tc = ToolCall {
        id: "tc1".to_string(),
        name: "mcp.stub.echo".to_string(),
        arguments: json!({"msg":"world"}),
    };
    let msg = reg
        .call_namespaced_tool(&tc, ToolArgsStrict::On)
        .await
        .expect("call");
    let content = msg.content.unwrap_or_default();
    assert!(content.contains("\"schema_version\":\"openagent.tool_result.v1\""));
    assert!(content.contains("\"tool_name\":\"mcp.stub.echo\""));
    assert!(content.contains("\"ok\":true"));
}

#[tokio::test]
async fn mcp_schema_validation_blocks_invalid_args_before_call() {
    let Some(stub) = stub_bin() else {
        eprintln!("skipping: CARGO_BIN_EXE_mcp_stub not set");
        return;
    };
    let tmp = tempdir().expect("tempdir");
    let cfg_path = tmp.path().join("mcp_servers.json");
    let call_count = tmp.path().join("calls.txt");
    let mut servers = std::collections::BTreeMap::new();
    servers.insert(
        "stub".to_string(),
        McpServerConfig {
            command: stub,
            args: vec![call_count.display().to_string()],
        },
    );
    let cfg = McpConfigFile {
        schema_version: "openagent.mcp_servers.v1".to_string(),
        servers,
    };
    fs::write(
        &cfg_path,
        serde_json::to_string_pretty(&cfg).expect("serialize"),
    )
    .expect("write config");
    let reg =
        McpRegistry::from_config_path(&cfg_path, &["stub".to_string()], Duration::from_secs(5))
            .await
            .expect("start registry");

    let tc = ToolCall {
        id: "tc_invalid".to_string(),
        name: "mcp.stub.echo".to_string(),
        arguments: json!({"wrong":"field"}),
    };
    let msg = reg
        .call_namespaced_tool(&tc, ToolArgsStrict::On)
        .await
        .expect("result");
    let content = msg.content.unwrap_or_default();
    assert!(content.contains("invalid tool arguments"));
    assert!(!call_count.exists());
}

#[test]
fn trust_policy_glob_matches_mcp_namespace() {
    let policy = Policy::from_yaml(
        r#"
version: 1
default: deny
rules:
  - tool: "mcp.playwright.*"
    decision: allow
"#,
    )
    .expect("parse policy");
    let d = policy.evaluate("mcp.playwright.browser_snapshot", &json!({}));
    assert!(matches!(d.decision, PolicyDecision::Allow));
}
