# Local Model Leaderboard

This leaderboard is a compact view derived from [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md).

Legend:
- `P` = pass / contract-clean on the current minimal matrix task
- `F` = fail
- `S` = stream mode
- `NS` = non-stream mode

| Model | Variant | Best Mode | T1 S | T1 NS | T2 S | T2 NS | T3 S | T3 NS | Accepted Limitation | Recommended Use |
|---|---|---|---|---|---|---|---|---|---|---|
| `qwen2.5-coder-7b-instruct@q8_0` | `Q8_0` | non-stream for general checks | P | P | P | P | F | F | `T3` validation/exact-output completion is an accepted limitation | Baseline |
| `qwen/qwen3.5-9b` (effective `Q6_k`) | `Q6_k` | stream, especially for `T3` | P | P | n/a | n/a | P | F | `T3` is strong in stream but weaker in non-stream and load-sensitive | Secondary comparison |
| `qwen/qwen3.5-9b` (current `Q8_0`) | `Q8_0` | none | F | F | mixed | mixed | F | F | broader exact-output drift and repeated tool misuse across reruns | Not recommended |
| `crow-9b-opus-4.6-distill-heretic_qwen3.5` | default tested load | either for `T1`/`T2`, stream for `T3` debugging | P | P | P | P | F | F | `T3` edit convergence / validation path instability | Secondary comparison |
| `deepseek-coder-v2-lite-instruct` | default tested load | either for `T1` only | P | P | F | F | F | F | ineffective write / provider crash / no-tool `T3` | Comparison only |
| `zai-org/glm-4.6v-flash` | default tested load | none | F | F | F | F | F | F | exact-output on `T1`, ineffective write on `T2`, repeat-guard on `T3` | Mid-tier comparison only |
| `qwen3.5-9b-ud` | `UD` | non-stream looked slightly better | F | P | P | F | F | F | contract-complete instability across both modes | Comparison only |
| `qwen/qwen2.5-coder-14b` | default tested load | none | F | F | F | F | F | F | prompt/protocol echo instead of next action | Not recommended |
| `phi-4` | default tested load | none | F | F | F | F | F | F | exact-output non-compliance after successful tool work | Exact-output stress only |
| `qwen2.5-coder-7b-instruct@q5_k_m` | `Q5_K_M` | either for `T1` only | P | P | F | F | F | F | ineffective write / no-tool completion | Not recommended |
| `starcoder2-7b` | default tested load | none | F | F | F | F | F | F | qualification fallback plus provider instability | Not recommended |
| `nanbeige4.1-3b@bf16` | `bf16` | none | F | F | F | F | F | F | read-before-write/apply discipline | Not recommended |
| `deepseek-r1-0528-qwen3-8b-ud` | `UD` | none | F | F | F | F | F | F | tool-protocol instability / provider crash | Not recommended |

## Notes

- `qwen/qwen3.5-9b` is split here on purpose:
  - the earlier stronger result is the effective `Q6_k` run
  - the current weaker reruns reflect `Q8_0` behavior under the same model ID
- `Variant` is the doc-level field for quantization, provider-side preset, or another load distinction that LocalAgent may not infer from the model ID alone.
- the effective `Q6_k` row is included because its earlier targeted investigation still makes it one of the most useful secondary LocalAgent comparisons even though it was not run as a full later leaderboard-style matrix.
- `mixed` means the task family is informative but comes from the earlier targeted Tool B investigation rather than the exact later leaderboard slice.
- The current baseline remains `qwen2.5-coder-7b-instruct@q8_0`, but `T3` is now treated as an accepted baseline-model limitation rather than a shared runtime defect.
- `qwen/qwen3.5-9b` streamed is the current positive control for the `T3` validation contract.
- Use [manual-testing/LOCAL_MODEL_EVAL_RUNBOOK.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/LOCAL_MODEL_EVAL_RUNBOOK.md) for repeatable comparisons.
