# Manual Eval D Pack

This pack mirrors the `C-tests` format, but the source pack under `manual-testing/D-tests` is not the default execution target.

Use a prepared copy for every run. Do not run tasks in place from the source pack.

Each task folder (`D1`..`D5`) contains:
- `PROMPT.txt`
- the fixture files for that task

## Prepare A Fresh Runnable Copy

From the repo root:

```powershell
pwsh -File .\manual-testing\scripts\prepare_manual_control_pack.ps1 -Pack D-tests
```

To prepare one task only:

```powershell
pwsh -File .\manual-testing\scripts\prepare_manual_control_pack.ps1 -Pack D-tests -Task D3
```

The script creates a fresh runnable instance under:

```text
.tmp/manual-testing/control/D-tests/<instance-id>/
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

For `zai-org/glm-4.6v-flash` in LM Studio:

```powershell
$p = Get-Content .\PROMPT.txt -Raw
localagent --provider lmstudio --model "zai-org/glm-4.6v-flash" --allow-shell --allow-write --enable-write-tools --workdir . --prompt $p run
```

## Results

Keep source fixtures immutable.

- use `manual-testing/D-tests/results/RESULTS_TEMPLATE_D.md` as the source template
- record actual run results outside the source task folders
- preserve exact `exit_reason`
- use `PREPARED_INSTANCE.json` to capture `prepared_instance_id`

To append one row to a results file:

```powershell
pwsh -File .\manual-testing\scripts\append_manual_control_result.ps1 `
  -ResultsFile .\manual-testing\D-tests\results\RESULTS_D_RUN_1.md `
  -Date 2026-03-08 `
  -Provider lmstudio `
  -Model "example-model" `
  -Task D1 `
  -RunId "run-123" `
  -ArtifactPath ".localagent/runs/run-123.json" `
  -PreparedInstanceId "20260308-070130-943-88fa53" `
  -SessionMode ephemeral `
  -ExitReason ok `
  -ReasonCode n/a `
  -FailureClass n/a `
  -TaskResult fail `
  -FinalAnswer "verified=yes file=notes/status.txt bytes=5" `
  -FileResult incorrect `
  -TestResult not_applicable `
  -PathPreexisted no `
  -ToolCallsTotal 1 `
  -BytesWritten 5 `
  -ExecutionQuality clean `
  -FailureLayer model `
  -Notes "Created the file but content or byte count was wrong."
```

## Hygiene Check

To verify that the source pack does not contain generated build or prior-run artifacts:

```powershell
pwsh -File .\manual-testing\scripts\verify_manual_control_pack_hygiene.ps1 -Pack D-tests
```
