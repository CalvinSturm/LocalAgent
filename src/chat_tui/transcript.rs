pub(crate) fn push_assistant_transcript_entry(
    transcript: &mut Vec<(String, String)>,
    transcript_thinking: &mut std::collections::BTreeMap<usize, String>,
    raw_text: &str,
) {
    let (visible, thinking) =
        crate::agent_output_sanitize::split_user_visible_and_thinking(raw_text);
    if visible.trim().is_empty() {
        return;
    }
    let idx = transcript.len();
    transcript.push(("assistant".to_string(), visible));
    if let Some(t) = thinking {
        if !t.trim().is_empty() {
            transcript_thinking.insert(idx, t);
        }
    }
}
