pub(crate) enum VerifiedWriteCompletionDecision {
    FinalizeNow,
    StartRequiredValidationPhase(String),
    FollowOnTurn(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidationFacts {
    pub(crate) required_command: Option<String>,
    pub(crate) satisfied: bool,
    pub(crate) repair_needed: bool,
    pub(crate) exact_final_answer_required: bool,
}

pub(crate) enum RequiredValidationCompletionDecision {
    FinalizeNow,
    ContinueRequiredValidation(String),
    FinalizeError(&'static str),
}

pub(crate) enum ValidationPhaseTransitionDecision {
    NoChange,
    EnterRepair,
    EnterPostValidationFinalAnswerOnly,
    ClearRepair,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeCheckpointResumeKind {
    ApprovalGranted,
    OperatorContinue,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeCheckpointResumeDecision {
    pub(crate) kind: RuntimeCheckpointResumeKind,
    pub(crate) phase: crate::agent_runtime::state::RunPhase,
    pub(crate) approval_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApprovalBoundaryTransitionDecision {
    pub(crate) from_phase: crate::agent_runtime::state::RunPhase,
    pub(crate) to_phase: crate::agent_runtime::state::RunPhase,
    pub(crate) interrupt_kind: crate::agent_runtime::state::InterruptKindV1,
    pub(crate) completion_reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimePhaseTransitionDecision {
    pub(crate) from_phase: crate::agent_runtime::state::RunPhase,
    pub(crate) to_phase: crate::agent_runtime::state::RunPhase,
    pub(crate) completion_reason: &'static str,
}

fn format_verified_paths(verified_paths: &[String]) -> String {
    verified_paths.join(", ")
}

fn required_validation_phase_message(required_command: &str) -> String {
    format!(
        "Validation required now. Return exactly one shell tool call and no prose. Run `{required_command}`. Example arguments: {{\"command\":\"{required_command}\"}}. After it succeeds, reply with the final answer only."
    )
}

fn follow_on_turn_message(verified_paths: &[String]) -> String {
    if verified_paths.is_empty() {
        "Post-write verification succeeded. One required step remains. If the prompt requires validation or tests, your next turn must be exactly one shell tool call for that command and no prose. Do not give the final answer yet. Do not call another write tool unless validation proves another code change is required."
            .to_string()
    } else {
        format!(
            "Post-write verification succeeded for {}. One required step remains. If the prompt requires validation or tests, your next turn must be exactly one shell tool call for that command and no prose. Do not give the final answer yet. Do not call another write tool for those paths unless validation proves another code change is required.",
            format_verified_paths(verified_paths)
        )
    }
}

fn required_validation_before_final_message(required_command: &str) -> String {
    format!(
        "Validation required now. Return exactly one shell tool call and no prose. Run `{required_command}`. Example arguments: {{\"command\":\"{required_command}\"}}. After it succeeds, reply with the final answer only."
    )
}

#[allow(dead_code)]
pub(crate) fn decide_runtime_checkpoint_resume(
    checkpoint: &crate::store::RuntimeRunCheckpointRecordV1,
) -> anyhow::Result<RuntimeCheckpointResumeDecision> {
    use crate::agent_runtime::state::RunPhase;

    match checkpoint.runtime_state_checkpoint.phase {
        RunPhase::WaitingForApproval => {
            let approval_id = checkpoint
                .runtime_state_checkpoint
                .approval_state
                .approval_id
                .clone()
                .or_else(|| {
                    checkpoint
                        .pending_tool_call
                        .as_ref()
                        .and_then(|tool| tool.approval_id.clone())
                });
            if checkpoint.checkpoint.is_none() {
                anyhow::bail!(
                    "runtime checkpoint '{}' is not resumable: missing terminal boundary checkpoint",
                    checkpoint.runtime_run_id
                );
            }
            let boundary = checkpoint.checkpoint.as_ref().expect("checked above");
            if boundary.phase != crate::store::RunCheckpointPhase::WaitingForApproval {
                anyhow::bail!(
                    "runtime checkpoint '{}' is not resumable: phase is {:?}",
                    checkpoint.runtime_run_id,
                    boundary.phase
                );
            }
            if boundary
                .pending_interrupt
                .as_ref()
                .map(|it| it.kind != crate::store::RunCheckpointInterruptKind::ApprovalRequired)
                .unwrap_or(true)
            {
                anyhow::bail!(
                    "runtime checkpoint '{}' is not resumable: waiting_for_approval is missing approval interrupt state",
                    checkpoint.runtime_run_id
                );
            }
            Ok(RuntimeCheckpointResumeDecision {
                kind: RuntimeCheckpointResumeKind::ApprovalGranted,
                phase: RunPhase::WaitingForApproval,
                approval_id,
            })
        }
        RunPhase::WaitingForOperatorInput => Ok(RuntimeCheckpointResumeDecision {
            kind: RuntimeCheckpointResumeKind::OperatorContinue,
            phase: RunPhase::WaitingForOperatorInput,
            approval_id: None,
        }),
        _ => anyhow::bail!(
            "runtime checkpoint '{}' is not resumable: runtime phase is {:?}",
            checkpoint.runtime_run_id,
            checkpoint.runtime_state_checkpoint.phase
        ),
    }
}

pub(crate) fn resume_phase_from_checkpoint_state(
    checkpoint: &crate::store::RuntimeRunCheckpointRecordV1,
) -> crate::agent_runtime::state::RunPhase {
    let state = &checkpoint.runtime_state_checkpoint;
    if state.validation_state.collecting_final_answer && state.validation_state.satisfied {
        return crate::agent_runtime::state::RunPhase::CollectingFinalAnswer;
    }
    if state.validation_state.required_command.is_some() && !state.validation_state.satisfied {
        return crate::agent_runtime::state::RunPhase::Validating;
    }
    if state.retry_state.post_write_guard_retry_count > 0
        || state.retry_state.post_write_follow_on_turn_count > 0
    {
        return crate::agent_runtime::state::RunPhase::VerifyingChanges;
    }
    crate::agent_runtime::state::RunPhase::Executing
}

pub(crate) fn approval_boundary_transition_decision() -> ApprovalBoundaryTransitionDecision {
    ApprovalBoundaryTransitionDecision {
        from_phase: crate::agent_runtime::state::RunPhase::Executing,
        to_phase: crate::agent_runtime::state::RunPhase::WaitingForApproval,
        interrupt_kind: crate::agent_runtime::state::InterruptKindV1::ApprovalRequired,
        completion_reason: "run blocked pending operator approval",
    }
}

pub(crate) fn operator_boundary_transition_decision() -> ApprovalBoundaryTransitionDecision {
    ApprovalBoundaryTransitionDecision {
        from_phase: crate::agent_runtime::state::RunPhase::Executing,
        to_phase: crate::agent_runtime::state::RunPhase::WaitingForOperatorInput,
        interrupt_kind: crate::agent_runtime::state::InterruptKindV1::OperatorInterrupt,
        completion_reason: "run interrupted by operator message",
    }
}

pub(crate) fn required_validation_boundary_transition_decision() -> RuntimePhaseTransitionDecision {
    RuntimePhaseTransitionDecision {
        from_phase: crate::agent_runtime::state::RunPhase::Executing,
        to_phase: crate::agent_runtime::state::RunPhase::Validating,
        completion_reason: "run blocked until required validation command is executed",
    }
}

pub(crate) fn validation_resume_execution_transition_decision() -> RuntimePhaseTransitionDecision {
    RuntimePhaseTransitionDecision {
        from_phase: crate::agent_runtime::state::RunPhase::Validating,
        to_phase: crate::agent_runtime::state::RunPhase::Executing,
        completion_reason: "validation phase handed control back to execution",
    }
}

pub(crate) fn post_validation_final_answer_transition_decision() -> RuntimePhaseTransitionDecision {
    RuntimePhaseTransitionDecision {
        from_phase: crate::agent_runtime::state::RunPhase::Validating,
        to_phase: crate::agent_runtime::state::RunPhase::CollectingFinalAnswer,
        completion_reason: "validation succeeded and the runtime is collecting the final answer",
    }
}

pub(crate) fn exact_final_answer_boundary_transition_decision() -> RuntimePhaseTransitionDecision {
    RuntimePhaseTransitionDecision {
        from_phase: crate::agent_runtime::state::RunPhase::Executing,
        to_phase: crate::agent_runtime::state::RunPhase::CollectingFinalAnswer,
        completion_reason: "task work is complete and the runtime is collecting the final answer",
    }
}

pub(crate) fn collect_validation_facts(
    user_prompt: &str,
    tool_facts: &[crate::agent::ToolFactV1],
) -> ValidationFacts {
    let required_command = crate::agent_impl_guard::prompt_required_validation_command(user_prompt);
    ValidationFacts {
        required_command: required_command.map(ToOwned::to_owned),
        satisfied: crate::agent::required_validation_command_satisfied_from_facts(
            user_prompt,
            tool_facts,
        ),
        repair_needed: crate::agent::required_validation_failure_needs_repair_from_facts(
            user_prompt,
            tool_facts,
        ),
        exact_final_answer_required: crate::agent_impl_guard::prompt_required_exact_final_answer(
            user_prompt,
        )
        .is_some(),
    }
}

pub(crate) fn collect_validation_facts_from_checkpoint(
    checkpoint: &crate::agent_runtime::state::RunCheckpointV1,
    tool_fact_envelopes: &[crate::agent::ToolFactEnvelopeV1],
) -> ValidationFacts {
    let tool_facts = tool_fact_envelopes
        .iter()
        .map(|envelope| envelope.fact.clone())
        .collect::<Vec<_>>();
    let required_command = checkpoint.validation_state.required_command.clone();
    ValidationFacts {
        required_command: required_command.clone(),
        satisfied: required_command.as_deref().is_none_or(|required| {
            tool_facts.iter().any(|fact| {
                matches!(
                    fact,
                    crate::agent::ToolFactV1::Validation {
                        command,
                        ok: true,
                        ..
                    } if command.to_ascii_lowercase().contains(required)
                )
            })
        }),
        repair_needed: required_command.as_deref().is_some_and(|required| {
            tool_facts.iter().any(|fact| {
                matches!(
                    fact,
                    crate::agent::ToolFactV1::Validation {
                        command,
                        ok: false,
                        ..
                    } if command.to_ascii_lowercase().contains(required)
                )
            })
        }),
        exact_final_answer_required: checkpoint.validation_state.collecting_final_answer,
    }
}

pub(crate) fn decide_required_validation_completion(
    facts: &ValidationFacts,
    required_validation_retry_count: u32,
) -> RequiredValidationCompletionDecision {
    let Some(required_command) = facts.required_command.as_deref() else {
        return RequiredValidationCompletionDecision::FinalizeNow;
    };
    if facts.satisfied {
        return RequiredValidationCompletionDecision::FinalizeNow;
    }
    if required_validation_retry_count < 1 {
        return RequiredValidationCompletionDecision::ContinueRequiredValidation(
            required_validation_before_final_message(required_command),
        );
    }
    RequiredValidationCompletionDecision::FinalizeError(
        "required validation command was not executed successfully before final answer",
    )
}

pub(crate) fn decide_validation_phase_transition(
    facts: &ValidationFacts,
    successful_write_tool_ok_this_step: bool,
) -> ValidationPhaseTransitionDecision {
    if facts.repair_needed {
        return ValidationPhaseTransitionDecision::EnterRepair;
    }
    if facts.required_command.is_some() && facts.exact_final_answer_required && facts.satisfied {
        return ValidationPhaseTransitionDecision::EnterPostValidationFinalAnswerOnly;
    }
    if successful_write_tool_ok_this_step {
        return ValidationPhaseTransitionDecision::ClearRepair;
    }
    ValidationPhaseTransitionDecision::NoChange
}

pub(crate) fn decide_verified_write_completion(
    user_prompt: &str,
    verified_paths: &[String],
    has_post_tool_assistant_closeout: bool,
    post_write_follow_on_turn_count: u32,
) -> VerifiedWriteCompletionDecision {
    let required_validation_command =
        crate::agent_impl_guard::prompt_required_validation_command(user_prompt);
    if required_validation_command.is_some() && !has_post_tool_assistant_closeout {
        return VerifiedWriteCompletionDecision::StartRequiredValidationPhase(
            required_validation_phase_message(
                required_validation_command.unwrap_or("the required validation command"),
            ),
        );
    }

    let needs_follow_on_turn =
        crate::agent_impl_guard::prompt_requires_post_write_follow_on(user_prompt)
            && !has_post_tool_assistant_closeout;
    if needs_follow_on_turn && post_write_follow_on_turn_count < 1 {
        return VerifiedWriteCompletionDecision::FollowOnTurn(follow_on_turn_message(
            verified_paths,
        ));
    }

    VerifiedWriteCompletionDecision::FinalizeNow
}

#[cfg(test)]
mod tests {
    use super::{
        approval_boundary_transition_decision, collect_validation_facts,
        decide_required_validation_completion, decide_runtime_checkpoint_resume,
        decide_validation_phase_transition, decide_verified_write_completion,
        exact_final_answer_boundary_transition_decision, operator_boundary_transition_decision,
        post_validation_final_answer_transition_decision,
        resume_phase_from_checkpoint_state,
        required_validation_boundary_transition_decision,
        validation_resume_execution_transition_decision, RequiredValidationCompletionDecision,
        RuntimeCheckpointResumeKind, ValidationFacts, ValidationPhaseTransitionDecision,
        VerifiedWriteCompletionDecision,
    };
    use crate::agent::ToolFactV1;

    #[test]
    fn verified_write_policy_starts_validation_when_prompt_requires_it() {
        let decision = decide_verified_write_completion(
            "Before finishing, run cargo test successfully.",
            &[],
            false,
            0,
        );
        match decision {
            VerifiedWriteCompletionDecision::StartRequiredValidationPhase(message) => {
                assert!(message.contains("cargo test"));
            }
            _ => panic!("expected validation phase"),
        }
    }

    #[test]
    fn verified_write_policy_requests_follow_on_when_closeout_missing() {
        let decision = decide_verified_write_completion(
            "Summarize what changed after the fix.",
            &["src/main.rs".to_string()],
            false,
            0,
        );
        match decision {
            VerifiedWriteCompletionDecision::FollowOnTurn(message) => {
                assert!(message.contains("src/main.rs"));
            }
            _ => panic!("expected follow-on turn"),
        }
    }

    #[test]
    fn validation_completion_requires_retry_when_missing() {
        let decision = decide_required_validation_completion(
            &ValidationFacts {
                required_command: Some("cargo test".to_string()),
                satisfied: false,
                repair_needed: false,
                exact_final_answer_required: false,
            },
            0,
        );
        match decision {
            RequiredValidationCompletionDecision::ContinueRequiredValidation(message) => {
                assert!(message.contains("cargo test"));
            }
            _ => panic!("expected validation retry"),
        }
    }

    #[test]
    fn validation_completion_fails_after_bounded_retry() {
        let decision = decide_required_validation_completion(
            &ValidationFacts {
                required_command: Some("cargo test".to_string()),
                satisfied: false,
                repair_needed: false,
                exact_final_answer_required: false,
            },
            1,
        );
        match decision {
            RequiredValidationCompletionDecision::FinalizeError(reason) => {
                assert!(reason.contains("required validation command"));
            }
            _ => panic!("expected validation failure"),
        }
    }

    #[test]
    fn collect_validation_facts_marks_successful_matching_shell_as_satisfied() {
        let facts = collect_validation_facts(
            "Before finishing, run cargo test successfully.",
            &[ToolFactV1::Validation {
                sequence: 1,
                tool_call_id: "tc1".to_string(),
                command: "cargo test".to_string(),
                ok: true,
            }],
        );
        assert_eq!(facts.required_command.as_deref(), Some("cargo test"));
        assert!(facts.satisfied);
        assert!(!facts.repair_needed);
        assert!(!facts.exact_final_answer_required);
    }

    #[test]
    fn validation_transition_enters_repair_when_failed_validation_needs_repair() {
        let decision = decide_validation_phase_transition(
            &ValidationFacts {
                required_command: Some("cargo test".to_string()),
                satisfied: true,
                repair_needed: true,
                exact_final_answer_required: true,
            },
            false,
        );
        assert!(matches!(
            decision,
            ValidationPhaseTransitionDecision::EnterRepair
        ));
    }

    #[test]
    fn validation_transition_enters_final_answer_only_after_satisfied_validation() {
        let decision = decide_validation_phase_transition(
            &ValidationFacts {
                required_command: Some("cargo test".to_string()),
                satisfied: true,
                repair_needed: false,
                exact_final_answer_required: true,
            },
            false,
        );
        assert!(matches!(
            decision,
            ValidationPhaseTransitionDecision::EnterPostValidationFinalAnswerOnly
        ));
    }

    #[test]
    fn validation_transition_clears_repair_after_successful_write() {
        let decision = decide_validation_phase_transition(
            &ValidationFacts {
                required_command: Some("cargo test".to_string()),
                satisfied: false,
                repair_needed: false,
                exact_final_answer_required: false,
            },
            true,
        );
        assert!(matches!(
            decision,
            ValidationPhaseTransitionDecision::ClearRepair
        ));
    }

    #[test]
    fn collect_validation_facts_marks_failed_matching_validation_as_repair_needed() {
        let facts = collect_validation_facts(
            "Before finishing, run cargo test successfully.",
            &[ToolFactV1::Validation {
                sequence: 0,
                tool_call_id: "tc1".to_string(),
                command: "cargo test".to_string(),
                ok: false,
            }],
        );
        assert_eq!(facts.required_command.as_deref(), Some("cargo test"));
        assert!(!facts.satisfied);
        assert!(facts.repair_needed);
    }

    #[test]
    fn runtime_checkpoint_resume_decision_accepts_waiting_for_approval() {
        let checkpoint = crate::store::RuntimeRunCheckpointRecordV1 {
            schema_version: "openagent.runtime_checkpoint.v1".to_string(),
            runtime_run_id: "run-1".to_string(),
            prompt: "continue".to_string(),
            resume_argv: Vec::new(),
            checkpoint: Some(crate::store::RunCheckpointV1 {
                schema_version: "openagent.run_checkpoint.v1".to_string(),
                phase: crate::store::RunCheckpointPhase::WaitingForApproval,
                terminal_boundary: true,
                pending_interrupt: Some(crate::store::RunCheckpointInterruptV1 {
                    kind: crate::store::RunCheckpointInterruptKind::ApprovalRequired,
                    reason: Some("approval required".to_string()),
                }),
            }),
            runtime_state_checkpoint: crate::agent_runtime::state::RunCheckpointV1 {
                schema_version: "openagent.runtime_state_checkpoint.v1".to_string(),
                phase: crate::agent_runtime::state::RunPhase::WaitingForApproval,
                step_index: 0,
                execution_tier: crate::agent_runtime::state::ExecutionTier::ScopedHostShell,
                terminal_boundary: true,
                retry_state: crate::agent_runtime::state::RetryState::default(),
                tool_protocol_state: crate::agent_runtime::state::ToolProtocolState::default(),
                validation_state: crate::agent_runtime::state::ValidationState::default(),
                approval_state: crate::agent_runtime::state::ApprovalState {
                    approval_id: Some("approval-1".to_string()),
                    tool_call_id: Some("tc-1".to_string()),
                    awaiting_approval: true,
                },
                active_plan_step_id: None,
                last_tool_fact_envelopes: Vec::new(),
            },
            execution_tier: crate::agent_runtime::state::ExecutionTier::ScopedHostShell,
            resume_session_messages: Vec::new(),
            interrupt_history: Vec::new(),
            phase_summary: Vec::new(),
            completion_decisions: Vec::new(),
            tool_facts: Vec::new(),
            tool_fact_envelopes: Vec::new(),
            pending_tool_call: None,
            boundary_output: None,
        };

        let decision = decide_runtime_checkpoint_resume(&checkpoint).expect("resumable");
        assert_eq!(decision.kind, RuntimeCheckpointResumeKind::ApprovalGranted);
        assert_eq!(decision.approval_id.as_deref(), Some("approval-1"));
    }

    #[test]
    fn resume_phase_prefers_validating_when_required_validation_is_still_unsatisfied() {
        let checkpoint = crate::store::RuntimeRunCheckpointRecordV1 {
            schema_version: "openagent.runtime_checkpoint.v1".to_string(),
            runtime_run_id: "run-1".to_string(),
            prompt: "continue".to_string(),
            resume_argv: Vec::new(),
            checkpoint: Some(crate::store::RunCheckpointV1 {
                schema_version: "openagent.run_checkpoint.v1".to_string(),
                phase: crate::store::RunCheckpointPhase::Interrupted,
                terminal_boundary: true,
                pending_interrupt: Some(crate::store::RunCheckpointInterruptV1 {
                    kind: crate::store::RunCheckpointInterruptKind::OperatorInterrupt,
                    reason: Some("paused".to_string()),
                }),
            }),
            runtime_state_checkpoint: crate::agent_runtime::state::RunCheckpointV1 {
                schema_version: "openagent.runtime_state_checkpoint.v1".to_string(),
                phase: crate::agent_runtime::state::RunPhase::WaitingForOperatorInput,
                step_index: 2,
                execution_tier: crate::agent_runtime::state::ExecutionTier::ScopedHostShell,
                terminal_boundary: true,
                retry_state: crate::agent_runtime::state::RetryState::default(),
                tool_protocol_state: crate::agent_runtime::state::ToolProtocolState::default(),
                validation_state: crate::agent_runtime::state::ValidationState {
                    required_command: Some("cargo test".to_string()),
                    satisfied: false,
                    repair_mode: false,
                    collecting_final_answer: false,
                },
                approval_state: crate::agent_runtime::state::ApprovalState::default(),
                active_plan_step_id: None,
                last_tool_fact_envelopes: Vec::new(),
            },
            execution_tier: crate::agent_runtime::state::ExecutionTier::ScopedHostShell,
            resume_session_messages: Vec::new(),
            interrupt_history: Vec::new(),
            phase_summary: Vec::new(),
            completion_decisions: Vec::new(),
            tool_facts: Vec::new(),
            tool_fact_envelopes: Vec::new(),
            pending_tool_call: None,
            boundary_output: Some("paused".to_string()),
        };

        assert_eq!(
            resume_phase_from_checkpoint_state(&checkpoint),
            crate::agent_runtime::state::RunPhase::Validating
        );
    }

    #[test]
    fn resume_phase_prefers_collecting_final_answer_when_validation_is_already_satisfied() {
        let checkpoint = crate::store::RuntimeRunCheckpointRecordV1 {
            schema_version: "openagent.runtime_checkpoint.v1".to_string(),
            runtime_run_id: "run-1".to_string(),
            prompt: "continue".to_string(),
            resume_argv: Vec::new(),
            checkpoint: Some(crate::store::RunCheckpointV1 {
                schema_version: "openagent.run_checkpoint.v1".to_string(),
                phase: crate::store::RunCheckpointPhase::Interrupted,
                terminal_boundary: true,
                pending_interrupt: Some(crate::store::RunCheckpointInterruptV1 {
                    kind: crate::store::RunCheckpointInterruptKind::OperatorInterrupt,
                    reason: Some("paused".to_string()),
                }),
            }),
            runtime_state_checkpoint: crate::agent_runtime::state::RunCheckpointV1 {
                schema_version: "openagent.runtime_state_checkpoint.v1".to_string(),
                phase: crate::agent_runtime::state::RunPhase::WaitingForOperatorInput,
                step_index: 2,
                execution_tier: crate::agent_runtime::state::ExecutionTier::ScopedHostShell,
                terminal_boundary: true,
                retry_state: crate::agent_runtime::state::RetryState::default(),
                tool_protocol_state: crate::agent_runtime::state::ToolProtocolState::default(),
                validation_state: crate::agent_runtime::state::ValidationState {
                    required_command: Some("cargo test".to_string()),
                    satisfied: true,
                    repair_mode: false,
                    collecting_final_answer: true,
                },
                approval_state: crate::agent_runtime::state::ApprovalState::default(),
                active_plan_step_id: None,
                last_tool_fact_envelopes: Vec::new(),
            },
            execution_tier: crate::agent_runtime::state::ExecutionTier::ScopedHostShell,
            resume_session_messages: Vec::new(),
            interrupt_history: Vec::new(),
            phase_summary: Vec::new(),
            completion_decisions: Vec::new(),
            tool_facts: Vec::new(),
            tool_fact_envelopes: Vec::new(),
            pending_tool_call: None,
            boundary_output: Some("paused".to_string()),
        };

        assert_eq!(
            resume_phase_from_checkpoint_state(&checkpoint),
            crate::agent_runtime::state::RunPhase::CollectingFinalAnswer
        );
    }

    #[test]
    fn approval_boundary_transition_goes_to_waiting_for_approval() {
        let decision = approval_boundary_transition_decision();
        assert_eq!(
            decision.from_phase,
            crate::agent_runtime::state::RunPhase::Executing
        );
        assert_eq!(
            decision.to_phase,
            crate::agent_runtime::state::RunPhase::WaitingForApproval
        );
        assert_eq!(
            decision.interrupt_kind,
            crate::agent_runtime::state::InterruptKindV1::ApprovalRequired
        );
    }

    #[test]
    fn operator_boundary_transition_goes_to_waiting_for_operator_input() {
        let decision = operator_boundary_transition_decision();
        assert_eq!(
            decision.from_phase,
            crate::agent_runtime::state::RunPhase::Executing
        );
        assert_eq!(
            decision.to_phase,
            crate::agent_runtime::state::RunPhase::WaitingForOperatorInput
        );
        assert_eq!(
            decision.interrupt_kind,
            crate::agent_runtime::state::InterruptKindV1::OperatorInterrupt
        );
    }

    #[test]
    fn validation_boundary_transition_goes_to_validating() {
        let decision = required_validation_boundary_transition_decision();
        assert_eq!(
            decision.from_phase,
            crate::agent_runtime::state::RunPhase::Executing
        );
        assert_eq!(
            decision.to_phase,
            crate::agent_runtime::state::RunPhase::Validating
        );
    }

    #[test]
    fn validation_resume_execution_transition_goes_back_to_executing() {
        let decision = validation_resume_execution_transition_decision();
        assert_eq!(
            decision.from_phase,
            crate::agent_runtime::state::RunPhase::Validating
        );
        assert_eq!(
            decision.to_phase,
            crate::agent_runtime::state::RunPhase::Executing
        );
    }

    #[test]
    fn post_validation_final_answer_transition_goes_to_collecting_final_answer() {
        let decision = post_validation_final_answer_transition_decision();
        assert_eq!(
            decision.from_phase,
            crate::agent_runtime::state::RunPhase::Validating
        );
        assert_eq!(
            decision.to_phase,
            crate::agent_runtime::state::RunPhase::CollectingFinalAnswer
        );
    }

    #[test]
    fn exact_final_answer_boundary_transition_goes_to_collecting_final_answer() {
        let decision = exact_final_answer_boundary_transition_decision();
        assert_eq!(
            decision.from_phase,
            crate::agent_runtime::state::RunPhase::Executing
        );
        assert_eq!(
            decision.to_phase,
            crate::agent_runtime::state::RunPhase::CollectingFinalAnswer
        );
    }
}
