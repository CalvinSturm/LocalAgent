use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;

use super::support::{
    ensure_trailing_newline, normalize_newlines, require_force_for_sensitive_promotion,
    truncate_utf8_chars, validate_promote_pack_id, validate_promote_slug, write_text_atomic,
};
use super::{
    compute_file_sha256_hex, emit_learning_promoted_event, emit_learning_promoted_event_for_check,
    learning_agents_target_path, learning_category_str, learning_check_path,
    learning_pack_target_path, load_learning_entry, update_learning_status, LearningEntryV1,
    LearningPromoteError, LearningStatusV1, LEARNED_GUIDANCE_MANAGED_SECTION_MARKER,
};

#[derive(Debug, Clone)]
pub struct PromoteToCheckResult {
    pub learning_id: String,
    pub slug: String,
    pub target_path: PathBuf,
    pub target_file_sha256_hex: String,
    pub forced: bool,
    pub entry_hash_hex: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedInsertResult {
    pub text: String,
    pub changed: bool,
    pub already_present: bool,
}

#[derive(Debug, Clone)]
pub struct PromoteToTargetResult {
    pub learning_id: String,
    pub target: String,
    pub target_path: PathBuf,
    pub target_file_sha256_hex: String,
    pub forced: bool,
    pub entry_hash_hex: String,
    pub changed: bool,
    pub noop: bool,
    pub pack_id: Option<String>,
}

pub fn promote_learning_to_check(
    state_dir: &Path,
    id: &str,
    slug: &str,
    force: bool,
) -> anyhow::Result<PromoteToCheckResult> {
    validate_promote_slug(slug)?;
    let mut entry = load_learning_entry(state_dir, id)?;
    require_force_for_sensitive_promotion(&entry, force)?;

    let target_path = learning_check_path(state_dir, slug);
    if target_path.exists() && !force {
        return Err(LearningPromoteError::TargetExistsRequiresForce.into());
    }

    let markdown = render_learning_to_check_markdown(&entry, slug)?;
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create check dir {}", parent.display()))?;
    }
    fs::write(&target_path, markdown.as_bytes())
        .with_context(|| format!("failed to write check file {}", target_path.display()))?;
    let target_file_sha256_hex = compute_file_sha256_hex(&target_path)?;

    update_learning_status(state_dir, &mut entry, LearningStatusV1::Promoted)?;
    emit_learning_promoted_event_for_check(
        state_dir,
        &entry,
        slug,
        &target_path,
        force,
        &target_file_sha256_hex,
    )?;

    Ok(PromoteToCheckResult {
        learning_id: entry.id.clone(),
        slug: slug.to_string(),
        target_path,
        target_file_sha256_hex,
        forced: force,
        entry_hash_hex: entry.entry_hash_hex.clone(),
    })
}

pub fn render_promote_to_check_confirmation(out: &PromoteToCheckResult) -> String {
    format!(
        "Promoted learning {} -> check {} (path={}, hash={}, entry_hash={}, forced={})",
        out.learning_id,
        out.slug,
        out.target_path.display(),
        out.target_file_sha256_hex,
        out.entry_hash_hex,
        out.forced
    )
}

pub fn render_promote_to_target_confirmation(out: &PromoteToTargetResult) -> String {
    let pack_suffix = out
        .pack_id
        .as_deref()
        .map(|p| format!(", pack_id={p}"))
        .unwrap_or_default();
    if out.noop {
        return format!(
            "Already promoted (noop): LEARN-{} already present in managed section (target={}, path={}{} )",
            out.learning_id,
            out.target,
            out.target_path.display(),
            pack_suffix
        )
        .replace(" )", ")");
    }
    format!(
        "Promoted learning {} -> {} (path={}, hash={}, entry_hash={}, forced={}, changed={}{} )",
        out.learning_id,
        out.target,
        out.target_path.display(),
        out.target_file_sha256_hex,
        out.entry_hash_hex,
        out.forced,
        out.changed,
        pack_suffix
    )
    .replace(" )", ")")
}

pub fn promote_learning_to_agents(
    state_dir: &Path,
    id: &str,
    force: bool,
) -> anyhow::Result<PromoteToTargetResult> {
    let target_path = learning_agents_target_path(state_dir);
    promote_learning_to_managed_target(state_dir, id, force, "agents", &target_path, None)
}

pub fn promote_learning_to_pack(
    state_dir: &Path,
    id: &str,
    pack_id: &str,
    force: bool,
) -> anyhow::Result<PromoteToTargetResult> {
    validate_promote_pack_id(pack_id)?;
    let target_path = learning_pack_target_path(state_dir, pack_id);
    promote_learning_to_managed_target(state_dir, id, force, "pack", &target_path, Some(pack_id))
}

pub fn render_learning_to_check_markdown(
    entry: &LearningEntryV1,
    slug: &str,
) -> anyhow::Result<String> {
    let fm = build_generated_check_from_learning(entry, slug);
    crate::checks::schema::validate_frontmatter(&fm)?;

    let name = serde_json::to_string(&fm.name)?;
    let description = serde_json::to_string(fm.description.as_deref().unwrap_or(""))?;
    let pass_value = serde_json::to_string(&fm.pass_criteria.value)?;
    let pass_kind = match fm.pass_criteria.kind {
        crate::checks::schema::PassCriteriaType::Contains => "output_contains",
        crate::checks::schema::PassCriteriaType::NotContains => "output_not_contains",
        crate::checks::schema::PassCriteriaType::Equals => "output_equals",
    };

    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("schema_version: {}\n", fm.schema_version));
    out.push_str(&format!("name: {name}\n"));
    out.push_str(&format!("description: {description}\n"));
    out.push_str(&format!("required: {}\n", fm.required));
    out.push_str("allowed_tools: []\n");
    out.push_str("pass_criteria:\n");
    out.push_str(&format!("  type: {pass_kind}\n"));
    out.push_str(&format!("  value: {pass_value}\n"));
    out.push_str("---\n");
    out.push_str(&render_generated_check_body(entry, slug));
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out.replace("\r\n", "\n").replace('\r', "\n"))
}

fn build_generated_check_from_learning(
    entry: &LearningEntryV1,
    slug: &str,
) -> crate::checks::schema::CheckFrontmatter {
    let summary_one_line = entry
        .summary
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let description = if summary_one_line.is_empty() {
        format!("Learned check candidate from learning {}", entry.id)
    } else {
        truncate_utf8_chars(&summary_one_line, 240)
    };
    crate::checks::schema::CheckFrontmatter {
        schema_version: 1,
        name: format!("learn_{slug}"),
        description: Some(description),
        required: false,
        allowed_tools: Some(vec![]),
        required_flags: Vec::new(),
        pass_criteria: crate::checks::schema::PassCriteria {
            kind: crate::checks::schema::PassCriteriaType::Contains,
            value: "TODO".to_string(),
        },
        budget: None,
    }
}

fn render_generated_check_body(entry: &LearningEntryV1, slug: &str) -> String {
    if let Some(text) = &entry.proposed_memory.check_text {
        let normalized = normalize_newlines(text);
        if !normalized.trim().is_empty() {
            return normalized;
        }
    }

    let mut out = String::new();
    out.push_str("# Learned Check Draft\n\n");
    out.push_str(&format!("Learning ID: {}\n", entry.id));
    out.push_str(&format!("Slug: {slug}\n\n"));
    out.push_str("Summary:\n");
    out.push_str(&normalize_newlines(&entry.summary));
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\nTODO: Replace this placeholder with concrete check instructions and pass criteria evidence expectations.\n");
    out
}

fn promote_learning_to_managed_target(
    state_dir: &Path,
    id: &str,
    force: bool,
    target: &str,
    target_path: &Path,
    pack_id: Option<&str>,
) -> anyhow::Result<PromoteToTargetResult> {
    let mut entry = load_learning_entry(state_dir, id)?;
    require_force_for_sensitive_promotion(&entry, force)?;

    let existing = if target_path.exists() {
        fs::read_to_string(target_path)
            .with_context(|| format!("failed to read target file {}", target_path.display()))?
    } else {
        String::new()
    };
    let block = render_learning_to_guidance_block(&entry, force);
    let insert = insert_managed_learning_block(&existing, &entry.id, &block);

    if insert.changed {
        write_text_atomic(target_path, &insert.text)
            .with_context(|| format!("failed to write target file {}", target_path.display()))?;
        let target_file_sha256_hex = compute_file_sha256_hex(target_path)?;
        update_learning_status(state_dir, &mut entry, LearningStatusV1::Promoted)?;
        emit_learning_promoted_event(
            state_dir,
            &entry,
            target,
            target_path,
            force,
            &target_file_sha256_hex,
            None,
            pack_id,
            false,
        )?;
        return Ok(PromoteToTargetResult {
            learning_id: entry.id.clone(),
            target: target.to_string(),
            target_path: target_path.to_path_buf(),
            target_file_sha256_hex,
            forced: force,
            entry_hash_hex: entry.entry_hash_hex.clone(),
            changed: true,
            noop: false,
            pack_id: pack_id.map(ToOwned::to_owned),
        });
    }

    let target_file_sha256_hex = if target_path.exists() {
        compute_file_sha256_hex(target_path)?
    } else {
        String::new()
    };
    Ok(PromoteToTargetResult {
        learning_id: entry.id.clone(),
        target: target.to_string(),
        target_path: target_path.to_path_buf(),
        target_file_sha256_hex,
        forced: force,
        entry_hash_hex: entry.entry_hash_hex.clone(),
        changed: false,
        noop: true,
        pack_id: pack_id.map(ToOwned::to_owned),
    })
}

pub fn render_learning_to_guidance_block(entry: &LearningEntryV1, forced: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!("### LEARN-{}\n", entry.id));
    out.push_str(&format!("learning_id: {}\n", entry.id));
    out.push_str(&format!("entry_hash_hex: {}\n", entry.entry_hash_hex));
    out.push_str(&format!(
        "category: {}\n",
        learning_category_str(&entry.category)
    ));
    out.push_str(&format!("forced: {}\n\n", forced));
    let body = entry
        .proposed_memory
        .guidance_text
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(normalize_newlines)
        .unwrap_or_else(|| {
            format!(
                "Learned guidance placeholder (deterministic draft)\n\nSummary:\n{}\n",
                normalize_newlines(&entry.summary)
            )
        });
    out.push_str(&body);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub fn insert_managed_learning_block(
    input: &str,
    learning_id: &str,
    block: &str,
) -> ManagedInsertResult {
    let normalized_input = normalize_newlines(input);
    let mut normalized_block = normalize_newlines(block);
    if !normalized_block.ends_with('\n') {
        normalized_block.push('\n');
    }
    let header = format!("### LEARN-{learning_id}");

    if let Some(marker_pos) = normalized_input.find(LEARNED_GUIDANCE_MANAGED_SECTION_MARKER) {
        let section_start = marker_pos;
        let after_marker_idx = marker_pos + LEARNED_GUIDANCE_MANAGED_SECTION_MARKER.len();
        let tail_after_marker = &normalized_input[after_marker_idx..];
        let next_section_rel = tail_after_marker.find("\n## ").map(|i| i + 1);
        let section_end = next_section_rel
            .map(|rel| after_marker_idx + rel)
            .unwrap_or(normalized_input.len());
        let section = &normalized_input[section_start..section_end];
        if section.contains(&header) {
            return ManagedInsertResult {
                text: ensure_trailing_newline(normalized_input),
                changed: false,
                already_present: true,
            };
        }

        let mut new_section = section.to_string();
        if !new_section.ends_with('\n') {
            new_section.push('\n');
        }
        if new_section == LEARNED_GUIDANCE_MANAGED_SECTION_MARKER {
            new_section.push('\n');
        }
        if !new_section.ends_with("\n\n") {
            if new_section.ends_with('\n') {
                new_section.push('\n');
            } else {
                new_section.push_str("\n\n");
            }
        }
        new_section.push_str(&normalized_block);
        let mut rebuilt = String::new();
        rebuilt.push_str(&normalized_input[..section_start]);
        rebuilt.push_str(&new_section);
        rebuilt.push_str(&normalized_input[section_end..]);
        return ManagedInsertResult {
            text: ensure_trailing_newline(rebuilt),
            changed: true,
            already_present: false,
        };
    }

    let mut out = normalized_input;
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(LEARNED_GUIDANCE_MANAGED_SECTION_MARKER);
    out.push_str("\n\n");
    out.push_str(&normalized_block);
    ManagedInsertResult {
        text: ensure_trailing_newline(out),
        changed: true,
        already_present: false,
    }
}
