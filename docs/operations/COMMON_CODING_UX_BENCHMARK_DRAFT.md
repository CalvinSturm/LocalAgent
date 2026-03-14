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
- [x] the benchmark now spans:
  - read-only analysis: `U1`, `U2`
  - single-file fix: `U3`, `U4`
  - edit plus validation: `U5`, `U12`
  - recovery: `U6`
  - multi-file coordination: `U7`
  - test work: `U9`
- [x] per-run UX fields exist under `runs[*].ux`
- [x] flattened per-run UX metric rows exist under `runs[*].ux_metric_rows`
- [x] aggregate UX rows exist at:
  - `ux_summary_metric_rows`
  - `ux_summary_metric_rows_by_model`
  - `ux_summary_metric_rows_by_task_family`
- [x] RunScope ingest path has been proven against real LocalAgent eval artifacts

Still open before PR1 closeout:
- [x] decide whether the current pack is broad enough to act as the active benchmark decision surface
- [x] document the first frozen baseline result in a stable location
- [x] document benchmark caveats that materially affect interpretation

## Historical Frozen Baseline Artifact

First pinned baseline artifact:
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
- this is the first pinned PR1 baseline artifact for the earlier four-task landing slice
- it is useful as a frozen comparison point even though the performance is poor
- it should be treated as a historical comparison point, not as the current broad-pack readout
- later PR1 or PR2 comparisons should reference this artifact explicitly rather than relying on ad hoc recollection

## Current Caveats

- `U5` and `U6` are currently high-signal but also highly sensitive to model-side protocol discipline during the validation-only shell phase
- `common_coding_ux` now accepts `edit` as a valid existing-file edit path alongside `apply_patch` and `str_replace`; earlier task assertions were too narrow for the repo's own preferred edit workflow
- current omnicoder instruction-profile tuning should be interpreted as PR1 benchmark evidence work, not as the formal start of PR3
- current `validation_passed` UX reporting is verifier-oriented; it does not always distinguish "the verifier command would pass" from "the model itself correctly emitted the required validation tool call"
- taskfile-authored contract metadata is now landed for `task_kind`, `validation_command`, and `exact_final_answer`, and has been exercised in real `D3` and `D5` coding-task workflows; it is still not a first-class benchmark dimension inside the core `common_coding_ux` pack
- the broad-pack qwen baseline is materially unstable across reruns; an immediate compare pass after the frozen `2026-03-13` baseline regressed from `1/9` to `0/9`, so future comparisons should rely on named artifacts and task-level deltas rather than treating one rerun as definitive in isolation

## Task Families

### 1. Read-Only Code Investigation

Goal:
- measure whether the agent can inspect the repo and answer accurately without making edits

Candidate tasks:
- [x] `U1` repo summary with file-grounded answer
  - prompt shape: identify the main entrypoint and summarize the runtime flow
  - success: cites the correct files/symbols and does not use write tools
  - UX focus: file targeting, evidence use, concise code-grounded answer
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
- [x] `U7` small feature addition touching implementation and test
  - prompt shape: add a narrow helper in `src/lib.rs` and update `tests/regression.rs` to cover the new behavior
  - success: both files change coherently, `cargo test` passes, exact closeout is satisfied
  - UX focus: multi-file coordination, minimal necessary surface
- [ ] `U8` workspace-local refactor without behavior change
  - prompt shape: rename or extract a small helper across files
  - success: all references updated, verifier passes
  - UX focus: consistency across files, no collateral damage

### 5. Test Repair / Test Addition

Goal:
- measure whether the agent can work directly in the test surface instead of only application code

Candidate tasks:
- [x] `U9` repair a broken existing unit test
  - prompt shape: repair a broken existing unit test so it matches the current implementation
  - success: `tests/regression.rs` is corrected, `cargo test` passes, exact closeout is satisfied
  - UX focus: test-surface reliability, editing the right layer instead of application code
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

Historical frozen baseline:
- [x] `qwen2.5-coder-7b-instruct@q8_0`
  - the first frozen artifact above is already recorded
  - it represents the earlier four-task landing slice, not the current broader pack

Broader-pack frozen baseline:
- [x] capture one explicit frozen artifact for the current broad `common_coding_ux` decision surface
  - model: `qwen2.5-coder-7b-instruct@q8_0`
  - date captured: `2026-03-13`
  - baseline name: `broad_common_coding_ux_qwen2_5_coder_7b_instruct_q8_0_2026_03_13`
  - local artifact:
    - [run.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.artifacts/eval/common_coding_ux/qwen2_5_coder_7b_instruct_q8_0-2026-03-13/run.json)
    - [SUMMARY.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/.artifacts/eval/common_coding_ux/qwen2_5_coder_7b_instruct_q8_0-2026-03-13/SUMMARY.md)
    - [junit.xml](/C:/Users/Calvin/Software%20Projects/LocalAgent/.artifacts/eval/common_coding_ux/qwen2_5_coder_7b_instruct_q8_0-2026-03-13/junit.xml)
  - baseline record:
    - [broad_common_coding_ux_qwen2_5_coder_7b_instruct_q8_0_2026_03_13.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.localagent/eval/baselines/broad_common_coding_ux_qwen2_5_coder_7b_instruct_q8_0_2026_03_13.json)
  - readout:
    - total runs: `9`
    - passed: `1`
    - failed: `8`
    - skipped: `0`
    - pass rate: `11.11%`
    - passing task: `U9`
    - failing tasks: `U1`, `U2`, `U3`, `U4`, `U5`, `U6`, `U7`, `U12`
  - interpretation:
    - this is now the frozen PR1 baseline for the current broad decision surface
    - the current qwen baseline remains weak overall, but it is explicit and stable enough to compare future changes against
    - future benchmark claims should compare against this named baseline rather than against isolated `D5` or `U9` anecdotes

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

- [x] `common_coding_ux` lives as its own `EvalPack`
- [ ] decide whether fixtures should extend `src/eval/fixtures_repo.rs` or move into a dedicated `tests/fixtures/common_coding_ux/` tree
- current state: the first landing slice extends `src/eval/fixtures_repo.rs`
- [x] raw per-run UX metrics use nested `ux`, flattened `ux_metric_rows`, and aggregate summary metric rows
- [x] `U12` is implemented as the first authored closeout-quality task

## Immediate Next Step

Recommended next action:
- treat the current `common_coding_ux` pack as the active benchmark decision surface for future improvement work

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

## PR4b Follow-on Workflow: D5 Recovery Fixture

Completed second real PR4b workflow:
- prompt-contract taskfile: `manual-testing/taskfiles/pr4b_d5_prompt_contract.json`
- authored-contract taskfile: `manual-testing/taskfiles/pr4b_d5_authored_contract.json`
- fixture: `manual-testing/D-tests/D5`

Observed result:
- `qwen2.5-coder-7b-instruct@q8_0`
  - prompt-contract baseline: reached edit plus validation, then failed on exact final-answer compliance
  - authored-contract variant: regressed to a generic prose response after a single repo-inspection step and no effective write
  - read: explicit authored contracts did not help on this harder recovery task and may have worsened task execution for qwen
- `omnicoder-9b@q8_0`
  - prompt-contract baseline: reached repo inspection and validation, then still failed on effective write
  - authored-contract variant: performed more repo inspection steps, but still failed on effective write
  - read: explicit authored contracts increased activity but did not improve the actual repair outcome

Current PR4b read after `D3` + `D5`:
- authored contracts are a real improvement surface, not just schema wiring
- the benefit is not broad or automatic across harder recovery/edit tasks
- authored contracts alone are unlikely to be the next dominant reliability lever for LocalAgent on more difficult coding workflows

Decision:
- keep the landed PR4b authored-contract surface
- make `PR4a` the next formal workstream
- use `D5`-style harder recovery/edit tasks as the primary validation surface for whether bounded structural grounding improves coding-task reliability where authored contracts did not

## PR4a Status Note

Completed PR4a grounding refinements:
- bounded likely-target grounding now prefers the active task workdir when candidates are available
- stale transient `.tmp/...` likely-target candidates are filtered out
- code surfaces are prioritized over generic docs/config matches for coding-task likely-target selection

Observed result from clean `D5` reruns:
- `repo_map_likely_target_files_count` now falls back to `0` instead of surfacing misleading `.tmp/...` or `.github/...` targets
- `qwen2.5-coder-7b-instruct@q8_0`
  - concurrent clean reruns still showed provider/qualification instability
  - a sequential rerun passed qualification and then failed later on `implementation guard: file-edit task finalized without an effective write`
  - read: qualification fallback is not the primary LocalAgent-side blocker for qwen on this path
- `omnicoder-9b@q8_0`
  - sequential prompt and authored reruns wrote a positive `orchestrator_qualification_cache.json`
  - both then failed before tool use with `HTTP 400: {"error":"Context size has been exceeded."}`
  - read: qualification also succeeded for omnicoder; the next blocker is LM Studio context-budget pressure on the task-graph path, not qualification fallback

Current PR4a read:
- PR4a grounding is now in a good enough state to keep
- bad likely-target injection is no longer the blocker on `D5`
- the qualification investigation does not justify a LocalAgent-side gating change:
  - qwen can pass qualification and then exposes a model-side ineffective-write failure
  - omnicoder can pass qualification but overruns LM Studio context before tool use
- the task-graph context-budget reduction is now also validated:
  - coding-node repo-map injection was capped more aggressively
  - `omnicoder-9b@q8_0` moved from pre-tool provider overflow to a real edit trace and now fails on validation-phase protocol discipline
  - `qwen3.5-9b-uncensored-hauhaucs-aggressive` moved past provider overflow but still fell back to read-only and then failed on write denial
- the next narrow blocker on this workflow is no longer prompt-size overflow; it is model/runtime behavior after the prompt fits

Decision:
- keep the current PR4a grounding slices
- close the qualification-stability investigation without a LocalAgent runtime change
- close the context-budget investigation as a successful PR4a refinement
- close the `omnicoder-9b@q8_0` validation-phase discipline branch on `D5`:
  - a narrow local follow-up profile was tested after the budget-cap and grounding refinements landed
  - it did not move the failure boundary
  - both baseline and shaped runs still failed with the same validation-phase protocol violation after a real write
- stop stacking more omnicoder profile variants on this branch
- close the narrow `D5` authored-contract audit without a code change:
  - the taskfile contract is already minimal and explicit
  - `task_kind = coding`, `validation_command = cargo test`, and `exact_final_answer = verified fix` are not the repeated blocker on this path
  - no additional taskfile-contract shaping is justified from this branch
- do not treat these clean reruns as evidence for planner/routing work yet
- move off the `D5` tuning branch and use the next benchmark expansion item as the next roadmap step

## U9 Result Note

Observed result from the first narrow `U9` comparison run:
- `omnicoder-9b@q8_0`
  - repaired `tests/regression.rs` correctly by changing the expected value from `6` to `5`
  - then failed at the same validation-only boundary seen on harder `D5` runs:
    - `required validation phase requires exactly one shell tool call and no prose`
  - read: `U9` confirms that omnicoder can target and repair the test surface, but the repeated blocker remains post-write validation-phase discipline
- `qwen2.5-coder-7b-instruct@q8_0`
  - did not produce a valid comparison run because LM Studio failed to load the model on the corrected rerun
  - read: `U9` is not yet a fair cross-model comparison surface while qwen remains provider-unstable on this branch

Decision:
- keep `U9` as a useful benchmark task
- do not treat the `U9` result as a reason to resume prompt tuning
- use `U7` as the next expansion task so the benchmark keeps broadening beyond `D5` and validation-only failures

## Clean U7/U9 Rerun Note

Observed result from the clean rerun set under fresh state dirs:
- `qwen2.5-coder-7b-instruct@q8_0`
  - `U9`: passed end to end
    - repaired `tests/regression.rs`
    - ran validation successfully
    - returned the exact final answer `validated: tests/regression.rs`
  - `U7`: failed on tool-step discipline
    - `multiple tool calls in a single assistant step (max 1, got 3)`
  - read: qwen can now succeed on test-surface work, but multi-file coordination still exposes step-discipline weakness
- `omnicoder-9b@q8_0`
  - `U7`: produced a useful partial-success signal
    - added `is_zero_or_even` in `src/lib.rs`
    - did not update `tests/regression.rs`
    - then failed at the familiar post-write boundary:
      - `required validation phase requires exactly one shell tool call and no prose`
  - `U9`: failed even earlier on step-discipline
    - `multiple tool calls in a single assistant step (max 1, got 2)`
  - read: omnicoder can reach meaningful multi-file edits, but its repeated blocker is still post-write protocol discipline

Decision:
- the current `common_coding_ux` pack is now broad enough to act as the active benchmark decision surface
- the benchmark now covers:
  - read-only investigation
  - single-file fixes
  - validation-required fixes
  - recovery
  - closeout quality
  - test-surface work
  - multi-file coordination
- future improvement work should be judged against this broader pack rather than against `D5`-only tuning loops

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

## Historical Experiment Plan: Qwen Write Reliability

This section is retained as historical context from the earlier four-task and closeout-shaping loop.
The current benchmark decision surface is the broader pack described in the clean `U7/U9` rerun note above.

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
