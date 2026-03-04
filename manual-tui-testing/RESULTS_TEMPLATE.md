# Manual Test Report (PR8-PR12)

## Metadata

- Date:
- Commit:
- Provider:
- Model:
- Tester:
- Workdir: `.tmp/manual-tui-testing/workdir`
- State dir: `.tmp/manual-tui-testing/state`

## Commands Used

```powershell
pwsh ./manual-tui-testing/scripts/01_setup.ps1
pwsh ./manual-tui-testing/scripts/02_launch_tui.ps1 -Provider <provider> -Model "<model>" -AgentMode plan
pwsh ./manual-tui-testing/scripts/03_run_json_mode.ps1 -Provider <provider> -Model "<model>" -Prompt "<prompt>"
pwsh ./manual-tui-testing/scripts/04_approvals_list.ps1
pwsh ./manual-tui-testing/scripts/05_replay_verify_latest.ps1 -Strict
pwsh ./manual-tui-testing/scripts/06_latest_run_summary.ps1
```

## PR8 Results

- Prompt(s):
- Observed tool sequence:
- Outcome:
- Pass/Fail:

## PR9 Results

- Prompt(s):
- Observed tool sequence:
- Outcome:
- Pass/Fail:

## PR10 Results

- Invalid args recovery:
- Unknown tool fallback:
- Shell behavior:
- Pass/Fail:

## PR11 Results

- Plan mode default behavior:
- Explicit override behavior:
- Artifact `cli.agent_mode`:
- Pass/Fail:

## PR12 Results

- JSONL stdout only:
- Sequence monotonic:
- Terminal `run_finished` exactly once:
- TUI incompatibility check:
- Artifact `cli.output_mode`:
- Pass/Fail:

## Evidence

- Latest run id:
- Replay verify:
- Replay verify --strict:
- approvals summary:
- audit highlights:

## Summary

- Overall status: PASS / FAIL
- Gaps:
- Follow-ups:

## Known Failure Modes Observed (if any)

- Placeholder tool name emitted (for example `running <function-name>`)
- Repeated malformed tool calls / schema violations
- Shell hard-gate denial (`shell requires --allow-shell`) when shell was not enabled
- Windows shell command-form issue (`echo` without `cmd /c` or `pwsh -Command`)
- Context contamination from earlier prompts/runs
