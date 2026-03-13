# Runtime Heuristic Reconciliation Plan (2026-03)

Status: active implementation plan  
Scope: reconcile remaining heuristic runtime controls against the vNext baseline  
Goal: preserve bounded compatibility/context heuristics, formalize runtime-semantic heuristics, and avoid broad speculative refactors

## Why This Exists

LocalAgent vNext is now the primary architectural model: runtime-owned phases, checkpoint-backed state, explicit contracts, and artifact-visible decisions.

Some older heuristic controls still remain in the codebase. They are not all equally risky:

- some are still the right bounded fallback behavior
- some still materially shape runtime semantics and should move into explicit contract/profile/config surfaces
- none currently require immediate wholesale removal

This plan tracks that reconciliation work as small, reviewable slices.

## Current Classification

### Keep

- [ ] Qualification probe and read-only fallback remain bounded compatibility logic in [qualification.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/qualification.rs)
- [ ] Planner-worker default plan-tool enforcement remains a runtime default in [runtime_flags.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/runtime_flags.rs)
- [ ] Agent-mode capability baseline remains a runtime default in [runtime_flags.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/runtime_flags.rs)
- [ ] Compact manual-repair context selection remains a narrow manual-testing optimization in [agent_runtime.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime.rs)
- [ ] Context augmentation assembly remains artifact-visible and bounded in [agent_runtime.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime.rs) and [launch.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/launch.rs)

### Formalize

- [ ] Replace prompt-derived validation requirement inference in [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs) and [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs) with an explicit validator requirement surface
- [ ] Replace prompt-derived exact final answer inference in [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs) and [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs) with an explicit output-contract surface
- [ ] Reduce keyword-based task kind normalization/inference in [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs) by preferring explicit task kind/profile metadata
- [ ] Reduce implementation-guard-driven write/completion inference in [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs) and [launch.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/launch.rs) by making the relevant contract fields more explicit
- [ ] Replace prompt-based effective-write / post-write-follow-on semantics in [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs) with explicit contract or runtime policy fields

### Remove

- [ ] No immediate removal candidates identified; reassess after the formalization slices land

## Ordered Implementation Phases

### Phase 1: Document Explicit Contract Targets

- [x] Decide where validator requirement should be authored first:
  - chosen first surface: check/eval/manual-pack metadata
  - follow-on surfaces: task profile, then CLI/config when a general operator-authored override is needed
- [x] Decide where exact final answer/output contract should be authored first
  - chosen first surface: check/eval/manual-pack metadata
  - follow-on surfaces: task profile, then CLI/config when a general operator-authored override is needed
- [x] Decide the minimum explicit task-kind source of truth for v1 follow-on work
  - chosen first sources: explicit `--task-kind` and explicit instruction task profile
  - fallback prompt/guard inference remains temporary compatibility behavior only
- [x] Record the chosen authoring surfaces in [LOCALAGENT_VNEXT_RUNTIME_TARGET.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/docs/architecture/LOCALAGENT_VNEXT_RUNTIME_TARGET.md) or another canonical runtime doc

Acceptance bar:
- there is one explicit answer for each of validator requirement, output contract, and task kind
- the answer does not rely on future broad refactors

### Phase 2: Validation Requirement Formalization

- [x] Add an explicit validator-requirement input surface
- [x] Thread that surface through runtime launch into `TaskContractV1`
- [x] Preserve prompt-derived validation inference only as fallback, with provenance clearly marked as inferred
- [x] Add tests showing explicit validator requirement overrides prompt inference
- [x] Add tests showing fallback inference still works where explicit metadata is absent

Implemented first slice:

- explicit runtime override path via `RunArgs.validation_command_override`
- explicit check metadata via `CheckFrontmatter.validation_command`
- contract resolution now prefers explicit validator requirement over prompt inference
- eval/manual-pack metadata wiring remains future work

Acceptance bar:
- validation requirement no longer depends primarily on substring matching
- artifacts/checkpoints show whether validator requirement was explicit or inferred

### Phase 3: Exact Final Answer / Output Contract Formalization

- [ ] Add an explicit output-contract surface for exact final answer requirements
- [ ] Thread that surface through runtime launch into `TaskContractV1`
- [ ] Preserve prompt-derived exact-answer inference only as fallback, with provenance clearly marked as inferred
- [ ] Add tests showing explicit output contract overrides prompt inference
- [ ] Add tests showing fallback inference still works where explicit metadata is absent

Acceptance bar:
- exact-answer runtime behavior no longer depends primarily on prompt markers
- artifacts/checkpoints show whether the output contract was explicit or inferred

### Phase 4: Task Kind and Write/Completion Contract Tightening

- [ ] Add or strengthen explicit task-kind/profile inputs so `task_kind` is not mostly keyword-normalized
- [ ] Revisit how implementation guard drives `write_requirement` and completion policy defaults
- [ ] Move any remaining write/completion-critical decisions out of prompt-wording inference where practical
- [ ] Add tests showing explicit task kind drives contract resolution deterministically

Acceptance bar:
- `task_kind` is primarily authored explicitly, not guessed
- write/completion contract behavior is explainable without prompt keyword reasoning

### Phase 5: Effective-Write / Post-Write Follow-On Cleanup

- [ ] Identify which current `agent_impl_guard.rs` prompt checks are still required after Phases 2-4
- [ ] Remove or reduce prompt-based effective-write and follow-on inference that became redundant
- [ ] Keep only narrow fallback heuristics that still have reproduced evidence behind them
- [ ] Add regressions covering the remaining allowed heuristic paths

Acceptance bar:
- prompt-wording heuristics are no longer the main driver of effective-write or follow-on runtime semantics
- remaining heuristics are narrow, documented, and evidence-backed

## Non-Goals

- [ ] Do not rewrite qualification probing into a totally different architecture just because it contains compatibility parsing
- [ ] Do not remove compact manual-testing context heuristics unless they produce real regressions
- [ ] Do not broaden shared runtime heuristics during this cleanup
- [ ] Do not reopen Phase 5 runtime migration/refactoring by default

## Verification Expectations

- [ ] `cargo test --quiet` remains green after each implementation slice
- [ ] Add targeted regressions near each formalized heuristic surface
- [ ] Preserve artifact/checkpoint provenance visibility for explicit vs inferred contract values
- [ ] Update runtime docs when a heuristic becomes explicit contract/state instead of fallback

## Suggested PR Breakdown

1. `runtime-contract-validator-surface`
   - explicit validator requirement
   - provenance + tests

2. `runtime-contract-output-surface`
   - explicit exact final answer/output contract
   - provenance + tests

3. `runtime-task-kind-tightening`
   - reduce keyword-driven task kind inference
   - tighten write/completion defaults

4. `runtime-heuristic-cleanup-follow-on`
   - reduce redundant prompt-based effective-write/follow-on heuristics
   - preserve only justified fallback logic
