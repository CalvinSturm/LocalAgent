use std::collections::BTreeSet;

use anyhow::anyhow;

use super::{
    build_sensitivity_scan_bundle, detect_contains_paths, detect_contains_secrets_suspected,
    CaptureLearningInput, EvidenceKindV1, EvidenceRefV1, FieldTruncationV1, LearningCategoryV1,
    LearningSourceV1, ProposedMemoryV1, SensitivityFlagsV1, MAX_CHECK_TEXT_CHARS,
    MAX_EVIDENCE_ITEMS, MAX_EVIDENCE_NOTE_CHARS, MAX_EVIDENCE_VALUE_CHARS,
    MAX_GUIDANCE_TEXT_CHARS, MAX_TAG_CHARS, MAX_TAG_COUNT,
};

pub(super) fn parse_evidence_specs(
    specs: &[String],
    truncations: &mut Vec<FieldTruncationV1>,
) -> anyhow::Result<Vec<EvidenceRefV1>> {
    let mut out = Vec::new();
    for (i, spec) in specs.iter().enumerate() {
        if out.len() >= MAX_EVIDENCE_ITEMS {
            truncations.push(FieldTruncationV1 {
                field: "evidence".to_string(),
                original_len: specs.len() as u32,
                truncated_to: MAX_EVIDENCE_ITEMS as u32,
            });
            break;
        }
        let (kind_raw, value_raw) = spec
            .split_once(':')
            .ok_or_else(|| anyhow!("invalid --evidence format (expected kind:value): {spec}"))?;
        if value_raw.is_empty() {
            return Err(anyhow!(
                "invalid --evidence format (missing value after kind:): {spec}"
            ));
        }
        let kind = parse_evidence_kind(kind_raw)?;
        out.push(EvidenceRefV1 {
            kind,
            value: truncate_string(
                value_raw.to_string(),
                &format!("evidence[{i}].value"),
                MAX_EVIDENCE_VALUE_CHARS,
                truncations,
            ),
            hash_hex: None,
            note: None,
        });
    }
    Ok(out)
}

fn parse_evidence_kind(raw: &str) -> anyhow::Result<EvidenceKindV1> {
    match raw {
        "run_id" => Ok(EvidenceKindV1::RunId),
        "event_id" => Ok(EvidenceKindV1::EventId),
        "artifact_path" => Ok(EvidenceKindV1::ArtifactPath),
        "tool_call_id" => Ok(EvidenceKindV1::ToolCallId),
        "reason_code" => Ok(EvidenceKindV1::ReasonCode),
        "exit_reason" => Ok(EvidenceKindV1::ExitReason),
        _ => Err(anyhow!("unknown --evidence kind '{raw}'")),
    }
}

pub(super) fn attach_evidence_notes(
    evidence: &mut [EvidenceRefV1],
    notes: &[String],
    truncations: &mut Vec<FieldTruncationV1>,
) -> anyhow::Result<()> {
    if notes.is_empty() {
        return Ok(());
    }
    if evidence.is_empty() {
        return Err(anyhow!("--evidence-note requires a prior --evidence"));
    }
    if notes.len() > evidence.len() {
        return Err(anyhow!(
            "--evidence-note count ({}) exceeds --evidence count ({})",
            notes.len(),
            evidence.len()
        ));
    }
    for (idx, note) in notes.iter().enumerate() {
        evidence[idx].note = Some(truncate_string(
            note.clone(),
            &format!("evidence[{idx}].note"),
            MAX_EVIDENCE_NOTE_CHARS,
            truncations,
        ));
    }
    Ok(())
}

pub(super) fn build_proposed_memory(
    guidance_text: Option<String>,
    check_text: Option<String>,
    tags: Vec<String>,
    truncations: &mut Vec<FieldTruncationV1>,
) -> ProposedMemoryV1 {
    let mut deduped = BTreeSet::new();
    let mut out_tags = Vec::new();
    for tag in tags {
        if out_tags.len() >= MAX_TAG_COUNT {
            truncations.push(FieldTruncationV1 {
                field: "proposed_memory.tags".to_string(),
                original_len: (out_tags.len() + 1) as u32,
                truncated_to: MAX_TAG_COUNT as u32,
            });
            break;
        }
        let normalized = truncate_string(
            tag,
            &format!("proposed_memory.tags[{}]", out_tags.len()),
            MAX_TAG_CHARS,
            truncations,
        );
        if deduped.insert(normalized.clone()) {
            out_tags.push(normalized);
        }
    }
    ProposedMemoryV1 {
        guidance_text: guidance_text.map(|s| {
            truncate_string(
                s,
                "proposed_memory.guidance_text",
                MAX_GUIDANCE_TEXT_CHARS,
                truncations,
            )
        }),
        check_text: check_text.map(|s| {
            truncate_string(
                s,
                "proposed_memory.check_text",
                MAX_CHECK_TEXT_CHARS,
                truncations,
            )
        }),
        tags: out_tags,
    }
}

pub(super) fn infer_sensitivity_flags(
    summary: &str,
    source: &LearningSourceV1,
    evidence: &[EvidenceRefV1],
    proposed: &ProposedMemoryV1,
) -> SensitivityFlagsV1 {
    let text = build_sensitivity_scan_bundle(summary, source, evidence, proposed);
    SensitivityFlagsV1 {
        contains_paths: detect_contains_paths(&text),
        contains_secrets_suspected: detect_contains_secrets_suspected(&text),
        contains_user_data: false,
    }
}

pub(super) fn truncate_string(
    s: String,
    field: &str,
    max_chars: usize,
    truncations: &mut Vec<FieldTruncationV1>,
) -> String {
    let original_len = s.chars().count();
    if original_len <= max_chars {
        return s;
    }
    let truncated: String = s.chars().take(max_chars).collect();
    truncations.push(FieldTruncationV1 {
        field: field.to_string(),
        original_len: original_len as u32,
        truncated_to: max_chars as u32,
    });
    truncated
}

pub(super) fn parse_learning_category_str(raw: &str) -> Option<LearningCategoryV1> {
    match raw.trim() {
        "workflow_hint" | "workflow-hint" => Some(LearningCategoryV1::WorkflowHint),
        "prompt_guidance" | "prompt-guidance" => Some(LearningCategoryV1::PromptGuidance),
        "check_candidate" | "check-candidate" => Some(LearningCategoryV1::CheckCandidate),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_capture_input(
    run: Option<String>,
    category: LearningCategoryV1,
    summary: String,
    task_summary: Option<String>,
    profile: Option<String>,
    guidance_text: Option<String>,
    check_text: Option<String>,
    tags: Vec<String>,
    evidence: Vec<String>,
    evidence_notes: Vec<String>,
) -> CaptureLearningInput {
    CaptureLearningInput {
        run_id: run,
        category,
        summary,
        task_summary,
        profile,
        guidance_text,
        check_text,
        tags,
        evidence_specs: evidence,
        evidence_notes,
        assist: None,
    }
}
