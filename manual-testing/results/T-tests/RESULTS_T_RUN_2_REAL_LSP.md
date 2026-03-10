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
| 2026-03-08 | lmstudio | qwen/qwen2.5-coder-14b + real typescript lsp | T1 | "c9d0e412-24fb-4f34-b57d-25c5dde14b33" | "C:\Users\Calvin\Software Projects\LocalAgent\.tmp\manual-testing\control\T-tests\20260308-163001-414-17d687\T1\.localagent\runs\c9d0e412-24fb-4f34-b57d-25c5dde14b33.json" | "20260308-163001-414-17d687" | ephemeral | "ok" | "n/a" | "n/a" | fail | "<empty>" | incorrect | not_applicable | no | 1 | 32 | noisy | model | Exact file content and final-answer contract required. |
| 2026-03-08 | lmstudio | qwen/qwen2.5-coder-14b + real typescript lsp | T2 | "e3f365a0-4379-40b0-b8ff-8227e6e25d0a" | "C:\Users\Calvin\Software Projects\LocalAgent\.tmp\manual-testing\control\T-tests\20260308-163001-414-17d687\T2\.localagent\runs\e3f365a0-4379-40b0-b8ff-8227e6e25d0a.json" | "20260308-163001-414-17d687" | ephemeral | "ok" | "n/a" | "n/a" | fail | "<empty>" | incorrect | not_applicable | no | 2 | 49 | noisy | model | Exact apply_patch edit and final-answer contract required. |
| 2026-03-08 | lmstudio | qwen/qwen2.5-coder-14b + real typescript lsp | T3 | "680b42b4-568f-45fa-8f90-bc00d8dabc45" | "C:\Users\Calvin\Software Projects\LocalAgent\.tmp\manual-testing\control\T-tests\20260308-163001-414-17d687\T3\.localagent\runs\680b42b4-568f-45fa-8f90-bc00d8dabc45.json" | "20260308-163001-414-17d687" | ephemeral | "planner_error" | "n/a" | "n/a" | fail | "<empty>" | incorrect | failed | no | 3 | 149 | noisy | model | JS parser bugfix with @ts-check diagnostics. |
| 2026-03-08 | lmstudio | qwen/qwen2.5-coder-14b + real typescript lsp | T4 | "af1f1973-9df9-48db-bd79-30e78e59fb9b" | "C:\Users\Calvin\Software Projects\LocalAgent\.tmp\manual-testing\control\T-tests\20260308-163001-414-17d687\T4\.localagent\runs\af1f1973-9df9-48db-bd79-30e78e59fb9b.json" | "20260308-163001-414-17d687" | ephemeral | "planner_error" | "n/a" | "n/a" | fail | "<empty>" | incorrect | not_applicable | no | 2 | 81 | noisy | model | Inspect-first real-definition typo fix. |
| 2026-03-08 | lmstudio | qwen/qwen2.5-coder-14b + real typescript lsp | T5 | "bf4aad78-02f0-4973-af0c-807d64b6cb26" | "C:\Users\Calvin\Software Projects\LocalAgent\.tmp\manual-testing\control\T-tests\20260308-163001-414-17d687\T5\.localagent\runs\bf4aad78-02f0-4973-af0c-807d64b6cb26.json" | "20260308-163001-414-17d687" | ephemeral | "planner_error" | "n/a" | "n/a" | fail | "<empty>" | incorrect | failed | no | 7 | 220 | noisy | model | Nested recovery parser fixture with exact success phrase. |
