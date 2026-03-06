use super::{
    parse_learning_category_str, AssistCaptureMetaV1, AssistedCaptureDraft,
    AssistedCapturePreview, CaptureLearningInput, LEARN_ASSIST_PROMPT_VERSION_V1,
    LEARN_SHOW_MAX_BYTES,
};
use super::{learning_category_str, redact_and_bound_terminal_output, AssistCaptureInputCanonical};

pub fn build_assist_capture_input_canonical(
    input: &CaptureLearningInput,
) -> AssistCaptureInputCanonical {
    AssistCaptureInputCanonical {
        run_id: input.run_id.clone(),
        category: Some(learning_category_str(&input.category).to_string()),
        summary: input.summary.clone(),
        task_summary: input.task_summary.clone(),
        profile: input.profile.clone(),
        guidance_text: input.guidance_text.clone(),
        check_text: input.check_text.clone(),
        tags: input.tags.clone(),
        evidence_specs: input.evidence_specs.clone(),
        evidence_notes: input.evidence_notes.clone(),
    }
}

pub fn compute_assist_input_hash_hex(
    input: &AssistCaptureInputCanonical,
) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec(input)?;
    Ok(crate::store::sha256_hex(&bytes))
}

pub fn parse_assisted_capture_draft(raw: &str) -> AssistedCaptureDraft {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return AssistedCaptureDraft::default();
    }
    if trimmed.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return AssistedCaptureDraft {
                category: v
                    .get("category")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .map(str::to_string)
                    .filter(|s| !s.is_empty()),
                summary: v
                    .get("summary")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .map(str::to_string)
                    .filter(|s| !s.is_empty()),
                guidance_text: v
                    .get("guidance_text")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .map(str::to_string)
                    .filter(|s| !s.is_empty()),
                check_text: v
                    .get("check_text")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .map(str::to_string)
                    .filter(|s| !s.is_empty()),
            };
        }
    }
    AssistedCaptureDraft {
        summary: Some(trimmed.to_string()),
        ..AssistedCaptureDraft::default()
    }
}

pub fn render_assist_capture_preview(preview: &AssistedCapturePreview) -> String {
    let mut out = String::new();
    out.push_str("ASSIST DRAFT PREVIEW (not saved). Use --write to persist.\n");
    out.push_str(&format!(
        "provider: {}\nmodel: {}\nprompt_version: {}\nassist_input_hash_hex: {}\n",
        preview.provider, preview.model, preview.prompt_version, preview.input_hash_hex
    ));
    out.push_str("draft:\n");
    out.push_str(&format!(
        "  category: {}\n",
        preview.draft.category.as_deref().unwrap_or("-")
    ));
    out.push_str("  summary:\n");
    out.push_str(preview.draft.summary.as_deref().unwrap_or("-"));
    out.push('\n');
    out.push_str("  guidance_text:\n");
    out.push_str(preview.draft.guidance_text.as_deref().unwrap_or("-"));
    out.push('\n');
    out.push_str("  check_text:\n");
    out.push_str(preview.draft.check_text.as_deref().unwrap_or("-"));
    out.push('\n');
    out.push_str("raw_model_output_preview:\n");
    out.push_str(&preview.raw_model_output);
    out.push('\n');
    redact_and_bound_terminal_output(&out, LEARN_SHOW_MAX_BYTES)
}

pub fn build_assist_capture_meta(
    provider: &str,
    model: &str,
    input_hash_hex: &str,
    source_run_id: Option<&str>,
    output_truncated: bool,
) -> AssistCaptureMetaV1 {
    AssistCaptureMetaV1 {
        enabled: true,
        provider: provider.to_string(),
        model: model.to_string(),
        prompt_version: LEARN_ASSIST_PROMPT_VERSION_V1.to_string(),
        input_hash_hex: input_hash_hex.to_string(),
        source_run_id: source_run_id.map(|s| s.to_string()),
        generated_at: crate::trust::now_rfc3339(),
        output_truncated,
    }
}

pub fn apply_assisted_draft_to_capture_input(
    mut input: CaptureLearningInput,
    draft: &AssistedCaptureDraft,
    assist_meta: AssistCaptureMetaV1,
) -> CaptureLearningInput {
    if let Some(cat) = &draft.category {
        if let Some(parsed) = parse_learning_category_str(cat) {
            input.category = parsed;
        }
    }
    if let Some(summary) = &draft.summary {
        input.summary = summary.clone();
    }
    if let Some(guidance) = &draft.guidance_text {
        input.guidance_text = Some(guidance.clone());
    }
    if let Some(check_text) = &draft.check_text {
        input.check_text = Some(check_text.clone());
    }
    input.assist = Some(assist_meta);
    input
}
