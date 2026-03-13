# Runtime Heuristic Reconciliation Plan (2026-03)

Status: completed closeout record  
Scope: reconcile remaining heuristic runtime controls against the vNext baseline  
Goal: preserve bounded compatibility/context heuristics, formalize runtime-semantic heuristics, and avoid broad speculative refactors

## Why This Exists

LocalAgent vNext is now the primary architectural model: runtime-owned phases, checkpoint-backed state, explicit contracts, and artifact-visible decisions.

Some older heuristic controls still remain in the codebase. They are not all equally risky:

- some are still the right bounded fallback behavior
- some still materially shape runtime semantics and should move into explicit contract/profile/config surfaces
- none currently require immediate wholesale removal

This document now records the completed reconciliation work and the small optional follow-on items that remain.

## Closeout Summary

Outcome:

- explicit validator requirement and exact-final-answer/output-contract surfaces are landed for checks/evals plus runtime overrides
- task-kind resolution now prefers explicit task/profile metadata and exact canonicalization over broad prompt substring inference
- write requirement and completion policy defaults now derive from resolved task kind instead of raw implementation-guard state
- effective-write and post-write follow-on prompt heuristics were reduced to narrow fallback compatibility behavior
- runtime docs/status were updated to reflect the landed behavior instead of leaving the slices as active work

Final validation pass:

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`

All three passed on the closeout tree.

## Current Classification

### Keep

- [x] Qualification probe and read-only fallback remain bounded compatibility logic in [qualification.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/qualification.rs)
- [x] Planner-worker default plan-tool enforcement remains a runtime default in [runtime_flags.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/runtime_flags.rs)
- [x] Agent-mode capability baseline remains a runtime default in [runtime_flags.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/runtime_flags.rs)
- [x] Compact manual-repair context selection remains a narrow manual-testing optimization in [agent_runtime.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime.rs)
- [x] Context augmentation assembly remains artifact-visible and bounded in [agent_runtime.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime.rs) and [launch.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/launch.rs)

### Formalize

- [x] Replace prompt-derived validation requirement inference in [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs) and [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs) with an explicit validator requirement surface
- [x] Replace prompt-derived exact final answer inference in [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs) and [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs) with an explicit output-contract surface
- [x] Reduce keyword-based task kind normalization/inference in [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs) by preferring explicit task kind/profile metadata
- [x] Reduce implementation-guard-driven write/completion inference in [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs) and [launch.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/launch.rs) by making the relevant contract fields more explicit
- [x] Replace prompt-based effective-write / post-write-follow-on semantics in [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs) with explicit contract or runtime policy fields

### Remove

- [x] No immediate removal candidates identified; reassess only if a future regression demonstrates that a remaining compatibility heuristic should be removed

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

Status note:
- fresh-run contract resolution is implemented
- resume/checkpoint persistence of explicit validator provenance is implemented for the current runtime checkpoint/replay path

- [x] Add an explicit validator-requirement input surface
- [x] Thread that surface through runtime launch into `TaskContractV1`
- [x] Preserve prompt-derived validation inference only as fallback, with provenance clearly marked as inferred
- [x] Add tests showing explicit validator requirement overrides prompt inference
- [x] Add tests showing fallback inference still works where explicit metadata is absent

Implemented slices:

- explicit runtime override path via `RunArgs.validation_command_override`
- explicit check metadata via `CheckFrontmatter.validation_command`
- eval task verifier metadata now feeds the same explicit runtime validation requirement path
- contract resolution and runtime state/final artifacts now prefer explicit validator requirement over prompt inference
- resumable checkpoint replay now restores the explicit validator requirement override path used by runtime launch/replay
- manual-pack validator metadata wiring still remains future work if packs need structured task contracts

Acceptance bar:
- validation requirement no longer depends primarily on substring matching
- run artifacts/events show whether validator requirement was explicit or inferred
- runtime checkpoint replay preserves the explicit validator requirement override used for resumed runs

### Phase 3: Exact Final Answer / Output Contract Formalization

Status note:
- fresh-run contract resolution is implemented
- resume/checkpoint persistence of explicit output-contract provenance is implemented for the current runtime checkpoint/replay path

- [x] Add an explicit output-contract surface for exact final answer requirements
- [x] Thread that surface through runtime launch into `TaskContractV1`
- [x] Preserve prompt-derived exact-answer inference only as fallback, with provenance clearly marked as inferred
- [x] Add tests showing explicit output contract overrides prompt inference
- [x] Add tests showing fallback inference still works where explicit metadata is absent

Implemented slices:

- explicit runtime override path via `RunArgs.exact_final_answer_override`
- explicit check metadata via `CheckFrontmatter.exact_final_answer`
- explicit eval task metadata via `EvalTask.exact_final_answer`
- runtime `Agent`, runtime state, and final-artifact paths now consume the explicit output contract instead of recomputing exact-answer semantics only from prompt markers
- resumable checkpoint replay now restores the explicit output-contract override path used by runtime launch/replay
- manual-pack output-contract metadata wiring still remains future work if packs need structured task contracts

Acceptance bar:
- exact-answer runtime behavior no longer depends primarily on prompt markers
- run artifacts/events show whether the output contract was explicit or inferred
- runtime checkpoint replay preserves the explicit output-contract override used for resumed runs

### Phase 4: Task Kind and Write/Completion Contract Tightening

- [x] Add or strengthen explicit task-kind/profile inputs so `task_kind` is not mostly keyword-normalized
- [x] Revisit how implementation guard drives `write_requirement` and completion policy defaults
- [x] Move any remaining write/completion-critical decisions out of prompt-wording inference where practical
- [x] Add tests showing explicit task kind drives contract resolution deterministically

Implemented slices:

- `task_kind` canonicalization now uses exact alias / exact phrase handling instead of broad substring matching
- explicit instruction task profiles now count as an explicit `task_kind` source during contract resolution
- implementation-guard enablement now reuses the same canonical task-kind logic, reducing accidental prompt-wording classification drift
- `write_requirement` and `completion_policy` defaults now derive from the resolved `task_kind` instead of directly from raw implementation-guard state
- targeted regressions now cover explicit `analysis`, `planning`, and `validation` task kinds plus the prior substring-regression case

Acceptance bar:
- `task_kind` is primarily authored explicitly, not guessed
- write/completion contract behavior is explainable without prompt keyword reasoning

### Phase 5: Effective-Write / Post-Write Follow-On Cleanup

- [x] Identify which current `agent_impl_guard.rs` prompt checks are still required after Phases 2-4
- [x] Remove or reduce prompt-based effective-write and follow-on inference that became redundant
- [x] Keep only narrow fallback heuristics that still have reproduced evidence behind them
- [x] Add regressions covering the remaining allowed heuristic paths

Implemented slices:

- removed dead wrapper helpers from [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs) so runtime behavior now routes through the live contract/fact-based paths
- deduplicated the effective-write heuristic onto the live implementation in [tool_facts.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/tool_facts.rs)
- narrowed post-write follow-on inference so it now acts only as a compatibility fallback for explicit user-facing closeout requests, not for generic validation wording
- preserved prompt-derived validation-command and exact-final-answer parsing as fallback-only compatibility inputs where no explicit contract metadata exists

Acceptance bar:
- prompt-wording heuristics are no longer the main driver of effective-write or follow-on runtime semantics
- remaining heuristics are narrow, documented, and evidence-backed

## Non-Goals

- [ ] Do not rewrite qualification probing into a totally different architecture just because it contains compatibility parsing
- [ ] Do not remove compact manual-testing context heuristics unless they produce real regressions
- [ ] Do not broaden shared runtime heuristics during this cleanup
- [ ] Do not reopen Phase 5 runtime migration/refactoring by default

## Verification Expectations

- [x] `cargo test --quiet` remains green after each implementation slice
- [x] Add targeted regressions near each formalized heuristic surface
- [x] Preserve artifact/checkpoint provenance visibility for explicit vs inferred contract values
- [x] Update runtime docs when a heuristic becomes explicit contract/state instead of fallback

## Optional Follow-On Work

- `prompt_requires_tool_only` in [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs) remains a bounded compatibility heuristic; only formalize it further if a concrete requirement appears
- manual-pack/task metadata is still the next explicit authoring surface if packs need the same validator/output-contract semantics already available to checks and evals
- broader removal of remaining fallback prompt parsing is not required for this plan closeout

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
