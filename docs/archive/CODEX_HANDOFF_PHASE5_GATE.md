# Codex Handoff: Phase 5 After `tui/state` Split

## Purpose

This handoff is for the next Codex instance to continue the oversized-file refactor from the current `main` state without needing prior chat history.

Immediate next target:

1. Split `src/gate.rs`

After that:

2. Split `src/chat_ui.rs`
3. Update `refactor_plan.3.5.26.md` to reflect Phase 5 progress

---

## Current Repo State

- Repo: `C:\Users\Calvin\Software Projects\LocalAgent`
- Branch: `main`
- Latest pushed commit: `f34bb79` `Split tui state event handling`

### Recent pushed commits relevant to this refactor

- `f34bb79` `Split tui state event handling`
- `5f3ed5e` `Refactor agent runtime orchestration`
- `440377e` `Update eval refactor tracking note`
- `bd6d7cf` `Refactor eval runner and restore strict lint gate`
- `4506aaa` `Extract eval runner output module`

### Important worktree warning

The worktree is still dirty with many unrelated local changes and untracked files that were not part of the last commit/push. Do not revert them unless the user explicitly asks.

Current notable unstaged/untracked items include:

- `refactor_plan.3.5.26.md`
- multiple `src/agent/*`, `src/chat_tui*`, `src/eval/*`, `src/learning/*`, and `src/tools*` files
- untracked: `AGENTS.md`
- untracked: `src/eval/runner_artifacts.rs`
- untracked: `src/learning/tests.rs`
- untracked: `src/tools/tests.rs`

Treat those as user-owned or previously in-progress changes unless you explicitly verify otherwise.

---

## What Was Just Completed

### `tui/state` split

Committed and pushed in `f34bb79`.

Files:

- `src/tui/state.rs`
- `src/tui/state/events.rs`
- `src/tui/state/support.rs`
- `src/tui/state/tests.rs`

Current shape:

- `src/tui/state.rs` is now the facade at 146 lines
- event application logic moved to `src/tui/state/events.rs`
- support/state-transition helpers moved to `src/tui/state/support.rs`
- the test block moved to `src/tui/state/tests.rs`

Validation that passed before commit:

- `cargo fmt --all`
- `cargo clippy -- -D warnings`
- `cargo test`

---

## Refactor Status Summary

### Already in good shape

- `src/eval/runner.rs` split into:
  - `src/eval/runner.rs`
  - `src/eval/runner_rows.rs`
  - `src/eval/runner_runtime.rs`
  - `src/eval/runner_artifacts.rs`
- `src/agent_runtime.rs` split into focused helpers under `src/agent_runtime/`
- `src/learning.rs` reduced to 446 lines with tests in `src/learning/tests.rs`
- `src/tui/state.rs` reduced below target via the new nested module layout

### Active remaining oversized runtime targets

- `src/gate.rs` at 1240 lines
- `src/chat_ui.rs` at 1185 lines

These are the next intended Phase 5 targets in that order.

---

## Immediate Next Task: Split `src/gate.rs`

### Recommended approach

Use the same move-first pattern that worked for `eval`, `agent_runtime`, and `tui/state`:

1. Keep `src/gate.rs` as the stable facade for `TrustGate`, public decision types, and top-level entrypoints.
2. Extract decision-evaluation helpers into a focused helper module.
3. Extract approval-store and audit-log support into a separate helper module if the seam is clean.
4. Preserve public signatures and existing decision behavior on the first pass.

### Likely seams

- decision evaluation and rule matching
- approval keying / approval lookup / approval consumption helpers
- audit append or decision recording helpers
- string/status/reason construction helpers

### Constraints

- Do not change trust semantics.
- Do not change approval behavior.
- Do not change audit schema or event meanings.
- Keep call sites stable where possible.

### Validation bar

Run after each meaningful slice:

- `cargo fmt --all`
- `cargo clippy -- -D warnings`
- `cargo test`

If the split lands cleanly, commit only the `gate`-related files, not the unrelated dirty worktree changes.

---

## Plan File State

`refactor_plan.3.5.26.md` was updated in the working tree to mark:

- Phase 4 complete
- `tui/state.rs` as the active next target
- `gate.rs` then `chat_ui.rs` as the follow-on order

But that file is currently still modified and uncommitted in the worktree. If you touch it, read the current local version first and do not overwrite unrelated edits casually.

---

## Suggested First Commands

Use these to resume safely:

```powershell
git status --short
Get-Content src\tui\state.rs -TotalCount 220
Get-Content src\gate.rs -TotalCount 260
rg -n "^(impl |pub struct |struct |enum |fn )" src\gate.rs
```

Then identify the smallest clean extraction seam and proceed with `apply_patch`.
