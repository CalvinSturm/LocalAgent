# Local Model Leaderboard

This leaderboard is a compact view derived from [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md).

Legend:
- `P` = pass / contract-clean on the current minimal matrix task
- `F` = fail
- `S` = stream mode
- `NS` = non-stream mode

| Model | T1 S | T1 NS | T2 S | T2 NS | T3 S | T3 NS | First Failure Boundary | Recommended Use |
|---|---|---|---|---|---|---|---|---|
| `qwen2.5-coder-7b-instruct@q8_0` | P | P | P | P | F | P | streamed `T3` ordering failure | Baseline |
| `qwen/qwen3.5-9b` (effective `Q6_k`) | P | P | n/a | n/a | mixed | mixed | non-stream Tool B repeat-`str_replace` after equivalent recovery | Secondary comparison |
| `qwen/qwen3.5-9b` (current `Q8_0`) | F | F | mixed | mixed | F | F | broader exact-output drift and repeated tool misuse across reruns | Not recommended |
| `crow-9b-opus-4.6-distill-heretic_qwen3.5` | P | P | P | P | F | F | `T3` edit convergence / tool protocol | Secondary comparison |
| `deepseek-coder-v2-lite-instruct` | P | P | F | F | F | F | clean `T1`, then ineffective write / provider crash / no-tool `T3` | Comparison only |
| `zai-org/glm-4.6v-flash` | F | F | F | F | F | F | exact-output on `T1`, ineffective write on `T2`, repeat-guard on `T3` | Mid-tier comparison only |
| `qwen3.5-9b-ud` | F | P | P | F | F | F | contract-complete instability across both modes | Comparison only |
| `qwen/qwen2.5-coder-14b` | F | F | F | F | F | F | prompt/protocol echo instead of next action | Not recommended |
| `phi-4` | F | F | F | F | F | F | exact-output non-compliance after successful tool work | Exact-output stress only |
| `qwen2.5-coder-7b-instruct@q5_k_m` | P | P | F | F | F | F | ineffective write / no-tool completion | Not recommended |
| `starcoder2-7b` | F | F | F | F | F | F | qualification fallback plus provider instability | Not recommended |
| `nanbeige4.1-3b@bf16` | F | F | F | F | F | F | read-before-write/apply discipline | Not recommended |
| `deepseek-r1-0528-qwen3-8b-ud` | F | F | F | F | F | F | tool-protocol instability / provider crash | Not recommended |

## Notes

- `qwen/qwen3.5-9b` is split here on purpose:
  - the earlier stronger result is the effective `Q6_k` run
  - the current weaker reruns reflect `Q8_0` behavior under the same model ID
- the effective `Q6_k` row is included because its earlier targeted investigation still makes it one of the most useful secondary LocalAgent comparisons even though it was not run as a full later leaderboard-style matrix.
- `mixed` means the task family is informative but comes from the earlier targeted Tool B investigation rather than the exact later leaderboard slice.
- The current baseline remains `qwen2.5-coder-7b-instruct@q8_0`.
- Use [manual-testing/LOCAL_MODEL_EVAL_RUNBOOK.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/LOCAL_MODEL_EVAL_RUNBOOK.md) for repeatable comparisons.
