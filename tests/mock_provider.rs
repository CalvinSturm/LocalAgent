use openagent::providers::mock::{MockProvider, MockProviderError};
use openagent::providers::{ModelProvider, StreamDelta};
use openagent::types::{GenerateRequest, Message, Role};
use serde_json::json;

fn req_with_user(content: &str) -> GenerateRequest {
    GenerateRequest {
        model: "mock-model".to_string(),
        messages: vec![Message {
            role: Role::User,
            content: Some(content.to_string()),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        }],
        tools: None,
    }
}

#[tokio::test]
async fn marker_absent_returns_mock_ok() {
    let provider = MockProvider::new();
    let resp = provider
        .generate(req_with_user("hello"))
        .await
        .expect("mock response");

    assert_eq!(resp.assistant.content.as_deref(), Some("mock: ok"));
    assert!(resp.tool_calls.is_empty());
}

#[tokio::test]
async fn marker_present_valid_json_returns_single_tool_call() {
    let provider = MockProvider::new();
    let resp = provider
        .generate(req_with_user(
            "__mock_tool_call__:read_file\n{\"path\":\"./Cargo.toml\"}",
        ))
        .await
        .expect("mock tool call");

    assert!(resp.assistant.content.is_none());
    assert_eq!(resp.tool_calls.len(), 1);
    assert_eq!(resp.tool_calls[0].id, "mock_tc_0");
    assert_eq!(resp.tool_calls[0].name, "read_file");
    assert_eq!(resp.tool_calls[0].arguments, json!({"path":"./Cargo.toml"}));
}

#[tokio::test]
async fn marker_present_invalid_json_returns_deterministic_typed_error() {
    let provider = MockProvider::new();
    let err = provider
        .generate(req_with_user("__mock_tool_call__:read_file\n{not-json}"))
        .await
        .expect_err("expected invalid JSON error");

    let typed = err
        .downcast_ref::<MockProviderError>()
        .expect("typed mock provider error");
    match typed {
        MockProviderError::InvalidJson { .. } => {}
        other => panic!("unexpected error variant: {other:?}"),
    }
    assert!(err
        .to_string()
        .starts_with("mock provider invalid tool-call JSON:"));
}

#[tokio::test]
async fn streaming_tool_call_emits_single_complete_fragment() {
    let provider = MockProvider::new();
    let mut deltas = Vec::new();
    let resp = provider
        .generate_streaming(
            req_with_user("__mock_tool_call__:shell\n{\"cmd\":\"echo\",\"args\":[\"hi\"]}"),
            &mut |d| deltas.push(d),
        )
        .await
        .expect("streaming mock tool call");

    assert_eq!(resp.tool_calls.len(), 1);
    assert_eq!(deltas.len(), 1);
    match &deltas[0] {
        StreamDelta::ToolCallFragment(frag) => {
            assert_eq!(frag.index, 0);
            assert_eq!(frag.id.as_deref(), Some("mock_tc_0"));
            assert_eq!(frag.name.as_deref(), Some("shell"));
            assert_eq!(
                frag.arguments_fragment.as_deref(),
                Some("{\"cmd\":\"echo\",\"args\":[\"hi\"]}")
            );
            assert!(frag.complete);
        }
        other => panic!("unexpected delta: {other:?}"),
    }
}
