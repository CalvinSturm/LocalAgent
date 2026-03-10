# Local Model Compatibility Summary

This summary condenses the current findings from [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md).

Use it as the short reference for:
- which local model to use as the current baseline
- which modes are currently more reliable
- which failure classes are LocalAgent issues versus accepted model limitations

## Current Baseline

Current recommended local-model baseline:
- `qwen2.5-coder-7b-instruct@q8_0`

Why:
- qualification passes cleanly
- `T1` passes in both stream modes
- `T2` passes in both stream modes
- `T3` passes in non-stream mode
- the only observed limitation on the current narrow matrix is a streamed `T3` ordering failure, where the model tests before editing

## Current Ranking

### Recommended Baseline

`qwen2.5-coder-7b-instruct@q8_0`
- strongest current result on the narrow `T1`/`T2`/`T3` matrix
- best current choice for local regression checks
- prefer non-stream mode for contract-complete multi-step tasks until streamed `T3` ordering is better understood

### Secondary Option

`qwen/qwen3.5-9b`
- viable for narrow LocalAgent tool-execution testing
- good streamed results on the current matrix
- known non-stream limitation on Tool B parser-fix:
  repeated `str_replace` after equivalent recovery, ending in repeat guard
- acceptable secondary comparison model, but not the current baseline

### Weaker Current Fits

`qwen3.5-9b-ud`
- qualification clean
- contract-complete behavior unstable across both modes
- weaker than `qwen/qwen3.5-9b` on the current matrix

`qwen/qwen2.5-coder-14b`
- qualification clean
- often echoes prompt/protocol content instead of taking the next correct action
- not recommended as a baseline

`phi-4`
- reaches the needed tool/edit step
- repeatedly fails exact-output discipline after successful tool use
- not recommended as a baseline for contract-complete evals

## What Is Already Fixed

These are no longer the main blockers for local-model evaluation:
- qualification false negatives
- stream-vs-non-stream qualification mismatch
- missing non-stream provider traces
- eager post-write finalization for prompts that explicitly required follow-on work

See the corresponding entries in [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md) for evidence and commit baselines.

## Current Repeated Failure Classes

### Qualification

No longer the dominant problem on the current tested models.

### Tool-use capability

Not the primary blocker for the stronger models.
Several models can reach read/write/edit steps successfully.

### Exact-output discipline

This is now the clearest repeated weakness across weaker models.

Observed pattern:
- model reaches the right tool step
- tool succeeds
- model responds with explanatory prose, patch text, or protocol-like formatting
- required exact final answer is not produced

This appears in:
- `phi-4`
- parts of `qwen/qwen2.5-coder-14b`
- parts of `qwen3.5-9b-ud`

### Multi-step task ordering

Still mode-sensitive for some models.

Observed pattern:
- model runs validation/tests before completing the required edit
- or fails to recover after an edit failure

This appears in:
- streamed `T3` for `qwen2.5-coder-7b-instruct@q8_0`
- non-stream Tool B limitation for `qwen/qwen3.5-9b`

## Current Recommendations

For local-model regression testing now:
- use `qwen2.5-coder-7b-instruct@q8_0` as the baseline
- prefer non-stream mode for contract-complete multi-step tasks
- use the run procedure in [manual-testing/LOCAL_MODEL_EVAL_RUNBOOK.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/LOCAL_MODEL_EVAL_RUNBOOK.md)
- log new findings in [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md)

For interpreting failures:
- do not collapse all failures into “runtime broken”
- separate:
  - qualification viability
  - tool-execution viability
  - contract-complete viability
  - exact-output discipline

## Next Improvement Target

The highest-signal next LocalAgent improvement target is:
- exact-output and final-answer compliance after successful tool work

That proposal is documented in:
- [LOCAL_MODEL_EXACT_OUTPUT_IMPROVEMENT_MEMO.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/docs/operations/LOCAL_MODEL_EXACT_OUTPUT_IMPROVEMENT_MEMO.md)
