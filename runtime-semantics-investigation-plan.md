# Runtime Artifact/Checkpoint Consistency Hardening Plan

## Status Summary

The earlier runtime-semantics investigation and contract-test phases are complete enough for this branch. Phase 5 runtime-loop consolidation is also effectively closed for v1.

This plan replaces the earlier broad investigation plan with the narrow follow-on slice now in progress: hardening runtime artifact and runtime checkpoint consistency around terminal boundaries, approval boundaries, cancellation, and resume-to-terminal finalization.

## Reconciled Current State

Verified during reconciliation:

- `cargo test --quiet` passes
- targeted checkpoint/replay tests for the current slice pass
- relevant runtime artifact-writing paths route through `validate_final_run_artifact_consistency`

Important scope guard:

- this is not a new open-ended runtime semantics investigation
- this is not a reason to reopen Phase 5 coordinator refactoring
- this is a small invariant-hardening slice on top of the checkpoint-backed phase model

Primary policy anchor:

- `docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md`

## Goal

Ensure the persisted runtime artifacts remain internally coherent with the actual terminal outcome and resumability state.

Concrete target outcomes:

- cancelled runs do not serialize resumable runtime checkpoints
- cancelled runs do not leave unresolved interrupts behind
- approval-required artifacts keep the unresolved approval boundary they need for resume
- resume-to-terminal finalization preserves prior interrupt/phase/completion history instead of flattening it
- runtime artifact writers share one final consistency validation path

## Touched Surfaces

- `src/agent_runtime/checkpoint.rs`
- `src/agent_runtime/finalize.rs`
- `src/agent_runtime/planner_phase.rs`
- `src/cli_dispatch_eval_replay.rs`
- `src/agent/interrupts.rs`

## Implementation Breakdown

### Slice A: Final Artifact Consistency Validation

Goal:

- validate final artifact phase, completion decision, phase summary, interrupt state, and resumable-checkpoint presence against the outcome before artifact persistence

Acceptance:

- all main runtime artifact writers call the same validator before persistence

### Slice B: Resume-to-Terminal History Preservation

Goal:

- preserve prior interrupt history, phase summary, and completion-decision history when a resumed run later finalizes

Acceptance:

- a resumed approval boundary that later completes still retains the prior waiting-for-approval / resume history in the final artifact

### Slice C: Cancel / Approval Boundary Semantics

Goal:

- cancelled outcomes become terminal-only artifacts
- approval-required outcomes remain resumable artifacts with the required unresolved approval interrupt

Acceptance:

- cancelled runs emit no resumable checkpoint record
- cancelled interrupt history is resolved
- approval-required artifacts preserve unresolved approval boundary state

## Verification Plan

Required:

- `cargo test --quiet`

Focused:

- `cargo test agent_runtime::checkpoint::tests:: --quiet`
- `cargo test replay_resume_approval_checkpoint_runs_to_completion --quiet`

Audit:

- confirm the runtime artifact writers in `src/agent_runtime/finalize.rs` and `src/agent_runtime/planner_phase.rs` both route through the final-artifact consistency validator

Optional before commit:

- none

## Out Of Scope

- broad Phase 5 refactoring
- unrelated runtime-loop cleanup
- repo-wide clippy cleanup
- speculative new resume surfaces without evidence

## Exit Criteria

- `cargo test --quiet` remains green
- the checkpoint/artifact hardening slice is internally consistent and documented
- no remaining artifact-writing path bypasses the consistency validator
- any further runtime work can move on to a new, narrower plan instead of pretending this is still the earlier investigation phase

## Completion Note

The additional cancelled-boundary replay regression has been added. This hardening slice is now at commit-ready scope.
