# Common Coding UX Benchmark Draft

Status: active PR1 benchmark note  
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
- the first landing slice is stable enough to act as a decision surface for later PRs
- each landed task has explicit pass criteria and UX notes
- at least one frozen baseline result is captured for the current recommended local baseline model
- protocol-only regressions remain covered separately by the existing narrow runtime/tool tests

## Current PR1 State

Implemented now:
- [x] `common_coding_ux` exists as an `EvalPack` on the eval path
- [x] first landing slice tasks are implemented: `U1`, `U3`, `U5`, `U6`
- [x] per-run UX fields exist under `runs[*].ux`
- [x] flattened per-run UX metric rows exist under `runs[*].ux_metric_rows`
- [x] aggregate UX rows exist at:
  - `ux_summary_metric_rows`
  - `ux_summary_metric_rows_by_model`
  - `ux_summary_metric_rows_by_task_family`
- [x] RunScope ingest path has been proven against real LocalAgent eval artifacts

Still open before PR1 closeout:
- [ ] decide whether the current four-task slice is stable enough to freeze as the initial PR1 benchmark
- [x] document the first frozen baseline result in a stable location
- [ ] document benchmark caveats that materially affect interpretation

## Pinned Initial PR1 Baseline Artifact

Pinned baseline candidate:
- model: `qwen2.5-coder-7b-instruct@q8_0`
- date captured: `2026-03-13`
- local eval artifact:
  - [run.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/runscope/common-coding-ux/qwen2.5-coder-7b-instruct-q8_0/run.json)
  - [SUMMARY.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/runscope/common-coding-ux/qwen2.5-coder-7b-instruct-q8_0/SUMMARY.md)
- RunScope ingest artifact:
  - run id: `01KKMATEMP1DF1737H5DDFMRXF`
  - [run.json](/C:/Users/Calvin/AppData/Local/RunScope/artifacts/localagent/2026/03/01KKMATEMP1DF1737H5DDFMRXF/run.json)

Pinned baseline readout:
- total runs: `4`
- passed: `0`
- failed: `4`
- skipped: `0`
- pass rate: `0.00%`

Interpretation:
- this is the first pinned PR1 baseline artifact for the current four-task landing slice
- it is useful as a frozen comparison point even though the performance is poor
- later PR1 or PR2 comparisons should reference this artifact explicitly rather than relying on ad hoc recollection

## Current Caveats

- `U5` and `U6` are currently high-signal but also highly sensitive to model-side protocol discipline during the validation-only shell phase
- `common_coding_ux` now accepts `edit` as a valid existing-file edit path alongside `apply_patch` and `str_replace`; earlier task assertions were too narrow for the repo's own preferred edit workflow
- current omnicoder instruction-profile tuning should be interpreted as PR1 benchmark evidence work, not as the formal start of PR3
- current `validation_passed` UX reporting is verifier-oriented; it does not always distinguish "the verifier command would pass" from "the model itself correctly emitted the required validation tool call"
- taskfile-authored contract metadata is now landed for `task_kind`, `validation_command`, and `exact_final_answer`, but it has only been verified through targeted task-graph/runtime tests so far, not yet through a benchmarked coding-task comparison

## Task Families

### 1. Read-Only Code Investigation

Goal:
- measure whether the agent can inspect the repo and answer accurately without making edits

Candidate tasks:
- [x] `U1` repo summary with file-grounded answer
  - prompt shape: identify the main entrypoint and summarize the runtime flow
  - success: cites the correct files/symbols and does not use write tools
  - UX focus: file targeting, evidence use, concise code-grounded answer
- [ ] `U2` bug-location analysis without editing
- [x] `U2` bug-location analysis without editing
  - prompt shape: inspect failing area and identify the likely bug location only
  - success: points to the correct file/function and avoids speculative edits
  - UX focus: investigation quality, read-only discipline

### 2. Single-File Bug Fix

Goal:
- measure whether the agent can make a small, correct edit in the right file

Candidate tasks:
- [x] `U3` straightforward single-file logic fix
  - prompt shape: fix a small bug in one file and return a simple exact answer
  - success: correct edit, no unnecessary file churn
  - UX focus: fast correct targeting, minimal edit path
- [ ] `U4` inspect-before-edit typo/string fix
- [x] `U4` inspect-before-edit typo/string fix
  - prompt shape: locate the source of a visible defect before editing
  - success: read-before-write, correct file only, exact closeout
  - UX focus: disciplined inspection, avoiding blind edits

### 3. Edit Plus Validation

Goal:
- measure whether the agent can complete a code change and then perform the required verification cleanly

Candidate tasks:
- [x] `U5` parser fix plus required test command
  - prompt shape: fix bug, run verifier, produce exact success string only if verification passes
  - success: correct edit, correct validation command, proper closeout
  - UX focus: verification discipline after a successful edit
- [x] `U6` nested-file recovery bug fix plus required test command
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
- [x] `U12` explicit closeout-quality task
  - prompt shape: after a successful edit/verification, mention the changed file and the validation result using task-authored wording
  - success: final answer contains the requested closeout details
  - UX focus: user-facing completion quality without forcing a repo-wide runtime rule

## Suggested First Landing Slice

Do not try to land all twelve tasks at once.

Recommended first implementation set:
- [x] `U1` repo summary with file-grounded answer
- [x] `U3` straightforward single-file logic fix
- [x] `U5` parser fix plus required test command
- [x] `U6` nested-file recovery bug fix plus required test command

Why this slice:
- covers read-only analysis, simple editing, validation-required editing, and recovery behavior
- overlaps the current strongest known model-separation signals
- is enough to produce the first frozen baseline without overbuilding PR1

## Baseline and Comparison Models

Initial frozen baseline:
- [ ] baseline model: `qwen2.5-coder-7b-instruct@q8_0`
  - current benchmark readout exists, but PR1 closeout should still pin one explicit frozen result path/artifact

Primary comparison models for early PR1 readouts:
- [ ] `omnicoder-9b@q8_0`
- [ ] `qwen/qwen3.5-9b` when using the previously stronger effective load/result path

Notes:
- prefer non-stream mode for contract-complete multi-step tasks unless a task explicitly needs a stream comparison
- preserve current narrow runtime/protocol tasks separately; do not fold them into this benchmark as the main measurement surface

## Metrics To Record

Per run:
- [x] task pass/fail
- [ ] correct file targeting
- [ ] unnecessary file edits
- [x] validation command attempted
- [x] validation command satisfied
- [x] closeout quality satisfied when required by the task
- [x] changed-file closeout satisfied when required by the task
- [x] validation-result closeout satisfied when required by the task
- [ ] recovery after wrong-path or wrong-tool first attempt
- [ ] tool churn / repeated failed edit attempts

Do not add weighted composite scoring in PR1.

## Open Design Notes

- [ ] decide whether the new pack should live as a new `EvalPack` variant or as additional coding tasks behind a narrower selector
- resolved: `common_coding_ux` now exists as its own `EvalPack`
- [ ] decide whether fixtures should extend `src/eval/fixtures_repo.rs` or move into a dedicated `tests/fixtures/common_coding_ux/` tree
- current state: the first landing slice extends `src/eval/fixtures_repo.rs`
- [ ] decide the minimum artifact/report extension needed for raw per-run UX metrics
- resolved for PR1 first slice: nested `ux`, flattened `ux_metric_rows`, and aggregate summary metric rows
- resolved for first PR3 slice: `U12` is implemented as the first authored closeout-quality task

## Immediate Next Step

Recommended next action:
- use the landed PR4b authored-contract surface in one real coding-task run or benchmark case so the benchmark can measure whether explicit taskfile contracts improve reliability beyond prompt inference alone

## PR4b Status Note

Completed PR4b slice:
- taskfiles now support authored `task_kind`, `validation_command`, and `exact_final_answer`
- those fields now flow through the explicit runtime contract path used by `tasks run`
- task-graph artifacts now expose authored node settings for operator inspection

Verification status:
- targeted task-graph/runtime tests now prove that:
  - node-authored contract values beat prompt inference
  - taskfile defaults apply when a node does not override them

Current read:
- PR4b is landed at the schema + apply + verification layer
- the next benchmark-facing step is to exercise authored task contracts in one real coding-task workflow before expanding the authored metadata surface further

Benchmark workflow status:
- completed: `manual-testing/taskfiles/pr4b_d3_prompt_contract.json`
- completed: `manual-testing/taskfiles/pr4b_d3_authored_contract.json`
- fixture: `manual-testing/D-tests/D3`

Observed result from the first real PR4b taskfile workflow:
- `qwen2.5-coder-7b-instruct@q8_0`
  - prompt-contract baseline: failed at the earliest boundary with no tool calls and `implementation guard: file-edit task finalized without any tool calls`
  - authored-contract variant: progressed farther, made real repo reads, and then failed later with `implementation guard: file-edit task finalized without an effective write`
  - read: explicit authored contracts improved task progression but did not yet produce a successful repair
- `omnicoder-9b@q8_0`
  - prompt-contract baseline: denied on an initial `grep` tool call
  - authored-contract variant: denied on the same initial `grep` tool call
  - read: explicit authored contracts did not improve this workflow because the model failed earlier on the same disallowed tool choice

Decision:
- PR4b is now exercised in one real coding-task workflow, not only unit tests
- explicit authored contracts can improve practical task progression for at least one local model even when the task still fails overall
- the next workstream choice should use this result rather than assuming authored metadata is only a paper improvement

## PR3 Result Note

Completed PR3 measurement slice:
- `U12` was added as the first authored closeout-quality task
- closeout-specific UX metrics were added for:
  - changed-file mention when required by the task
  - validation-result mention when required by the task
- `coding_closeout_quality_v1` was tested as the first closeout-focused task profile

Observed result from fresh model comparisons:
- `qwen2.5-coder-7b-instruct@q8_0` baseline vs `coding_closeout_quality_v1`
- `omnicoder-9b@q8_0` baseline vs `coding_closeout_quality_v1`
- `omnicoder-9b` baseline vs `coding_closeout_quality_v1`

Conclusion:
- the profile reduced some tool churn in several runs
- it did not improve `ux.closeout_changed_files_rate`
- it did not improve `ux.closeout_validation_result_rate`
- both qwen and omnicoder still failed before the authored closeout contract was meaningfully reachable

Decision:
- stop PR3 closeout-profile tuning here
- keep `U12` plus the new closeout metrics as benchmark infrastructure
- do not add another `coding_closeout_quality_v*` iteration right now

## Next Experiment Plan: Qwen Write Reliability

Objective:
- improve qwen on the earlier coding-task boundary where it is currently failing before successful validation or authored closeout becomes reachable

Primary target model:
- `qwen2.5-coder-7b-instruct@q8_0`

Scope:
- focus on basic write reliability, not closeout phrasing
- stay on the eval/instruction surface before considering broader planner or runtime changes

Task focus:
- `U3`
- `U5`
- `U6`

Primary questions:
- does qwen reliably produce an effective file change on simple and validation-required edit tasks
- when it fails, is the blocker:
  - wrong file targeting
  - ineffective edit application
  - edit applied but validation never reached
  - validation reached but tool protocol failed

Minimal experiment loop:
1. run baseline qwen on `common-coding-ux`
2. inspect `U3`, `U5`, and `U6` artifacts only
3. classify each failure at the earliest real boundary:
   - targeting
   - write
   - validation
   - tool protocol
4. add one narrow qwen-specific profile aimed at write reliability rather than closeout
5. rerun once and compare:
   - `ux.task_success_rate`
   - `U3`, `U5`, `U6` artifact outcomes
   - step/tool-call churn

Profile guidance for the first qwen experiment:
- prefer inspect -> single minimal edit -> verify
- explicitly discourage repeated edit retries without rereading file state
- prefer one concrete edit path over switching among multiple edit tools
- do not add closeout wording instructions in this loop

Stop conditions:
- if qwen still fails before an effective write after one narrow profile pass, do not keep stacking prompt variants
- if the failures suggest a LocalAgent-side authored-contract or edit-surface issue instead of a model-side issue, document that before considering broader PR3 or PR4 work

## Narrow LocalAgent-Side Investigation Result

Scope reviewed:
- accepted edit paths and task assertions around `U5`, `U6`, and `U12`
- how validation-required tasks are authored
- whether `planner_error` is too coarse to usefully interpret this benchmark

Concrete finding:
- `U5` and `U6` had the same assertion mismatch that `U3` had earlier:
  - real runs were using `edit` as the existing-file write path
  - task assertions only accepted `{apply_patch,str_replace}`
- this amplified some qwen failures as benchmark/task-design noise instead of pure model failure

Action taken:
- `U5` and `U6` now accept `{edit,apply_patch,str_replace}` the same way `U3` and `U12` do

Read after the fix:
- the accepted-edit-path mismatch was a real LocalAgent-side issue and is now fixed
- no second concrete authored-contract issue was found in `U12`; its closeout contract is behaving as intended
- `planner_error` remains coarse, but the stored run artifacts still expose the underlying error string well enough for the current benchmark loop
- remaining qwen failures should now be treated as mostly model-side unless a new LocalAgent-side defect is reproduced

Decision:
- keep the `U3`, `U5`, and `U6` assertion parity fixes
- stop adding more qwen prompt variants for now
- choose the next workstream from here instead of continuing this tuning loop
