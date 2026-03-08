# Generic Run Validator Investigation (2026-03)

Status: investigation only  
Scope: generic `run` validator semantics  
Behavior changes: none

Evidence:
- `docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md`
- `docs/policy/RUNTIME_SEMANTICS_INVESTIGATION_NOTES_2026-03.md`
- `src/agent/runtime_completion.rs`
- `src/agent/run_finalize.rs`
- `src/agent/run_setup.rs`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/10a2c817-cd55-425f-949b-112d566d9363.json`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/c73e1f09-4fd8-4b08-9a52-1d70b3917af6.json`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/f0233fd2-bf88-4aa0-9f7e-3d2422486f3d.json`
- `.tmp/eval-glm-4_6v-flash-coding-20260307-195639.json`

## Purpose

This note opens a narrow investigation into validator semantics for generic `run`.

It does not propose runtime-loop changes. It exists to answer whether plain `run` should ever gain an explicit validator surface for task-style acceptance, or whether that responsibility should remain outside the shared runtime contract.

## Current Observed Behavior

In generic `run` mode, the shared runtime finalizes `ok` when it reaches a valid terminal runtime boundary:

- no pending planner/tool-only/implementation guard continuation reason
- no caught protocol artifact
- no terminal guard failure

Current finalize path:

- `src/agent/runtime_completion.rs`: `FinalizeOk -> finalize_ok_with_end`
- `src/agent/run_setup.rs`: `final_output_for_completion`
- `src/agent/run_finalize.rs`: terminal result shaping

This means generic `run` has runtime validation for structural/runtime concerns, but not task-specific acceptance checks such as:

- exact string match
- required shell/test execution
- semantic correctness of file contents
- repo-specific acceptance criteria

## Why This Matters

Recent manual exact artifacts show that runtime `ok` and task correctness are not the same thing:

- `10a2c817-cd55-425f-949b-112d566d9363`: runtime `ok`, but file content was literal `hello\n` bytes instead of a real newline
- `c73e1f09-4fd8-4b08-9a52-1d70b3917af6`: runtime `ok`, but final output included wrapper tokens
- `f0233fd2-bf88-4aa0-9f7e-3d2422486f3d`: runtime `ok`, but the task was not actually completed

PR 2 already addressed the reproduced runtime-boundary defects in those cases:

- wrapper rejection improved
- verified-write terminality restored
- `fix` wording now participates in effective-write enforcement

The remaining question is narrower:

- should generic `run` itself ever enforce task-style acceptance
- or should that remain the job of eval harnesses, explicit validators, checks, hooks, or higher-level task contracts

## Investigation Questions

1. What is the intended contract of generic `run`?
2. Is `AgentExitReason::Ok` in generic `run` meant to mean:
   - runtime completed coherently
   - or task acceptance criteria were satisfied
3. What validator surfaces already exist outside generic `run`?
4. If generic `run` gained validators, would they be:
   - explicit opt-in
   - task-contract-driven
   - hook/check based
   - or implicit heuristics
5. Would adding generic-run validators broaden the shared runtime loop in a way that conflicts with `AGENT_RUNTIME_PRINCIPLES_2026.md`?

## Existing Validator-Like Surfaces

Already present in the repo:

- eval harness assertions and verifiers
- planner/phase contracts
- trust/guard enforcement
- protocol-artifact rejection
- implementation-integrity guard
- hooks/checks infrastructure

These suggest the repo already has extension points for correctness checks outside the generic `run` terminal contract.

## Working Hypotheses

Hypothesis A:
Generic `run` should remain structurally validated but task-agnostic. In this model, runtime `ok` means the runtime completed coherently, not that external acceptance criteria were satisfied.

Hypothesis B:
Generic `run` may support explicit validator attachments, but only as declared opt-in contract surfaces. In this model, runtime `ok` without validators still means structural completion only.

Hypothesis C:
Generic `run` should not infer task validators from prompt wording. That would turn shared runtime semantics into heuristic task interpretation and likely violate the repo policy against speculative loop broadening.

## Evidence Needed Before Any Behavior Change

Do not change generic `run` semantics until there is artifact-backed evidence answering:

- what user/operator expectation is actually being violated
- whether an explicit validator surface would solve it better than eval/check/hook layers
- whether the change belongs in shared runtime at all
- what nearby negative cases must remain terminal without continuation

Required evidence types:

- real run artifacts
- current code-path inspection
- explicit user workflows that expect validator-backed generic `run`
- targeted regressions derived from reproduced failures

## Non-Goals

This investigation does not do any of the following:

- add new runtime continuation branches
- add implicit prompt-derived task validators
- change `AgentExitReason::Ok` semantics yet
- broaden the shared runtime loop

## Exit Criteria For The Investigation

This investigation is complete when it produces a short answer to:

1. Should generic `run` remain task-agnostic by default?
2. If validators are needed, where should they live:
   - shared runtime
   - explicit run option
   - hook/check surface
   - eval/task harness only
3. What artifact-backed evidence would justify any future PR3 behavior change?

## Current Recommendation

Do not open a behavior-change PR from this note alone.

Treat generic `run` validator semantics as a separate, evidence-gathering question. If future work happens, prefer an explicit validator surface over heuristic prompt interpretation.
