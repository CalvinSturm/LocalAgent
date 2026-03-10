# RESULTS_TEMPLATE_T

Use one row per run. Preserve exact runtime truth and exact task outcome separately.

| date | provider | model | task | run_id | artifact_path | prepared_instance_id | session_mode | exit_reason | reason_code | failure_class | task_result | final_answer | file_result | test_result | path_preexisted | tool_calls_total | bytes_written | execution_quality | failure_layer | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 2026-03-08 | lmstudio | example-model | T1 | run-123 | .localagent/runs/run-123.json | 20260308-000000-000-abcdef | ephemeral | ok | n/a | n/a | pass | `verified=yes` | correct | not_applicable | no | 1 | 30 | clean | none | Example row |

Task-specific pass checks:
- `T1`: `src/status.ts` created exactly and final answer matches prompt contract.
- `T2`: `src/score.ts` updated exactly with `apply_patch` and final answer matches prompt contract.
- `T3`: `node --test` passes and final answer matches prompt contract.
- `T4`: typo fixed in the real definition file and final answer matches prompt contract.
- `T5`: nested parser fix is verified by `node --test` and final answer matches prompt contract.

