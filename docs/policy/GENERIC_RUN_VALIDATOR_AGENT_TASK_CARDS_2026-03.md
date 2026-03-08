# Generic Run Validator Agent Task Cards (2026-03)

Purpose: gather artifact-backed evidence for `GENERIC_RUN_VALIDATOR_INVESTIGATION_2026-03.md` without changing runtime behavior.

Rules for all tasks:

- investigation only
- no runtime-loop behavior changes
- no speculative fixes
- prefer existing repo artifacts before creating new runs
- use `GENERIC_RUN_VALIDATOR_EVIDENCE_ARTIFACT_TEMPLATE.md` for case writeups
- keep structural/runtime validity distinct from task acceptance validity

## Task 1: Generic Run False-Confidence Cases

### Objective

Collect two representative generic `run` cases where the runtime reached `exit_reason=ok` but task acceptance was not actually proven.

### Inputs

- `docs/policy/GENERIC_RUN_VALIDATOR_INVESTIGATION_2026-03.md`
- `docs/policy/GENERIC_RUN_VALIDATOR_EVIDENCE_ARTIFACT_TEMPLATE.md`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/10a2c817-cd55-425f-949b-112d566d9363.json`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/c73e1f09-4fd8-4b08-9a52-1d70b3917af6.json`
- `.tmp/manual-exact-eval-glm-4_6v-flash-20260307-202500/state/runs/f0233fd2-bf88-4aa0-9f7e-3d2422486f3d.json`
- any directly referenced output files needed to verify what actually happened

### Required Work

1. Pick two cases from the listed artifacts.
2. Fill out one evidence artifact per case.
3. Explain whether the runtime result was structurally complete.
4. Explain whether task acceptance was actually proven.
5. Classify the right layer for each case.

### Outputs

- `docs/policy/generic-run-validator-cases/<case_id>.md` for each case
- one compact matrix row per case, ready to paste into a recommendation matrix

### Acceptance Criteria

- both cases come from real artifacts
- both cases use repo/runtime terminology correctly
- each case explicitly distinguishes:
  - structural/runtime validity
  - task acceptance validity
- each case includes a recommended layer classification
- no behavior changes are proposed

## Task 2: Generic Run Contrast Case

### Objective

Collect one generic `run` case where structural completion was the correct contract and no task validator was needed.

### Inputs

- `docs/policy/GENERIC_RUN_VALIDATOR_INVESTIGATION_2026-03.md`
- `docs/policy/GENERIC_RUN_VALIDATOR_EVIDENCE_ARTIFACT_TEMPLATE.md`
- existing run artifacts under `.localagent/runs/` or `.tmp/**/state/runs/`
- code references:
  - `src/agent/runtime_completion.rs`
  - `src/agent/run_setup.rs`
  - `src/agent/run_finalize.rs`

### Required Work

1. Identify one real case where `exit_reason=ok` was appropriate as a structural/runtime result.
2. Show why a plain task validator would not have added value in that case.
3. Fill out the evidence artifact.

### Outputs

- `docs/policy/generic-run-validator-cases/<case_id>.md`
- one compact matrix row for the contrast case

### Acceptance Criteria

- the case is grounded in a real run artifact
- the note clearly argues why structural completion was sufficient
- the analysis does not smuggle task-acceptance expectations into plain `run`
- the recommended layer is justified

## Task 3: Validator Surface Inventory

### Objective

Inventory validator-like surfaces already present in the repo and classify what they validate.

### Inputs

- `docs/policy/GENERIC_RUN_VALIDATOR_INVESTIGATION_2026-03.md`
- `docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md`
- relevant code and docs, including:
  - `src/eval/`
  - `src/checks/`
  - `src/hooks/`
  - `src/agent/runtime_completion.rs`
  - `src/agent/run_finalize.rs`
  - `docs/reference/CLI_REFERENCE.md`
  - `docs/architecture/RUNTIME_ARCHITECTURE.md`

### Required Work

Produce a short inventory table with these columns:

- surface
- trigger
- what it validates
- runtime continuation impact
- why it is or is not appropriate for generic `run`

### Outputs

- `docs/policy/generic-run-validator-surface-inventory-2026-03.md`

### Acceptance Criteria

- includes at least:
  - eval
  - checks
  - hooks
  - planner/phase contract surfaces
  - implementation-integrity guard
  - protocol-artifact rejection
- distinguishes guards from validators where relevant
- does not recommend behavior changes without evidence

## Task 4: Generic Run Terminal Contract Trace

### Objective

Trace the exact terminal path for generic `run` and state what `exit_reason=ok` means today.

### Inputs

- `src/agent/runtime_completion.rs`
- `src/agent/run_setup.rs`
- `src/agent/run_finalize.rs`
- `src/agent.rs`
- `docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md`
- one or more real `run` artifacts with `exit_reason=ok`

### Required Work

1. Trace the code path for generic `run` terminal success.
2. Identify which checks are structural/runtime checks versus task acceptance checks.
3. Write one short answer to:
   - what does `exit_reason=ok` mean today in generic `run`?

### Outputs

- `docs/policy/generic-run-terminal-contract-trace-2026-03.md`

### Acceptance Criteria

- cites the actual code path
- names the runtime checks involved in terminal success
- explicitly states whether task acceptance is part of the current generic `run` contract
- stays descriptive, not prescriptive

## Task 5: Recommendation Matrix

### Objective

Synthesize the evidence cases and inventory into a recommendation matrix for the investigation note.

### Inputs

- outputs from Tasks 1-4
- `docs/policy/GENERIC_RUN_VALIDATOR_INVESTIGATION_2026-03.md`

### Required Work

1. Build a matrix with one row per collected case.
2. For each case, classify the right layer:
   - `shared_runtime`
   - `opt_in_validator`
   - `hooks_checks`
   - `eval_only`
3. Summarize whether there is any artifact-backed reason to change shared runtime behavior now.

### Outputs

- `docs/policy/generic-run-validator-recommendation-matrix-2026-03.md`

### Acceptance Criteria

- includes all collected cases
- each row has a justified layer classification
- final summary answers:
  - is there evidence for changing shared runtime now
  - or should generic `run` remain task-agnostic by default
- if the answer is “no change now,” the note says so directly

## Suggested Execution Order

1. Task 1
2. Task 2
3. Task 3
4. Task 4
5. Task 5

## Handoff Standard

Each task result should be reviewable on its own and should avoid these failure modes:

- replacing evidence with opinion
- confusing `exit_reason=ok` with proven task correctness
- proposing runtime-loop changes without artifact-backed need
- treating prompt semantics as a substitute for explicit validator design
