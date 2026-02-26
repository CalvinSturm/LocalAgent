use crate::agent::Agent;
use crate::events::EventKind;
use crate::operator_queue::{DeliveryBoundary, QueueMessageKind};
use crate::providers::ModelProvider;
use crate::types::{Message, Role};

impl<P: ModelProvider> Agent<P> {
    pub(crate) fn deliver_operator_queue_at_boundary(
        &mut self,
        run_id: &str,
        step: u32,
        boundary: DeliveryBoundary,
        messages: &mut Vec<Message>,
    ) -> (bool, bool) {
        let Some(delivery) = self.operator_queue.deliver_at_boundary(boundary) else {
            return (false, false);
        };
        self.emit_event(
            run_id,
            step,
            EventKind::QueueDelivered,
            serde_json::json!({
                "queue_id": delivery.message.queue_id,
                "sequence_no": delivery.message.sequence_no,
                "kind": delivery.message.kind,
                "truncated": delivery.message.truncated,
                "bytes_kept": delivery.message.bytes_kept,
                "bytes_loaded": delivery.message.bytes_loaded,
                "delivery_boundary": delivery.delivery_boundary,
            }),
        );
        messages.push(Message {
            role: Role::User,
            content: Some(delivery.message.content.clone()),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        });
        if delivery.cancelled_remaining_work {
            self.emit_event(
                run_id,
                step,
                EventKind::QueueInterrupt,
                serde_json::json!({
                    "queue_id": delivery.message.queue_id,
                    "sequence_no": delivery.message.sequence_no,
                    "kind": delivery.message.kind,
                    "delivery_boundary": delivery.delivery_boundary,
                    "cancelled_remaining_work": true,
                    "cancelled_reason": delivery.cancelled_reason.unwrap_or("operator_steer"),
                }),
            );
            return (true, true);
        }
        (true, false)
    }

    pub(crate) fn drain_external_operator_queue(&mut self, run_id: &str, step: u32) {
        let mut drained = Vec::new();
        if let Some(rx) = &self.operator_queue_rx {
            while let Ok(req) = rx.try_recv() {
                drained.push(req);
            }
        }
        for req in drained {
            let submitted = self
                .operator_queue
                .submit(req.kind, &req.content, &self.operator_queue_limits)
                .queued;
            self.emit_event(
                run_id,
                step,
                EventKind::QueueSubmitted,
                serde_json::json!({
                    "queue_id": submitted.queue_id,
                    "sequence_no": submitted.sequence_no,
                    "kind": submitted.kind,
                    "truncated": submitted.truncated,
                    "bytes_kept": submitted.bytes_kept,
                    "bytes_loaded": submitted.bytes_loaded,
                    "next_delivery": match submitted.kind {
                        QueueMessageKind::Steer => DeliveryBoundary::PostTool.user_phrase(),
                        QueueMessageKind::FollowUp => DeliveryBoundary::TurnIdle.user_phrase(),
                    }
                }),
            );
        }
    }
}
