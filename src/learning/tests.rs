use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use tempfile::tempdir;

use super::*;

fn secret_ghp() -> String {
    format!("{}{}", "ghp_", "A".repeat(32))
}

fn secret_github_pat() -> String {
    format!("{}{}", "github_pat_", "a".repeat(24) + "_1234567890")
}

fn secret_aws_akia() -> String {
    format!("{}{}", "AKIA", "ABCDEFGHIJKLMNOP")
}

fn secret_aws_asia() -> String {
    format!("{}{}", "ASIA", "ABCDEFGHIJKLMNOP")
}

fn secret_private_key_marker() -> String {
    ["BEGIN", "PRIVATE", "KEY"].join(" ")
}

fn sample_entry() -> LearningEntryV1 {
    LearningEntryV1 {
        schema_version: LEARNING_ENTRY_SCHEMA_V1.to_string(),
        id: "01JTESTENTRY".to_string(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
        source: LearningSourceV1 {
            run_id: Some("run1".to_string()),
            task_summary: None,
            profile: Some("p".to_string()),
        },
        category: LearningCategoryV1::CheckCandidate,
        summary: "s".to_string(),
        evidence: vec![EvidenceRefV1 {
            kind: EvidenceKindV1::ReasonCode,
            value: "X".to_string(),
            hash_hex: None,
            note: None,
        }],
        proposed_memory: ProposedMemoryV1::default(),
        assist: None,
        sensitivity_flags: SensitivityFlagsV1::default(),
        status: LearningStatusV1::Captured,
        truncations: Vec::new(),
        entry_hash_hex: String::new(),
    }
}

fn write_entry(state_dir: &Path, mut entry: LearningEntryV1) {
    if entry.entry_hash_hex.is_empty() {
        entry.entry_hash_hex = compute_entry_hash_hex(&entry).expect("hash");
    }
    let path = learning_entry_path(state_dir, &entry.id);
    store::write_json_atomic(&path, &entry).expect("write entry");
}

#[test]
fn entry_hash_excludes_id_created_at_status() {
    let mut a = sample_entry();
    let mut b = sample_entry();
    a.id = "01JAAA".to_string();
    a.created_at = "2026-02-01T00:00:00Z".to_string();
    a.status = LearningStatusV1::Archived;
    b.id = "01JBBB".to_string();
    b.created_at = "2030-02-01T00:00:00Z".to_string();
    b.status = LearningStatusV1::Promoted;
    let ha = compute_entry_hash_hex(&a).expect("hash a");
    let hb = compute_entry_hash_hex(&b).expect("hash b");
    assert_eq!(ha, hb);
}

#[test]
fn entry_hash_excludes_assist_generated_at_but_includes_input_hash() {
    let mut a = sample_entry();
    let mut b = sample_entry();
    a.assist = Some(AssistCaptureMetaV1 {
        enabled: true,
        provider: "ollama".to_string(),
        model: "mock-model".to_string(),
        prompt_version: LEARN_ASSIST_PROMPT_VERSION_V1.to_string(),
        input_hash_hex: "abc".to_string(),
        source_run_id: Some("run1".to_string()),
        generated_at: "2026-01-01T00:00:00Z".to_string(),
        output_truncated: false,
    });
    b.assist = Some(AssistCaptureMetaV1 {
        generated_at: "2027-01-01T00:00:00Z".to_string(),
        ..a.assist.clone().expect("assist")
    });
    let ha = compute_entry_hash_hex(&a).expect("hash a");
    let hb = compute_entry_hash_hex(&b).expect("hash b");
    assert_eq!(ha, hb, "generated_at should not affect entry hash");

    b.assist.as_mut().expect("assist").input_hash_hex = "def".to_string();
    let hc = compute_entry_hash_hex(&b).expect("hash c");
    assert_ne!(ha, hc, "assist.input_hash_hex should affect entry hash");
}

#[test]
fn assist_input_hash_is_stable_for_fixed_fixture() {
    let a = build_assist_capture_input_canonical(&CaptureLearningInput {
        run_id: Some("run1".to_string()),
        category: LearningCategoryV1::PromptGuidance,
        summary: "summary".to_string(),
        task_summary: Some("task".to_string()),
        profile: Some("dev".to_string()),
        guidance_text: Some("do x".to_string()),
        check_text: Some("assert y".to_string()),
        tags: vec!["a".to_string(), "b".to_string()],
        evidence_specs: vec!["run_id:r1".to_string(), "reason_code:OK".to_string()],
        evidence_notes: vec!["note".to_string()],
        assist: None,
    });
    let b = build_assist_capture_input_canonical(&CaptureLearningInput {
        run_id: Some("run1".to_string()),
        category: LearningCategoryV1::PromptGuidance,
        summary: "summary".to_string(),
        task_summary: Some("task".to_string()),
        profile: Some("dev".to_string()),
        guidance_text: Some("do x".to_string()),
        check_text: Some("assert y".to_string()),
        tags: vec!["a".to_string(), "b".to_string()],
        evidence_specs: vec!["run_id:r1".to_string(), "reason_code:OK".to_string()],
        evidence_notes: vec!["note".to_string()],
        assist: None,
    });
    let ha = compute_assist_input_hash_hex(&a).expect("hash a");
    let hb = compute_assist_input_hash_hex(&b).expect("hash b");
    assert_eq!(ha, hb);
}

#[test]
fn assist_preview_is_bounded_redacted_and_labeled() {
    let ghp = secret_ghp();
    let preview = AssistedCapturePreview {
        provider: "mock".to_string(),
        model: "mock".to_string(),
        prompt_version: LEARN_ASSIST_PROMPT_VERSION_V1.to_string(),
        input_hash_hex: "abc".to_string(),
        draft: AssistedCaptureDraft {
            summary: Some(format!("contains token {} {}", ghp, "x".repeat(20_000))),
            ..AssistedCaptureDraft::default()
        },
        raw_model_output: format!("{{\"summary\":\"{} {}\"}}", ghp, "x".repeat(20_000)),
    };
    let out = render_assist_capture_preview(&preview);
    assert!(out.contains("ASSIST DRAFT PREVIEW (not saved). Use --write to persist."));
    assert!(out.contains(REDACTED_SECRET_TOKEN));
    assert!(!out.contains("ghp_"));
    assert!(out.len() <= LEARN_SHOW_MAX_BYTES + "\n...[truncated]".len());
}

#[test]
fn parse_evidence_specs_rejects_invalid() {
    let mut trunc = Vec::new();
    let err = parse_evidence_specs(&["bad".to_string()], &mut trunc).expect_err("invalid");
    assert!(err.to_string().contains("invalid --evidence format"));
}

#[test]
fn capture_writes_under_learning_entries() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let out = capture_learning_entry(
        &state_dir,
        CaptureLearningInput {
            category: LearningCategoryV1::WorkflowHint,
            summary: "hello".to_string(),
            ..CaptureLearningInput::default()
        },
    )
    .expect("capture");
    let path = learning_entry_path(&state_dir, &out.entry.id);
    assert!(path.starts_with(state_dir.join("learn").join("entries")));
    assert!(path.exists());
}

#[test]
fn truncation_metadata_recorded() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let long = "x".repeat(MAX_SUMMARY_CHARS + 10);
    let out = capture_learning_entry(
        &state_dir,
        CaptureLearningInput {
            category: LearningCategoryV1::PromptGuidance,
            summary: long,
            ..CaptureLearningInput::default()
        },
    )
    .expect("capture");
    assert!(out.entry.summary.len() <= MAX_SUMMARY_CHARS);
    assert!(out
        .entry
        .truncations
        .iter()
        .any(|t| t.field == "summary" && t.truncated_to == MAX_SUMMARY_CHARS as u32));
}

#[test]
fn evidence_notes_require_evidence() {
    let mut evidence = Vec::<EvidenceRefV1>::new();
    let mut trunc = Vec::new();
    let err = attach_evidence_notes(&mut evidence, &["n".to_string()], &mut trunc)
        .expect_err("note without evidence");
    assert!(err
        .to_string()
        .contains("--evidence-note requires a prior --evidence"));
}

#[test]
fn list_learning_entries_sorts_by_id() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut a = sample_entry();
    a.id = "01JZZZ".to_string();
    let mut b = sample_entry();
    b.id = "01JAAA".to_string();
    write_entry(&state_dir, a);
    write_entry(&state_dir, b);
    let entries = list_learning_entries(&state_dir).expect("list");
    let ids = entries.into_iter().map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(ids, vec!["01JAAA".to_string(), "01JZZZ".to_string()]);
}

#[test]
fn load_learning_entry_unknown_id_errors() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let err = load_learning_entry(&state_dir, "01JNOPE").expect_err("missing");
    assert!(err.to_string().contains("failed to read learning entry"));
}

#[test]
fn learn_show_redacts_and_bounds_output() {
    let mut e = sample_entry();
    e.summary = format!("token {} and {}", secret_ghp(), "x".repeat(20_000));
    let out = render_learning_show_text(&e, true, true);
    assert!(out.contains(REDACTED_SECRET_TOKEN));
    assert!(!out.contains("ghp_"));
    assert!(out.len() <= LEARN_SHOW_MAX_BYTES + "\n...[truncated]".len());
}

#[test]
fn list_table_preview_is_bounded() {
    let mut e = sample_entry();
    e.summary = "x".repeat(300);
    let out = render_learning_list_table(&[e]);
    assert!(out.contains("..."));
}

#[test]
fn list_show_do_not_modify_files() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let e = sample_entry();
    write_entry(&state_dir, e);
    let before = fs::read_dir(learning_entries_dir(&state_dir))
        .expect("read_dir")
        .map(|r| r.expect("dirent").file_name().to_string_lossy().to_string())
        .collect::<BTreeSet<_>>();
    let entries = list_learning_entries(&state_dir).expect("list");
    let _ = render_learning_list_json_preview(&entries).expect("list json");
    let loaded = load_learning_entry(&state_dir, &entries[0].id).expect("load");
    let _ = render_learning_show_json_preview(&loaded, true, true).expect("show json");
    let after = fs::read_dir(learning_entries_dir(&state_dir))
        .expect("read_dir")
        .map(|r| r.expect("dirent").file_name().to_string_lossy().to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(before, after);
}

#[test]
fn archive_learning_entry_updates_status_to_archived() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut e = sample_entry();
    e.status = LearningStatusV1::Promoted;
    e.entry_hash_hex = compute_entry_hash_hex(&e).expect("hash");
    write_entry(&state_dir, e.clone());

    let out = archive_learning_entry(&state_dir, &e.id).expect("archive");
    assert!(out.archived);
    assert_eq!(out.previous_status, LearningStatusV1::Promoted);

    let updated = load_learning_entry(&state_dir, &e.id).expect("reload");
    assert_eq!(updated.status, LearningStatusV1::Archived);
    let msg = render_archive_confirmation(&out);
    assert!(msg.contains("Archived learning"));
    assert!(msg.contains("previous_status=promoted"));
}

#[test]
fn archive_learning_entry_is_noop_when_already_archived() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut e = sample_entry();
    e.status = LearningStatusV1::Archived;
    e.entry_hash_hex = compute_entry_hash_hex(&e).expect("hash");
    write_entry(&state_dir, e.clone());

    let before = fs::read_to_string(learning_entry_path(&state_dir, &e.id)).expect("before");
    let out = archive_learning_entry(&state_dir, &e.id).expect("archive noop");
    let after = fs::read_to_string(learning_entry_path(&state_dir, &e.id)).expect("after");

    assert!(!out.archived);
    assert_eq!(out.previous_status, LearningStatusV1::Archived);
    assert_eq!(before, after);
    assert_eq!(
        render_archive_confirmation(&out),
        format!("Already archived (noop): {}", e.id)
    );
}

#[test]
fn sensitivity_detects_private_key_and_tokens_case_sensitive() {
    let flags = detect_contains_secrets_suspected(&secret_private_key_marker());
    assert!(flags);
    assert!(!detect_contains_secrets_suspected("Begin Private Key"));
    assert!(detect_contains_secrets_suspected(&format!(
        "x {} y",
        secret_ghp()
    )));
    assert!(detect_contains_secrets_suspected(&secret_github_pat()));
}

#[test]
fn sensitivity_detects_paths_but_not_urls() {
    assert!(detect_contains_paths(r"C:\Users\Calvin\project"));
    assert!(detect_contains_paths("/home/calvin/project"));
    assert!(!detect_contains_paths("https://example.com/var/test"));
}

#[test]
fn redaction_replaces_non_overlapping_left_to_right_with_cap() {
    let input = format!(
        "{} {} {} {} {}",
        secret_ghp(),
        secret_github_pat(),
        secret_private_key_marker(),
        secret_aws_akia(),
        secret_aws_asia()
    );
    let out = redact_secrets_for_display(&input);
    assert_eq!(
        out.matches(REDACTED_SECRET_TOKEN).count(),
        MAX_REDACTIONS_IN_DISPLAY
    );
    assert!(!out.contains("ghp_"));
    assert!(!out.contains("github_pat_"));
    assert!(!out.contains(&secret_private_key_marker()));
}

#[test]
fn promotion_gating_requires_force_for_sensitive_entries() {
    let mut e = sample_entry();
    e.sensitivity_flags.contains_secrets_suspected = true;
    let err = require_force_for_sensitive_promotion(&e, false).expect_err("must fail");
    let typed = err
        .downcast_ref::<LearningPromoteError>()
        .expect("typed learning promote error");
    assert_eq!(typed.code(), "LEARN_PROMOTE_SENSITIVE_REQUIRES_FORCE");
    require_force_for_sensitive_promotion(&e, true).expect("force should pass");
    e.sensitivity_flags.contains_secrets_suspected = false;
    require_force_for_sensitive_promotion(&e, false).expect("non-sensitive should pass");
}

#[test]
fn capture_persists_sensitivity_flags_from_secret_patterns() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let out = capture_learning_entry(
        &state_dir,
        CaptureLearningInput {
            category: LearningCategoryV1::PromptGuidance,
            summary: format!("Contains {}", secret_ghp()),
            ..CaptureLearningInput::default()
        },
    )
    .expect("capture");
    assert!(out.entry.sensitivity_flags.contains_secrets_suspected);
}

#[test]
fn build_sensitivity_scan_bundle_is_bounded() {
    let summary = "x".repeat(MAX_SCAN_BUNDLE_BYTES * 2);
    let bundle = build_sensitivity_scan_bundle(
        &summary,
        &LearningSourceV1::default(),
        &[],
        &ProposedMemoryV1::default(),
    );
    assert!(bundle.len() <= MAX_SCAN_BUNDLE_BYTES + "\n...[truncated]".len());
}

fn sample_check_candidate_learning_entry() -> LearningEntryV1 {
    let mut e = sample_entry();
    e.id = "01JPR3ENTRY".to_string();
    e.summary = "Ensure output includes success marker".to_string();
    e.proposed_memory.check_text = Some("Check body line 1\nCheck body line 2\n".to_string());
    e
}

fn read_learning_events_lines(state_dir: &Path) -> Vec<String> {
    let path = learning_events_path(state_dir);
    if !path.exists() {
        return Vec::new();
    }
    fs::read_to_string(path)
        .expect("read events")
        .lines()
        .map(|s| s.to_string())
        .collect()
}

fn collect_state_files(state_dir: &Path) -> BTreeSet<String> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeSet<String>) {
        if let Ok(rd) = fs::read_dir(dir) {
            for ent in rd.flatten() {
                let path = ent.path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else if path.is_file() {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.insert(rel);
                }
            }
        }
    }
    let mut out = BTreeSet::new();
    if state_dir.exists() {
        walk(state_dir, state_dir, &mut out);
    }
    out
}

#[test]
fn render_learning_to_check_markdown_is_deterministic_and_canonical() {
    let e = sample_check_candidate_learning_entry();
    let a = render_learning_to_check_markdown(&e, "my_check").expect("render a");
    let b = render_learning_to_check_markdown(&e, "my_check").expect("render b");
    assert_eq!(a, b);
    assert!(a.contains("\nallowed_tools: []\n"));
    assert!(a.ends_with('\n'));
    assert!(!a.contains("\r\n"));
    let i_schema = a.find("schema_version: 1\n").expect("schema");
    let i_name = a.find("\nname: ").expect("name");
    let i_desc = a.find("\ndescription: ").expect("desc");
    let i_required = a.find("\nrequired: false\n").expect("required");
    let i_allowed = a.find("\nallowed_tools: []\n").expect("allowed");
    let i_pass = a.find("\npass_criteria:\n").expect("pass");
    assert!(i_schema < i_name);
    assert!(i_name < i_desc);
    assert!(i_desc < i_required);
    assert!(i_required < i_allowed);
    assert!(i_allowed < i_pass);
    assert!(!a.contains("\nrequired_flags:"));
    assert!(!a.contains("\nbudget:"));
}

#[test]
fn promote_to_check_creates_target_file_and_updates_status() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut e = sample_check_candidate_learning_entry();
    e.entry_hash_hex = compute_entry_hash_hex(&e).expect("hash");
    write_entry(&state_dir, e.clone());

    let out = promote_learning_to_check(&state_dir, &e.id, "my_check", false).expect("promote");
    assert_eq!(out.slug, "my_check");
    assert!(out.target_path.exists());

    let updated = load_learning_entry(&state_dir, &e.id).expect("load updated");
    assert_eq!(updated.status, LearningStatusV1::Promoted);
}

#[test]
fn promote_to_check_enforces_sensitive_requires_force() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut e = sample_check_candidate_learning_entry();
    e.sensitivity_flags.contains_secrets_suspected = true;
    e.entry_hash_hex = compute_entry_hash_hex(&e).expect("hash");
    write_entry(&state_dir, e.clone());

    let err =
        promote_learning_to_check(&state_dir, &e.id, "secure_check", false).expect_err("must fail");
    let typed = err
        .downcast_ref::<LearningPromoteError>()
        .expect("typed promote error");
    assert_eq!(typed.code(), LEARN_PROMOTE_SENSITIVE_REQUIRES_FORCE);

    let ok = promote_learning_to_check(&state_dir, &e.id, "secure_check", true);
    assert!(ok.is_ok());
}

#[test]
fn promote_to_check_enforces_overwrite_requires_force() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut e = sample_check_candidate_learning_entry();
    e.entry_hash_hex = compute_entry_hash_hex(&e).expect("hash");
    write_entry(&state_dir, e.clone());
    let target = learning_check_path(&state_dir, "dup");
    fs::create_dir_all(target.parent().expect("parent")).expect("mkdirs");
    fs::write(&target, "existing").expect("seed target");

    let err = promote_learning_to_check(&state_dir, &e.id, "dup", false).expect_err("must fail");
    let typed = err
        .downcast_ref::<LearningPromoteError>()
        .expect("typed promote error");
    assert_eq!(typed.code(), LEARN_PROMOTE_TARGET_EXISTS_REQUIRES_FORCE);

    promote_learning_to_check(&state_dir, &e.id, "dup", true).expect("overwrite promote");
    let body = fs::read_to_string(&target).expect("read target");
    assert!(body.contains("allowed_tools: []"));
}

#[test]
fn promote_to_check_rejects_invalid_slug_with_stable_code() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut e = sample_check_candidate_learning_entry();
    e.entry_hash_hex = compute_entry_hash_hex(&e).expect("hash");
    write_entry(&state_dir, e.clone());

    let err =
        promote_learning_to_check(&state_dir, &e.id, "../bad", false).expect_err("invalid slug");
    let typed = err
        .downcast_ref::<LearningPromoteError>()
        .expect("typed promote error");
    assert_eq!(typed.code(), LEARN_PROMOTE_INVALID_SLUG);
}

#[test]
fn promote_to_check_emits_learning_promoted_event_with_target_file_hash() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut e = sample_check_candidate_learning_entry();
    e.entry_hash_hex = compute_entry_hash_hex(&e).expect("hash");
    write_entry(&state_dir, e.clone());

    let out = promote_learning_to_check(&state_dir, &e.id, "event_check", false).expect("promote");
    let lines = read_learning_events_lines(&state_dir);
    let last = lines.last().expect("event line");
    let v: serde_json::Value = serde_json::from_str(last).expect("parse event");
    assert_eq!(v["kind"], "learning_promoted");
    assert_eq!(v["data"]["schema"], LEARNING_PROMOTED_SCHEMA_V1);
    assert_eq!(v["data"]["learning_id"], e.id);
    assert_eq!(v["data"]["target"], "check");
    assert_eq!(v["data"]["slug"], "event_check");
    assert_eq!(
        v["data"]["target_file_sha256_hex"],
        out.target_file_sha256_hex
    );
}

#[test]
fn promote_to_check_failed_check_write_is_atomic_no_status_no_event() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut e = sample_check_candidate_learning_entry();
    e.entry_hash_hex = compute_entry_hash_hex(&e).expect("hash");
    write_entry(&state_dir, e.clone());

    let checks_path = state_dir.join("checks");
    if let Some(parent) = checks_path.parent() {
        fs::create_dir_all(parent).expect("parent");
    }
    fs::write(&checks_path, "not a dir").expect("poison checks path");

    let err = promote_learning_to_check(&state_dir, &e.id, "will_fail", false)
        .expect_err("write should fail");
    assert!(err.to_string().contains("failed to create check dir"));

    let updated = load_learning_entry(&state_dir, &e.id).expect("reload");
    assert_eq!(updated.status, LearningStatusV1::Captured);
    assert!(read_learning_events_lines(&state_dir).is_empty());
}

#[test]
fn promote_to_check_path_safety_only_expected_files_modified() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut e = sample_check_candidate_learning_entry();
    e.entry_hash_hex = compute_entry_hash_hex(&e).expect("hash");
    write_entry(&state_dir, e.clone());

    let before = collect_state_files(&state_dir);
    assert_eq!(
        before,
        BTreeSet::from([format!("learn/entries/{}.json", e.id)])
    );

    let _ = promote_learning_to_check(&state_dir, &e.id, "safe_paths", false).expect("promote");
    let after = collect_state_files(&state_dir);
    let expected = BTreeSet::from([
        format!("learn/entries/{}.json", e.id),
        "learn/events.jsonl".to_string(),
        "checks/safe_paths.md".to_string(),
    ]);
    assert_eq!(after, expected);
}

#[test]
fn promote_to_check_generated_file_loads_as_schema_valid_check() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut e = sample_check_candidate_learning_entry();
    e.entry_hash_hex = compute_entry_hash_hex(&e).expect("hash");
    write_entry(&state_dir, e.clone());

    promote_learning_to_check(&state_dir, &e.id, "schema_valid", false).expect("promote");
    let loaded = crate::checks::loader::load_checks(tmp.path(), None);
    assert!(loaded.errors.is_empty(), "errors: {:?}", loaded.errors);
    let check = loaded
        .checks
        .iter()
        .find(|c| c.path == ".localagent/checks/schema_valid.md")
        .expect("generated check");
    assert_eq!(check.frontmatter.schema_version, 1);
    assert_eq!(check.frontmatter.allowed_tools, Some(vec![]));
    assert_eq!(check.frontmatter.required_flags, Vec::<String>::new());
}

#[test]
fn managed_insert_creates_section_when_missing() {
    let e = sample_check_candidate_learning_entry();
    let block = render_learning_to_guidance_block(&e, false);
    let out = insert_managed_learning_block("", &e.id, &block);
    assert!(out.changed);
    assert!(!out.already_present);
    assert!(out
        .text
        .starts_with(LEARNED_GUIDANCE_MANAGED_SECTION_MARKER));
    assert!(out.text.contains(&format!("### LEARN-{}", e.id)));
    assert!(out.text.ends_with('\n'));
}

#[test]
fn managed_insert_is_idempotent_for_same_learning_id() {
    let e = sample_check_candidate_learning_entry();
    let block = render_learning_to_guidance_block(&e, true);
    let a = insert_managed_learning_block("", &e.id, &block);
    let b = insert_managed_learning_block(&a.text, &e.id, &block);
    assert!(a.changed);
    assert!(!a.already_present);
    assert!(!b.changed);
    assert!(b.already_present);
    assert_eq!(a.text, b.text);
    assert_eq!(b.text.matches(&format!("### LEARN-{}", e.id)).count(), 1);
}

#[test]
fn managed_insert_preserves_unmanaged_content_outside_section() {
    let e1 = sample_check_candidate_learning_entry();
    let mut e2 = sample_check_candidate_learning_entry();
    e2.id = "01JPR4OTHER".to_string();
    e2.entry_hash_hex = compute_entry_hash_hex(&e2).expect("hash");

    let existing_block = render_learning_to_guidance_block(&e1, false);
    let new_block = render_learning_to_guidance_block(&e2, true);
    let original = format!(
            "PRELUDE line 1\nPRELUDE line 2\n\n{marker}\n\n{existing}## User Section\nkeep this exact\n",
            marker = LEARNED_GUIDANCE_MANAGED_SECTION_MARKER,
            existing = existing_block
        );

    let out = insert_managed_learning_block(&original, &e2.id, &new_block);
    assert!(out.changed);
    assert!(!out.already_present);
    assert!(out.text.starts_with("PRELUDE line 1\nPRELUDE line 2\n\n"));
    assert!(out.text.contains("\n## User Section\nkeep this exact\n"));
    assert_eq!(
        out.text
            .matches(LEARNED_GUIDANCE_MANAGED_SECTION_MARKER)
            .count(),
        1
    );
    assert_eq!(out.text.matches(&format!("### LEARN-{}", e1.id)).count(), 1);
    assert_eq!(out.text.matches(&format!("### LEARN-{}", e2.id)).count(), 1);
}

#[test]
fn promote_to_agents_creates_agents_md_and_emits_event() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut e = sample_check_candidate_learning_entry();
    e.proposed_memory.guidance_text = Some("Use ripgrep before grep.\n".to_string());
    e.entry_hash_hex = compute_entry_hash_hex(&e).expect("hash");
    write_entry(&state_dir, e.clone());

    let out = promote_learning_to_agents(&state_dir, &e.id, false).expect("promote agents");
    assert_eq!(out.target, "agents");
    assert!(out.changed);
    assert!(!out.noop);
    let agents = tmp.path().join("AGENTS.md");
    let text = fs::read_to_string(&agents).expect("read agents");
    assert!(text.contains(LEARNED_GUIDANCE_MANAGED_SECTION_MARKER));
    assert!(text.contains(&format!("### LEARN-{}", e.id)));

    let updated = load_learning_entry(&state_dir, &e.id).expect("load updated");
    assert_eq!(updated.status, LearningStatusV1::Promoted);

    let lines = read_learning_events_lines(&state_dir);
    let v: serde_json::Value =
        serde_json::from_str(lines.last().expect("event line")).expect("event json");
    assert_eq!(v["data"]["target"], "agents");
    assert_eq!(v["data"]["target_path"], "AGENTS.md");
}

#[test]
fn promote_to_agents_rerun_is_noop_and_does_not_emit_event() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut e = sample_check_candidate_learning_entry();
    e.proposed_memory.guidance_text = Some("Always confirm assumptions.\n".to_string());
    e.entry_hash_hex = compute_entry_hash_hex(&e).expect("hash");
    write_entry(&state_dir, e.clone());

    let first = promote_learning_to_agents(&state_dir, &e.id, false).expect("first");
    assert!(first.changed);
    let event_count_before = read_learning_events_lines(&state_dir).len();

    let second = promote_learning_to_agents(&state_dir, &e.id, false).expect("second");
    assert!(!second.changed);
    assert!(second.noop);
    assert_eq!(
        read_learning_events_lines(&state_dir).len(),
        event_count_before
    );
    let msg = render_promote_to_target_confirmation(&second);
    assert!(msg.contains("Already promoted (noop)"));
}

#[test]
fn pack_id_validation_rejects_invalid_and_allows_hierarchical_safe_segments() {
    for bad in [
        "",
        "../x",
        "x/../y",
        "/abs",
        "web\\play",
        "web//play",
        "UPPER",
    ] {
        let err = validate_promote_pack_id(bad).expect_err("invalid pack id");
        let typed = err
            .downcast_ref::<LearningPromoteError>()
            .expect("typed pack id error");
        assert_eq!(typed.code(), LEARN_PROMOTE_INVALID_PACK_ID);
    }
    validate_promote_pack_id("web/playwright").expect("valid hierarchical");
    validate_promote_pack_id("a_b/c-d").expect("valid segments");
}

#[test]
fn promote_to_pack_creates_nested_pack_md_and_emits_event_with_pack_id() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut e = sample_check_candidate_learning_entry();
    e.proposed_memory.guidance_text = Some("Use Playwright MCP for browser checks.\n".to_string());
    e.entry_hash_hex = compute_entry_hash_hex(&e).expect("hash");
    write_entry(&state_dir, e.clone());

    let out =
        promote_learning_to_pack(&state_dir, &e.id, "web/playwright", false).expect("promote pack");
    assert_eq!(out.target, "pack");
    assert_eq!(out.pack_id.as_deref(), Some("web/playwright"));
    let pack_md = state_dir
        .join("packs")
        .join("web")
        .join("playwright")
        .join("PACK.md");
    assert!(pack_md.exists());
    let text = fs::read_to_string(&pack_md).expect("read pack");
    assert!(text.contains(LEARNED_GUIDANCE_MANAGED_SECTION_MARKER));
    assert!(text.contains(&format!("### LEARN-{}", e.id)));

    let lines = read_learning_events_lines(&state_dir);
    let v: serde_json::Value =
        serde_json::from_str(lines.last().expect("event line")).expect("event json");
    assert_eq!(v["data"]["target"], "pack");
    assert_eq!(v["data"]["pack_id"], "web/playwright");
    assert_eq!(
        v["data"]["target_path"],
        ".localagent/packs/web/playwright/PACK.md"
    );
}

#[test]
fn promote_to_pack_path_safety_only_expected_files_modified() {
    let tmp = tempdir().expect("tempdir");
    let state_dir = tmp.path().join(".localagent");
    let mut e = sample_check_candidate_learning_entry();
    e.entry_hash_hex = compute_entry_hash_hex(&e).expect("hash");
    write_entry(&state_dir, e.clone());

    let _ =
        promote_learning_to_pack(&state_dir, &e.id, "web/playwright", false).expect("promote pack");
    let after = collect_state_files(&state_dir);
    let expected = BTreeSet::from([
        format!("learn/entries/{}.json", e.id),
        "learn/events.jsonl".to_string(),
        "packs/web/playwright/PACK.md".to_string(),
    ]);
    assert_eq!(after, expected);
}
