# Manual Eval T Pack

This pack mirrors the `D-tests` workflow, but targets TypeScript/JavaScript fixtures so the TypeScript LSP provider can contribute real context.

Use a prepared copy for every run. Do not run tasks in place from the source pack.

## Start Here For Agents

If you are a new Codex instance and the goal is evaluation info gathering, read these first:
- [manual-testing/LOCAL_MODEL_EVAL_RUNBOOK.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/LOCAL_MODEL_EVAL_RUNBOOK.md)
- [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md)
- [manual-testing/LOCAL_MODEL_COMPATIBILITY_SUMMARY.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/LOCAL_MODEL_COMPATIBILITY_SUMMARY.md)
- [manual-testing/LOCAL_MODEL_LEADERBOARD.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/LOCAL_MODEL_LEADERBOARD.md)
- [docs/operations/LOCAL_MODEL_IMPROVEMENT_BACKLOG.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/docs/operations/LOCAL_MODEL_IMPROVEMENT_BACKLOG.md)

Use this scope:
- focus on evaluation evidence gathering, not runtime changes
- use fresh prepared task instances, fresh state dirs, and explicit trace dirs
- record provider, base URL, model, model variant, preset, stream mode, temperature, top_p, max_tokens, and seed
- stop at the first concrete divergence that explains the result
- log conclusions in [manual-testing/model-investigation-log.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/model-investigation-log.md)
- only update the compatibility summary or leaderboard when new evidence materially changes recommendations

For each completed eval slice, report:
- scenario and model
- stream vs non-stream outcome
- first exact divergence
- classification: provider bug / runtime bug / compatibility gap / pure model-choice
- artifact paths
- whether the result changes the baseline or recommendations

Each task folder (`T1`..`T5`) contains:
- `PROMPT.txt`
- the fixture files for that task

## Prepare A Fresh Runnable Copy

From the repo root:

```powershell
pwsh -File .\manual-testing\scripts\prepare_manual_control_pack.ps1 -Pack T-tests
```

To prepare one task only:

```powershell
pwsh -File .\manual-testing\scripts\prepare_manual_control_pack.ps1 -Pack T-tests -Task T3
```

The script creates a fresh runnable instance under:

```text
.tmp/manual-testing/control/T-tests/<instance-id>/
```

Each prepared instance also includes `PREPARED_INSTANCE.json` describing:
- source pack
- prepared instance ID
- optional single-task selection
- prepared timestamp

## Run A Task

Change into the prepared task directory and run LocalAgent there.

Example:

```powershell
$p = Get-Content .\PROMPT.txt -Raw
localagent --provider lmstudio --model "your-model" --allow-shell --allow-write --enable-write-tools --workdir . --prompt $p run
```

With the TypeScript provider enabled:

```powershell
$p = Get-Content .\PROMPT.txt -Raw
localagent --provider lmstudio --model "your-model" --allow-shell --allow-write --enable-write-tools --workdir . --lsp-provider typescript --prompt $p run
```

## Results

Keep source fixtures immutable.

- use `manual-testing/T-tests/results/RESULTS_TEMPLATE_T.md` as the source template
- record actual run results outside the source task folders
- historical archived run results live under `manual-testing/results/T-tests/`
- preserve exact `exit_reason`
- use `PREPARED_INSTANCE.json` to capture `prepared_instance_id`

## Hygiene Check

To verify that the source pack does not contain generated build or prior-run artifacts:

```powershell
pwsh -File .\manual-testing\scripts\verify_manual_control_pack_hygiene.ps1 -Pack T-tests
```
