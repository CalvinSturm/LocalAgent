# Common Coding UX Benchmark Draft

Status: draft for PR1  
Owner: LocalAgent maintainers  
Scope: common coding UX benchmark on the eval path

## Purpose

This document turns PR1 from [LOCALAGENT_CODING_UX_PR_STACK.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/docs/operations/LOCALAGENT_CODING_UX_PR_STACK.md) into a concrete task list.

Use it to:
- define the first `common_coding_ux` benchmark pack
- track which task families are still draft vs implemented
- keep PR1 focused on realistic coding-task outcomes instead of speculative runtime changes

## PR1 Success Criteria

PR1 is complete when:
- a `common_coding_ux` eval pack exists on the eval path
- the pack covers the main user-facing coding task families
- each task has explicit pass criteria and UX notes
- one frozen baseline result is captured for the current recommended local baseline model
- protocol-only regressions remain covered separately by the existing narrow runtime/tool tests

## Task Families

### 1. Read-Only Code Investigation

Goal:
- measure whether the agent can inspect the repo and answer accurately without making edits

Candidate tasks:
- [ ] `U1` repo summary with file-grounded answer
  - prompt shape: identify the main entrypoint and summarize the runtime flow
  - success: cites the correct files/symbols and does not use write tools
  - UX focus: file targeting, evidence use, concise code-grounded answer
- [ ] `U2` bug-location analysis without editing
  - prompt shape: inspect failing area and identify the likely bug location only
  - success: points to the correct file/function and avoids speculative edits
  - UX focus: investigation quality, read-only discipline

### 2. Single-File Bug Fix

Goal:
- measure whether the agent can make a small, correct edit in the right file

Candidate tasks:
- [ ] `U3` straightforward single-file logic fix
  - prompt shape: fix a small bug in one file and return a simple exact answer
  - success: correct edit, no unnecessary file churn
  - UX focus: fast correct targeting, minimal edit path
- [ ] `U4` inspect-before-edit typo/string fix
  - prompt shape: locate the source of a visible defect before editing
  - success: read-before-write, correct file only, exact closeout
  - UX focus: disciplined inspection, avoiding blind edits

### 3. Edit Plus Validation

Goal:
- measure whether the agent can complete a code change and then perform the required verification cleanly

Candidate tasks:
- [ ] `U5` parser fix plus required test command
  - prompt shape: fix bug, run verifier, produce exact success string only if verification passes
  - success: correct edit, correct validation command, proper closeout
  - UX focus: verification discipline after a successful edit
- [ ] `U6` nested-file recovery bug fix plus required test command
  - prompt shape: recover from wrong-path guesses, find the real file, fix, validate
  - success: reaches semantic fix boundary and completes required validation
  - UX focus: recovery behavior, validation-only follow-on discipline

### 4. Small Multi-File Change

Goal:
- measure whether the agent can coordinate a small feature or refactor across more than one file

Candidate tasks:
- [ ] `U7` small feature addition touching implementation and test
  - prompt shape: add a narrow behavior change and update/add one test
  - success: both files changed coherently and tests pass
  - UX focus: multi-file coordination, minimal necessary surface
- [ ] `U8` workspace-local refactor without behavior change
  - prompt shape: rename or extract a small helper across files
  - success: all references updated, verifier passes
  - UX focus: consistency across files, no collateral damage

### 5. Test Repair / Test Addition

Goal:
- measure whether the agent can work directly in the test surface instead of only application code

Candidate tasks:
- [ ] `U9` repair a broken existing unit test
  - prompt shape: make the smallest test-side change needed after reading the failing expectation
  - success: test file is correctly edited and validation passes
  - UX focus: reading failures, editing the right layer
- [ ] `U10` add a missing regression test for an already-fixed bug
  - prompt shape: inspect implementation and add one targeted regression test
  - success: new test meaningfully covers the bug and passes
  - UX focus: writing useful tests, not just code edits

### 6. Recovery and Closeout Quality

Goal:
- measure behavior that users notice even when the edit itself is mostly correct

Candidate tasks:
- [ ] `U11` wrong-file first guess but eventual recovery
  - prompt shape: task layout encourages one plausible wrong turn
  - success: agent recovers and finishes instead of looping or bailing
  - UX focus: resilience after an early mistake
- [ ] `U12` explicit closeout-quality task
  - prompt shape: after a successful edit/verification, summarize changed files, validation result, and remaining risk
  - success: final answer contains the requested closeout details
  - UX focus: user-facing completion quality without forcing a repo-wide runtime rule

## Suggested First Landing Slice

Do not try to land all twelve tasks at once.

Recommended first implementation set:
- [ ] `U1` repo summary with file-grounded answer
- [ ] `U3` straightforward single-file logic fix
- [ ] `U5` parser fix plus required test command
- [ ] `U6` nested-file recovery bug fix plus required test command

Why this slice:
- covers read-only analysis, simple editing, validation-required editing, and recovery behavior
- overlaps the current strongest known model-separation signals
- is enough to produce the first frozen baseline without overbuilding PR1

## Baseline and Comparison Models

Initial frozen baseline:
- [ ] baseline model: `qwen2.5-coder-7b-instruct@q8_0`

Primary comparison models for early PR1 readouts:
- [ ] `omnicoder-9b@q8_0`
- [ ] `qwen/qwen3.5-9b` when using the previously stronger effective load/result path

Notes:
- prefer non-stream mode for contract-complete multi-step tasks unless a task explicitly needs a stream comparison
- preserve current narrow runtime/protocol tasks separately; do not fold them into this benchmark as the main measurement surface

## Metrics To Record

Per run:
- [ ] task pass/fail
- [ ] correct file targeting
- [ ] unnecessary file edits
- [ ] validation command attempted
- [ ] validation command satisfied
- [ ] closeout quality satisfied when required by the task
- [ ] recovery after wrong-path or wrong-tool first attempt
- [ ] tool churn / repeated failed edit attempts

Do not add weighted composite scoring in PR1.

## Open Design Notes

- [ ] decide whether the new pack should live as a new `EvalPack` variant or as additional coding tasks behind a narrower selector
- [ ] decide whether fixtures should extend `src/eval/fixtures_repo.rs` or move into a dedicated `tests/fixtures/common_coding_ux/` tree
- [ ] decide the minimum artifact/report extension needed for raw per-run UX metrics
- [ ] decide whether `U12` belongs in the first landing slice or should wait until PR3

## Immediate Next Step

Recommended next action:
- implement the first landing slice (`U1`, `U3`, `U5`, `U6`) and keep the remaining tasks as draft backlog for later expansion
