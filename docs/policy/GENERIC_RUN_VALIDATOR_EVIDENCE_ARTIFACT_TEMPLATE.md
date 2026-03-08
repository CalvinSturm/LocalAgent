# Generic Run Validator Evidence Artifact

Use this template to capture one representative case for `GENERIC_RUN_VALIDATOR_INVESTIGATION_2026-03.md`.

This template is aligned to LocalAgent run artifacts and runtime terminology.

Preferred source artifact:

- a LocalAgent run record under `.localagent/runs/<run_id>.json` or another exported `runs/<run_id>.json`

Use repo/runtime terms consistently:

- `exit_reason`: use the serialized `AgentExitReason` string from the run record
- `final_output`: summarize the recorded runtime final output, not a rewritten ideal answer
- `tool_use_summary`: derive from recorded `tool_calls` and `tool_decisions`
- `structurally_complete`: answer whether the runtime reached a coherent terminal boundary, not whether the task was correct
- `task acceptance`: keep distinct from structural/runtime validity

## Case ID

- `case_id`:
- `date`:
- `owner`:

## Source

- `artifact_path`:
- `run_id`:
- `mode`: `run` | `check` | `eval` | `hooks`
- `provider`:
- `model`:
- `workdir`:

Suggested artifact fields:

- `metadata.run_id`
- `metadata.exit_reason`
- `metadata.provider`
- `metadata.model`
- `resolved_paths.workdir`
- `final_output`
- `tool_calls`
- `tool_decisions`
- `messages`

## Prompt / Intent

- `prompt_summary`:
- `operator_expected_outcome`:
- `did_operator_expect_task_acceptance`: `yes` | `no`

## Observed Runtime Result

- `exit_reason`:
  - use LocalAgent serialized values such as `ok`, `planner_error`, `max_steps`, `provider_error`, `approval_required`, `denied`
- `final_output_summary`:
- `files_changed`:
- `tool_use_summary`:
- `structurally_complete`: `yes` | `no`

Guidance for `structurally_complete`:

- `yes` when the runtime reached a coherent terminal result boundary for the chosen mode
- `no` when the run ended in malformed/protocol-broken/non-terminal behavior
- do not use this field to mean “the task was actually correct”

## Acceptance Analysis

- `was_task_acceptance_actually_proven`: `yes` | `no` | `unclear`
- `what_was_missing`:
- `would_a_plain_run_validator_have_helped`: `yes` | `no` | `maybe`
- `why`:

## Right Layer Classification

- `recommended_layer`:
  - `shared_runtime`
  - `opt_in_validator`
  - `hooks_checks`
  - `eval_only`
- `classification_reason`:

## Evidence

- `transcript_excerpt_or_summary`:
- `artifact_fields_used`:
- `related_docs_or_tests`:

Suggested `artifact_fields_used` entries:

- `metadata.exit_reason`
- `final_output`
- `tool_calls[*].name`
- `tool_decisions[*].decision`
- assistant/tool `messages`
- any repo-side file artifact inspected after the run

## Recommendation

- `recommendation_for_generic_run_validator_note`:
- `follow_up_needed`: `yes` | `no`

---

## Compact Matrix Row

Copy this into the recommendation matrix after filling the case:

| case_id | artifact_path | operator_expected_task_acceptance | exit_reason | structurally_complete | task_acceptance_proven | recommended_layer | plain_run_validator_helpful | notes |
|---|---|---|---|---|---|---|---|---|
| `<case_id>` | `<artifact_path>` | `yes/no` | `<exit_reason>` | `yes/no` | `yes/no/unclear` | `shared_runtime / opt_in_validator / hooks_checks / eval_only` | `yes/no/maybe` | `<short note>` |
