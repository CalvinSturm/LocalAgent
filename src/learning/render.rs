use super::{
    has_any_sensitivity, learning_category_str, preview_text, redact_and_bound_terminal_output,
    ArchiveLearningResult, LearningEntryV1, LearningStatusV1, LEARN_SHOW_MAX_BYTES,
    LIST_SUMMARY_PREVIEW_CHARS,
};

pub fn render_archive_confirmation(out: &ArchiveLearningResult) -> String {
    if out.archived {
        return format!(
            "Archived learning {} (previous_status={})",
            out.learning_id,
            learning_status_str(&out.previous_status)
        );
    }
    format!("Already archived (noop): {}", out.learning_id)
}

pub fn render_capture_confirmation(entry: &LearningEntryV1) -> String {
    format!(
        "Captured learning {} (category={}, hash={})",
        entry.id,
        learning_category_str(&entry.category),
        entry.entry_hash_hex
    )
}

pub fn render_learning_list_table(entries: &[LearningEntryV1]) -> String {
    let mut out = String::new();
    out.push_str("ID  STATUS  CATEGORY  RUN_ID  S  SUMMARY\n");
    for e in entries {
        let run_id = e.source.run_id.as_deref().unwrap_or("-");
        let sensitive = if has_any_sensitivity(&e.sensitivity_flags) {
            "!"
        } else {
            "-"
        };
        let summary = preview_text(&e.summary, LIST_SUMMARY_PREVIEW_CHARS);
        let summary = redact_and_bound_terminal_output(&summary, 512);
        out.push_str(&format!(
            "{}  {}  {}  {}  {}  {}\n",
            e.id,
            learning_status_str(&e.status),
            learning_category_str(&e.category),
            run_id,
            sensitive,
            summary
        ));
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

pub fn render_learning_list_json_preview(entries: &[LearningEntryV1]) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec_pretty(entries)?;
    Ok(redact_and_bound_terminal_output(
        &String::from_utf8_lossy(&bytes),
        LEARN_SHOW_MAX_BYTES,
    ))
}

pub fn render_learning_show_text(
    entry: &LearningEntryV1,
    show_evidence: bool,
    show_proposed: bool,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("id: {}\n", entry.id));
    out.push_str(&format!("status: {}\n", learning_status_str(&entry.status)));
    out.push_str(&format!(
        "category: {}\n",
        learning_category_str(&entry.category)
    ));
    out.push_str(&format!("hash: {}\n", entry.entry_hash_hex));
    out.push_str(&format!("created_at: {}\n", entry.created_at));
    out.push_str("source:\n");
    out.push_str(&format!(
        "  run_id: {}\n",
        entry.source.run_id.as_deref().unwrap_or("-")
    ));
    out.push_str(&format!(
        "  task_summary: {}\n",
        entry.source.task_summary.as_deref().unwrap_or("-")
    ));
    out.push_str(&format!(
        "  profile: {}\n",
        entry.source.profile.as_deref().unwrap_or("-")
    ));
    out.push_str("summary:\n");
    out.push_str(&entry.summary);
    out.push('\n');
    out.push_str("sensitivity:\n");
    out.push_str(&format!(
        "  contains_paths: {}\n  contains_secrets_suspected: {}\n  contains_user_data: {}\n",
        entry.sensitivity_flags.contains_paths,
        entry.sensitivity_flags.contains_secrets_suspected,
        entry.sensitivity_flags.contains_user_data
    ));
    if show_evidence {
        out.push_str("evidence:\n");
        if entry.evidence.is_empty() {
            out.push_str("  - none\n");
        } else {
            for ev in &entry.evidence {
                out.push_str(&format!(
                    "  - {}: {}\n",
                    evidence_kind_str(&ev.kind),
                    ev.value
                ));
                if let Some(hash) = &ev.hash_hex {
                    out.push_str(&format!("    hash_hex: {}\n", hash));
                }
                if let Some(note) = &ev.note {
                    out.push_str(&format!("    note: {}\n", note));
                }
            }
        }
    }
    if show_proposed {
        out.push_str("proposed_memory:\n");
        out.push_str(&format!(
            "  guidance_text: {}\n",
            entry
                .proposed_memory
                .guidance_text
                .as_deref()
                .unwrap_or("-")
        ));
        out.push_str(&format!(
            "  check_text: {}\n",
            entry.proposed_memory.check_text.as_deref().unwrap_or("-")
        ));
        out.push_str(&format!(
            "  tags: {}\n",
            if entry.proposed_memory.tags.is_empty() {
                "-".to_string()
            } else {
                entry.proposed_memory.tags.join(", ")
            }
        ));
    }
    if !entry.truncations.is_empty() {
        out.push_str("truncations:\n");
        for t in &entry.truncations {
            out.push_str(&format!(
                "  - {}: {} -> {}\n",
                t.field, t.original_len, t.truncated_to
            ));
        }
    }
    redact_and_bound_terminal_output(&out, LEARN_SHOW_MAX_BYTES)
}

pub fn render_learning_show_json_preview(
    entry: &LearningEntryV1,
    show_evidence: bool,
    show_proposed: bool,
) -> anyhow::Result<String> {
    let mut value = serde_json::to_value(entry)?;
    if !show_evidence {
        value["evidence"] = serde_json::json!([]);
    }
    if !show_proposed {
        value["proposed_memory"] = serde_json::json!({});
    }
    let bytes = serde_json::to_vec_pretty(&value)?;
    Ok(redact_and_bound_terminal_output(
        &String::from_utf8_lossy(&bytes),
        LEARN_SHOW_MAX_BYTES,
    ))
}

pub fn learning_status_str(status: &LearningStatusV1) -> &'static str {
    match status {
        LearningStatusV1::Captured => "captured",
        LearningStatusV1::Promoted => "promoted",
        LearningStatusV1::Archived => "archived",
    }
}

pub(super) fn evidence_kind_str(kind: &super::EvidenceKindV1) -> &'static str {
    match kind {
        super::EvidenceKindV1::RunId => "run_id",
        super::EvidenceKindV1::EventId => "event_id",
        super::EvidenceKindV1::ArtifactPath => "artifact_path",
        super::EvidenceKindV1::ToolCallId => "tool_call_id",
        super::EvidenceKindV1::ReasonCode => "reason_code",
        super::EvidenceKindV1::ExitReason => "exit_reason",
    }
}
