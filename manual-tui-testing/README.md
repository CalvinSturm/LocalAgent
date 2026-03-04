# Manual PR8-PR12 Test Pack

Purpose: isolated, repeatable manual validation for PR8-PR12:
- PR8: `glob` / `grep` built-ins and search semantics
- PR9: read/search-only safe posture behavior
- PR10: tool-call recovery behavior (invalid args, unknown tool fallback)
- PR11: `agent_mode` (`build|plan`) semantics and overrides
- PR12: `--output json` projected event stream contract

## Directory Layout

- `scripts/01_setup.ps1` - create isolated work/state dirs and seed fixture project
- `scripts/02_launch_tui.ps1` - launch TUI chat with controlled flags
- `scripts/03_run_json_mode.ps1` - non-TUI JSON-mode run helper for PR12
- `scripts/04_approvals_list.ps1` - list current approvals in isolated state
- `scripts/05_replay_verify_latest.ps1` - replay verify latest run
- `scripts/06_latest_run_summary.ps1` - print latest run artifact summary fields
- `PROMPTS_PR8_PR12.md` - copy-paste prompts grouped by PR
- `CHECKLIST_PR8_PR12.md` - pass/fail checklist
- `RESULTS_TEMPLATE.md` - report template

## Default Isolated Paths

- Workdir: `.tmp/manual-tui-testing/workdir`
- State dir: `.tmp/manual-tui-testing/state`

This keeps manual tests isolated from normal `.localagent` usage.

## Avoid State Contamination

Persistent `.localagent` session/state can leak prior context into new prompts (wrong constraints, stale paths, repeated malformed retries).

For clean manual runs, always use:

- isolated `--state-dir` (this pack defaults to `.tmp/manual-tui-testing/state`)
- isolated `--workdir` (this pack defaults to `.tmp/manual-tui-testing/workdir`)
- `--no-session` when launching ad-hoc commands outside these scripts

If behavior looks contaminated, stop and reset:

- remove `.tmp/manual-tui-testing/state`
- remove `.tmp/manual-tui-testing/workdir/.localagent` if present
- restart in a fresh session and rerun only affected prompts

## Quick Start

1. Setup fixture:

```powershell
pwsh ./manual-tui-testing/scripts/01_setup.ps1
```

2. Launch TUI for PR8-PR11 manual flows:

```powershell
pwsh ./manual-tui-testing/scripts/02_launch_tui.ps1 -Provider lmstudio -Model "<model-id>" -AgentMode plan
```

For shell-prompt checks (PR10/PR11), launch with shell explicitly enabled:

```powershell
pwsh ./manual-tui-testing/scripts/02_launch_tui.ps1 -Provider lmstudio -Model "<model-id>" -AgentMode plan -AllowShell
```

3. Run PR12 JSON event checks (non-TUI):

```powershell
pwsh ./manual-tui-testing/scripts/03_run_json_mode.ps1 -Provider lmstudio -Model "<model-id>" -Prompt "Use glob with pattern 'src/**/*.rs' and then grep TODO."
```

4. Validate approvals / replay / artifact summary:

```powershell
pwsh ./manual-tui-testing/scripts/04_approvals_list.ps1
pwsh ./manual-tui-testing/scripts/05_replay_verify_latest.ps1 -Strict
pwsh ./manual-tui-testing/scripts/06_latest_run_summary.ps1
```

5. Fill `RESULTS_TEMPLATE.md`.

## Shell Test Notes (Windows)

- If `-AllowShell` is not set, shell calls are denied by hard gate (`shell requires --allow-shell`) before trust approvals.
- With `-AllowShell` set, trust policy can require approval and should show in the approvals panel.
- Use `cmd /c echo hi-manual-test` for shell tests; plain `echo` is a shell builtin and can fail as "program not found" when executed directly.

## Known Failure Modes

- Placeholder tool name emitted (for example `running <function-name>`): treat as protocol failure, interrupt, and restart session.
- Repeated malformed tool calls (for example missing required args like `path` on `read_file`): mark failed for that prompt and move on.
- Shell denied with `shell requires --allow-shell`: expected when run was launched without `-AllowShell`; this is not a runtime bug.
- Shell `program not found` for `echo`: command form issue on Windows; use `cmd /c echo ...` or `pwsh -Command ...`.
- Contaminated context (assistant references prior runs/paths/tools not used in current prompt): mark run as contaminated and rerun affected prompts in a fresh session.

## Tool-Call Schema Guardrail

Use native tool JSON with exact argument keys. For `shell`, the schema is:

```json
{
  "name": "shell",
  "arguments": {
    "cmd": "cmd",
    "args": ["/c", "echo", "hi-manual-test"]
  }
}
```

Do not use `arguments.command` for LocalAgent `shell`; it will not map to the expected runtime fields.
