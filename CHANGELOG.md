# Changelog

All notable changes to LocalAgent are documented in this file.

This changelog is a concise release summary. For deeper release context, see `docs/release-notes/`.
Older releases may appear in `docs/release-notes/` before they are backfilled here.

## Unreleased

- No unreleased changes yet.

## v0.2.1 - 2026-02-22

### Added

- Chat TUI:
  - `/mode` command support (`safe`, `coding`, `web`, `custom`) to switch runtime mode in-session
  - `/timeout` command (`/timeout`, `/timeout <seconds|+N|-N>`) to tune request/stream idle timeout in-session
  - `/dismiss` command to clear active timeout notification
- Startup UI:
  - provider details toggle (`D`)

### Changed

- Chat TUI:
  - provider timeout failures now emit a `/timeout` guidance notice
  - aligned `?` keybind overlay rows for uniform formatting
  - updated header/footer presentation (mode label in header, right-justified help marker, cwd + connection status footer)
  - replaced single-line prompt row with a boxed input area
- Startup UI:
  - refreshed layout with compact `Mode` + `Provider` panes and footer controls
  - centered footer control rows

## v0.2.0 - 2026-02-25

### Added

- Automatic `.localagent/` initialization on first project use when missing
- Runtime modularization across execution, startup, and runtime helper seams (for maintainability and safer iteration)

### Changed

- `main.rs` responsibilities reduced through runtime module decomposition
- Docs aligned to shipped behavior (auto-init flow, instruction profile path, timeout command semantics)

### Notes

- Runtime internals were reorganized without intentional breaking CLI flag removals
- See `docs/release-notes/RELEASE_NOTES_v0.2.0.md` for the full module-level breakdown and verification summary

## v0.1.0 - 2026-02-21

### Added

- `run` and `exec` command aliases for one-shot usage
- Deterministic instruction profiles support:
  - `--instructions-config`
  - `--instruction-model-profile`
  - `--instruction-task-profile`
  - `--task-kind`
- Scaffolded `instructions.yaml` via `localagent init`

### Changed

- Set primary CLI command to `localagent`
- Updated chat TUI UX:
  - pane toggles (`Ctrl+1/2/3`)
  - slash command dropdown (`/` + Up/Down + Enter)
  - keybinds dropdown (`?`)
  - `Esc` to quit
  - tools/approvals/logs hidden by default
- Updated README and docs for current command patterns and behavior
