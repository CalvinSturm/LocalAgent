# Manual TUI Coding Test Pack

Purpose: run repeatable manual validation of tool-calling behavior for a real provider/model in TUI coding mode.

This pack is for real-provider testing (for example LM Studio), not the deterministic CI harness.

## Included Files

- `01_setup.ps1` - creates isolated work/state directories and runs `init`
- `02_launch_tui.ps1` - launches TUI chat with explicit coding-mode flags
- `03_approvals_list.ps1` - lists current approvals for the test state dir
- `04_replay_verify_latest.ps1` - verifies the newest run artifact (normal + optional strict)
- `PROMPT_SUITE.md` - fixed prompt sequence to run in the TUI
- `CHECKLIST.md` - pass/fail checklist
- `RESULTS_TEMPLATE.md` - report template for a completed test session

## Quick Start (LM Studio Example)

1. Confirm provider is reachable:

```powershell
cargo run -- doctor --provider lmstudio
```

2. Prepare isolated dirs:

```powershell
pwsh ./manual-testing/tui-coding-provider/01_setup.ps1
```

3. Launch TUI session:

```powershell
pwsh ./manual-testing/tui-coding-provider/02_launch_tui.ps1 `
  -Provider lmstudio `
  -Model "<exact-lmstudio-model-id>" `
  -AllowShell
```

4. Run prompts from `PROMPT_SUITE.md`.
   - Pending approvals should appear automatically when a tool decision requires approval.
   - `Ctrl+R` remains a manual refresh fallback.

5. Verify latest run:

```powershell
pwsh ./manual-testing/tui-coding-provider/04_replay_verify_latest.ps1 -Strict
```

6. Fill `RESULTS_TEMPLATE.md` with observed outcomes.

## Default Test Paths

- Workdir: `.tmp/manual-tui-testing/workdir`
- State dir: `.tmp/manual-tui-testing/state`

These defaults keep manual tests isolated from normal `.localagent` state.

## Seeded Fixture (Isolated)

`01_setup.ps1` seeds a minimal fixture project in the isolated workdir:

- `Cargo.toml`
- `README.md` (contains TODO lines for search/recovery prompts)
- `src/providers/mod.rs`
- `src/providers/common.rs`
- `src/providers/http.rs`
- `src/providers/mock.rs`
- `src/providers/ollama.rs`
- `src/providers/openai_compat.rs`

This ensures prompt paths like `src/providers` and `Cargo.toml` exist without touching the main repo workdir.
