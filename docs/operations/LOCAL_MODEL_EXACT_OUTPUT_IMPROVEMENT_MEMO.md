# Local Model Exact-Output Improvement Memo

## Goal

Propose one narrow improvement pass aimed at exact-output and final-answer compliance after successful tool work.

This memo is based on the current findings in:
- [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md)
- [manual-testing/LOCAL_MODEL_COMPATIBILITY_SUMMARY.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/LOCAL_MODEL_COMPATIBILITY_SUMMARY.md)

This is a proposal only. It does not change runtime behavior by itself.

## Problem Statement

Across multiple local models, the current repeated failure is no longer qualification or provider transport.

The repeated pattern is:
- the model reaches the correct tool or write step
- the tool succeeds
- LocalAgent correctly knows the prompt still requires a final answer
- the model emits explanatory prose, patch text, or protocol-like content instead of the required exact final response

This is visible most clearly in:
- `phi-4`
- `qwen/qwen2.5-coder-14b`
- parts of `qwen3.5-9b-ud`

## Proposed Improvement

Add one narrow final-answer compliance nudge after successful task completion when all of the following are true:
- the task contract requires a specific final answer or exact output shape
- the required tool/edit work has already completed successfully
- there is no further required tool step remaining
- the model's most recent assistant turn is not compliant with the required final-answer contract

This should be one bounded retry only.

## What This Is Not

This is not:
- a provider fix
- a qualification change
- a repeat-guard change
- a broad runtime-loop redesign
- a silent rewrite of model output
- a hidden extra tool attempt

## Candidate Runtime Seam

Most likely seam:
- the existing post-write/follow-on completion path around:
  - [src/agent/runtime_completion.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/runtime_completion.rs)
  - [src/agent/run_finalize.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/run_finalize.rs)
  - [src/agent.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent.rs)

Rationale:
- LocalAgent already knows when a write has been verified
- LocalAgent already knows when the prompt still requires one more follow-on turn
- the missing piece is a more explicit final-answer compliance check when the model uses that last turn poorly

## Proposed Trigger

Trigger a bounded compliance retry only when:
- verified write or other required tool work has already succeeded
- no further mandatory tool/test step remains
- the prompt contract includes an exact-output requirement
- the current assistant response does not satisfy that exact-output requirement

Examples:
- prompt says `Your final answer must be exactly: ...`
- prompt requires a tightly structured summary such as:
  - `verified=yes`
  - `file=...`
  - `command=node --test`
  - `result=passed`

## Inserted Behavior

Insert one explicit developer/runtime message such as:

`The task work is complete. Do not explain your steps. Reply now with the required final answer only, exactly matching the requested format. Do not call tools.`

Then allow exactly one more assistant turn.

If that turn is still non-compliant, finalize the run as it does today.

## Why This Is Narrow Enough

This proposal stays bounded because:
- it only happens after successful task work
- it only triggers when the contract explicitly demands an exact final answer
- it allows one extra assistant turn only
- it does not reopen general planning or tool-use loops
- it does not change provider semantics
- it does not hide failure

## Invariants To Preserve

Any implementation should preserve:
- no hidden retries for tool execution
- no silent transformation of model prose into a fabricated final answer
- no weakening of implementation guards
- no weakening of repeat guards
- no provider-specific branching
- auditable runtime behavior through transcript/events
- strict failure remains visible if the model still misses the contract after the one bounded retry

## Why This Proposal Is Justified

This improvement is justified because the repeated problem is now localized and cross-model:
- tool execution has already succeeded
- qualification has already succeeded
- provider traces are already good enough
- the remaining failure is final-answer compliance after success

That is exactly the kind of narrow, reviewable compatibility improvement worth considering.

## Why Broader Changes Are Not Justified

The current evidence does not justify:
- changing provider transport behavior
- weakening repeat guards for edit failures
- broad heuristics for tool substitution
- changing shared runtime-loop architecture again

Those would go beyond the observed failure boundary.

## Suggested Acceptance Criteria

If implemented, the improvement should satisfy:
- stronger models that already pass remain unchanged
- `phi-4` and similar models get one explicit final-answer-only retry after successful tool work
- the runtime remains deterministic and auditable
- no extra tool call is allowed in the compliance retry
- failures still fail clearly if the second chance is missed

## Suggested Validation Slice

Use a very small matrix:
- `phi-4` on `T1`, `T2`, `T3`
- `qwen/qwen2.5-coder-14b` on `T1` and `T2`
- `qwen2.5-coder-7b-instruct@q8_0` as the guardrail control

Measure:
- exact-final-answer pass rate
- whether any previously passing run regresses
- whether the extra retry stays bounded to one turn
- whether any new tool calls are emitted during the compliance retry
