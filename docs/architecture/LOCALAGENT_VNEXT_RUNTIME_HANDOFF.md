# LocalAgent vNext Runtime Handoff

## Purpose

This file is the stable handoff summary for the LocalAgent vNext runtime work after the v1 migration reached its achieved baseline.

Canonical architecture and guardrail doc:

- [LOCALAGENT_VNEXT_RUNTIME_TARGET.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/docs/architecture/LOCALAGENT_VNEXT_RUNTIME_TARGET.md)

Historical temporary pickup/implementation docs from the migration closeout have been moved to [docs/archive](/C:/Users/Calvin/Software%20Projects/LocalAgent/docs/archive).

## Current Baseline

Current verified baseline at `HEAD` `1f03e4600527da1ce1d948aa6fb72d55f542e93a`:

- worktree is clean
- `cargo test --quiet` passes
- `cargo clippy -- -D warnings` passes
- the v1 runtime target is effectively achieved

## What Landed

The main v1 runtime migration outcomes are now in place:

- `TaskContractV1` exists and is resolved at launch
- typed tool facts and envelopes exist and persist in artifacts/checkpoints
- `RunCheckpointV1`, execution tier, interrupt history, phase summary, and completion decisions persist
- resume is checkpoint-backed rather than boundary-only
- approval and operator-interrupt boundaries emit explicit runtime-owned phase events
- active runtime phases dispatch explicitly
- completion policy and runtime guard behavior now rely materially on checkpoint-backed state
- terminal runtime checkpoints are validated before artifact persistence
- final runtime artifacts are validated for consistency across approval, cancellation, and resume-to-terminal boundaries
- replay coverage exists for approval-boundary and operator-boundary resume-to-completion artifact preservation

## Current Runtime Status

Phase status for v1:

- Phase 1 contract introduction: complete
- Phase 2 typed tool facts: complete for v1
- Phase 3 checkpointed interrupt boundaries: complete enough for v1 baseline use
- Phase 4 central completion policy: complete enough for v1 baseline use
- Phase 5 explicit phase loop: closed for v1 unless a concrete regression appears
- Phase 6 execution tier integration: complete for v1 visibility

What remains true:

- some coordinator/orchestration logic still lives inline in [agent.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent.rs)
- that is no longer treated as active migration debt by itself
- future runtime work should be justified by concrete regressions, reproduced artifacts, or clearly scoped new capabilities

## Most Relevant Files

- [agent.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent.rs)
- [completion_policy.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/completion_policy.rs)
- [interrupts.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/interrupts.rs)
- [phase_transitions.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/phase_transitions.rs)
- [response_guards.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/response_guards.rs)
- [runtime_effects.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/runtime_effects.rs)
- [checkpoint.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/checkpoint.rs)
- [finalize.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/finalize.rs)
- [planner_phase.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/planner_phase.rs)
- [cli_dispatch_eval_replay.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/cli_dispatch_eval_replay.rs)

## Guidance From Here

Treat the runtime target as the achieved v1 baseline plus future guardrails.

Do:

- use targeted regressions when changing runtime behavior
- keep `cargo test --quiet` and `cargo clippy -- -D warnings` green
- prefer narrow evidence-backed slices
- use the target doc to judge future runtime changes against the achieved architecture

Do not:

- reopen Phase 5 structural refactoring by default
- treat inline coordinator code as migration debt without a concrete bug or readability issue
- extend checkpoint/artifact hardening without a concrete uncovered boundary or failing regression
