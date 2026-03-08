# D-Tests Control-Pack Results Template

Use one row per model run per task.

Record:
- what LocalAgent actually did
- what the task outcome actually was
- where the primary failure belongs

Do not replace exact runtime truth with a simplified summary. Preserve the exact `exit_reason` emitted by LocalAgent.

## Required Fields

- `date`: run date in `YYYY-MM-DD`
- `provider`: for example `lmstudio`, `llamacpp`, `openai_compat`
- `model`: exact model identifier
- `task`: `D1` | `D2` | `D3` | `D4` | `D5`
- `run_id`: LocalAgent run ID if available
- `artifact_path`: path to the run artifact or prepared instance result context if available
- `prepared_instance_id`: prepared-copy instance ID from `PREPARED_INSTANCE.json`
- `session_mode`: `ephemeral` | `persistent`
- `exit_reason`: exact LocalAgent exit reason such as `ok`, `planner_error`, `approval_required`
- `reason_code`: machine-readable continuation or block reason if known, otherwise `n/a`
- `failure_class`: machine-readable failure class if known, otherwise `n/a`
- `task_result`: `pass` | `fail` | `partial`
- `final_answer`: exact final assistant response if available
- `file_result`: `correct` | `incorrect` | `not_checked`
- `test_result`: `passed` | `failed` | `not_applicable`
- `path_preexisted`: `yes` | `no` | `unknown`
- `tool_calls_total`: integer count if known, otherwise `unknown`
- `bytes_written`: integer count if known, otherwise `unknown`
- `execution_quality`: `clean` | `recovered` | `noisy` | `blocked`
- `failure_layer`: `model` | `runtime` | `task_design` | `fixture_hygiene` | `provider_interop` | `mixed` | `unresolved`
- `notes`: short factual summary

## Results

| date | provider | model | task | run_id | artifact_path | prepared_instance_id | session_mode | exit_reason | reason_code | failure_class | task_result | final_answer | file_result | test_result | path_preexisted | tool_calls_total | bytes_written | execution_quality | failure_layer | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 2026-03-08 | example | example-model | D1 | `run-123` | `.localagent/runs/run-123.json` | `20260308-070130-943-88fa53` | ephemeral | `ok` | `n/a` | `n/a` | fail | `verified=yes file=notes/status.txt bytes=5` | incorrect | not_applicable | no | 1 | 5 | clean | model | Created the file, but content or byte count was wrong. |
| 2026-03-08 | example | example-model | D3 | `run-456` | `.localagent/runs/run-456.json` | `20260308-070130-944-daf213` | ephemeral | `planner_error` | `implementation_requires_effective_write` | `n/a` | fail | `` | incorrect | failed | unknown | 2 | unknown | blocked | runtime | No effective write landed before the runtime stopped the run. |

## Interpretation Rules

- `exit_reason` is the official LocalAgent runtime result.
- `task_result` is the task outcome, not the runtime outcome.
- If the runtime stops the run, do not infer that the model would have recovered later.
- Use `failure_layer` to classify the primary cause of failure, not every contributing factor.
- Use `execution_quality` to distinguish a clean pass from a noisy or barely recovered pass.

## Task Checks

- `D1`: `notes/status.txt` exists, contents are exactly `ready\n`, and final answer matches the prompt exactly.
- `D2`: `main.rs` changes `41` to `42`, there are no unrelated edits, and final answer matches the prompt exactly.
- `D3`: parser fix is correct, `cargo test` passes, and final answer matches the prompt exactly.
- `D4`: the real definition file is edited from `sucess` to `success`, there are no unrelated edits, and the final answer is exactly `edited: src/labels.rs`.
- `D5`: parser trims whitespace before parsing, `cargo test` passes, and the final answer is exactly `verified fix`.

## Field Notes

- `reason_code` is most useful when a run is blocked or continued by a known runtime path.
- `failure_class` is most useful when a concrete runtime/tool failure class is available from events or artifacts.
- `bytes_written` can stay `unknown` if you did not inspect it.
- `artifact_path` can point to a run JSON, manual artifact bundle, or another stable run record.
