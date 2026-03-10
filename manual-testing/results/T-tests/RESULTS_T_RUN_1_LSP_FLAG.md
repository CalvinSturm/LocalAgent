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
| 2026-03-08 | lmstudio | qwen/qwen2.5-coder-14b + lsp-provider=typescript | T1 | "e4cdbbf8-b1f6-41e3-bb0c-316319fef067" | "C:\Users\Calvin\Software Projects\LocalAgent\.tmp\manual-testing\control\T-tests\20260308-155348-522-f502b9\T1\.localagent\runs\e4cdbbf8-b1f6-41e3-bb0c-316319fef067.json" | "20260308-155348-522-f502b9" | ephemeral | "ok" | "n/a" | "n/a" | fail | "<empty>" | incorrect | not_applicable | no | 2 | 31 | noisy | model | Exact file content and final-answer contract required. |
| 2026-03-08 | lmstudio | qwen/qwen2.5-coder-14b + lsp-provider=typescript | T2 | "636738d5-ad59-4e1a-b8e3-1c63e47b575e" | "C:\Users\Calvin\Software Projects\LocalAgent\.tmp\manual-testing\control\T-tests\20260308-155348-522-f502b9\T2\.localagent\runs\636738d5-ad59-4e1a-b8e3-1c63e47b575e.json" | "20260308-155348-522-f502b9" | ephemeral | "ok" | "n/a" | "n/a" | fail | "<empty>" | incorrect | not_applicable | no | 2 | 49 | noisy | model | Exact apply_patch edit and final-answer contract required. |
| 2026-03-08 | lmstudio | qwen/qwen2.5-coder-14b + lsp-provider=typescript | T3 | "a1db59d9-c212-4bfb-941c-ff82765f7f66" | "C:\Users\Calvin\Software Projects\LocalAgent\.tmp\manual-testing\control\T-tests\20260308-155348-522-f502b9\T3\.localagent\runs\a1db59d9-c212-4bfb-941c-ff82765f7f66.json" | "20260308-155348-522-f502b9" | ephemeral | "planner_error" | "n/a" | "n/a" | fail | "<empty>" | incorrect | failed | no | 2 | 149 | noisy | model | JS parser bugfix with @ts-check diagnostics. |
| 2026-03-08 | lmstudio | qwen/qwen2.5-coder-14b + lsp-provider=typescript | T4 | "d4fcabad-7a60-4adf-8da3-98dea3772a67" | "C:\Users\Calvin\Software Projects\LocalAgent\.tmp\manual-testing\control\T-tests\20260308-155348-522-f502b9\T4\.localagent\runs\d4fcabad-7a60-4adf-8da3-98dea3772a67.json" | "20260308-155348-522-f502b9" | ephemeral | "planner_error" | "n/a" | "n/a" | fail | "<empty>" | incorrect | not_applicable | no | 2 | 81 | noisy | model | Inspect-first real-definition typo fix. |
| 2026-03-08 | lmstudio | qwen/qwen2.5-coder-14b + lsp-provider=typescript | T5 | "56d0bc55-cc10-466f-babb-2a04d8e6a4ad" | "C:\Users\Calvin\Software Projects\LocalAgent\.tmp\manual-testing\control\T-tests\20260308-155348-522-f502b9\T5\.localagent\runs\56d0bc55-cc10-466f-babb-2a04d8e6a4ad.json" | "20260308-155348-522-f502b9" | ephemeral | "planner_error" | "n/a" | "n/a" | fail | "<empty>" | incorrect | failed | no | 1 | 220 | noisy | model | Nested recovery parser fixture with exact success phrase. |
