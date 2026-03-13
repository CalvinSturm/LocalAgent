use std::collections::BTreeMap;

use super::{PlanStepConstraint, WorkerStepStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PlannerResponseDecision {
    Proceed,
    RemindControlEnvelope {
        blocked_count: u32,
    },
    MissingControlEnvelopeFatal {
        blocked_count: u32,
    },
    StepDone {
        completed_step_id: String,
        next_step_id: Option<String>,
        next_active_plan_step_idx: usize,
        user_output: Option<String>,
    },
    InvalidDoneTransition {
        step_id: String,
        expected_step_id: String,
    },
    InvalidNextStepId {
        step_id: String,
        next_step_id: String,
    },
    StepRetry {
        step_id: String,
        retry_count: u32,
        user_output: Option<String>,
    },
    RetryLimitExceeded {
        step_id: String,
        retry_count: u32,
    },
    InvalidRetryTransition {
        step_id: String,
        expected_step_id: String,
    },
    ReplanRequested {
        step_id: String,
        status: String,
    },
    FailRequested {
        step_id: String,
        status: String,
    },
}

pub(super) fn evaluate_planner_response(
    plan_enforcement_active: bool,
    has_actionable_tool_calls: bool,
    model_signaled_finalize: bool,
    worker_step_status: Option<&WorkerStepStatus>,
    blocked_control_envelope_count: u32,
    active_plan_step_idx: usize,
    plan_step_constraints: &[PlanStepConstraint],
    step_retry_counts: &BTreeMap<String, u32>,
) -> PlannerResponseDecision {
    if plan_enforcement_active
        && !has_actionable_tool_calls
        && worker_step_status.is_none()
        && model_signaled_finalize
    {
        let blocked_count = blocked_control_envelope_count.saturating_add(1);
        return if blocked_count >= 2 {
            PlannerResponseDecision::MissingControlEnvelopeFatal { blocked_count }
        } else {
            PlannerResponseDecision::RemindControlEnvelope { blocked_count }
        };
    }

    let Some(step_status) = worker_step_status else {
        return PlannerResponseDecision::Proceed;
    };

    let user_output = step_status
        .user_output
        .as_deref()
        .map(str::trim)
        .filter(|content| !content.is_empty())
        .map(ToOwned::to_owned);
    let current_step_id = plan_step_constraints
        .get(active_plan_step_idx)
        .map(|constraint| constraint.step_id.clone())
        .unwrap_or_else(|| "unknown".to_string());

    match step_status.status.as_str() {
        "done" => {
            if step_status.step_id != current_step_id {
                return PlannerResponseDecision::InvalidDoneTransition {
                    step_id: step_status.step_id.clone(),
                    expected_step_id: current_step_id,
                };
            }
            let next_active_plan_step_idx = match step_status.next_step_id.as_deref() {
                Some("final") => plan_step_constraints.len(),
                Some(next_step_id) => {
                    let Some(next_idx) = plan_step_constraints
                        .iter()
                        .position(|constraint| constraint.step_id == next_step_id)
                    else {
                        return PlannerResponseDecision::InvalidNextStepId {
                            step_id: step_status.step_id.clone(),
                            next_step_id: next_step_id.to_string(),
                        };
                    };
                    next_idx
                }
                None if active_plan_step_idx < plan_step_constraints.len() => {
                    active_plan_step_idx.saturating_add(1)
                }
                None => active_plan_step_idx,
            };

            PlannerResponseDecision::StepDone {
                completed_step_id: step_status.step_id.clone(),
                next_step_id: step_status.next_step_id.clone(),
                next_active_plan_step_idx,
                user_output,
            }
        }
        "retry" => {
            if step_status.step_id != current_step_id {
                return PlannerResponseDecision::InvalidRetryTransition {
                    step_id: step_status.step_id.clone(),
                    expected_step_id: current_step_id,
                };
            }
            let retry_count = step_retry_counts
                .get(&step_status.step_id)
                .copied()
                .unwrap_or(0)
                .saturating_add(1);
            if retry_count > 2 {
                return PlannerResponseDecision::RetryLimitExceeded {
                    step_id: step_status.step_id.clone(),
                    retry_count,
                };
            }
            PlannerResponseDecision::StepRetry {
                step_id: step_status.step_id.clone(),
                retry_count,
                user_output,
            }
        }
        "replan" => PlannerResponseDecision::ReplanRequested {
            step_id: step_status.step_id.clone(),
            status: step_status.status.clone(),
        },
        "fail" => PlannerResponseDecision::FailRequested {
            step_id: step_status.step_id.clone(),
            status: step_status.status.clone(),
        },
        _ => PlannerResponseDecision::Proceed,
    }
}

#[cfg(test)]
mod tests {
    use super::{evaluate_planner_response, PlannerResponseDecision};
    use crate::agent::{PlanStepConstraint, WorkerStepStatus};
    use std::collections::BTreeMap;

    fn constraints() -> Vec<PlanStepConstraint> {
        vec![
            PlanStepConstraint {
                step_id: "S1".to_string(),
                intended_tools: vec!["read_file".to_string()],
            },
            PlanStepConstraint {
                step_id: "S2".to_string(),
                intended_tools: vec!["shell".to_string()],
            },
        ]
    }

    #[test]
    fn planner_response_requires_control_envelope_before_failing() {
        let decision = evaluate_planner_response(
            true,
            false,
            true,
            None,
            0,
            0,
            &constraints(),
            &BTreeMap::new(),
        );
        assert!(matches!(
            decision,
            PlannerResponseDecision::RemindControlEnvelope { blocked_count: 1 }
        ));
    }

    #[test]
    fn planner_response_advances_to_explicit_next_step() {
        let decision = evaluate_planner_response(
            true,
            false,
            true,
            Some(&WorkerStepStatus {
                step_id: "S1".to_string(),
                status: "done".to_string(),
                next_step_id: Some("S2".to_string()),
                user_output: Some("  ready  ".to_string()),
            }),
            1,
            0,
            &constraints(),
            &BTreeMap::new(),
        );
        assert!(matches!(
            decision,
            PlannerResponseDecision::StepDone {
                next_active_plan_step_idx: 1,
                ..
            }
        ));
    }

    #[test]
    fn planner_response_rejects_invalid_retry_after_limit() {
        let mut retry_counts = BTreeMap::new();
        retry_counts.insert("S1".to_string(), 2);
        let decision = evaluate_planner_response(
            true,
            false,
            true,
            Some(&WorkerStepStatus {
                step_id: "S1".to_string(),
                status: "retry".to_string(),
                next_step_id: None,
                user_output: None,
            }),
            0,
            0,
            &constraints(),
            &retry_counts,
        );
        assert!(matches!(
            decision,
            PlannerResponseDecision::RetryLimitExceeded { retry_count: 3, .. }
        ));
    }
}
