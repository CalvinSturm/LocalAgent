use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::events::{Event, EventKind, EventSink, JsonlFileSink};
use crate::store;
mod assist;
mod capture;
mod promotion;
mod render;
mod store_ops;
mod support;
#[allow(unused_imports)]
pub use assist::{
    apply_assisted_draft_to_capture_input, build_assist_capture_input_canonical,
    build_assist_capture_meta, compute_assist_input_hash_hex, parse_assisted_capture_draft,
    render_assist_capture_preview,
};
#[allow(unused_imports)]
pub use capture::build_capture_input;
use capture::{
    attach_evidence_notes, build_proposed_memory, infer_sensitivity_flags, parse_evidence_specs,
    truncate_string,
};
#[allow(unused_imports)]
pub use promotion::{
    insert_managed_learning_block, promote_learning_to_agents, promote_learning_to_check,
    promote_learning_to_pack, render_learning_to_check_markdown, render_learning_to_guidance_block,
    render_promote_to_check_confirmation, render_promote_to_target_confirmation,
    ManagedInsertResult, PromoteToCheckResult, PromoteToTargetResult,
};
#[allow(unused_imports)]
pub use render::{
    learning_status_str, render_archive_confirmation, render_capture_confirmation,
    render_learning_list_json_preview, render_learning_list_table,
    render_learning_show_json_preview, render_learning_show_text,
};
#[allow(unused_imports)]
pub use store_ops::{
    archive_learning_entry, learning_entries_dir, learning_entry_path, learning_events_path,
    list_learning_entries, load_learning_entry,
};
use store_ops::{
    compute_file_sha256_hex, emit_learning_promoted_event, emit_learning_promoted_event_for_check,
    learning_agents_target_path, learning_check_path, learning_pack_target_path,
    update_learning_status,
};
#[cfg(test)]
use support::redact_secrets_for_display;
#[allow(unused_imports)]
pub use support::require_force_for_sensitive_promotion;
#[cfg(test)]
use support::validate_promote_pack_id;
use support::{
    build_sensitivity_scan_bundle, detect_contains_paths, detect_contains_secrets_suspected,
    has_any_sensitivity, preview_text, redact_and_bound_terminal_output,
    stable_learning_target_path,
};

pub const LEARNING_ENTRY_SCHEMA_V1: &str = "openagent.learning_entry.v1";
const MAX_RUN_ID_CHARS: usize = 128;
const MAX_TASK_SUMMARY_CHARS: usize = 256;
const MAX_PROFILE_CHARS: usize = 128;
const MAX_SUMMARY_CHARS: usize = 512;
const MAX_GUIDANCE_TEXT_CHARS: usize = 2048;
const MAX_CHECK_TEXT_CHARS: usize = 4096;
const MAX_EVIDENCE_ITEMS: usize = 32;
const MAX_EVIDENCE_VALUE_CHARS: usize = 512;
const MAX_EVIDENCE_NOTE_CHARS: usize = 256;
const MAX_TAG_COUNT: usize = 16;
const MAX_TAG_CHARS: usize = 32;
const LIST_SUMMARY_PREVIEW_CHARS: usize = 96;
const LEARN_SHOW_MAX_BYTES: usize = 8 * 1024;
const MAX_REDACTIONS_IN_DISPLAY: usize = 3;
const MAX_SCAN_BUNDLE_BYTES: usize = 64 * 1024;
const REDACTED_SECRET_TOKEN: &str = "[REDACTED_SECRET]";
#[allow(dead_code)]
pub const LEARN_PROMOTE_SENSITIVE_REQUIRES_FORCE: &str = "LEARN_PROMOTE_SENSITIVE_REQUIRES_FORCE";
#[allow(dead_code)]
pub const LEARN_PROMOTE_TARGET_EXISTS_REQUIRES_FORCE: &str =
    "LEARN_PROMOTE_TARGET_EXISTS_REQUIRES_FORCE";
#[allow(dead_code)]
pub const LEARN_PROMOTE_INVALID_SLUG: &str = "LEARN_PROMOTE_INVALID_SLUG";
pub const LEARN_PROMOTE_INVALID_PACK_ID: &str = "LEARN_PROMOTE_INVALID_PACK_ID";
pub const LEARNING_PROMOTED_SCHEMA_V1: &str = "openagent.learning_promoted.v1";
#[allow(dead_code)]
pub const LEARNED_GUIDANCE_MANAGED_SECTION_MARKER: &str = "## LocalAgent Learned Guidance";
#[allow(dead_code)]
pub const LEARN_ASSIST_PROMPT_VERSION_V1: &str = "openagent.learn_assist_prompt.v1";
pub const LEARN_ASSIST_WRITE_REQUIRES_ASSIST: &str = "LEARN_ASSIST_WRITE_REQUIRES_ASSIST";
pub const LEARN_ASSIST_PROVIDER_REQUIRED: &str = "LEARN_ASSIST_PROVIDER_REQUIRED";
pub const LEARN_ASSIST_MODEL_REQUIRED: &str = "LEARN_ASSIST_MODEL_REQUIRED";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningEntryV1 {
    pub schema_version: String,
    pub id: String,
    pub created_at: String,
    pub source: LearningSourceV1,
    pub category: LearningCategoryV1,
    pub summary: String,
    pub evidence: Vec<EvidenceRefV1>,
    pub proposed_memory: ProposedMemoryV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assist: Option<AssistCaptureMetaV1>,
    pub sensitivity_flags: SensitivityFlagsV1,
    pub status: LearningStatusV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub truncations: Vec<FieldTruncationV1>,
    pub entry_hash_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LearningSourceV1 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LearningCategoryV1 {
    #[default]
    WorkflowHint,
    PromptGuidance,
    CheckCandidate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRefV1 {
    pub kind: EvidenceKindV1,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKindV1 {
    RunId,
    EventId,
    ArtifactPath,
    ToolCallId,
    ReasonCode,
    ExitReason,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProposedMemoryV1 {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssistCaptureMetaV1 {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub prompt_version: String,
    pub input_hash_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
    pub generated_at: String,
    #[serde(default)]
    pub output_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssistCaptureHashInputV1 {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub prompt_version: String,
    pub input_hash_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
    #[serde(default)]
    pub output_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SensitivityFlagsV1 {
    pub contains_paths: bool,
    pub contains_secrets_suspected: bool,
    pub contains_user_data: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningStatusV1 {
    Captured,
    Promoted,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldTruncationV1 {
    pub field: String,
    pub original_len: u32,
    pub truncated_to: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum LearningPromoteError {
    SensitiveRequiresForce,
    TargetExistsRequiresForce,
    InvalidSlug,
    InvalidPackId,
}

impl LearningPromoteError {
    #[allow(dead_code)]
    pub fn code(&self) -> &'static str {
        match self {
            LearningPromoteError::SensitiveRequiresForce => LEARN_PROMOTE_SENSITIVE_REQUIRES_FORCE,
            LearningPromoteError::TargetExistsRequiresForce => {
                LEARN_PROMOTE_TARGET_EXISTS_REQUIRES_FORCE
            }
            LearningPromoteError::InvalidSlug => LEARN_PROMOTE_INVALID_SLUG,
            LearningPromoteError::InvalidPackId => LEARN_PROMOTE_INVALID_PACK_ID,
        }
    }
}

impl std::fmt::Display for LearningPromoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LearningPromoteError::SensitiveRequiresForce => write!(
                f,
                "Sensitive content suspected (contains_secrets_suspected). Re-run with --force to promote."
            ),
            LearningPromoteError::TargetExistsRequiresForce => write!(
                f,
                "Promotion target already exists. Re-run with --force to overwrite."
            ),
            LearningPromoteError::InvalidSlug => write!(
                f,
                "Invalid slug. Use lowercase letters, numbers, '_' or '-', no path separators."
            ),
            LearningPromoteError::InvalidPackId => write!(
                f,
                "Invalid pack_id. Use lowercase '/'-separated segments with [a-z0-9_-]."
            ),
        }
    }
}

impl std::error::Error for LearningPromoteError {}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LearningEntryHashInputV1 {
    pub schema_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_profile: Option<String>,
    pub category: String,
    pub summary: String,
    pub evidence: Vec<EvidenceRefV1>,
    pub proposed_memory: ProposedMemoryV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assist: Option<AssistCaptureHashInputV1>,
    pub sensitivity_flags: SensitivityFlagsV1,
}

#[derive(Debug, Clone, Default)]
pub struct CaptureLearningInput {
    pub run_id: Option<String>,
    pub category: LearningCategoryV1,
    pub summary: String,
    pub task_summary: Option<String>,
    pub profile: Option<String>,
    pub guidance_text: Option<String>,
    pub check_text: Option<String>,
    pub tags: Vec<String>,
    pub evidence_specs: Vec<String>,
    pub evidence_notes: Vec<String>,
    pub assist: Option<AssistCaptureMetaV1>,
}

#[derive(Debug, Clone)]
pub struct CaptureLearningOutput {
    pub entry: LearningEntryV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssistCaptureInputCanonical {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_specs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssistedCaptureDraft {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guidance_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AssistedCapturePreview {
    pub provider: String,
    pub model: String,
    pub prompt_version: String,
    pub input_hash_hex: String,
    pub draft: AssistedCaptureDraft,
    pub raw_model_output: String,
}

#[derive(Debug, Clone)]
pub struct ArchiveLearningResult {
    pub learning_id: String,
    pub previous_status: LearningStatusV1,
    pub archived: bool,
}

pub fn capture_learning_entry(
    state_dir: &Path,
    input: CaptureLearningInput,
) -> anyhow::Result<CaptureLearningOutput> {
    let mut truncations = Vec::new();
    let id = Ulid::new().to_string();
    let created_at = crate::trust::now_rfc3339();
    let category = input.category;

    let source = LearningSourceV1 {
        run_id: input
            .run_id
            .map(|s| truncate_string(s, "source.run_id", MAX_RUN_ID_CHARS, &mut truncations)),
        task_summary: input.task_summary.map(|s| {
            truncate_string(
                s,
                "source.task_summary",
                MAX_TASK_SUMMARY_CHARS,
                &mut truncations,
            )
        }),
        profile: input
            .profile
            .map(|s| truncate_string(s, "source.profile", MAX_PROFILE_CHARS, &mut truncations)),
    };

    let summary = truncate_string(
        input.summary,
        "summary",
        MAX_SUMMARY_CHARS,
        &mut truncations,
    );

    let mut evidence = parse_evidence_specs(&input.evidence_specs, &mut truncations)?;
    attach_evidence_notes(&mut evidence, &input.evidence_notes, &mut truncations)?;

    let proposed_memory = build_proposed_memory(
        input.guidance_text,
        input.check_text,
        input.tags,
        &mut truncations,
    );
    let assist = input.assist.clone();

    let sensitivity_flags = infer_sensitivity_flags(&summary, &source, &evidence, &proposed_memory);

    let mut entry = LearningEntryV1 {
        schema_version: LEARNING_ENTRY_SCHEMA_V1.to_string(),
        id: id.clone(),
        created_at,
        source,
        category,
        summary,
        evidence,
        proposed_memory,
        assist,
        sensitivity_flags,
        status: LearningStatusV1::Captured,
        truncations,
        entry_hash_hex: String::new(),
    };

    entry.entry_hash_hex = compute_entry_hash_hex(&entry)?;

    let path = learning_entry_path(state_dir, &id);
    store::write_json_atomic(&path, &entry)
        .with_context(|| format!("failed to write learning entry {}", path.display()))?;

    Ok(CaptureLearningOutput { entry })
}

pub fn emit_learning_captured_event(
    state_dir: &Path,
    entry: &LearningEntryV1,
) -> anyhow::Result<()> {
    let mut sink = JsonlFileSink::new(&learning_events_path(state_dir))?;
    let mut data = serde_json::json!({
        "schema": "openagent.learning_captured.v1",
        "learning_id": entry.id,
        "entry_hash_hex": entry.entry_hash_hex,
        "category": learning_category_str(&entry.category),
    });
    if let Some(run_id) = &entry.source.run_id {
        data["run_id"] = serde_json::Value::String(run_id.clone());
    }
    sink.emit(Event::new(
        format!("learn:{}", entry.id),
        0,
        EventKind::LearningCaptured,
        data,
    ))?;
    Ok(())
}

pub fn learning_entry_hash_input(entry: &LearningEntryV1) -> LearningEntryHashInputV1 {
    LearningEntryHashInputV1 {
        schema_version: entry.schema_version.clone(),
        source_run_id: entry.source.run_id.clone(),
        source_profile: entry.source.profile.clone(),
        category: learning_category_str(&entry.category).to_string(),
        summary: entry.summary.clone(),
        evidence: entry.evidence.clone(),
        proposed_memory: entry.proposed_memory.clone(),
        assist: entry.assist.as_ref().map(|a| AssistCaptureHashInputV1 {
            enabled: a.enabled,
            provider: a.provider.clone(),
            model: a.model.clone(),
            prompt_version: a.prompt_version.clone(),
            input_hash_hex: a.input_hash_hex.clone(),
            source_run_id: a.source_run_id.clone(),
            output_truncated: a.output_truncated,
        }),
        sensitivity_flags: entry.sensitivity_flags.clone(),
    }
}

pub fn compute_entry_hash_hex(entry: &LearningEntryV1) -> anyhow::Result<String> {
    let input = learning_entry_hash_input(entry);
    let bytes = serde_json::to_vec(&input)?;
    Ok(store::sha256_hex(&bytes))
}

pub fn learning_category_str(category: &LearningCategoryV1) -> &'static str {
    match category {
        LearningCategoryV1::WorkflowHint => "workflow_hint",
        LearningCategoryV1::PromptGuidance => "prompt_guidance",
        LearningCategoryV1::CheckCandidate => "check_candidate",
    }
}

#[derive(Debug, Clone, Copy)]
struct MatchRange {
    start: usize,
    end: usize,
}

#[cfg(test)]
mod tests;
