# PR8-PR12 Prompt Suite (Copy/Paste)

Run these in order. Keep wording unchanged for repeatability.

Tool-call schema note (LocalAgent):
- `shell` expects `arguments.cmd` + optional `arguments.args` array.
- Do not use `arguments.command` for `shell`.

## PR8 - `glob` / `grep` built-ins

1. `Use glob with pattern "src/**/*.rs" and list all matches. Then use grep for "TODO" across exactly those files and return path:line:text only.`

2. `Use glob with pattern "src/**/*.rs". Then grep for "pub mod" and return a short grouped summary by file.`

3. `Use grep to search for "TODO" in fixtures/binary.bin and explain how non-UTF8/binary input was handled.`

## PR9 - read/search-safe behavior

4. `Do not use shell. Use only read/search tools to find where provider modules are declared and summarize in 3 bullets.`

5. `Without write or shell, inspect Cargo.toml and README.md and report any TODO items and package name.`

## PR10 - tool-call recovery behavior

6. `Read Cargo.toml using read_file. If your first tool call fails due to argument/schema issues, repair it and retry once, then continue.`

7. `Try tool grep_search for TODO in README.md. If unavailable, recover using available tools and clearly state the fallback path used.`

8. `Run shell command cmd /c echo hi-manual-test and show output.`

Optional PR10 stress prompt (use only if model is stable):

`Make one intentionally malformed read_file call (missing required path), then immediately repair it and read Cargo.toml successfully.`

## PR11 - `agent_mode` behavior

9. `In this mode, attempt shell command cmd /c echo should-be-blocked and report whether it was blocked and why.`

10. `Now continue with read/search only: glob src/**/*.rs then grep TODO and return path:line:text.`

11. `If shell is enabled explicitly, run shell command cmd /c echo pr11-override-ok and show output.`

Windows note:
- `echo` is a shell builtin, not an executable. Use `cmd /c echo ...` (or `pwsh -Command ...`) for shell-tool tests.

## PR12 - JSON projection behavior (non-TUI helper)

Use `scripts/03_run_json_mode.ps1` with these prompts:

12. `Use glob with pattern "src/**/*.rs" and then grep for "TODO". End with a one-line summary.`

13. `Attempt one read_file call and one grep call, then finish.`

Pass checks for PR12:
- stdout is JSONL only
- has monotonic `sequence`
- has exactly one terminal `type: "run_finished"`
