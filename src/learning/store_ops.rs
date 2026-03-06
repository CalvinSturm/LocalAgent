use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context};

use crate::events::{Event, EventKind, EventSink, JsonlFileSink};
use crate::store;

use super::{
    stable_learning_target_path, ArchiveLearningResult, LearningEntryV1, LearningStatusV1,
    LEARNING_PROMOTED_SCHEMA_V1,
};

pub fn archive_learning_entry(state_dir: &Path, id: &str) -> anyhow::Result<ArchiveLearningResult> {
    let mut entry = load_learning_entry(state_dir, id)?;
    let previous_status = entry.status.clone();
    let archived = previous_status != LearningStatusV1::Archived;
    if archived {
        update_learning_status(state_dir, &mut entry, LearningStatusV1::Archived)?;
    }
    Ok(ArchiveLearningResult {
        learning_id: entry.id,
        previous_status,
        archived,
    })
}

pub(crate) fn update_learning_status(
    state_dir: &Path,
    entry: &mut LearningEntryV1,
    status: LearningStatusV1,
) -> anyhow::Result<()> {
    entry.status = status;
    write_learning_entry(state_dir, entry)
}

pub(crate) fn write_learning_entry(
    state_dir: &Path,
    entry: &LearningEntryV1,
) -> anyhow::Result<()> {
    let path = learning_entry_path(state_dir, &entry.id);
    store::write_json_atomic(&path, entry)
        .with_context(|| format!("failed to write learning entry {}", path.display()))
}

pub(crate) fn compute_file_sha256_hex(path: &Path) -> anyhow::Result<String> {
    let bytes =
        fs::read(path).with_context(|| format!("failed to read file {}", path.display()))?;
    Ok(store::sha256_hex(&bytes))
}

pub(crate) fn emit_learning_promoted_event_for_check(
    state_dir: &Path,
    entry: &LearningEntryV1,
    slug: &str,
    target_path: &Path,
    forced: bool,
    target_file_sha256_hex: &str,
) -> anyhow::Result<()> {
    emit_learning_promoted_event(
        state_dir,
        entry,
        "check",
        target_path,
        forced,
        target_file_sha256_hex,
        Some(slug),
        None,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_learning_promoted_event(
    state_dir: &Path,
    entry: &LearningEntryV1,
    target: &str,
    target_path: &Path,
    forced: bool,
    target_file_sha256_hex: &str,
    slug: Option<&str>,
    pack_id: Option<&str>,
    noop: bool,
) -> anyhow::Result<()> {
    let mut sink = JsonlFileSink::new(&learning_events_path(state_dir))?;
    let mut data = serde_json::json!({
        "schema": LEARNING_PROMOTED_SCHEMA_V1,
        "learning_id": entry.id,
        "entry_hash_hex": entry.entry_hash_hex,
        "target": target,
        "target_path": stable_learning_target_path(state_dir, target_path),
        "forced": forced,
        "target_file_sha256_hex": target_file_sha256_hex,
    });
    if let Some(slug) = slug {
        data["slug"] = serde_json::Value::String(slug.to_string());
    }
    if let Some(pack_id) = pack_id {
        data["pack_id"] = serde_json::Value::String(pack_id.to_string());
    }
    if noop {
        data["noop"] = serde_json::Value::Bool(true);
    }
    sink.emit(Event::new(
        format!("learn:{}", entry.id),
        0,
        EventKind::LearningPromoted,
        data,
    ))?;
    Ok(())
}

pub fn learning_entries_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("learn").join("entries")
}

pub fn learning_entry_path(state_dir: &Path, id: &str) -> PathBuf {
    learning_entries_dir(state_dir).join(format!("{id}.json"))
}

pub fn learning_events_path(state_dir: &Path) -> PathBuf {
    state_dir.join("learn").join("events.jsonl")
}

pub(crate) fn learning_check_path(state_dir: &Path, slug: &str) -> PathBuf {
    state_dir.join("checks").join(format!("{slug}.md"))
}

pub(crate) fn learning_agents_target_path(state_dir: &Path) -> PathBuf {
    state_dir.parent().unwrap_or(state_dir).join("AGENTS.md")
}

pub(crate) fn learning_pack_target_path(state_dir: &Path, pack_id: &str) -> PathBuf {
    let mut path = state_dir.join("packs");
    for segment in pack_id.split('/') {
        path = path.join(segment);
    }
    path.join("PACK.md")
}

pub fn load_learning_entry(state_dir: &Path, id: &str) -> anyhow::Result<LearningEntryV1> {
    let path = learning_entry_path(state_dir, id);
    let bytes = fs::read(&path)
        .with_context(|| format!("failed to read learning entry {}", path.display()))?;
    let entry: LearningEntryV1 = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse learning entry {}", path.display()))?;
    if entry.id != id {
        return Err(anyhow!(
            "learning entry id mismatch for {} (file id={}, entry id={})",
            path.display(),
            id,
            entry.id
        ));
    }
    Ok(entry)
}

pub fn list_learning_entries(state_dir: &Path) -> anyhow::Result<Vec<LearningEntryV1>> {
    let dir = learning_entries_dir(state_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for ent in fs::read_dir(&dir)
        .with_context(|| format!("failed to read learning entries dir {}", dir.display()))?
    {
        let ent = ent?;
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        paths.push(path);
    }
    paths.sort_by(|a, b| {
        a.file_name()
            .and_then(|s| s.to_str())
            .cmp(&b.file_name().and_then(|s| s.to_str()))
    });
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read learning entry {}", path.display()))?;
        let entry: LearningEntryV1 = serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse learning entry {}", path.display()))?;
        out.push(entry);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}
