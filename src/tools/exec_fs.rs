use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use globset::Glob;
use regex::RegexBuilder;
use serde_json::{json, Value};

use crate::target::{ListReq, ReadReq};
use crate::types::SideEffects;

use super::{
    base_meta, failed_exec, has_git_segment, invalid_args_detail, path_is_workdir_scoped,
    target_to_exec, ToolErrorCode, ToolErrorDetail, ToolExecution, ToolResultMeta, ToolRuntime,
    ToolWarningDetail,
};

type SearchFileEntry = (String, PathBuf);
type SearchFileList = Vec<SearchFileEntry>;
type SearchWarnings = Vec<ToolWarningDetail>;
type CollectSearchFilesResult = Result<(SearchFileList, SearchWarnings), Box<ToolExecution>>;

pub(super) async fn run_list_dir(rt: &ToolRuntime, args: &Value) -> ToolExecution {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    if !path_is_workdir_scoped(path) && !rt.unsafe_bypass_allow_flags {
        return failed_exec(
            rt,
            SideEffects::FilesystemRead,
            "path must stay within workdir (no absolute paths or '..' traversal)".to_string(),
            Some(ToolErrorDetail {
                code: ToolErrorCode::ToolPathDenied,
                message: "Path must stay within workdir.".to_string(),
                expected_schema: None,
                received_args: Some(args.clone()),
                minimal_example: Some(json!({"path":"."})),
                available_tools: None,
            }),
        );
    }
    let out = rt
        .exec_target
        .list_dir(ListReq {
            workdir: rt.workdir.clone(),
            path: path.to_string(),
        })
        .await;
    target_to_exec(SideEffects::FilesystemRead, out)
}

pub(super) async fn run_read_file(rt: &ToolRuntime, args: &Value) -> ToolExecution {
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if !path_is_workdir_scoped(path) && !rt.unsafe_bypass_allow_flags {
        return failed_exec(
            rt,
            SideEffects::FilesystemRead,
            "path must stay within workdir (no absolute paths or '..' traversal)".to_string(),
            Some(ToolErrorDetail {
                code: ToolErrorCode::ToolPathDenied,
                message: "Path must stay within workdir.".to_string(),
                expected_schema: None,
                received_args: Some(args.clone()),
                minimal_example: Some(json!({"path":"src/main.rs"})),
                available_tools: None,
            }),
        );
    }
    let out = rt
        .exec_target
        .read_file(ReadReq {
            workdir: rt.workdir.clone(),
            path: path.to_string(),
            max_read_bytes: rt.max_read_bytes,
        })
        .await;
    target_to_exec(SideEffects::FilesystemRead, out)
}

pub(super) async fn run_glob(rt: &ToolRuntime, args: &Value) -> ToolExecution {
    let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return failed_exec(
                rt,
                SideEffects::FilesystemRead,
                "invalid tool arguments: pattern must be a non-empty string".to_string(),
                Some(invalid_args_detail(
                    "glob",
                    args,
                    "pattern must be a non-empty string",
                )),
            )
        }
    };
    let max_results = match max_results_from_args(args) {
        Ok(v) => v,
        Err(e) => {
            return failed_exec(
                rt,
                SideEffects::FilesystemRead,
                format!("invalid tool arguments: {e}"),
                Some(invalid_args_detail("glob", args, &e)),
            )
        }
    };
    let matcher = match Glob::new(pattern) {
        Ok(g) => g.compile_matcher(),
        Err(e) => {
            return failed_exec(
                rt,
                SideEffects::FilesystemRead,
                format!("invalid pattern: {e}"),
                Some(ToolErrorDetail {
                    code: ToolErrorCode::InvalidPattern,
                    message: format!("Invalid pattern: {e}"),
                    expected_schema: super::compact_builtin_schema("glob"),
                    received_args: Some(args.clone()),
                    minimal_example: super::minimal_builtin_example("glob"),
                    available_tools: None,
                }),
            )
        }
    };

    let (files, warnings) = match collect_search_files(rt, search_path_from_args(args)) {
        Ok(v) => v,
        Err(exec) => return *exec,
    };
    let mut matches = files
        .into_iter()
        .filter_map(|(rel, _)| if matcher.is_match(&rel) { Some(rel) } else { None })
        .collect::<Vec<_>>();
    matches.sort();
    let total = matches.len();
    let truncated = total > max_results;
    if truncated {
        matches.truncate(max_results);
    }
    let content = json!({
        "matches": matches,
        "match_count": total,
        "truncated": truncated,
        "max_results": max_results
    })
    .to_string();
    let mut meta = base_meta(rt, SideEffects::FilesystemRead);
    attach_warnings(&mut meta, warnings);
    ToolExecution {
        ok: true,
        content,
        truncated: false,
        error: None,
        meta,
    }
}

pub(super) async fn run_grep(rt: &ToolRuntime, args: &Value) -> ToolExecution {
    let pattern = match args.get("pattern").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return failed_exec(
                rt,
                SideEffects::FilesystemRead,
                "invalid tool arguments: pattern must be a non-empty string".to_string(),
                Some(invalid_args_detail(
                    "grep",
                    args,
                    "pattern must be a non-empty string",
                )),
            )
        }
    };
    let max_results = match max_results_from_args(args) {
        Ok(v) => v,
        Err(e) => {
            return failed_exec(
                rt,
                SideEffects::FilesystemRead,
                format!("invalid tool arguments: {e}"),
                Some(invalid_args_detail("grep", args, &e)),
            )
        }
    };
    let ignore_case = args
        .get("ignore_case")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let re = match RegexBuilder::new(pattern)
        .case_insensitive(ignore_case)
        .build()
    {
        Ok(v) => v,
        Err(e) => {
            return failed_exec(
                rt,
                SideEffects::FilesystemRead,
                format!("invalid pattern: {e}"),
                Some(ToolErrorDetail {
                    code: ToolErrorCode::InvalidPattern,
                    message: format!("Invalid pattern: {e}"),
                    expected_schema: super::compact_builtin_schema("grep"),
                    received_args: Some(args.clone()),
                    minimal_example: super::minimal_builtin_example("grep"),
                    available_tools: None,
                }),
            )
        }
    };
    let (files, warnings) = match collect_search_files(rt, search_path_from_args(args)) {
        Ok(v) => v,
        Err(exec) => return *exec,
    };

    let mut skipped_non_text = 0usize;
    let mut matches = Vec::new();
    for (rel, abs) in files {
        let bytes = match std::fs::read(&abs) {
            Ok(v) => v,
            Err(e) => {
                return failed_exec(
                    rt,
                    SideEffects::FilesystemRead,
                    format!("io error: failed to read {}: {e}", abs.display()),
                    Some(ToolErrorDetail {
                        code: ToolErrorCode::IoError,
                        message: format!("failed to read {}", abs.display()),
                        expected_schema: None,
                        received_args: Some(args.clone()),
                        minimal_example: super::minimal_builtin_example("grep"),
                        available_tools: None,
                    }),
                );
            }
        };
        if bytes.contains(&0) {
            skipped_non_text += 1;
            continue;
        }
        let text = match std::str::from_utf8(&bytes) {
            Ok(v) => v,
            Err(_) => {
                skipped_non_text += 1;
                continue;
            }
        };
        for (idx, line) in text.split('\n').enumerate() {
            let line_text = line.strip_suffix('\r').unwrap_or(line);
            for m in re.find_iter(line_text) {
                matches.push(json!({
                    "path": rel,
                    "line": idx + 1,
                    "column": m.start() + 1,
                    "text": line_text
                }));
            }
        }
    }
    matches.sort_by(|a, b| {
        let ap = a.get("path").and_then(|v| v.as_str()).unwrap_or_default();
        let bp = b.get("path").and_then(|v| v.as_str()).unwrap_or_default();
        let al = a.get("line").and_then(|v| v.as_u64()).unwrap_or(0);
        let bl = b.get("line").and_then(|v| v.as_u64()).unwrap_or(0);
        let ac = a.get("column").and_then(|v| v.as_u64()).unwrap_or(0);
        let bc = b.get("column").and_then(|v| v.as_u64()).unwrap_or(0);
        let at = a.get("text").and_then(|v| v.as_str()).unwrap_or_default();
        let bt = b.get("text").and_then(|v| v.as_str()).unwrap_or_default();
        ap.cmp(bp)
            .then(al.cmp(&bl))
            .then(ac.cmp(&bc))
            .then(at.cmp(bt))
    });
    let total = matches.len();
    let truncated = total > max_results;
    if truncated {
        matches.truncate(max_results);
    }
    let content = json!({
        "matches": matches,
        "match_count": total,
        "truncated": truncated,
        "max_results": max_results,
        "skipped_binary_or_non_utf8_files": skipped_non_text
    })
    .to_string();
    let mut meta = base_meta(rt, SideEffects::FilesystemRead);
    attach_warnings(&mut meta, warnings);
    ToolExecution {
        ok: true,
        content,
        truncated: false,
        error: None,
        meta,
    }
}

fn search_path_from_args(args: &Value) -> &str {
    args.get("path").and_then(|v| v.as_str()).unwrap_or(".")
}

fn max_results_from_args(args: &Value) -> Result<usize, String> {
    let raw = args
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(200);
    if !(1..=1000).contains(&raw) {
        return Err("max_results must be between 1 and 1000".to_string());
    }
    Ok(raw as usize)
}

fn normalize_rel_path(path: &Path) -> String {
    let mut parts = Vec::new();
    for comp in path.components() {
        if let std::path::Component::Normal(s) = comp {
            parts.push(s.to_string_lossy().to_string());
        }
    }
    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

fn attach_warnings(meta: &mut ToolResultMeta, mut warnings: Vec<ToolWarningDetail>) {
    warnings.sort_by(|a, b| a.path.cmp(&b.path).then(a.code.cmp(&b.code)));
    let warnings_max = 50usize;
    let truncated = warnings.len() > warnings_max;
    if truncated {
        warnings.truncate(warnings_max);
    }
    if !warnings.is_empty() {
        meta.warnings = Some(warnings);
        meta.warnings_max = Some(warnings_max as u32);
        meta.warnings_truncated = Some(truncated);
    }
}

fn collect_search_files(rt: &ToolRuntime, search_path: &str) -> CollectSearchFilesResult {
    if !path_is_workdir_scoped(search_path) && !rt.unsafe_bypass_allow_flags {
        return Err(Box::new(failed_exec(
            rt,
            SideEffects::FilesystemRead,
            "path must stay within workdir (no absolute paths or '..' traversal)".to_string(),
            Some(ToolErrorDetail {
                code: ToolErrorCode::PathOutOfScope,
                message: "Path must stay within workdir.".to_string(),
                expected_schema: None,
                received_args: Some(json!({"path": search_path})),
                minimal_example: Some(json!({"path":"."})),
                available_tools: None,
            }),
        )));
    }

    let base = rt.workdir.join(search_path);
    if !base.exists() {
        return Err(Box::new(failed_exec(
            rt,
            SideEffects::FilesystemRead,
            format!("io error: path does not exist: {}", base.display()),
            Some(ToolErrorDetail {
                code: ToolErrorCode::IoError,
                message: format!("Path does not exist: {}", base.display()),
                expected_schema: None,
                received_args: Some(json!({"path": search_path})),
                minimal_example: Some(json!({"path":"."})),
                available_tools: None,
            }),
        )));
    }

    let canonical_workdir = std::fs::canonicalize(&rt.workdir).unwrap_or(rt.workdir.clone());
    let mut warnings = Vec::new();
    let mut files = Vec::new();
    let mut stack = vec![base];
    let mut seen_dirs = BTreeSet::new();

    while let Some(current) = stack.pop() {
        let metadata = match std::fs::symlink_metadata(&current) {
            Ok(m) => m,
            Err(e) => {
                return Err(Box::new(failed_exec(
                    rt,
                    SideEffects::FilesystemRead,
                    format!("io error: failed to stat {}: {e}", current.display()),
                    Some(ToolErrorDetail {
                        code: ToolErrorCode::IoError,
                        message: format!("failed to stat {}", current.display()),
                        expected_schema: None,
                        received_args: Some(json!({"path": search_path})),
                        minimal_example: Some(json!({"path":"."})),
                        available_tools: None,
                    }),
                )));
            }
        };

        let rel = current
            .strip_prefix(&rt.workdir)
            .map(normalize_rel_path)
            .unwrap_or_else(|_| normalize_rel_path(&current));

        if metadata.file_type().is_symlink() {
            if let Ok(target) = std::fs::canonicalize(&current) {
                if !target.starts_with(&canonical_workdir) {
                    warnings.push(ToolWarningDetail {
                        code: "symlink_out_of_scope_skipped".to_string(),
                        path: rel,
                        target: "OUT_OF_SCOPE".to_string(),
                        reason: "target escapes workdir".to_string(),
                    });
                }
            }
            continue;
        }

        if metadata.is_dir() {
            if rel != "." && has_git_segment(Path::new(&rel)) {
                continue;
            }
            let canonical_dir = std::fs::canonicalize(&current).unwrap_or_else(|_| current.clone());
            if !seen_dirs.insert(canonical_dir) {
                continue;
            }
            let rd = match std::fs::read_dir(&current) {
                Ok(v) => v,
                Err(e) => {
                    return Err(Box::new(failed_exec(
                        rt,
                        SideEffects::FilesystemRead,
                        format!(
                            "io error: failed to read directory {}: {e}",
                            current.display()
                        ),
                        Some(ToolErrorDetail {
                            code: ToolErrorCode::IoError,
                            message: format!("failed to read directory {}", current.display()),
                            expected_schema: None,
                            received_args: Some(json!({"path": search_path})),
                            minimal_example: Some(json!({"path":"."})),
                            available_tools: None,
                        }),
                    )));
                }
            };
            let mut children = Vec::new();
            for entry in rd.flatten() {
                children.push(entry.path());
            }
            children.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
            while let Some(child) = children.pop() {
                stack.push(child);
            }
            continue;
        }

        if metadata.is_file() {
            if rel != "." && has_git_segment(Path::new(&rel)) {
                continue;
            }
            files.push((rel, current));
        }
    }

    Ok((files, warnings))
}
