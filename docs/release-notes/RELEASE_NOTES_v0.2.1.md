# LocalAgent v0.2.1 Release Notes

Release date: 2026-02-22

## Highlights

- Chat TUI quality-of-life improvements, including new slash commands for mode and timeout control
- Startup UI refresh with improved provider visibility and controls
- Better timeout guidance surfaced in chat when providers are slow

## Included Changes

### Chat TUI

- Added `/mode` command support (`safe`, `coding`, `web`, `custom`) to switch runtime mode in-session
- Added `/timeout` command (`/timeout`, `/timeout <seconds|+N|-N>`) to tune request/stream idle timeout in-session
- Added `/dismiss` command to clear active timeout notification
- Provider timeout failures now emit a `/timeout` guidance notice
- Aligned `?` keybind overlay rows for uniform formatting
- Updated header/footer presentation (mode label in header, right-justified help marker, cwd + connection status footer)
- Replaced single-line prompt row with a boxed input area

### Startup UI

- Refreshed layout with compact `Mode` + `Provider` panes and footer controls
- Added provider details toggle (`D`)
- Centered footer control rows

## Schema Notes

- Compatibility impact: additive-only runtime schema evolution in the `v0.2.1` line.
- Run artifact records and event streams gained new optional fields/events used for reliability and operator-control features (for example MCP metadata/drift reporting, project guidance/repomap/profile/pack metadata, queue events, and Docker config summaries).
- No intentional breaking schema ID replacement was introduced; consumers should continue to parse successfully if they ignore unknown fields.
