# LocalAgent vNext Runtime Handoff

## Purpose

This file is the handoff summary for the vNext runtime migration work.

Canonical architecture and migration plan:

- [LOCALAGENT_VNEXT_RUNTIME_TARGET.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/docs/architecture/LOCALAGENT_VNEXT_RUNTIME_TARGET.md)

This handoff is a status snapshot of what has already landed in code, what remains, and what the next logical implementation slices should be.

## Current Baseline

Current verified baseline:

- `cargo test --quiet` passes
- behavior-repair work for validation / exact-final-answer / post-write paths is complete
- runtime phase/checkpoint work was preserved and repaired on top of, not reverted
- a narrow runtime artifact/checkpoint consistency hardening slice is currently in progress in the worktree

Current non-baseline note:

- `cargo clippy -- -D warnings` is not currently green; present failures are preexisting lint findings in runtime coordinator files outside the current hardening slice

Recent runtime-target commits relevant to this line of work:

- `c5a4f87` `Update runtime target progress status`
- `29a5797` `Advance checkpoint-backed runtime phases`
- `293ffa1` `Advance checkpoint-backed phase loop and resume state`
- `376e31b` `Close out phase 5 runtime coordination`
- `c59dbc2` `Validate terminal runtime checkpoints`

## Plan Summary

The implementation plan being executed is the migration plan in the runtime target doc.

High-level order:

1. Finish behavior-parity repair on the current runtime loop.
2. Make `RunCheckpointV1` more authoritative inside the live loop.
3. Expand resume beyond boundary replay into richer checkpoint-backed continuation.
4. Remove transitional heuristic / loop-local branches once parity is proven.
5. Finish the explicit phase-loop consolidation so `src/agent.rs` becomes a coordinator over checkpoint-backed helpers.

## Progress By Phase

### Phase 1: Contract Introduction

Status: complete

Implemented:

- `TaskContractV1` exists
- task contract resolves at launch
- contract provenance persists in artifacts

Notes:

- prompt heuristics still exist as compatibility fallback, which matches the target doc

### Phase 2: Typed Tool Facts

Status: complete for v1 fact emission and artifact/checkpoint persistence

Implemented:

- `ToolFactV1` exists
- `ToolFactEnvelopeV1` exists
- tool facts persist in artifacts and checkpoints
- runtime policy now consumes more of the fact-backed state than before

Notes:

- this phase can still grow more policy consumers later without needing a new migration branch

### Phase 3: Checkpointed Interrupt Boundaries

Status: substantially complete

Implemented:

- `RunCheckpointV1` exists and persists
- execution tier, interrupt history, phase summary, and completion decisions persist
- approval and operator-interrupt transitions emit explicit runtime-owned phase events
- approval resume exists
- resume now restores richer checkpoint-backed state instead of only replaying a boundary
- resumed runs can return to `Validating`, `VerifyingChanges`, and `CollectingFinalAnswer` based on checkpoint state

Still incomplete:

- explicit phase-loop consolidation is not finished, so not every resumable/nonterminal boundary is yet represented through a fully phase-dispatched live loop

### Phase 4: Central Completion Policy

Status: substantially complete

Implemented:

- verified-write completion policy is centralized
- required-validation completion policy is centralized
- validation phase transitions are centralized
- approval/operator/final-answer transition helpers exist
- validation / exact-final-answer / post-write behavior now runs on checkpoint-backed phase and retry state
- tool-protocol loop state moved from separate loop locals into `RunCheckpointV1`

Still incomplete:

- some transition/orchestration logic is still inline in `src/agent.rs`

### Phase 5: Explicit Phase Loop

Status: effectively complete for v1

Implemented so far:

- `RunPhase` exists
- approval, operator-interrupt, validation, and final-answer boundaries emit explicit phase transitions
- artifacts and checkpoints persist phase-oriented state
- `RunCheckpointV1` is materially more authoritative in the live loop
- `src/agent.rs` has begun being split into checkpoint-backed helper paths instead of one fully inline loop

Recent consolidation slices already landed in `src/agent.rs`:

- required-validation phase enforcement extracted into helper logic
- post-response phase guards extracted into helper logic
- runtime completion action application extracted into helper logic
- tool-fact / validation phase refresh extracted into helper logic
- verified-write follow-on handling extracted into helper logic
- planner control-envelope handling extracted into helper logic
- planner-response evaluation extracted into `src/agent/planner_phase.rs`
- assistant tool-call normalization / protocol validation extracted into `src/agent/response_normalization.rs`
- tool execution planning/gate loop extracted into helper logic
- active runtime phases now route through an explicit dispatcher in `src/agent.rs`
- executing / validating / verifying-changes / collecting-final-answer phase turns now have named helper entrypoints
- normalized-response handling and verified-write follow-on handling are now split out of the shared active-turn path
- completion / tool-execution handling is now split out of the shared active-turn path
- provider response acquisition and assistant protocol normalization are now split out of the shared active-turn path
- `Validating` and `CollectingFinalAnswer` now own their phase entrypoint flow instead of routing through the same top-level phase function as `Executing`
- `VerifyingChanges` now also owns its phase entrypoint flow instead of routing through the same top-level phase function as `Executing`
- `Executing` now also owns its phase entrypoint flow; no active phase routes through a shared top-level phase function anymore
- the phase dispatcher now has explicit non-active phase handlers instead of one generic fallback branch
- the dispatcher now names `Setup`, `Planning`, `Finalizing`, interrupt, and terminal handling individually rather than classifying them only via a shared non-active branch
- the repeated active-turn setup now routes through one lower-level helper for response generation, normalization, and runtime-owned response processing while preserving explicit per-phase entrypoints
- the remaining completion/tool coordinator path is now split into separate helpers for runtime completion decisions, tool execution, and post-tool follow-on handling
- runtime completion checkpoint transitions and post-tool/post-write checkpoint refresh logic now route through `src/agent/phase_transitions.rs`, leaving `agent.rs` to handle orchestration and event emission
- required-validation phase and post-response guard checkpoint logic now route through `src/agent/response_guards.rs`, leaving `agent.rs` to handle repair injection, events, and planner-error finalization
- the remaining decision-to-effects translation for guard outcomes and post-tool follow-on now routes through `src/agent/runtime_effects.rs`, further reducing inline message/event/control adaptation in `agent.rs`
- the outer per-step runtime loop now routes through a dedicated coordinator helper, leaving `run_with_checkpoint` closer to setup -> iterate -> finalize

Phase 5 closeout assessment:

- `src/agent.rs` is now materially coordinator-like for v1 purposes
- the explicit phase-loop target is effectively satisfied for v1
- any remaining cleanup is optional unless a concrete runtime regression or readability issue appears

### Phase 6: Execution Tier Integration

Status: complete for v1 visibility

Implemented:

- execution tier resolves at launch
- execution tier persists in checkpoint and artifacts
- execution tier is emitted in startup/runtime evidence

## Runtime Behavior Repair Status

This was the blocker before architectural consolidation resumed.

Status: complete

Fixed areas:

- exact-final-answer retry / validation interaction
- post-write verification / follow-on transitions
- required-validation phase handoff and repair behavior
- repeated failed `apply_patch` / `str_replace` pivot behavior
- new-file create allowances
- tool-surface ordering / recovery wording regressions in `src/tools/tests.rs`

Result:

- `cargo test --quiet` is green

## Current Code Shape

The migration is now in the "architectural consolidation" stage rather than "behavior repair".

What is true now:

- checkpoint-backed state is the main live control surface for validation / final-answer / post-write / tool-protocol runtime state
- resume is checkpoint-backed rather than boundary-only
- the main runtime loop is materially cleaner than before and now explicitly dispatches active runtime phases, but it is still not fully phase-dispatched end to end
- terminal runtime checkpoints are now validated before final artifact writing so `Done` cannot be serialized with unsatisfied required validation evidence
- the current in-progress slice extends that hardening to final artifact consistency across approval, cancellation, and resume-to-terminal boundaries
- the relevant runtime artifact-writing paths now route through the same final-artifact consistency validator

Most relevant files for the next person picking this up:

- [agent.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent.rs)
- [completion_policy.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/completion_policy.rs)
- [interrupts.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/interrupts.rs)
- [run_setup.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/run_setup.rs)
- [state.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/state.rs)
- [checkpoint.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/checkpoint.rs)
- [agent_tests.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_tests.rs)

## Remaining Work

The next logical work is no longer more open-ended Phase 5 extraction.

Recommended order:

1. Treat Phase 5 as effectively closed unless a concrete runtime-loop regression appears.
2. If future runtime work touches the loop again, use targeted regressions plus `cargo test --quiet` and avoid reopening broad coordinator refactors without evidence.
3. Finish the current narrow invariant hardening slice for runtime artifact/checkpoint consistency, then stop unless a concrete regression remains.
4. After that, shift attention to later runtime priorities that build on the checkpoint-backed phase model rather than more structural cleanup for its own sake.
5. Prefer narrow invariant hardening like terminal-checkpoint/runtime-artifact consistency checks over more coordinator reshaping.

Current hardening-slice checklist:

- terminal runtime checkpoint validation is already landed
- final artifact consistency validation across finalize/planner artifact writers is now wired
- prior interrupt history / phase summary / completion-decision history is preserved across resume-to-terminal finalization
- cancelled runs no longer keep resumable runtime checkpoints
- cancelled resume attempts are explicitly rejected by replay/resume tests
- `cargo test --quiet` is green after reconciliation
- the slice is ready to commit unless a new concrete regression appears

## Suggested Handoff Rules

For the next slice:

- do not reopen behavior repair unless a new failing regression appears
- do not revert the checkpoint/phase work
- do not broaden into unrelated refactors
- preserve `cargo test --quiet` green status after each slice
- prefer small coordinator-style extractions over semantic rewrites

## Verification Expectation

Minimum verification after each further runtime-loop slice:

- `cargo test --quiet`

Preferred additional verification when the touched area is relevant:

- targeted `src/agent_tests.rs` regression tests for the affected phase family
- `cargo clippy -- -D warnings`
