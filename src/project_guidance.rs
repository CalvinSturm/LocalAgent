use std::path::{Path, PathBuf};

use crate::store::sha256_hex;

#[derive(Debug, Clone)]
pub struct ProjectGuidanceSource {
    pub path: String,
    pub sha256_hex: String,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
pub struct ResolvedProjectGuidance {
    pub sources: Vec<ProjectGuidanceSource>,
    pub merged_text: String,
    pub truncated: bool,
    pub bytes_loaded: u64,
    pub bytes_kept: u64,
    pub guidance_hash_hex: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ProjectGuidanceLimits {
    pub max_total_bytes: usize,
}

impl Default for ProjectGuidanceLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: 32 * 1024,
        }
    }
}

pub fn resolve_project_guidance(
    workdir: &Path,
    limits: ProjectGuidanceLimits,
) -> anyhow::Result<ResolvedProjectGuidance> {
    let workdir = std::fs::canonicalize(workdir).unwrap_or_else(|_| workdir.to_path_buf());
    let git_root = discover_git_root(&workdir);
    let discovered = discover_agents_files(&workdir, git_root.as_deref())?;

    let mut sources = Vec::new();
    let mut sections = Vec::new();
    let mut bytes_loaded: u64 = 0;
    for file_path in discovered {
        let raw = std::fs::read(&file_path)?;
        let rel = render_guidance_path(&file_path, git_root.as_deref(), &workdir);
        bytes_loaded = bytes_loaded.saturating_add(raw.len() as u64);
        let normalized = normalize_newlines(&String::from_utf8_lossy(&raw));
        let section = format!("## AGENTS.md: {rel}\n\n{normalized}");
        sections.push(section);
        sources.push(ProjectGuidanceSource {
            path: rel,
            sha256_hex: sha256_hex(&raw),
            bytes: raw.len() as u64,
        });
    }
    let merged = sections.join("\n\n");
    let (merged_text, truncated) = truncate_utf8_to_bytes(&merged, limits.max_total_bytes);
    let bytes_kept = merged_text.len() as u64;
    let guidance_hash_hex = sha256_hex(merged_text.as_bytes());
    Ok(ResolvedProjectGuidance {
        sources,
        merged_text,
        truncated,
        bytes_loaded,
        bytes_kept,
        guidance_hash_hex,
    })
}

pub fn project_guidance_message(g: &ResolvedProjectGuidance) -> Option<crate::types::Message> {
    if g.merged_text.is_empty() {
        return None;
    }
    Some(crate::types::Message {
        role: crate::types::Role::Developer,
        content: Some(format!(
            "Project guidance (AGENTS.md):\n\n{}",
            g.merged_text
        )),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
    })
}

pub fn render_project_guidance_text(g: &ResolvedProjectGuidance) -> String {
    let mut out = String::new();
    out.push_str(&format!("guidance_hash_hex: {}\n", g.guidance_hash_hex));
    out.push_str(&format!("truncated: {}\n", g.truncated));
    out.push_str(&format!("bytes_loaded: {}\n", g.bytes_loaded));
    out.push_str(&format!("bytes_kept: {}\n", g.bytes_kept));
    out.push_str("sources:\n");
    if g.sources.is_empty() {
        out.push_str("  - none\n");
    } else {
        for s in &g.sources {
            out.push_str(&format!(
                "  - {} (bytes={}, sha256={})\n",
                s.path, s.bytes, s.sha256_hex
            ));
        }
    }
    out.push_str("merged_preview:\n");
    if g.merged_text.is_empty() {
        out.push_str("  (none)");
    } else {
        for line in g.merged_text.lines() {
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }
        if out.ends_with('\n') {
            out.pop();
        }
    }
    out
}

fn discover_agents_files(workdir: &Path, git_root: Option<&Path>) -> anyhow::Result<Vec<PathBuf>> {
    let mut chain = Vec::new();
    let mut cur = Some(workdir);
    while let Some(dir) = cur {
        chain.push(dir.to_path_buf());
        if git_root.is_some_and(|r| r == dir) {
            break;
        }
        cur = dir.parent();
    }
    chain.reverse(); // root -> leaf
    let mut out = Vec::new();
    for dir in chain {
        let p = dir.join("AGENTS.md");
        if p.is_file() {
            out.push(p);
        }
    }
    Ok(out)
}

fn discover_git_root(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        let marker = dir.join(".git");
        if marker.is_dir() || marker.is_file() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

fn render_guidance_path(path: &Path, git_root: Option<&Path>, workdir: &Path) -> String {
    let base = git_root.unwrap_or(workdir);
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn normalize_newlines(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

fn truncate_utf8_to_bytes(input: &str, max_bytes: usize) -> (String, bool) {
    if input.len() <= max_bytes {
        return (input.to_string(), false);
    }
    let mut end = max_bytes.min(input.len());
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    (input[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{resolve_project_guidance, ProjectGuidanceLimits};

    #[test]
    fn discovers_root_to_leaf_and_stops_at_git_root_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("repo");
        let leaf = root.join("a").join("b");
        fs::create_dir_all(root.join(".git")).expect("git root");
        fs::create_dir_all(&leaf).expect("leaf");
        fs::write(root.join("AGENTS.md"), "root rules").expect("root agents");
        fs::write(root.join("a").join("AGENTS.md"), "a rules").expect("a agents");
        fs::write(leaf.join("AGENTS.md"), "b rules").expect("b agents");

        let g = resolve_project_guidance(
            &leaf,
            ProjectGuidanceLimits {
                max_total_bytes: 10_000,
            },
        )
        .expect("resolve");
        let paths = g
            .sources
            .iter()
            .map(|s| s.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(paths, vec!["AGENTS.md", "a/AGENTS.md", "a/b/AGENTS.md"]);
        let merged = &g.merged_text;
        let i_root = merged.find("root rules").expect("root");
        let i_a = merged.find("a rules").expect("a");
        let i_b = merged.find("b rules").expect("b");
        assert!(i_root < i_a && i_a < i_b);
    }

    #[test]
    fn git_root_file_marker_is_supported() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("repo");
        let leaf = root.join("x");
        fs::create_dir_all(&leaf).expect("leaf");
        fs::write(root.join(".git"), "gitdir: /tmp/fake").expect(".git file");
        fs::write(root.join("AGENTS.md"), "root").expect("agents");
        fs::write(leaf.join("AGENTS.md"), "leaf").expect("agents");
        let g = resolve_project_guidance(
            &leaf,
            ProjectGuidanceLimits {
                max_total_bytes: 10_000,
            },
        )
        .expect("resolve");
        assert_eq!(g.sources.len(), 2);
        assert_eq!(g.sources[0].path, "AGENTS.md");
        assert_eq!(g.sources[1].path, "x/AGENTS.md");
    }

    #[test]
    fn no_git_root_walks_to_filesystem_root_deterministically() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let leaf = tmp.path().join("a").join("b");
        fs::create_dir_all(&leaf).expect("leaf");
        fs::write(tmp.path().join("AGENTS.md"), "root-ish").expect("agents");
        fs::write(leaf.join("AGENTS.md"), "leaf").expect("agents");
        let g = resolve_project_guidance(
            &leaf,
            ProjectGuidanceLimits {
                max_total_bytes: 10_000,
            },
        )
        .expect("resolve");
        assert_eq!(g.sources.len(), 2);
    }

    #[test]
    fn newline_normalization_makes_hash_stable_across_crlf() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let d1 = tmp.path().join("r1");
        let d2 = tmp.path().join("r2");
        fs::create_dir_all(&d1).expect("d1");
        fs::create_dir_all(&d2).expect("d2");
        fs::write(d1.join("AGENTS.md"), "a\r\nb\r\n").expect("crlf");
        fs::write(d2.join("AGENTS.md"), "a\nb\n").expect("lf");
        let g1 = resolve_project_guidance(
            &d1,
            ProjectGuidanceLimits {
                max_total_bytes: 10_000,
            },
        )
        .expect("g1");
        let g2 = resolve_project_guidance(
            &d2,
            ProjectGuidanceLimits {
                max_total_bytes: 10_000,
            },
        )
        .expect("g2");
        assert_eq!(g1.guidance_hash_hex, g2.guidance_hash_hex);
    }

    #[test]
    fn total_cap_truncates_utf8_safely() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("AGENTS.md"),
            format!("x{}y", "🙂".repeat(200)),
        )
        .expect("agents");
        let g = resolve_project_guidance(
            &tmp.path().to_path_buf(),
            ProjectGuidanceLimits {
                max_total_bytes: 40,
            },
        )
        .expect("resolve");
        assert!(g.truncated);
        assert!(std::str::from_utf8(g.merged_text.as_bytes()).is_ok());
        assert!(g.bytes_loaded >= g.bytes_kept);
    }
}
