use std::fmt;

use anyhow::anyhow;
use async_trait::async_trait;
use serde_json::Value;

use crate::providers::{ModelProvider, StreamDelta, ToolCallFragment};
use crate::types::{GenerateRequest, GenerateResponse, Message, Role, ToolCall};

const MOCK_OK: &str = "mock: ok";
const MARKER_PREFIX: &str = "__mock_tool_call__:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockProviderError {
    InvalidJson { message: String },
    ExpectedJsonObject,
    EmptyToolName,
}

impl fmt::Display for MockProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson { message } => {
                write!(f, "mock provider invalid tool-call JSON: {message}")
            }
            Self::ExpectedJsonObject => {
                write!(f, "mock provider tool-call payload must be a JSON object")
            }
            Self::EmptyToolName => {
                write!(f, "mock provider tool-call marker must include a tool name")
            }
        }
    }
}

impl std::error::Error for MockProviderError {}

#[derive(Debug, Clone, Default)]
pub struct MockProvider;

impl MockProvider {
    pub fn new() -> Self {
        Self
    }

    fn build_response(&self, req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        match extract_mock_tool_call(&req.messages)? {
            Some(invocation) => Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: None,
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: vec![ToolCall {
                    id: "mock_tc_0".to_string(),
                    name: invocation.tool_name,
                    arguments: invocation.args,
                }],
                usage: None,
            }),
            None => Ok(GenerateResponse {
                assistant: Message {
                    role: Role::Assistant,
                    content: Some(MOCK_OK.to_string()),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                },
                tool_calls: Vec::new(),
                usage: None,
            }),
        }
    }
}

struct MockToolInvocation {
    tool_name: String,
    args: Value,
    raw_args: String,
}

fn extract_mock_tool_call(messages: &[Message]) -> anyhow::Result<Option<MockToolInvocation>> {
    let latest_user = messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, Role::User))
        .and_then(|m| m.content.as_deref());
    let Some(content) = latest_user else {
        return Ok(None);
    };

    let Some((first_line, rest)) = content.split_once('\n') else {
        return Ok(None);
    };
    let Some(tool_name) = first_line.strip_prefix(MARKER_PREFIX) else {
        return Ok(None);
    };
    if tool_name.is_empty() {
        return Err(anyhow!(MockProviderError::EmptyToolName));
    }

    let raw_args = rest.to_string();
    let args: Value = serde_json::from_str(rest).map_err(|e| {
        anyhow!(MockProviderError::InvalidJson {
            message: e.to_string()
        })
    })?;
    if !args.is_object() {
        return Err(anyhow!(MockProviderError::ExpectedJsonObject));
    }

    Ok(Some(MockToolInvocation {
        tool_name: tool_name.to_string(),
        args,
        raw_args,
    }))
}

#[async_trait]
impl ModelProvider for MockProvider {
    async fn generate(&self, req: GenerateRequest) -> anyhow::Result<GenerateResponse> {
        self.build_response(req)
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn generate_streaming(
        &self,
        req: GenerateRequest,
        on_delta: &mut (dyn FnMut(StreamDelta) + Send),
    ) -> anyhow::Result<GenerateResponse> {
        match extract_mock_tool_call(&req.messages)? {
            Some(invocation) => {
                on_delta(StreamDelta::ToolCallFragment(ToolCallFragment {
                    index: 0,
                    id: Some("mock_tc_0".to_string()),
                    name: Some(invocation.tool_name.clone()),
                    arguments_fragment: Some(invocation.raw_args),
                    complete: true,
                }));
                Ok(GenerateResponse {
                    assistant: Message {
                        role: Role::Assistant,
                        content: None,
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                    },
                    tool_calls: vec![ToolCall {
                        id: "mock_tc_0".to_string(),
                        name: invocation.tool_name,
                        arguments: invocation.args,
                    }],
                    usage: None,
                })
            }
            None => {
                on_delta(StreamDelta::Content(MOCK_OK.to_string()));
                Ok(GenerateResponse {
                    assistant: Message {
                        role: Role::Assistant,
                        content: Some(MOCK_OK.to_string()),
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                    },
                    tool_calls: Vec::new(),
                    usage: None,
                })
            }
        }
    }
}
