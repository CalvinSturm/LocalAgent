# Runtime Semantics Investigation Notes (2026-03)

Status: PR 1 investigation note, reconciled after PR 2 contract-test alignment  
Scope: Shared runtime semantics only  
Primary policy source: `docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md`

Evidence:
- `docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md`
- `docs/architecture/RUNTIME_ARCHITECTURE.md`
- `src/agent.rs#run`
- `src/agent/runtime_completion.rs`
- `src/agent/run_finalize.rs`
- `src/agent/run_setup.rs`
- `src/agent_impl_guard.rs`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/10a2c817-cd55-425f-949b-112d566d9363.json`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/c73e1f09-4fd8-4b08-9a52-1d70b3917af6.json`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/f0233fd2-bf88-4aa0-9f7e-3d2422486f3d.json`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/aabc6b83-595c-4036-b441-6499cf0b6cc1.json`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/db767054-b223-461b-975a-d870c85f4e62.json`
- `.tmp/eval-glm-4_6v-flash-coding-20260307-195639.json`

## Purpose

This note documents the investigated runtime semantics, the pre-fix mismatches found from artifacts, and the resulting post-PR-2 aligned behavior.

It is evidence-first. It distinguishes:

- current code-path behavior
- repo policy expectations
- model-caused failures
- runtime-caused or mixed failures

## Investigated Artifacts

Primary artifacts:

- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/10a2c817-cd55-425f-949b-112d566d9363.json`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/c73e1f09-4fd8-4b08-9a52-1d70b3917af6.json`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/f0233fd2-bf88-4aa0-9f7e-3d2422486f3d.json`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/aabc6b83-595c-4036-b441-6499cf0b6cc1.json`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/db767054-b223-461b-975a-d870c85f4e62.json`
- `.tmp/eval-glm-4_6v-flash-coding-20260307-195639.json`

Primary code paths:

- `src/agent.rs`
- `src/agent/runtime_completion.rs`
- `src/agent/run_finalize.rs`
- `src/agent/run_setup.rs`
- `src/agent/gate_paths.rs`
- `src/agent_impl_guard.rs`

## Current Implementation Shape

### 1. Runtime completion has four top-level outcomes

`src/agent/runtime_completion.rs` currently models completion as:

- `ExecuteTools`
- `Continue`
- `FinalizeOk`
- `FinalizeError`

This is explicit, and the current `Continue` branches in `src/agent/runtime_completion.rs` are machine-classified with explicit `reason_code`s. The main remaining semantic split is that verified-write handling is modeled separately via `VerifiedWriteResult`.

- planner/runtime corrective continuation
- terminal planner error
- terminal ok
- verified-write finalize-or-retry

### 2. Verified successful write is terminal by default in current code

Current code path:

- `src/agent.rs`
- `src/agent/runtime_completion.rs`
- `src/agent/run_finalize.rs`

Observed behavior:

- after a successful write tool, `Agent::run` calls `finalize_verified_write_step_or_error`
- if post-write verification succeeds and no guard violation is found, `finalize_verified_write_completion` now returns `VerifiedWriteResult::Done`
- `finalize_verified_write_completion` finalizes `ok` from the last non-empty assistant content instead of requesting another model turn

This now matches the repo default in `docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md`:

> A successful verified write is terminal by default.

Regression evidence:

- `src/agent/run_finalize.rs`
- `src/agent_tests.rs`: `runtime_post_write_verification_allows_finalize_without_model_read_back`

### 3. Protocol-artifact filtering now covers the observed box-wrapper shape

Current code path:

- `src/agent/run_setup.rs`
- `src/agent_tool_exec.rs`

Current filter catches:

- wrapper markers detected by `contains_tool_wrapper_markers`
- `[TOOL_RESULT]`
- `[END_TOOL_RESULT]`
- `<|begin_of_box|>`
- `<|end_of_box|>`

It still does not claim to catch every possible malformed wrapper form, but it now covers the concrete artifact shape observed in the manual GLM runs.

Regression evidence:

- `src/agent_tool_exec.rs`
- `src/agent_tests.rs`: `wrapper_marker_detection_works`
- `src/agent_tests.rs`: `echoed_tool_result_wrapper_is_blocked_before_finalization`
- `src/agent_tests.rs`: `echoed_box_wrapper_is_blocked_before_finalization`

### 4. Generic run mode has no task-specific validator

The shared runtime can finalize `ok` without any repo/task-specific acceptance check. Exact-output and test-running requirements in a plain `run` invocation are enforced only by the model unless another validator is explicitly present.

That is expected for generic run mode, but it means artifact review must separate:

- runtime contract behavior
- missing task-specific validation
- plain model non-compliance

### 5. Effective-write enforcement remains heuristic, but now covers `fix`

Current code path:

- `src/agent_impl_guard.rs`

`prompt_requires_effective_write` currently keys off prompt substrings such as:

- `apply_patch`
- `write_file`
- `edit`
- `fix`
- `modify`
- `update`
- `change`

This still remains heuristic overall, but the specific `fix` wording gap seen in artifact review is now closed.

Regression evidence:

- `src/agent_impl_guard.rs`
- `src/agent_tests.rs`: `prompt_requires_effective_write_for_fix_prompt`

## Continuation Authorization Model

PR 2 should not encode continuation as a bare yes or no. Each continued turn needs two explicit answers:

- who authorized the additional turn
- what machine-readable reason, if any, exists at the runtime boundary

Current investigated authority classes are:

- runtime guard or finalize path
- validator or acceptance check
- declared next phase
- user-directed follow-on step

Current observed machine-readable reasons from code:

- `pending_plan_step`
- `tool_only_requires_tool_call`
- `implementation_requires_tool_calls`
- `assistant_protocol_artifact_echo`
- `implementation_requires_effective_write`
- `post_write_guard_retry`

Conclusion for PR 2 scope:

- the `runtime_completion.rs` `Continue` paths are explicitly classified and emit machine-readable audit signals
- no additional PR 2b was required for continuation reason uniformity in these branches

## State-Transition Table

| Event or condition | Runtime classification | Continuation authority | Machine-readable reason | Allowed continuation | Terminal outcome | Governing policy source | Source artifact / code path |
|---|---|---|---|---:|---|---|---|
| No tool calls under implementation guard, first blocked completion | Guard-driven corrective continuation | Runtime guard | `implementation_requires_tool_calls` | Yes | No | `AGENT_RUNTIME_PRINCIPLES_2026.md`: continuation requires classified reason | `src/agent/runtime_completion.rs`: `observed_tool_calls_len == 0` returns `Continue` with `reason_code`; emitted on `Error` and `StepBlocked`; seen in eval run `8b3041ab-a9a2-42da-8592-470a6c737e83` before terminal planner error |
| No tool calls under implementation guard after repeated blocked completion | Guard-driven terminal failure | Runtime guard | finalize error path, no retry authority remains | No | `PlannerError` | same | `src/agent/runtime_completion.rs`: repeated blocked completion returns `FinalizeError` |
| Assistant final content contains protocol artifacts such as `[TOOL_RESULT]` or `<|begin_of_box|>` | Protocol-artifact corrective continuation | Runtime guard | `assistant_protocol_artifact_echo` | Yes | No | policy requires protocol artifacts not become final output | `src/agent/runtime_completion.rs`, `src/agent/run_setup.rs`, `src/agent_tool_exec.rs`; covered by wrapper rejection tests |
| Successful write tool followed by successful post-write verification and no guard violation | Verified-write terminal success | Runtime finalize path | None; no continuation is authorized | No | `Ok` | repo default says verified successful write should be terminal by default | `src/agent/run_finalize.rs`: `finalize_verified_write_completion` returns `Done`; covered by `runtime_post_write_verification_allows_finalize_without_model_read_back` |
| Post-write verification failure for prior-read or missing-read verification, first retry | Classified guard retry | Runtime guard | `post_write_guard_retry` | Yes, bounded to one retry | No | policy allows classified bounded recovery only | `src/agent/runtime_completion.rs`: `post_write_guard_retry`; `src/agent/run_finalize.rs`: retry on `requires prior read_file` or `post-write verification missing` |
| Post-write verification failure after retry budget exhausted | Guard-driven terminal failure | Runtime guard | retry budget exhausted; no continuation authority remains | No | `PlannerError` | bounded retry max | `src/agent/runtime_completion.rs`, `src/agent/run_finalize.rs` |
| Write tool succeeded but `changed:false` or no effective write detected, first bounded recovery | Guard-driven corrective continuation | Runtime guard | `implementation_requires_effective_write` | Yes, bounded | No | ineffective write is not terminal success | `src/agent/runtime_completion.rs`; covered by `runtime_read_then_done_recovers_with_corrective_write_instruction` |
| Write tool succeeded but `changed:false` or no effective write detected after recovery budget exhausted | Guard-driven terminal failure | Runtime guard | effective-write verification failure | No | `PlannerError` | ineffective write is not terminal success | `src/agent_impl_guard.rs`, `src/agent_tests.rs` noop `apply_patch changed:false`; seen in run `db767054-b223-461b-975a-d870c85f4e62` |
| Finalize path with no explicit validator and no caught protocol artifact | Terminal success | None | None | No | `Ok` | finalization is contract boundary | `src/agent/runtime_completion.rs`: `FinalizeOk -> finalize_ok_with_end`; seen in `10a2c817-cd55-425f-949b-112d566d9363`, `c73e1f09-4fd8-4b08-9a52-1d70b3917af6`, `f0233fd2-bf88-4aa0-9f7e-3d2422486f3d` |
| Max steps reached before valid final answer | Terminal failure by budget/loop bound | Runtime loop budget | max-step boundary | No | `MaxSteps` | bounded runtime state machine | `src/agent.rs` -> `finalize_max_steps_with_end`; seen in earlier manual exact C4 run `849b3753-c999-424d-88a3-839f9aaf5084` |
| Gate deny or approval-required decision | Permission-driven terminal result | Trust gate | gate decision | No | `Denied` or `ApprovalRequired` | permissions remain runtime policy | `src/agent/gate_paths.rs` |

## Artifact Classification Per Run ID

| Run ID | Observed outcome | Classification | Justification |
|---|---|---|---|
| `10a2c817-cd55-425f-949b-112d566d9363` | `exit_reason=ok`, final output exactly `done: src/hello.txt`, file content is literal `hello\n` bytes | model-caused | Runtime did what generic run mode allows: one successful write, no explicit validator, final user-facing answer accepted. The content failure is the model writing backslash-n instead of newline. |
| `c73e1f09-4fd8-4b08-9a52-1d70b3917af6` | `exit_reason=ok`, file edited correctly, final output is `<|begin_of_box|>patched answer()<|end_of_box|>` | mixed | Model added wrapper tokens, but runtime accepted them as final output because current protocol-artifact filter is narrow and does not appear to catch this shape. |
| `f0233fd2-bf88-4aa0-9f7e-3d2422486f3d` | `exit_reason=ok`, no effective fix, no `cargo test`, malformed wrapper-like text became final output | mixed | Model did not complete the task, but runtime also accepted a non-user-facing final shape and did not require effective write here because the heuristic in `prompt_requires_effective_write` does not key off `fix`. |
| `aabc6b83-595c-4036-b441-6499cf0b6cc1` | `exit_reason=planner_error`, message says patching `src/messages.rs`, no effective change landed | model-caused | Runtime guard behaved as designed: ineffective write path finalized as planner error instead of allowing false success. |
| `db767054-b223-461b-975a-d870c85f4e62` | `exit_reason=planner_error`, repeated `apply_patch`, no effective write landed, final prose only | model-caused | Runtime guard again behaved as designed: ineffective write path produced planner error. |

## Supporting Eval-Runner Cross-Check

The built-in eval result file `.tmp/eval-glm-4_6v-flash-coding-20260307-195639.json` shows the same broad patterns:

- `C1`: `exit_reason=ok` but eval assertion fails on newline content
- `C2`: `planner_error` with zero tool calls
- `C3`: `planner_error` with tool use plus verifier failure
- `C4`: `exit_reason=ok` but eval assertion fails on resulting file content
- `C5`: `planner_error` with no effective repair

This corroborates that the manual exact run is not a one-off.

## Current Semantics vs Repo Policy

### Confirmed alignment

- guard failures can authorize bounded corrective retry
- ineffective writes are not treated as successful completion
- planner errors preserve assistant content in some finalize paths
- verified successful write is terminal by default
- observed box-wrapper protocol artifacts are rejected before finalization
- `fix` prompt wording now participates in effective-write enforcement
- touched continuation paths emit machine-readable reasons

### Remaining mismatch or ambiguity

1. Generic `run` mode has no explicit validator for task-level acceptance, so runtime `ok` does not imply task correctness.
2. Effective-write enforcement still depends on prompt wording heuristics and can miss future semantic variants beyond the now-covered `fix` case.
3. Protocol-artifact rejection is improved for the reproduced artifact shapes, but not yet a proof of complete wrapper-form coverage.

## Questions PR 2 Must Encode As Contract Tests

Resolved in PR 2:

1. Verified successful write remains terminal by default.
2. The reproduced final-output wrapper shapes from the manual GLM runs are rejected at the runtime boundary.
3. The touched corrective retry cases emit machine-readable reason codes and have contract tests.
4. The reproduced `fix` wording gap now requires effective write.

Open beyond PR 2:

1. Should generic `run` mode ever gain an explicit validator surface for task-style acceptance?
2. Should effective-write heuristics be replaced or supplemented by a less prompt-wording-dependent contract?
3. Should protocol-artifact detection broaden further beyond the reproduced shapes now covered?

## Minimal Conclusions

- The recent GLM artifacts were useful because they exposed both model failures and runtime-boundary weaknesses.
- The investigated/runtime-touched continuation paths are now explicitly classified enough to treat PR 2 as complete.
- No additional PR 2b was required for `runtime_completion.rs` continuation reason uniformity.
