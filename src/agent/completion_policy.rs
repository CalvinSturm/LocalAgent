pub(crate) enum VerifiedWriteCompletionDecision {
    FinalizeNow,
    StartRequiredValidationPhase(String),
    FollowOnTurn(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidationFacts {
    pub(crate) required_command: Option<&'static str>,
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

pub(crate) fn collect_validation_facts(
    user_prompt: &str,
    tool_facts: &[crate::agent::ToolFactV1],
) -> ValidationFacts {
    let required_command = crate::agent_impl_guard::prompt_required_validation_command(user_prompt);
    ValidationFacts {
        required_command,
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

pub(crate) fn decide_required_validation_completion(
    facts: &ValidationFacts,
    required_validation_retry_count: u32,
) -> RequiredValidationCompletionDecision {
    let Some(required_command) = facts.required_command else {
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
    if facts.exact_final_answer_required && facts.satisfied {
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
        collect_validation_facts, decide_required_validation_completion,
        decide_validation_phase_transition, decide_verified_write_completion,
        RequiredValidationCompletionDecision, ValidationFacts,
        ValidationPhaseTransitionDecision, VerifiedWriteCompletionDecision,
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
                required_command: Some("cargo test"),
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
                required_command: Some("cargo test"),
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
        assert_eq!(facts.required_command, Some("cargo test"));
        assert!(facts.satisfied);
        assert!(!facts.repair_needed);
        assert!(!facts.exact_final_answer_required);
    }

    #[test]
    fn validation_transition_enters_repair_when_failed_validation_needs_repair() {
        let decision = decide_validation_phase_transition(
            &ValidationFacts {
                required_command: Some("cargo test"),
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
                required_command: Some("cargo test"),
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
                required_command: Some("cargo test"),
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
        assert_eq!(facts.required_command, Some("cargo test"));
        assert!(!facts.satisfied);
        assert!(facts.repair_needed);
    }
}
