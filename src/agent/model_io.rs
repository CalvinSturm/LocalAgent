use crate::events::EventKind;
use crate::providers::{ModelProvider, StreamDelta};
use crate::types::GenerateRequest;

use super::Agent;

impl<P: ModelProvider> Agent<P> {
    pub(super) async fn execute_model_request(
        &mut self,
        run_id: &str,
        step: u32,
        req: GenerateRequest,
    ) -> anyhow::Result<crate::types::GenerateResponse> {
        self.emit_event(
            run_id,
            step,
            EventKind::ModelRequestStart,
            serde_json::json!({
                "message_count": req.messages.len(),
                "tool_count": req.tools.as_ref().map(|t| t.len()).unwrap_or(0)
            }),
        );

        if self.stream {
            if self.provider.supports_streaming() {
                let mut collected = Vec::<StreamDelta>::new();
                let mut callback = |delta| collected.push(delta);
                let out = self
                    .provider
                    .generate_streaming(req.clone(), &mut callback)
                    .await;
                for delta in collected {
                    match delta {
                        StreamDelta::Content(text) => {
                            self.emit_event(
                                run_id,
                                step,
                                EventKind::ModelDelta,
                                serde_json::json!({"delta": text}),
                            );
                        }
                        StreamDelta::ToolCallFragment(fragment) => {
                            self.emit_event(
                                run_id,
                                step,
                                EventKind::ModelDelta,
                                serde_json::json!({
                                    "tool_call_fragment": {
                                        "index": fragment.index,
                                        "id": fragment.id,
                                        "name": fragment.name,
                                        "arguments_fragment": fragment.arguments_fragment,
                                        "complete": fragment.complete
                                    }
                                }),
                            );
                        }
                    }
                }
                out
            } else {
                eprintln!(
                    "WARN: provider does not support streaming; falling back to non-streaming"
                );
                self.provider.generate(req).await
            }
        } else {
            self.provider.generate(req).await
        }
    }
}
