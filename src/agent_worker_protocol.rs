use crate::agent::{PlanStepConstraint, WorkerStepStatus};
use crate::agent_tool_exec::parse_jsonish;

pub(crate) fn parse_worker_step_status(
    raw: &str,
    constraints: &[PlanStepConstraint],
) -> Option<WorkerStepStatus> {
    let value = parse_jsonish(raw)?;
    let obj = value.as_object()?;
    let schema = obj.get("schema_version").and_then(|v| v.as_str())?;
    if schema != crate::planner::STEP_RESULT_SCHEMA_VERSION {
        return None;
    }
    let step_id = obj.get("step_id").and_then(|v| v.as_str())?.to_string();
    if step_id != "final" && !constraints.iter().any(|s| s.step_id == step_id) {
        return None;
    }
    let status = obj.get("status").and_then(|v| v.as_str())?.to_string();
    if !matches!(status.as_str(), "done" | "retry" | "replan" | "fail") {
        return None;
    }
    let next_step_id = obj
        .get("next_step_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let user_output = obj
        .get("user_output")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Some(WorkerStepStatus {
        step_id,
        status,
        next_step_id,
        user_output,
    })
}
