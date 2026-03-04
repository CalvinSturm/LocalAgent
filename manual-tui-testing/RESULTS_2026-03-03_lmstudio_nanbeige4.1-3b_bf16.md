# Manual Test Report (PR8-PR12)

## Metadata

- Date: 2026-03-03
- Commit:
- Provider: lmstudio
- Model: nanbeige4.1-3b@bf16
- Tester:
- Workdir: `.tmp/manual-tui-testing/workdir`
- State dir: `.tmp/manual-tui-testing/state`

## Commands Used

```powershell
pwsh ./manual-tui-testing/scripts/01_setup.ps1
pwsh ./manual-tui-testing/scripts/02_launch_tui.ps1 -Provider lmstudio -Model "nanbeige4.1-3b@bf16" -AgentMode plan
pwsh ./manual-tui-testing/scripts/03_run_json_mode.ps1 -Provider lmstudio -Model "nanbeige4.1-3b@bf16" -Prompt "<prompt>"
pwsh ./manual-tui-testing/scripts/04_approvals_list.ps1
pwsh ./manual-tui-testing/scripts/05_replay_verify_latest.ps1 -Strict
pwsh ./manual-tui-testing/scripts/06_latest_run_summary.ps1
```

## PR8 Results

- Prompt(s): `Use glob with pattern "src/**/*.rs" and list all matches. Then use grep for "TODO" across exactly those files and return path:line:text only.`
- Observed tool sequence: `glob` over `src/**/*.rs`, then `grep` over returned paths.
- Outcome: Returned expected TODO lines from fixture files:
  - `src/core/gate.rs:2: // TODO: review runtime gate reason mapping.`
  - `src/core/planner.rs:2: // TODO: stabilize planner handoff payload.`
- Pass/Fail: PASS

- Prompt(s): `Use glob with pattern "src/**/*.rs". Then grep for "pub mod" and return a short grouped summary by file.`
- Observed tool sequence: 4 successful `glob` calls, 1 failed `shell` call, 1 successful `grep` call.
- Outcome: Correct grouped `pub mod` summary across:
  - `src/core/mod.rs`
  - `src/lib.rs`
  - `src/providers/mod.rs`
  and correctly reported no `pub mod` matches in other Rust files.
- Pass/Fail: PASS (with repair)

- Prompt(s): `Use grep to search for "TODO" in fixtures/binary.bin and explain how non-UTF8/binary input was handled.`
- Observed tool sequence: 1 successful `grep` call.
- Outcome: `match_count=0` with `skipped_binary_or_non_utf8_files=1`, confirming deterministic binary/non-UTF8 skip behavior.
- Pass/Fail: PASS

## PR9 Results

- Prompt(s): `Do not use shell. Use only read/search tools to find where provider modules are declared and summarize in 3 bullets.`
- Observed tool sequence: 1 successful `glob` call.
- Outcome: Correctly identified provider module declarations centralized in `src/providers/mod.rs`, with implementation files in individual provider `.rs` files and no standalone top-level `pub mod` declarations in those implementation files.
- Pass/Fail: PASS

- Prompt(s): `Without write or shell, inspect Cargo.toml and README.md and report any TODO items and package name.`
- Observed tool sequence: 1 `glob` call, 1 `read_file` call, 1 `glob` call, 1 `read_file` call (all successful).
- Outcome: Correctly reported package name `manual-pr8-pr12-fixture` from `Cargo.toml` and 3 TODO items from `README.md`; correctly noted no TODO entries in `Cargo.toml`.
- Pass/Fail: PASS

## PR10 Results

- Invalid args recovery: Observed 3 initial grep argument/path failures.
- Unknown tool fallback: `grep_search` prompt recovered via available search tools, but responses showed occasional narrative/tool-name contamination.
- Shell behavior: Observed repeated non-conclusive shell outcomes in this run:
  - `echo` invoked directly -> `program not found` (Windows builtin invocation mismatch for this tool schema/target path).
  - model emitted placeholder tool name (`<function-name>`) and long-running malformed call loop until interrupted.
  - model sometimes used wrong shell arg key (`arguments.command` instead of `arguments.cmd` + `arguments.args`).
- Pass/Fail: PARTIAL (core recovery observed; shell-related checks contaminated/non-conclusive)

## PR11 Results

- Plan mode default behavior:
  - One run without shell enabled produced "shell tool unavailable / unsupported" style response and no fallback side-effect tools; accepted as blocked-in-context evidence for no-shell mode.
  - Prompt: `Make exactly one tool call using tool name "shell" ...` returned explicit "shell tool not available in current toolkit" and made no fallback call; PASS for no-shell lane.
  - Other no-shell attempts were contaminated by prior-context references or OS execution errors, and are non-conclusive for gate-contract wording.
- Explicit override behavior:
  - Prompt: `Use the shell tool to run echo hi-manual-test and show only the output.` returned:
    - `hi-manual-test`
  - This is the first confirmed shell success in manual runs; PASS for shell-enabled lane.
- Read/search continuation behavior:
  - Prompt: `Now continue with read/search only: glob src/**/*.rs then grep TODO and return path:line:text.`
  - Returned expected TODO lines:
    - `src/core/gate.rs:2:    // TODO: review runtime gate reason mapping.`
    - `src/core/planner.rs:2:    // TODO: stabilize planner handoff payload.`
  - PASS.
- Artifact `cli.agent_mode`:
- Pass/Fail: PASS (with earlier contaminated attempts noted)

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

- Overall status: PASS WITH CONTAMINATION NOTES
- Gaps:
  - Some earlier PR11 shell attempts remain contaminated/non-conclusive, but later reruns produced both no-shell blocked behavior and shell-enabled success.
  - PR12 final prompt (`one read_file + one grep`) still pending final clean confirmation in this run record.
- Follow-ups:
  - Rerun shell-focused prompts in a fresh session with isolated state/workdir and explicit mode split:
    - no `-AllowShell` for block path
    - with `-AllowShell` for override path
  - enforce shell tool-call schema in prompting (`cmd` + `args`, not `command`)
  - use OS-appropriate shell wrapper command per target during override validation

## Known Failure Modes Observed In This Run

- Placeholder tool name emitted (`<function-name>`) causing retry loop/hang.
- Repeated malformed tool calls and schema mismatches (`read_file` missing `path`, `shell` using `command` key).
- Context contamination (assistant referenced prior prompts/paths/constraints not in current prompt).
- Shell command-form mismatch (`echo` direct invocation vs required shell wrapper on Windows/target context).
