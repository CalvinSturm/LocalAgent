# Local Model Compatibility Summary

This summary condenses the current findings from [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md).

Use it as the short reference for:
- which local model is the current baseline
- which models are worth using as secondary comparisons
- which failure classes are mostly runtime history versus accepted model limitations

## Current Baseline

Current recommended local-model baseline:
- `qwen2.5-coder-7b-instruct@q8_0`

Why:
- qualification passes cleanly
- `T1` passes in both stream modes
- `T2` passes in both stream modes
- `T3` passes in non-stream mode
- the main remaining limitation on the narrow matrix is a streamed `T3` ordering failure

## Current Ranking

### Recommended Baseline

`qwen2.5-coder-7b-instruct@q8_0`
- strongest current result on the narrow `T1`/`T2`/`T3` matrix
- best current choice for local regression checks
- prefer non-stream mode for contract-complete multi-step tasks

### Strong Secondary Comparisons

`qwen/qwen3.5-9b` (effective `Q6_k` result)
- good narrow tool-use viability
- strong streamed behavior after the qualification fixes
- accepted non-stream Tool B limitation is pure model-choice after equivalent recovery
- keep this separated from the newer weaker `Q8_0` reruns under the same model ID

`crow-9b-opus-4.6-distill-heretic_qwen3.5`
- passes `T1` and `T2` in both modes
- fails `T3` in both modes, but with coherent tool use rather than broad protocol collapse
- useful secondary contrast for stronger local coding behavior

### Mid-Tier Fits

`zai-org/glm-4.6v-flash`
- coherent enough to reach real tool work
- blocked by exact-output discipline on `T1` and failed edit convergence on `T2`/`T3`
- better than the worst protocol-breaking models, but not baseline-ready

`deepseek-coder-v2-lite-instruct`
- clean `T1` in both modes
- drops sharply on `T2` and `T3`
- not strong enough on contract-complete work to replace the current secondary comparisons

`qwen3.5-9b-ud`
- qualification clean
- contract-complete behavior unstable across both modes
- weaker than `qwen/qwen3.5-9b`

`qwen/qwen3.5-9b` (current `Q8_0` load)
- materially weaker than the earlier stronger run under the same model ID
- reruns showed broader exact-output drift, bad argument/closeout discipline, and repeated tool misuse
- do not treat it as equivalent to the older stronger `qwen/qwen3.5-9b` result

### Weak Fits On The Current Matrix

`qwen/qwen2.5-coder-14b`
- often echoes prompt/protocol content instead of taking the next correct action
- not recommended as a baseline

`phi-4`
- reaches the needed tool/edit step
- repeatedly fails exact-output discipline after successful tool use
- useful only for exact-output stress checks, not as a baseline

`qwen2.5-coder-7b-instruct@q5_k_m`
- handles `T1` in both modes
- falls off sharply on `T2` and `T3`

`nanbeige4.1-3b@bf16`
- dominant issue is read-before-write/apply discipline
- also showed one streamed provider/body decode timeout on `T1`

`deepseek-r1-0528-qwen3-8b-ud`
- broad tool-protocol instability
- one outright provider crash
- not recommended for LocalAgent eval baselines

`starcoder2-7b`
- repeated qualification fallback and provider instability under the current LM Studio setup
- often fails before useful task execution is established

## What Is Already Fixed

These are no longer the main blockers for local-model evaluation:
- qualification false negatives
- stream-vs-non-stream qualification mismatch
- missing non-stream provider traces
- eager post-write finalization when the prompt explicitly required follow-on work
- silent `ok` misclassification when pre-tool planning prose masked a missing post-tool closeout

See the corresponding entries in [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md) for evidence and commit baselines.

## Current Repeated Failure Classes

### Qualification

No longer the dominant problem on the current tested models.

### Tool-use capability

Not the primary blocker for the stronger models.
Several models can reach read/write/edit steps successfully.

### Exact-output discipline

This is still one of the clearest repeated weaknesses across weaker models.

Observed pattern:
- model reaches the right tool step
- tool succeeds
- model emits explanatory prose, patch narration, or protocol-like formatting
- required exact final answer is not produced

This appears most clearly in:
- `phi-4`
- `zai-org/glm-4.6v-flash`
- parts of `qwen/qwen2.5-coder-14b`
- parts of `qwen3.5-9b-ud`

### Multi-step task ordering and edit convergence

Still mode-sensitive for some models.

Observed pattern:
- model runs validation/tests before completing the required edit
- or fails to recover after repeated `str_replace` failures
- or reaches the edit seam but never produces an effective write

This appears in:
- streamed `T3` for `qwen2.5-coder-7b-instruct@q8_0`
- non-stream Tool B for `qwen/qwen3.5-9b` (effective `Q6_k` result)
- both `Q8_0` reruns for `qwen/qwen3.5-9b`
- both modes of `crow-9b-opus-4.6-distill-heretic_qwen3.5` on `T3`
- `zai-org/glm-4.6v-flash` on `T2`/`T3`

### Tool-discipline failures

Observed pattern:
- `write_file` or `apply_patch` issued before the required `read_file`
- or malformed tool protocol before any useful work

This appears in:
- `nanbeige4.1-3b@bf16`
- `deepseek-r1-0528-qwen3-8b-ud`

## Current Recommendations

For local-model regression testing now:
- use `qwen2.5-coder-7b-instruct@q8_0` as the baseline
- prefer non-stream mode for contract-complete multi-step tasks
- use `qwen/qwen3.5-9b` (effective `Q6_k` result) or `crow-9b-opus-4.6-distill-heretic_qwen3.5` as secondary comparison models
- use the run procedure in [manual-testing/LOCAL_MODEL_EVAL_RUNBOOK.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/LOCAL_MODEL_EVAL_RUNBOOK.md)
- log new findings in [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md)

For interpreting failures:
- do not collapse all failures into “runtime broken”
- separate:
  - qualification viability
  - tool-execution viability
  - contract-complete viability
  - exact-output discipline
  - tool-discipline / tool-protocol failures

## Recommended Next Candidates

Most promising next eval targets from the currently loaded local models:
- `orchestrator-8b-claude-4.5-opus-distill`
- `qwen3-4b-instruct-2507-ud`

Why these next:
- they are the next best contrast after the two coding-oriented candidates already tested today
- one is an orchestrator-style distill and the other is a newer qwen-family instruct model
- both are more likely to add ranking signal than continuing with obviously weak small-model variants
