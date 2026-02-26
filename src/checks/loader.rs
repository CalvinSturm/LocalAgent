use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Serialize;

use crate::checks::schema::{validate_frontmatter, CheckFrontmatter};
use crate::store::sha256_hex;

pub const CODE_DISCOVERY_IO_ERROR: &str = "CHECK_DISCOVERY_IO_ERROR";
pub const CODE_FILE_NOT_UTF8: &str = "CHECK_FILE_NOT_UTF8";
pub const CODE_FRONTMATTER_MISSING: &str = "CHECK_FRONTMATTER_MISSING";
pub const CODE_YAML_PARSE_ERROR: &str = "CHECK_YAML_PARSE_ERROR";
pub const CODE_SCHEMA_UNKNOWN_KEY: &str = "CHECK_SCHEMA_UNKNOWN_KEY";
pub const CODE_SCHEMA_MISSING_FIELD: &str = "CHECK_SCHEMA_MISSING_FIELD";
pub const CODE_DUPLICATE_NAME: &str = "CHECK_DUPLICATE_NAME";

#[derive(Debug, Clone)]
pub struct LoadedCheck {
    pub path: String,
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
    pub body: String,
    pub file_bytes_hash_hex: String,
    pub frontmatter_hash_hex: String,
    pub check_hash_hex: String,
    pub frontmatter: CheckFrontmatter,
}

#[derive(Debug, Clone)]
pub struct CheckLoadError {
    pub path: Option<String>,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct CheckLoadResult {
    pub checks: Vec<LoadedCheck>,
    pub errors: Vec<CheckLoadError>,
}

pub fn load_checks(root: &Path, dir_override: Option<&Path>) -> CheckLoadResult {
    let workdir = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let checks_dir = dir_override
        .map(|p| {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                workdir.join(p)
            }
        })
        .unwrap_or_else(|| workdir.join(".localagent").join("checks"));
    let mut out = CheckLoadResult::default();
    let mut files = Vec::new();
    if let Err(e) = discover_check_files(&workdir, &checks_dir, &mut files) {
        out.errors.push(CheckLoadError {
            path: Some(render_rel_path(&checks_dir, &workdir)),
            code: CODE_DISCOVERY_IO_ERROR.to_string(),
            message: e.to_string(),
        });
        return out;
    }
    files.sort();
    let mut names = BTreeSet::new();
    let mut by_name: BTreeMap<String, String> = BTreeMap::new();
    for file in files {
        match load_one_check(&workdir, &file) {
            Ok(c) => {
                if !names.insert(c.name.clone()) {
                    out.errors.push(CheckLoadError {
                        path: Some(c.path.clone()),
                        code: CODE_DUPLICATE_NAME.to_string(),
                        message: format!("duplicate check name '{}'", c.name),
                    });
                    if let Some(prev_path) = by_name.get(&c.name) {
                        out.errors.push(CheckLoadError {
                            path: Some(prev_path.clone()),
                            code: CODE_DUPLICATE_NAME.to_string(),
                            message: format!("duplicate check name '{}'", c.name),
                        });
                    }
                    continue;
                }
                by_name.insert(c.name.clone(), c.path.clone());
                out.checks.push(c);
            }
            Err(e) => out.errors.push(e),
        }
    }
    out
}

fn discover_check_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let mut ents = fs::read_dir(dir)
        .with_context(|| format!("read_dir {}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("iterate {}", dir.display()))?;
    ents.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
    for ent in ents {
        let path = ent.path();
        let md = fs::symlink_metadata(&path)?;
        if md.file_type().is_symlink() {
            continue;
        }
        if md.is_dir() {
            discover_check_files(root, &path, out)?;
            continue;
        }
        if !md.is_file() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let rel = render_rel_path(&path, root);
        if rel.starts_with('/') || rel.contains("..") {
            continue;
        }
        out.push(path);
    }
    Ok(())
}

fn load_one_check(root: &Path, path: &Path) -> Result<LoadedCheck, CheckLoadError> {
    let rel = render_rel_path(path, root);
    let raw = fs::read(path).map_err(|e| CheckLoadError {
        path: Some(rel.clone()),
        code: CODE_DISCOVERY_IO_ERROR.to_string(),
        message: e.to_string(),
    })?;
    let file_bytes_hash_hex = sha256_hex(&raw);
    let text = std::str::from_utf8(&raw).map_err(|_| CheckLoadError {
        path: Some(rel.clone()),
        code: CODE_FILE_NOT_UTF8.to_string(),
        message: "check file is not valid UTF-8".to_string(),
    })?;
    let (fm_text, body_text) = split_frontmatter(text).ok_or_else(|| CheckLoadError {
        path: Some(rel.clone()),
        code: CODE_FRONTMATTER_MISSING.to_string(),
        message: "missing YAML frontmatter delimited by ---".to_string(),
    })?;
    let frontmatter: CheckFrontmatter =
        serde_yaml::from_str(fm_text).map_err(|e| classify_yaml_error(&rel, &e.to_string()))?;
    if let Err(e) = validate_frontmatter(&frontmatter) {
        let msg = e.to_string();
        let code = if msg.contains("missing field") {
            CODE_SCHEMA_MISSING_FIELD
        } else {
            CODE_SCHEMA_UNKNOWN_KEY
        };
        return Err(CheckLoadError {
            path: Some(rel.clone()),
            code: code.to_string(),
            message: msg,
        });
    }

    let canonical_frontmatter = canonical_frontmatter_json(&frontmatter);
    let body = normalize_body(body_text);
    let frontmatter_hash_hex = sha256_hex(canonical_frontmatter.as_bytes());
    let check_hash_hex = sha256_hex(format!("{canonical_frontmatter}\n---\n{body}").as_bytes());
    Ok(LoadedCheck {
        path: rel,
        name: frontmatter.name.clone(),
        description: frontmatter.description.clone(),
        required: frontmatter.required,
        body,
        file_bytes_hash_hex,
        frontmatter_hash_hex,
        check_hash_hex,
        frontmatter,
    })
}

fn split_frontmatter(input: &str) -> Option<(&str, &str)> {
    let s = input
        .strip_prefix("---\n")
        .or_else(|| input.strip_prefix("---\r\n"))?;
    let end_idx = s.find("\n---\n").or_else(|| s.find("\r\n---\r\n"))?;
    let fm = &s[..end_idx];
    let rest = &s[end_idx..];
    let body = rest
        .strip_prefix("\n---\n")
        .or_else(|| rest.strip_prefix("\r\n---\r\n"))?;
    Some((fm, body))
}

fn normalize_body(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

fn classify_yaml_error(path: &str, msg: &str) -> CheckLoadError {
    let code = if msg.contains("missing field") {
        CODE_SCHEMA_MISSING_FIELD
    } else {
        CODE_YAML_PARSE_ERROR
    };
    CheckLoadError {
        path: Some(path.to_string()),
        code: code.to_string(),
        message: msg.to_string(),
    }
}

fn canonical_frontmatter_json(fm: &CheckFrontmatter) -> String {
    #[derive(Serialize)]
    struct Canonical<'a> {
        schema_version: u32,
        name: &'a str,
        description: &'a Option<String>,
        required: bool,
        allowed_tools: &'a Option<Vec<String>>,
        required_flags: &'a Vec<String>,
        pass_criteria: &'a crate::checks::schema::PassCriteria,
        budget: &'a Option<crate::checks::schema::CheckBudget>,
    }
    serde_json::to_string(&Canonical {
        schema_version: fm.schema_version,
        name: &fm.name,
        description: &fm.description,
        required: fm.required,
        allowed_tools: &fm.allowed_tools,
        required_flags: &fm.required_flags,
        pass_criteria: &fm.pass_criteria,
        budget: &fm.budget,
    })
    .expect("canonical frontmatter json")
}

fn render_rel_path(path: &Path, root: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{load_checks, CODE_DUPLICATE_NAME, CODE_FRONTMATTER_MISSING};

    #[test]
    fn loader_discovers_and_hashes_deterministically() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let checks = root.join(".localagent").join("checks");
        fs::create_dir_all(checks.join("nested")).expect("checks dir");
        fs::write(
            checks.join("b.md"),
            "---\nschema_version: 1\nname: b\npass_criteria:\n  type: output_contains\n  value: ok\n---\nhello\r\n",
        )
        .expect("b");
        fs::write(
            checks.join("nested").join("a.md"),
            "---\nschema_version: 1\nname: a\npass_criteria:\n  type: output_equals\n  value: ok\n---\nworld\n",
        )
        .expect("a");
        let out1 = load_checks(root, None);
        let out2 = load_checks(root, None);
        assert!(out1.errors.is_empty());
        assert!(out2.errors.is_empty());
        let names1 = out1
            .checks
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>();
        let names2 = out2
            .checks
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names1, vec!["b", "a"]);
        assert_eq!(names1, names2);
        assert_eq!(out1.checks[0].check_hash_hex, out2.checks[0].check_hash_hex);
        assert_eq!(out1.checks[0].body, "hello\n");
    }

    #[test]
    fn loader_reports_missing_frontmatter() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let checks = tmp.path().join(".localagent").join("checks");
        fs::create_dir_all(&checks).expect("checks");
        fs::write(checks.join("x.md"), "no frontmatter").expect("x");
        let out = load_checks(tmp.path(), None);
        assert!(out.checks.is_empty());
        assert_eq!(out.errors.len(), 1);
        assert_eq!(out.errors[0].code, CODE_FRONTMATTER_MISSING);
    }

    #[test]
    fn duplicate_names_fail_deterministically() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let checks = tmp.path().join(".localagent").join("checks");
        fs::create_dir_all(checks.join("z")).expect("checks");
        let body = "---\nschema_version: 1\nname: dup\npass_criteria:\n  type: output_contains\n  value: ok\n---\nbody\n";
        fs::write(checks.join("a.md"), body).expect("a");
        fs::write(checks.join("z").join("b.md"), body).expect("b");
        let out = load_checks(tmp.path(), None);
        assert!(out.errors.iter().any(|e| e.code == CODE_DUPLICATE_NAME));
    }
}
