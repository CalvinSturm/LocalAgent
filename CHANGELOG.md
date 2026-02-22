# Changelog

## v0.1.2 - 2026-02-22

- Chat TUI:
  - added `/mode` command support (`safe`, `coding`, `web`, `custom`) to switch runtime mode in-session
  - added `/timeout` command (`/timeout`, `/timeout <seconds|+N|-N>`) to tune request/stream idle timeout in-session
  - added `/dismiss` command to clear active timeout notification
  - provider timeout failures now emit a `/timeout` guidance notice
  - aligned `?` keybind overlay rows for uniform formatting
  - updated header/footer presentation (mode label in header, right-justified help marker, cwd + connection status footer)
  - replaced single-line prompt row with a boxed input area
- Startup UI:
  - refreshed layout with compact `Mode` + `Provider` panes and footer controls
  - added provider details toggle (`D`) and centered footer control rows

## v0.1.0 - 2026-02-21

- Released LocalAgent v0.1.0 (local-runtime agent CLI).
- Set primary CLI command to `localagent`.
- Added `run` and `exec` command aliases for one-shot usage.
- Updated chat TUI UX:
  - pane toggles (`Ctrl+1/2/3`)
  - slash command dropdown (`/` + Up/Down + Enter)
  - keybinds dropdown (`?`)
  - `Esc` to quit
  - tools/approvals/logs hidden by default
- Added deterministic instruction profiles support:
  - `--instructions-config`
  - `--instruction-model-profile`
  - `--instruction-task-profile`
  - `--task-kind`
- Added scaffolded `instructions.yaml` via `localagent init`.
- Updated README and docs for current command patterns and behavior.
