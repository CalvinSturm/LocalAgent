use std::fs;
use std::path::PathBuf;

use localagent::target::{ExecTargetKind, HostTarget};
use localagent::tools::{
    execute_tool, LspToolProvider, LspToolRuntime, ToolArgsStrict, ToolRuntime,
};
use localagent::types::ToolCall;
use serde_json::{json, Value};
use tempfile::tempdir;

fn runtime(workdir: PathBuf, provider: LspToolProvider) -> ToolRuntime {
    ToolRuntime {
        workdir,
        allow_shell: false,
        allow_shell_in_workdir_only: false,
        allow_write: false,
        max_tool_output_bytes: 200_000,
        max_read_bytes: 200_000,
        unsafe_bypass_allow_flags: false,
        tool_args_strict: ToolArgsStrict::On,
        lsp: Some(LspToolRuntime {
            provider,
            command: Some(PathBuf::from(env!("CARGO_BIN_EXE_lsp_stub"))),
        }),
        exec_target_kind: ExecTargetKind::Host,
        exec_target: std::sync::Arc::new(HostTarget),
    }
}

async fn call(rt: &ToolRuntime, name: &str, arguments: Value) -> Value {
    let msg = execute_tool(
        rt,
        &ToolCall {
            id: format!("tc_{name}"),
            name: name.to_string(),
            arguments,
        },
    )
    .await;
    let outer: Value = serde_json::from_str(&msg.content.expect("tool content")).expect("outer");
    assert_eq!(outer["ok"], json!(true));
    serde_json::from_str(outer["content"].as_str().expect("inner content")).expect("inner")
}

#[tokio::test]
async fn lsp_tools_call_stub_language_server_read_only() {
    let tmp = tempdir().expect("tmp");
    fs::create_dir_all(tmp.path().join("src")).expect("mkdir src");
    fs::write(
        tmp.path().join("src").join("index.ts"),
        "const value: number = \"oops\";\n",
    )
    .expect("write source");
    let rt = runtime(tmp.path().to_path_buf(), LspToolProvider::Typescript);

    let diagnostics = call(
        &rt,
        "lsp.diagnostics",
        json!({"path":"src/index.ts","max_results":10}),
    )
    .await;
    assert_eq!(diagnostics["diagnostic_count"], json!(1));
    assert_eq!(diagnostics["diagnostics"][0]["code"], json!("2322"));

    let symbols = call(&rt, "lsp.document_symbols", json!({"path":"src/index.ts"})).await;
    assert_eq!(symbols["symbols"][0]["label"], json!("value"));

    let definitions = call(
        &rt,
        "lsp.goto_definition",
        json!({"path":"src/index.ts","line":1,"column":7}),
    )
    .await;
    assert_eq!(definitions["definition_count"], json!(1));

    let references = call(
        &rt,
        "lsp.find_references",
        json!({"path":"src/index.ts","line":1,"column":7}),
    )
    .await;
    assert_eq!(references["reference_count"], json!(2));

    let hover = call(
        &rt,
        "lsp.hover",
        json!({"path":"src/index.ts","line":1,"column":7}),
    )
    .await;
    assert_eq!(hover["hover"]["contents"], json!("const value: number"));
}

#[tokio::test]
async fn rust_lsp_provider_accepts_rust_source_files() {
    let tmp = tempdir().expect("tmp");
    fs::create_dir_all(tmp.path().join("src")).expect("mkdir src");
    fs::write(
        tmp.path().join("src").join("lib.rs"),
        "const VALUE: i32 = 1;\n",
    )
    .expect("write source");
    let rt = runtime(tmp.path().to_path_buf(), LspToolProvider::Rust);

    let symbols = call(&rt, "lsp.document_symbols", json!({"path":"src/lib.rs"})).await;

    assert_eq!(symbols["provider"], json!("rust_analyzer"));
    assert_eq!(symbols["symbols"][0]["label"], json!("value"));
}
