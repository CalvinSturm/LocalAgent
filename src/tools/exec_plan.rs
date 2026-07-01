use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::SideEffects;

use super::exec_support::{base_meta, failed_exec, ToolExecution};
use super::{ToolErrorCode, ToolErrorDetail, ToolRuntime};

pub(crate) const MAX_PLAN_ITEMS: usize = 20;
pub(crate) const MAX_PLAN_STEP_CHARS: usize = 240;
pub(crate) const MAX_PLAN_EXPLANATION_CHARS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
}

impl PlanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }

    fn from_str(raw: &str) -> Option<Self> {
        match raw {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanItem {
    pub step: String,
    pub status: PlanStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanUpdate {
    pub(crate) explanation: Option<String>,
    pub(crate) items: Vec<PlanItem>,
}

pub(crate) fn parse_update_plan_args(args: &Value) -> Result<PlanUpdate, String> {
    let obj = args
        .as_object()
        .ok_or_else(|| "arguments must be a JSON object".to_string())?;
    let explanation = obj
        .get("explanation")
        .and_then(|v| v.as_str())
        .map(|s| truncate_chars(s.trim(), MAX_PLAN_EXPLANATION_CHARS))
        .filter(|s| !s.is_empty());
    let items = obj
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "items must be an array".to_string())?;
    if items.is_empty() {
        return Err("items must contain at least one plan item".to_string());
    }
    if items.len() > MAX_PLAN_ITEMS {
        return Err(format!(
            "items must contain at most {MAX_PLAN_ITEMS} entries"
        ));
    }

    let mut parsed = Vec::with_capacity(items.len());
    let mut in_progress_count = 0usize;
    for (idx, item) in items.iter().enumerate() {
        let item_obj = item
            .as_object()
            .ok_or_else(|| format!("items[{idx}] must be an object"))?;
        let step = item_obj
            .get("step")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("items[{idx}].step must be a non-empty string"))?;
        let status_raw = item_obj
            .get("status")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("items[{idx}].status must be a string"))?;
        let status = PlanStatus::from_str(status_raw).ok_or_else(|| {
            format!("items[{idx}].status must be one of pending, in_progress, completed")
        })?;
        if matches!(status, PlanStatus::InProgress) {
            in_progress_count += 1;
        }
        parsed.push(PlanItem {
            step: truncate_chars(step, MAX_PLAN_STEP_CHARS),
            status,
        });
    }
    if in_progress_count > 1 {
        return Err("at most one plan item can be in_progress".to_string());
    }

    Ok(PlanUpdate {
        explanation,
        items: parsed,
    })
}

pub(super) async fn run_update_plan(rt: &ToolRuntime, args: &Value) -> ToolExecution {
    let update = match parse_update_plan_args(args) {
        Ok(update) => update,
        Err(err) => {
            return failed_exec(
                rt,
                SideEffects::None,
                format!("update_plan failed: {err}"),
                Some(ToolErrorDetail {
                    code: ToolErrorCode::ToolArgsInvalid,
                    message: err,
                    expected_schema: super::compact_builtin_schema("update_plan"),
                    received_args: Some(args.clone()),
                    minimal_example: super::minimal_builtin_example("update_plan"),
                    available_tools: None,
                }),
            );
        }
    };
    let pending = update
        .items
        .iter()
        .filter(|item| matches!(item.status, PlanStatus::Pending))
        .count();
    let completed = update
        .items
        .iter()
        .filter(|item| matches!(item.status, PlanStatus::Completed))
        .count();
    let in_progress = update
        .items
        .iter()
        .find(|item| matches!(item.status, PlanStatus::InProgress))
        .map(|item| item.step.clone());

    ToolExecution {
        ok: true,
        content: serde_json::json!({
            "updated": true,
            "items": update.items.len(),
            "pending": pending,
            "completed": completed,
            "in_progress": in_progress
        })
        .to_string(),
        truncated: false,
        error: None,
        meta: base_meta(rt, SideEffects::None),
    }
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    input.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{parse_update_plan_args, PlanStatus, MAX_PLAN_STEP_CHARS};

    #[test]
    fn parse_update_plan_accepts_one_in_progress() {
        let update = parse_update_plan_args(&json!({
            "explanation": "working",
            "items": [
                {"step":"Inspect code", "status":"completed"},
                {"step":"Implement plan tool", "status":"in_progress"},
                {"step":"Run tests", "status":"pending"}
            ]
        }))
        .expect("valid plan");
        assert_eq!(update.explanation.as_deref(), Some("working"));
        assert_eq!(update.items.len(), 3);
        assert_eq!(update.items[1].status, PlanStatus::InProgress);
    }

    #[test]
    fn parse_update_plan_rejects_multiple_in_progress_items() {
        let err = parse_update_plan_args(&json!({
            "items": [
                {"step":"A", "status":"in_progress"},
                {"step":"B", "status":"in_progress"}
            ]
        }))
        .expect_err("multiple in-progress entries rejected");
        assert!(err.contains("at most one"));
    }

    #[test]
    fn parse_update_plan_truncates_long_step_text() {
        let update = parse_update_plan_args(&json!({
            "items": [
                {"step":"x".repeat(MAX_PLAN_STEP_CHARS + 20), "status":"pending"}
            ]
        }))
        .expect("valid plan");
        assert_eq!(update.items[0].step.len(), MAX_PLAN_STEP_CHARS);
    }
}
