# LocalAgent vNext Runtime Handoff

## Purpose

This file is the handoff summary for the vNext runtime migration work.

Canonical architecture and migration plan:

- [LOCALAGENT_VNEXT_RUNTIME_TARGET.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/docs/architecture/LOCALAGENT_VNEXT_RUNTIME_TARGET.md)

This handoff is a status snapshot of what has already landed in code, what remains, and what the next logical implementation slices should be.

## Current Baseline

Current verified baseline:

- `cargo clippy -- -D warnings` passes
- `cargo test --quiet` passes
- behavior-repair work for validation / exact-final-answer / post-write paths is complete
- runtime phase/checkpoint work was preserved and repaired on top of, not reverted

Recent runtime-target commits relevant to this line of work:

- `c5a4f87` `Update runtime target progress status`
- `29a5797` `Advance checkpoint-backed runtime phases`
- `293ffa1` `Advance checkpoint-backed phase loop and resume state`

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

Status: in progress

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
- tool execution planning/gate loop extracted into helper logic

Still incomplete:

- `src/agent.rs` still contains mixed model/response orchestration and top-level step driving logic
- the live loop is not yet a simple `match checkpoint.phase { ... }` dispatcher matching the target pseudocode

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
- the main runtime loop is materially cleaner than before, but still not fully phase-dispatched

Most relevant files for the next person picking this up:

- [agent.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent.rs)
- [completion_policy.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/completion_policy.rs)
- [interrupts.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/interrupts.rs)
- [run_setup.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/run_setup.rs)
- [state.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/state.rs)
- [checkpoint.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/checkpoint.rs)
- [agent_tests.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_tests.rs)

## Remaining Work

The next logical work is all under Phase 5.

Recommended order:

1. Extract the remaining model/assistant normalization and planner-response handling from `src/agent.rs`.
2. Continue shrinking `src/agent.rs` into a coordinator over checkpoint-backed helper methods.
3. Move any remaining inline completion/transition logic into `completion_policy.rs` or other phase-specific helpers where appropriate.
4. Tighten any remaining implicit nonterminal checkpoint/resume boundaries as the loop becomes more explicitly phase-dispatched.
5. Update the runtime target doc progress text again when another meaningful consolidation slice lands.

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
