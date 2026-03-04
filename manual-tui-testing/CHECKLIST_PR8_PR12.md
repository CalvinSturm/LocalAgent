# Manual Checklist (PR8-PR12)

## Session Info

- Date:
- Tester:
- Commit:
- Provider:
- Model:
- Workdir:
- State dir:

## Setup

- [ ] `pwsh ./manual-tui-testing/scripts/01_setup.ps1` succeeded
- [ ] Isolated fixture files exist under `.tmp/manual-tui-testing/workdir`
- [ ] Provider/model is reachable

## PR8

- [ ] `glob` returned recursive `src/**/*.rs` coverage (not a single subfolder only)
- [ ] `grep` returned deterministic `path:line:text` output
- [ ] binary/non-UTF8 grep behavior observed and recorded

## PR9

- [ ] read/search-only prompts completed without shell/write usage
- [ ] summaries consistent with fixture contents

## PR10

- [ ] invalid-args flow showed error then successful repair
- [ ] unknown-tool flow recovered via fallback tools
- [ ] shell test result recorded (allowed/blocked and reason)

## PR11

- [ ] `--agent-mode plan` baseline blocks write/shell unless explicitly enabled
- [ ] explicit allow override behavior validated
- [ ] latest run artifact shows `cli.agent_mode` as expected

## PR12

- [ ] `--output json` emitted JSONL-only stdout
- [ ] monotonic `sequence` confirmed
- [ ] exactly one `run_finished` record confirmed
- [ ] `--output json` + `--tui` rejection checked
- [ ] latest run artifact shows `cli.output_mode` as expected

## Artifacts

- [ ] `approvals list` reviewed
- [ ] `replay verify latest` reviewed
- [ ] `replay verify latest --strict` reviewed
- [ ] `scripts/06_latest_run_summary.ps1` output saved

## Result

- Overall: PASS / FAIL
- Notes:
