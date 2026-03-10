# Local Model Improvement Backlog

Status: Active
Owner: LocalAgent maintainers
Last reviewed: 2026-03-10

This file is the bridge between:
- evidence in [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md)
- current recommendations in [manual-testing/LOCAL_MODEL_COMPATIBILITY_SUMMARY.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/LOCAL_MODEL_COMPATIBILITY_SUMMARY.md)
- repeatable eval workflow in [manual-testing/LOCAL_MODEL_EVAL_RUNBOOK.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/LOCAL_MODEL_EVAL_RUNBOOK.md)

Use it to answer:
- what should be improved next in LocalAgent from local-model evidence
- what should not be reopened without new evidence
- which model should be used as the current regression baseline

## Current Baseline And Secondary Models

Current local-model baseline:
- `qwen2.5-coder-7b-instruct@q8_0`

Current secondary comparison models:
- `qwen/qwen3.5-9b` (effective `Q6_k` result)
- `crow-9b-opus-4.6-distill-heretic_qwen3.5`

Use these three first when validating runtime or prompt/tool-affordance changes.

## Highest-Value Next Improvements

### 1. TUI And One-Shot `run` Parity

Priority: high

Why:
- interactive TUI and one-shot `run` are both important product surfaces
- TUI currently forces streaming on normal submit, which can change model behavior before the shared runtime loop sees the turn
- this creates noisy eval differences and makes LocalAgent harder to tune systematically

Target:
- make TUI respect the configured stream setting, or make the difference explicit and operator-controlled
- define a clean TUI eval mode with fresh state and no hidden context carryover

Do this next if:
- the same task/model behaves differently in TUI vs `run`
- the first divergence appears before shared runtime/tool execution

### 2. Tool Affordance For Edit Tasks

Priority: high

Why:
- the strongest remaining repeated failure on harder tasks is edit convergence, especially repeated `str_replace` failure on `T3`-class tasks
- OpenCode succeeded earlier on the same parser-fix shape by using a different edit affordance sooner
- LocalAgent stream success on some runs shows the runtime is capable enough; the remaining question is whether tool affordance or prompt framing can improve convergence

Target:
- inspect how stronger open-source agents steer models toward patch/diff-style editing
- prefer explicit affordance improvements over hidden retries or weakened guards

Acceptable change shape:
- clearer tool descriptions
- better tool examples
- better recovery guidance after failed exact-match edits

Not acceptable:
- silent tool rewriting
- weakening repeat guards just to raise pass rate

### 3. Eval Metadata And Reproducibility

Priority: medium

Why:
- recent `qwen/qwen3.5-9b` reruns showed that quantization, preset, and temperature materially change outcomes even under the same model ID
- this is now documented, but future work should keep using the same explicit metadata discipline

Target:
- keep every leaderboard-affecting run tagged with:
  - provider
  - base URL
  - model
  - model variant
  - provider-side preset
  - stream
  - temperature
  - top_p
  - max_tokens
  - seed

This is a process requirement, not a new runtime feature request.

## Lower-Priority Improvements

### Exact-Output Stress Handling

Priority: medium

Why:
- several weaker models still fail exact final-answer discipline after successful tool work
- LocalAgent now classifies these failures more clearly

Current stance:
- classification is good enough for now
- do not reopen broader continuation or compliance-retry changes without evidence that a narrow intervention improves multiple models materially

### Provider-Specific Investigation

Priority: low

Why:
- provider and transport issues were real earlier in the investigation, but they are no longer the dominant blocker on the current baseline models

Current stance:
- only reopen provider work when a new failure is still visible after:
  - qualification
  - tool/runtime
  - prompt/tool-affordance
  analysis has already ruled out simpler causes

## Do Not Revisit Without New Evidence

These items are closed enough that they should not be reopened casually:

- qualification false negatives as the primary explanation for current local-model failures
- stream-vs-non-stream qualification mismatch as the primary blocker
- missing non-stream provider traces
- eager post-write terminalization on tasks that explicitly require follow-on work
- silent `ok` closeout when pre-tool prose masked a missing post-tool response

Reopen only if a new regression is shown with current commits and artifacts.

## Current Accepted Limitations

These are limitations to document, not current runtime bugs to fix immediately:

- `qwen/qwen3.5-9b`:
  - effective `Q6_k` result remains useful, but `T3`-class repeated `str_replace` convergence is still an accepted limitation
- `qwen/qwen3.5-9b` current `Q8_0` load:
  - unstable across reruns under the same model ID
- `phi-4`:
  - useful as an exact-output stress model, not a baseline
- `nanbeige4.1-3b@bf16` and `deepseek-r1-0528-qwen3-8b-ud`:
  - tool-discipline / tool-protocol failures dominate

## Standard Decision Rule

When a new local-model issue appears:

1. Confirm it on the runbook workflow with fresh state and explicit metadata.
2. Compare stream and non-stream only if that distinction is still relevant.
3. Stop at the first concrete divergence.
4. Classify it as one of:
   - provider bug
   - runtime bug
   - compatibility gap
   - pure model-choice
5. Only propose a shared runtime change if:
   - the issue is reproduced cleanly
   - it is not better explained by model choice or missing eval metadata
   - the proposed fix is narrow and reviewable

## Recommended Next Work Order

1. TUI and `run` parity
2. Edit-tool affordance on `T3`-class tasks
3. Continue selective model evaluation using the current baseline and secondary comparisons

## Evidence Sources

- [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md)
- [manual-testing/LOCAL_MODEL_COMPATIBILITY_SUMMARY.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/LOCAL_MODEL_COMPATIBILITY_SUMMARY.md)
- [manual-testing/LOCAL_MODEL_LEADERBOARD.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/LOCAL_MODEL_LEADERBOARD.md)
- [manual-testing/LOCAL_MODEL_EVAL_RUNBOOK.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/LOCAL_MODEL_EVAL_RUNBOOK.md)
