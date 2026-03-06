use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use super::{
    EvidenceRefV1, LearningEntryV1, LearningPromoteError, LearningSourceV1, MatchRange,
    ProposedMemoryV1, MAX_REDACTIONS_IN_DISPLAY, MAX_SCAN_BUNDLE_BYTES, REDACTED_SECRET_TOKEN,
};

pub(super) fn preview_text(text: &str, max_chars: usize) -> String {
    let mut s: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        s.push_str("...");
    }
    s
}

pub(super) fn redact_and_bound_terminal_output(input: &str, max_bytes: usize) -> String {
    let redacted = redact_secrets_for_display(input);
    truncate_utf8_bytes(redacted, max_bytes)
}

pub(super) fn redact_secrets_for_display(input: &str) -> String {
    let mut matches = collect_secret_matches(input);
    matches.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));
    let mut chosen = Vec::new();
    for m in matches {
        if chosen.len() >= MAX_REDACTIONS_IN_DISPLAY {
            break;
        }
        let overlaps = chosen
            .last()
            .map(|prev: &MatchRange| m.start < prev.end)
            .unwrap_or(false);
        if overlaps {
            continue;
        }
        chosen.push(m);
    }
    if chosen.is_empty() {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0usize;
    for m in chosen {
        out.push_str(&input[cursor..m.start]);
        out.push_str(REDACTED_SECRET_TOKEN);
        cursor = m.end;
    }
    out.push_str(&input[cursor..]);
    out
}

pub(super) fn build_sensitivity_scan_bundle(
    summary: &str,
    source: &LearningSourceV1,
    evidence: &[EvidenceRefV1],
    proposed: &ProposedMemoryV1,
) -> String {
    let mut out = String::new();
    out.push_str("summary:\n");
    out.push_str(summary);
    out.push_str("\n\n");
    out.push_str("task_summary:\n");
    out.push_str(source.task_summary.as_deref().unwrap_or(""));
    out.push_str("\n\n");
    out.push_str("guidance_text:\n");
    out.push_str(proposed.guidance_text.as_deref().unwrap_or(""));
    out.push_str("\n\n");
    out.push_str("check_text:\n");
    out.push_str(proposed.check_text.as_deref().unwrap_or(""));
    out.push_str("\n\n");
    out.push_str("evidence:\n");
    for ev in evidence {
        out.push_str("- value: ");
        out.push_str(&ev.value);
        out.push('\n');
        if let Some(note) = &ev.note {
            out.push_str("  note: ");
            out.push_str(note);
            out.push('\n');
        }
    }
    truncate_utf8_bytes(out, MAX_SCAN_BUNDLE_BYTES)
}

pub(super) fn detect_contains_secrets_suspected(text: &str) -> bool {
    secret_detection_patterns()
        .iter()
        .any(|re| re.find(text).is_some())
}

pub(super) fn detect_contains_paths(text: &str) -> bool {
    if windows_path_pattern().is_match(text) {
        return true;
    }
    unix_path_pattern().is_match(text) || text.contains("~/")
}

pub(super) fn validate_promote_slug(slug: &str) -> anyhow::Result<()> {
    if slug.is_empty()
        || slug.contains("..")
        || slug.contains('/')
        || slug.contains('\\')
        || slug.contains(':')
    {
        return Err(LearningPromoteError::InvalidSlug.into());
    }
    if !promote_slug_pattern().is_match(slug) {
        return Err(LearningPromoteError::InvalidSlug.into());
    }
    Ok(())
}

fn promote_slug_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[a-z0-9][a-z0-9_-]{0,63}$").expect("promote slug regex"))
}

pub(super) fn validate_promote_pack_id(pack_id: &str) -> anyhow::Result<()> {
    if pack_id.is_empty()
        || pack_id.starts_with('/')
        || pack_id.contains('\\')
        || pack_id.contains(':')
        || pack_id.contains("//")
    {
        return Err(LearningPromoteError::InvalidPackId.into());
    }
    for segment in pack_id.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(LearningPromoteError::InvalidPackId.into());
        }
        if !promote_slug_pattern().is_match(segment) {
            return Err(LearningPromoteError::InvalidPackId.into());
        }
    }
    Ok(())
}

fn collect_secret_matches(input: &str) -> Vec<MatchRange> {
    let mut out = Vec::new();
    for re in secret_detection_patterns() {
        for m in re.find_iter(input) {
            out.push(MatchRange {
                start: m.start(),
                end: m.end(),
            });
        }
    }
    out
}

fn secret_detection_patterns() -> &'static [Regex] {
    static PATS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATS.get_or_init(|| {
        vec![
            Regex::new(r"BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY").expect("private key regex"),
            Regex::new(r"ghp_[A-Za-z0-9]{20,}").expect("ghp regex"),
            Regex::new(r"github_pat_[A-Za-z0-9_]{20,}").expect("github pat regex"),
            Regex::new(r"AKIA[0-9A-Z]{16}").expect("aws akia regex"),
            Regex::new(r"ASIA[0-9A-Z]{16}").expect("aws asia regex"),
        ]
    })
}

fn windows_path_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|[^A-Za-z0-9_])[A-Za-z]:\\").expect("windows path regex"))
}

fn unix_path_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?m)(?:^|[\s"'(\[{])/(?:home|Users|etc|var)/"#).expect("unix path regex")
    })
}

pub fn require_force_for_sensitive_promotion(
    entry: &LearningEntryV1,
    force: bool,
) -> anyhow::Result<()> {
    if entry.sensitivity_flags.contains_secrets_suspected && !force {
        return Err(LearningPromoteError::SensitiveRequiresForce.into());
    }
    Ok(())
}

fn stable_forward_slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(super) fn stable_learning_target_path(state_dir: &Path, target_path: &Path) -> String {
    let base = state_dir.parent().unwrap_or(state_dir);
    let rel = target_path.strip_prefix(base).unwrap_or(target_path);
    stable_forward_slash_path(rel)
}

pub(super) fn write_text_atomic(path: &Path, content: &str) -> anyhow::Result<()> {
    use uuid::Uuid;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension(format!("tmp.{}", Uuid::new_v4()));
    fs::write(&tmp_path, content)?;
    if let Err(rename_err) = fs::rename(&tmp_path, path) {
        #[cfg(windows)]
        {
            if path.exists() {
                let _ = fs::remove_file(path);
                fs::rename(&tmp_path, path)?;
                return Ok(());
            }
        }
        return Err(rename_err.into());
    }
    Ok(())
}

pub(super) fn normalize_newlines(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

#[allow(dead_code)]
pub(super) fn ensure_trailing_newline(mut input: String) -> String {
    if !input.ends_with('\n') {
        input.push('\n');
    }
    input
}

pub(super) fn truncate_utf8_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    input.chars().take(max_chars).collect()
}

pub(super) fn truncate_utf8_bytes(input: String, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input;
    }
    let suffix = "\n...[truncated]";
    let mut end = max_bytes.saturating_sub(suffix.len()).min(input.len());
    while !input.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = input[..end].to_string();
    if out.len() < input.len() {
        out.push_str(suffix);
    }
    out
}

pub(super) fn has_any_sensitivity(flags: &super::SensitivityFlagsV1) -> bool {
    flags.contains_paths || flags.contains_secrets_suspected
}
