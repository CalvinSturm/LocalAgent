use crate::events::{Event, EventSink};

pub(crate) fn short_error(s: &str) -> String {
    s.chars().take(200).collect()
}

pub(crate) fn node_summary_line(node_id: &str, exit_reason: &str, final_output: &str) -> String {
    let digest = crate::store::sha256_hex(final_output.as_bytes());
    let head = final_output
        .chars()
        .take(200)
        .collect::<String>()
        .replace('\n', " ");
    format!(
        "- [{}] exit_reason={} output_sha256={} head={}",
        node_id, exit_reason, digest, head
    )
}

pub(crate) fn emit_event(
    sink: &mut Option<Box<dyn EventSink>>,
    run_id: &str,
    step: u32,
    kind: EventKind,
    data: serde_json::Value,
) {
    if let Some(s) = sink {
        if let Err(e) = s.emit(Event::new(run_id.to_string(), step, kind, data)) {
            eprintln!("WARN: failed to emit event: {e}");
        }
    }
}

use crate::events::EventKind;
