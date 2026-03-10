# Local Model Eval Runbook

Use this runbook to repeat LocalAgent local-model investigations across different models, providers, and stream modes.

This document ties together:
- pack preparation from [manual-testing/T-tests/README.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/T-tests/README.md)
- result capture from [manual-testing/T-tests/results/RESULTS_TEMPLATE_T.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/T-tests/results/RESULTS_TEMPLATE_T.md)
- state and CLI behavior from [docs/reference/CONFIGURATION_AND_STATE.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/docs/reference/CONFIGURATION_AND_STATE.md) and [docs/reference/CLI_REFERENCE.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/docs/reference/CLI_REFERENCE.md)
- investigation logging in [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md)

## Scope

Use this workflow when you want to answer questions like:
- does a model behave differently in stream vs non-stream mode?
- is a failure caused by qualification, provider transport, tool use, or runtime policy?
- does a model/provider pair remain usable across multiple task shapes?

This runbook is for reproducible comparisons, not ad hoc one-off prompts.

## Inputs To Fix Before You Start

Record these up front and keep them constant across a paired comparison unless the comparison is explicitly about that field:
- commit baseline
- provider
- base URL
- model
- model variant or quantization label
- LM Studio preset or equivalent provider-side preset, if any
- prompt/task
- tool permissions
- instruction profile, if any
- stream mode
- temperature
- top_p
- max_tokens
- seed
- `--state-dir`
- `--workdir`
- relevant env vars

When comparing `stream on` vs `stream off`, the intended difference should be `--stream` only.

If the provider UI can change settings outside LocalAgent, record those settings explicitly even when they are not passed on the CLI.

## Required Eval Metadata

For every model-eval run, capture this metadata explicitly in results and investigation notes:
- provider
- base URL
- model ID
- freeform `model_variant` label
  - examples: `Q6_k`, `Q8_0`, `bf16`, `temp0 preset`
- stream mode
- temperature
- top_p
- max_tokens
- seed
- LM Studio preset or equivalent provider-side preset, if any

Prefer runtime truth when available:
- the run record already stores provider/base URL/model and LocalAgent sampling fields in its `cli` section
- use `model_variant` and preset fields for details LocalAgent cannot infer, such as quantization or a provider-side preset under the same model ID

## Standard Artifact Surfaces

For every meaningful run, preserve:
- run record under `.tmp/repro-state/<scenario>/runs/<run-id>.json`
- provider traces under `.tmp/openai-traces/<scenario>/`
- qualification traces under `.tmp/qualification-traces/<scenario>/`
- prepared pack metadata in `PREPARED_INSTANCE.json`
- external control transcript, if comparing against OpenCode or another runtime

Write final conclusions to [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md).

## Standard Environment

Set trace dirs per scenario and per stream mode so artifacts do not mix:

```powershell
$env:LOCALAGENT_OPENAI_TRACE_DIR = ".tmp/openai-traces/<scenario>-stream-on"
$env:LOCALAGENT_QUAL_TRACE_DIR   = ".tmp/qualification-traces/<scenario>-stream-on"
```

For the matching non-stream run:

```powershell
$env:LOCALAGENT_OPENAI_TRACE_DIR = ".tmp/openai-traces/<scenario>-stream-off"
$env:LOCALAGENT_QUAL_TRACE_DIR   = ".tmp/qualification-traces/<scenario>-stream-off"
```

Use a fresh `--state-dir` for every run.

## Prepare A Fresh Control Pack

Do not run tasks from the source pack in place.

Prepare a fresh instance:

```powershell
pwsh -File .\manual-testing\scripts\prepare_manual_control_pack.ps1 -Pack T-tests
```

Or one task only:

```powershell
pwsh -File .\manual-testing\scripts\prepare_manual_control_pack.ps1 -Pack T-tests -Task T3
```

The prepared instance is written under:

```text
.tmp/manual-testing/control/T-tests/<instance-id>/
```

Each prepared instance includes `PREPARED_INSTANCE.json`. Record the `prepared_instance_id` in results.

## Paired Run Procedure

For each scenario:

1. Prepare or select a fresh runnable task directory.
2. Create a fresh state dir for the streamed run.
3. Set streamed trace dirs.
4. Run the task with `--stream`.
5. Record run ID, exit reason, and artifact paths.
6. Create a separate fresh state dir for the non-stream run.
7. Set non-stream trace dirs.
8. Run the same task again without `--stream`.
9. Record run ID, exit reason, and artifact paths.
10. Compare the two runs before moving on.

Do not reuse state dirs or workdirs across the paired runs if that could contaminate the comparison.

## Standard Command Pattern

From the prepared task directory:

```powershell
$p = Get-Content .\PROMPT.txt -Raw
localagent --provider lmstudio --model "your-model" --allow-shell --allow-write --enable-write-tools --workdir . --state-dir C:\path\to\state --prompt $p run
```

Streamed variant:

```powershell
$p = Get-Content .\PROMPT.txt -Raw
localagent --provider lmstudio --model "your-model" --allow-shell --allow-write --enable-write-tools --workdir . --state-dir C:\path\to\state --stream --prompt $p run
```

With the TypeScript provider enabled:

```powershell
$p = Get-Content .\PROMPT.txt -Raw
localagent --provider lmstudio --model "your-model" --allow-shell --allow-write --enable-write-tools --workdir . --state-dir C:\path\to\state --lsp-provider typescript --prompt $p run
```

## Per-Run Capture Checklist

For every run, record:
- scenario ID
- stream on/off
- provider
- base URL
- model ID
- model variant / quantization
- LM Studio preset, if any
- temperature
- top_p
- max_tokens
- seed
- fresh state dir path
- trace dir paths
- exact command used

Qualification:
- verdict
- reason
- cache written
- cache value
- whether tools were preserved

First assistant turn:
- plain text or native tool call
- tool name, if any
- finish reason, if visible

Tool execution:
- whether a tool executed
- whether policy denied it
- whether the edit or test step succeeded
- short result summary

Completion:
- whether later turns remained well formed
- whether there was a final assistant response
- terminal status
- final exit reason

Trace check:
- provider trace present
- qualification trace present
- first suspicious boundary
- exact artifact paths

## Comparison Order

When investigating a failure, compare in this order:

1. request envelope
   - model
   - stream flag
   - tool list
   - prompt/messages
   - max tokens and sampling fields
2. qualification result
   - verdict
   - reason
   - whether tools were stripped
3. provider result
   - success or error
   - finish reason
   - native tool calls
   - raw or parsed response shape
4. tool sequence
   - first tool
   - first read
   - first edit tool
   - first failed edit
   - first strategy switch
5. runtime completion
   - repeat guard
   - implementation guard
   - post-write follow-on
   - final response

Stop at the first concrete divergence that plausibly explains the outcome.

## OpenCode Comparison

If you have an OpenCode control run, preserve:
- exact prompt text
- transcript
- tool-call sequence
- tool result payloads
- final answer
- config file, if available

Compare OpenCode against both LocalAgent stream and LocalAgent non-stream.

The goal is to find the first concrete divergence, not to prove one runtime is generally better.

## Results Recording

Use [manual-testing/T-tests/results/RESULTS_TEMPLATE_T.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/T-tests/results/RESULTS_TEMPLATE_T.md) for per-pack results.

Use [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md) for investigation conclusions that affect:
- runtime behavior
- provider compatibility
- qualification policy
- model-specific accepted limitations

Each investigation entry should include:
- commit baseline
- provider, base URL, model variant, and mode
- eval settings
- prompt/task
- outcome
- first exact divergence
- classification
- decision
- exact artifact paths

Do not leave quantization, preset, or sampling assumptions implicit when they can affect the result.

Suggested results fields for any checked-in or external results sheet:
- `provider`
- `base_url`
- `model`
- `model_variant`
- `provider_preset`
- `stream`
- `temperature`
- `top_p`
- `max_tokens`
- `seed`
- `run_id`
- `artifact_path`
- `task_result`
- `exit_reason`
- `failure_class`

## Recommended Decision Labels

Use one of these classifications:
- provider bug
- runtime bug
- compatibility gap
- pure model-choice

Use one of these decisions:
- fixed
- accepted limitation
- follow-up needed

## Guardrails

Do not:
- treat plain-chat prompts as build-path failures when the runtime is enforcing implementation behavior
- weaken repeat guards just to improve pass rate
- change runtime semantics based on one noisy local-model failure
- rely only on chat history for conclusions
- leave artifact paths out of the written record

## Minimal Cross-Model Matrix

When qualifying a new model, start here:
- T1 in stream and non-stream
- one contract-complete single-edit task
- one slightly longer multi-step task

Expand only if the smaller matrix is stable and the first failure boundary is understood.
