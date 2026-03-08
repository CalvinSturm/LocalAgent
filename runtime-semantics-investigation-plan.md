# Runtime Semantics Investigation Plan

## Request Summary

This plan scopes a short, evidence-first investigation into LocalAgent runtime semantics, especially around continuation, guard failures, ineffective writes, and final-output shaping. The immediate goal is to turn recent eval and manual-run artifacts into explicit runtime rules before changing shared loop behavior.

## Relevant Codebase Findings

The governing policy is explicit: extra turns require a classified, bounded reason, and a verified successful write is terminal by default unless a validator, guard, phase transition, or explicit user-directed next step says otherwise in `docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md`.

The main runtime path is `main -> run_cli -> run_agent -> run_agent_with_ui -> Agent::run`, documented in `docs/architecture/RUNTIME_ARCHITECTURE.md` and implemented in `src/agent_runtime.rs`, `src/cli_dispatch.rs`, and `src/agent.rs`.

Recent proving artifacts already exist:

- Built-in eval results: `.tmp/eval-glm-4_6v-flash-coding-20260307-195639.json`
- Exact manual run records: `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/*.json`

## Proposed Implementation Approach

First, define the semantic questions using real failures rather than code-first speculation. Then trace those questions through the governed runtime surfaces and write down the current effective state transitions. Finally, convert each confirmed rule into contract tests and only then decide whether code changes are needed.

Investigation order:

1. Terminality after write and ineffective write handling.
   Use:
   - `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/10a2c817-cd55-425f-949b-112d566d9363.json`
   - `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/c73e1f09-4fd8-4b08-9a52-1d70b3917af6.json`
   - `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/aabc6b83-595c-4036-b441-6499cf0b6cc1.json`
   - `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/db767054-b223-461b-975a-d870c85f4e62.json`

   Questions:
   - What counts as verified success?
   - What counts as ineffective write?
   - Why do some cases end as `ok` while others end as `planner_error`?

2. Final-output shaping versus protocol-artifact leakage.
   Use:
   - `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/f0233fd2-bf88-4aa0-9f7e-3d2422486f3d.json`
   - `.tmp/eval-glm-4_6v-flash-coding-20260307-195639.json`

   Question:
   - When is malformed wrapper text being accepted as terminal output instead of being normalized or rejected?

3. Guard-triggered retry versus terminal failure.
   Trace where implementation-guard decisions are injected and finalized in:
   - `src/agent_runtime.rs`
   - `src/agent.rs`
   - governed finalize/guard helpers under `src/agent_runtime/*`

   Question:
   - Are guard failures machine-classified distinctly enough to satisfy repo policy?

4. Validator-driven continuation.
   Use `C3` and `C5` artifacts to answer:
   - Is failed or skipped validation explicitly represented as validator failure?
   - Or is the runtime relying on model narration and generic planner errors?

## Ordered PR Breakdown

1. `runtime-semantics-investigation-notes`
   Primary goal: document the current runtime state transitions from evidence and identify mismatches against repo policy.

2. `runtime-contract-tests-for-confirmed-semantics`
   Primary goal: encode the confirmed rules as positive and negative contract tests before any behavior change.

3. `runtime-semantics-fixups`
   Primary goal: only if needed, narrow runtime behavior to match the documented semantics and tests.

## Per-PR Scope Details

### PR 1: `runtime-semantics-investigation-notes`

In scope:
- Trace current behavior
- Map artifact to code path
- Classify each observed transition

Out of scope:
- Behavior changes

Key files:
- `docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md`
- `docs/architecture/RUNTIME_ARCHITECTURE.md`
- `src/agent_runtime.rs`
- `src/agent.rs`
- recent run JSON artifacts

Acceptance criteria:
- A short design note answers four questions:
  - terminal after write
  - ineffective write semantics
  - final-output shaping
  - guard or validator authorization for another turn
- A state-transition table exists with the following columns:
  - event or condition
  - runtime classification
  - allowed continuation
  - terminal outcome
  - governing policy source
  - source artifact or code path
- An artifact classification table exists for each investigated run ID with the following columns:
  - run ID
  - observed outcome
  - runtime-caused / model-caused / mixed / unresolved
  - justification

Test or verification expectations:
- Artifact-to-code trace is reproducible from cited files and run IDs

Notes on why this PR boundary is correct:
- It prevents speculative code changes and raises the semantic bar first
- It gives PR 2 an explicit contract surface instead of forcing tests to infer semantics from prose

### PR 2: `runtime-contract-tests-for-confirmed-semantics`

In scope:
- Add contract tests for confirmed positive and negative cases

Out of scope:
- Broad refactors
- Heuristic additions

Key files or subsystems likely to change:
- Runtime tests near `src/agent_runtime.rs`
- Runtime tests near `src/agent.rs`
- Governed finalize and guard test surfaces

Acceptance criteria:
- Tests prove at least:
  - successful verified write does not itself authorize another turn
  - ineffective write is not treated as terminal success
  - protocol-artifact wrapper text does not become valid final output
  - guard or validator continuation, if allowed, is explicitly classified

Test or verification expectations:
- `cargo test` targeted to touched runtime tests

Notes on why this PR boundary is correct:
- It locks the contract before behavior edits

### PR 3: `runtime-semantics-fixups`

In scope:
- Only the narrow code changes needed to align runtime behavior with policy and new tests

Out of scope:
- Unrelated eval harness changes
- Broad architecture rewrites

Key files or subsystems likely to change:
- Governed runtime files implicated by PR 1 findings
- Likely `src/agent_runtime/*`
- Likely `src/agent.rs`

Acceptance criteria:
- Runtime behavior matches the documented state transitions
- All contract tests pass

Test or verification expectations:
- Targeted runtime tests
- Replay against the exact failing artifacts where possible

Notes on why this PR boundary is correct:
- It isolates actual semantic fixes from investigation and test-definition work

## Risks / Open Questions

The biggest risk is mistaking model failure for runtime failure. The recent artifacts already show both kinds, so PR 1 needs to separate them carefully.

The second risk is blessing incidental loop shape in tests. The policy explicitly forbids that, so every test should be phrased as a contract assertion, not as a specific step-count expectation.

The third risk is letting validator-driven continuation and guard-triggered retry remain semantically fuzzy. PR 1 should explicitly define:

- who authorizes another turn
- what machine-readable reason is required
- whether the case is classified as guard retry, validator continuation, declared phase transition, or terminal result
