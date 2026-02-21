use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::types::TokenUsage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostModel {
    pub schema_version: String,
    pub rules: Vec<CostRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostRule {
    pub model_glob: String,
    pub prompt_per_1k: f64,
    pub completion_per_1k: f64,
}

pub fn load_cost_model(path: &Path) -> anyhow::Result<CostModel> {
    let bytes = std::fs::read(path)?;
    match serde_json::from_slice::<CostModel>(&bytes) {
        Ok(m) => Ok(m),
        Err(_) => Ok(serde_yaml::from_slice::<CostModel>(&bytes)?),
    }
}

pub fn estimate_cost_usd(model_name: &str, usage: &TokenUsage, model: &CostModel) -> Option<f64> {
    let prompt = usage.prompt_tokens?;
    let completion = usage.completion_tokens?;
    for rule in &model.rules {
        let glob = globset::Glob::new(&rule.model_glob).ok()?;
        let matcher = glob.compile_matcher();
        if matcher.is_match(model_name) {
            let cost = (prompt as f64 / 1000.0) * rule.prompt_per_1k
                + (completion as f64 / 1000.0) * rule.completion_per_1k;
            return Some(cost);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{estimate_cost_usd, CostModel, CostRule};
    use crate::types::TokenUsage;

    #[test]
    fn cost_rule_match_is_deterministic() {
        let model = CostModel {
            schema_version: "openagent.cost_model.v1".to_string(),
            rules: vec![CostRule {
                model_glob: "qwen3:*".to_string(),
                prompt_per_1k: 0.1,
                completion_per_1k: 0.2,
            }],
        };
        let usage = TokenUsage {
            prompt_tokens: Some(1000),
            completion_tokens: Some(500),
            total_tokens: Some(1500),
        };
        let cost = estimate_cost_usd("qwen3:8b", &usage, &model).expect("cost");
        assert!((cost - 0.2).abs() < 1e-9);
    }
}
