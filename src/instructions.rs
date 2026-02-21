use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context};
use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};

use crate::store::sha256_hex;
use crate::types::{Message, Role};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstructionConfig {
    pub version: u32,
    #[serde(default)]
    pub base: Vec<InstructionMessage>,
    #[serde(default)]
    pub model_profiles: Vec<NamedProfile>,
    #[serde(default)]
    pub task_profiles: Vec<NamedProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionMessage {
    pub role: InstructionRole,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionRole {
    System,
    Developer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedProfile {
    pub name: String,
    pub selector: String,
    #[serde(default)]
    pub messages: Vec<InstructionMessage>,
}

#[derive(Debug, Clone)]
pub struct InstructionResolution {
    pub config_path: Option<PathBuf>,
    pub config_hash_hex: Option<String>,
    pub selected_model_profile: Option<String>,
    pub selected_task_profile: Option<String>,
    pub messages: Vec<Message>,
}

impl InstructionResolution {
    pub fn empty() -> Self {
        Self {
            config_path: None,
            config_hash_hex: None,
            selected_model_profile: None,
            selected_task_profile: None,
            messages: Vec::new(),
        }
    }
}

pub fn default_config_path(state_dir: &Path) -> PathBuf {
    state_dir.join("instructions.yaml")
}

pub fn load_config(path: &Path) -> anyhow::Result<(InstructionConfig, String)> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read instructions config: {}", path.display()))?;
    let cfg: InstructionConfig = serde_yaml::from_slice(&bytes)
        .with_context(|| format!("failed to parse instructions config: {}", path.display()))?;
    if cfg.version != 1 {
        return Err(anyhow!(
            "unsupported instructions config version {} at {}",
            cfg.version,
            path.display()
        ));
    }
    Ok((cfg, sha256_hex(&bytes)))
}

pub fn resolve_messages(
    cfg: &InstructionConfig,
    model: &str,
    task: Option<&str>,
    model_profile: Option<&str>,
    task_profile: Option<&str>,
) -> anyhow::Result<(Vec<Message>, Option<String>, Option<String>)> {
    let mut out = Vec::new();
    out.extend(to_messages(&cfg.base));

    let selected_model =
        select_profile(&cfg.model_profiles, model, model_profile, "model profile")?;
    if let Some(p) = selected_model.as_ref() {
        out.extend(to_messages(&p.messages));
    }

    let selected_task = if let Some(task_selector) = task {
        select_profile(
            &cfg.task_profiles,
            task_selector,
            task_profile,
            "task profile",
        )?
    } else {
        None
    };
    if let Some(p) = selected_task.as_ref() {
        out.extend(to_messages(&p.messages));
    }

    Ok((
        out,
        selected_model.map(|p| p.name.clone()),
        selected_task.map(|p| p.name.clone()),
    ))
}

fn to_messages(src: &[InstructionMessage]) -> Vec<Message> {
    src.iter()
        .map(|m| Message {
            role: match m.role {
                InstructionRole::System => Role::System,
                InstructionRole::Developer => Role::Developer,
            },
            content: Some(m.content.clone()),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        })
        .collect()
}

fn select_profile<'a>(
    profiles: &'a [NamedProfile],
    value: &str,
    explicit_name: Option<&str>,
    kind: &str,
) -> anyhow::Result<Option<&'a NamedProfile>> {
    if let Some(name) = explicit_name {
        return profiles
            .iter()
            .find(|p| p.name == name)
            .map(Some)
            .ok_or_else(|| anyhow!("{} '{}' not found in instructions config", kind, name));
    }
    for p in profiles {
        if selector_matches(&p.selector, value)? {
            return Ok(Some(p));
        }
    }
    Ok(None)
}

fn selector_matches(selector: &str, value: &str) -> anyhow::Result<bool> {
    if selector == value {
        return Ok(true);
    }
    if selector.contains('*') || selector.contains('?') || selector.contains('[') {
        let matcher: GlobMatcher = Glob::new(selector)
            .with_context(|| format!("invalid selector glob '{selector}'"))?
            .compile_matcher();
        return Ok(matcher.is_match(value));
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_messages, InstructionConfig, InstructionMessage, InstructionRole, NamedProfile,
    };

    #[test]
    fn resolve_merges_base_model_task_in_order() {
        let cfg = InstructionConfig {
            version: 1,
            base: vec![InstructionMessage {
                role: InstructionRole::System,
                content: "base".to_string(),
            }],
            model_profiles: vec![NamedProfile {
                name: "m".to_string(),
                selector: "qwen*".to_string(),
                messages: vec![InstructionMessage {
                    role: InstructionRole::Developer,
                    content: "model".to_string(),
                }],
            }],
            task_profiles: vec![NamedProfile {
                name: "t".to_string(),
                selector: "coding".to_string(),
                messages: vec![InstructionMessage {
                    role: InstructionRole::Developer,
                    content: "task".to_string(),
                }],
            }],
        };
        let (msgs, m, t) =
            resolve_messages(&cfg, "qwen3:8b", Some("coding"), None, None).expect("resolve");
        let contents: Vec<String> = msgs
            .into_iter()
            .map(|m| m.content.unwrap_or_default())
            .collect();
        assert_eq!(contents, vec!["base", "model", "task"]);
        assert_eq!(m.as_deref(), Some("m"));
        assert_eq!(t.as_deref(), Some("t"));
    }
}
