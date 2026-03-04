# Tool Call Accuracy PR10 Spec

## Purpose
Improve approval UX in chat TUI by making pending approvals visible immediately and reducing operator confusion.

## Scope

### In scope
- Keep approval list auto-refresh behavior active
- Add visible pending-approval indicator in TUI status/header
- Add deterministic auto-open policy for approvals pane on first pending request
- Clarify help text (`Ctrl+R` as manual fallback)
- Add tests for approval visibility behavior

### Out of scope
- Policy logic changes (PR9)
- Agent mode model (`build`/`plan`) (PR11)
- JSON run format/events (PR12)

## File-level changes

- `src/chat_tui_runtime.rs`
  - Ensure approvals refresh during active and idle loops
  - Trigger immediate refresh on `require_approval` decision events
  - Define/implement deterministic pane auto-open behavior
- `src/tui/render.rs`
  - Add/confirm concise pending approvals indicator contract
- `src/tui/state.rs`
  - Expose deterministic pending-count helper and stable approval row ordering contract
- `docs/reference/CLI_REFERENCE.md`
  - Document TUI approvals controls (`Ctrl+J/K`, `Ctrl+A`, `Ctrl+X`, `Ctrl+R`)
- `manual-tui-testing/README.md`
  - Update operator guidance for approval behavior

## Behavioral contract

### Refresh triggers and staleness bound
- `UiState::refresh_approvals(...)` is called:
  - once per active-turn loop iteration
  - once per idle loop iteration
  - immediately when `EventKind::ToolDecision` with `decision=require_approval` is observed
  - immediately after approve/deny action handlers complete
  - on explicit `Ctrl+R`
- Staleness target: newly created pending approvals appear by next render tick; event-triggered path should update within the same loop iteration.

### Data source and ordering
- Approval rows are sourced from `approvals.json` via `ApprovalsStore::list()`.
- Rows are sorted deterministically by `id` ascending before render.
- Missing approvals file is treated as empty list (no hard failure in UI loop).
- Approvals pane row scope is all approval rows (`pending`, `approved`, `denied`); header indicator remains pending-only.

### Pending indicator semantics
- Header indicator shows pending-only count (rows with `status == "pending"`), not total approval row count.
- Display contract:
  - `A0` when no pending approvals
  - `A{n}` for `n > 0`
- Indicator is updated from the same refreshed snapshot used by approvals table in that frame.

### Pane auto-open policy
- Default behavior for PR10: auto-open approvals pane on first transition from pending count `0 -> >0` during a turn.
- Auto-open occurs once per transition (no repeated forced open while pending remains >0).
- Auto-open must not steal selection state in tools pane; only visibility toggle changes.

### Manual refresh and help text
- `Ctrl+R` always performs a direct approvals refresh and is safe to call repeatedly.
- `/help` command text and operator docs must explicitly list:
  - `Ctrl+R` refresh approvals
  - `Ctrl+J/K` select approval row
  - `Ctrl+A` approve selected
  - `Ctrl+X` deny selected

### Failure handling
- If approvals refresh fails, UI remains interactive and logs a single-line error (`approvals refresh failed: ...`).
- Failed refresh does not clear last known approval rows.

## Test plan

### TUI tests
- `pending_approval_auto_refresh_updates_rows`
- `require_approval_event_triggers_refresh`
- `pending_indicator_shows_pending_only_count`
- `approvals_rows_are_sorted_by_id_deterministically`
- `manual_refresh_still_works`
- `approve_and_deny_actions_trigger_immediate_refresh`
- `auto_open_approvals_on_first_pending_transition`
- `auto_open_does_not_repeat_while_pending_nonzero`
- `refresh_failure_logs_error_and_preserves_last_snapshot`
- `missing_approvals_file_is_treated_as_empty`

### Manual validation
- run shell-gated prompt in TUI and verify approval appears without `Ctrl+R`
- confirm `/help` text includes `Ctrl+R` and approvals keybindings

## Verification commands
```bash
cargo test chat_tui_runtime -- --nocapture
cargo test tui::state::
cargo test
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Implementation checklist
- [ ] Keep/confirm auto-refresh in both TUI loops
- [ ] Ensure event-triggered immediate refresh on `require_approval`
- [ ] Ensure post-action refresh after approve/deny
- [ ] Add/confirm pending-only indicator semantics
- [ ] Enforce deterministic row ordering by approval `id`
- [ ] Implement deterministic auto-open policy (`0 -> >0` transition)
- [ ] Update in-app/help/manual docs
- [ ] Add/adjust TUI tests
- [ ] Run verification commands

## Exit criteria
- approvals are visible without manual refresh in normal flow
- header pending count matches pending rows in approvals table
- operator can still use explicit refresh commands
- test suite passes
