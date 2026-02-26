use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CheckFrontmatter {
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub required_flags: Vec<String>,
    pub pass_criteria: PassCriteria,
    #[serde(default)]
    pub budget: Option<CheckBudget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PassCriteria {
    #[serde(rename = "type")]
    pub kind: PassCriteriaType,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PassCriteriaType {
    #[serde(rename = "output_contains")]
    Contains,
    #[serde(rename = "output_not_contains")]
    NotContains,
    #[serde(rename = "output_equals")]
    Equals,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct CheckBudget {
    #[serde(default)]
    pub max_steps: Option<u32>,
    #[serde(default)]
    pub max_tool_calls: Option<u32>,
    #[serde(default)]
    pub max_time_ms: Option<u64>,
}

pub fn validate_frontmatter(fm: &CheckFrontmatter) -> anyhow::Result<()> {
    if fm.schema_version != 1 {
        anyhow::bail!(
            "unsupported schema_version {} (expected 1)",
            fm.schema_version
        );
    }
    if fm.name.trim().is_empty() {
        anyhow::bail!("name must not be empty");
    }
    if let Some(tools) = &fm.allowed_tools {
        for t in tools {
            if t.trim().is_empty() {
                anyhow::bail!("allowed_tools contains empty entry");
            }
        }
    }
    if let Some(b) = &fm.budget {
        if b.max_steps == Some(0) {
            anyhow::bail!("budget.max_steps must be > 0 when set");
        }
        if b.max_tool_calls == Some(0) {
            anyhow::bail!("budget.max_tool_calls must be > 0 when set");
        }
        if b.max_time_ms == Some(0) {
            anyhow::bail!("budget.max_time_ms must be > 0 when set");
        }
    }
    Ok(())
}
