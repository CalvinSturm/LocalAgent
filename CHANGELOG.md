# Changelog

All notable changes to LocalAgent are documented in this file.

This changelog is a concise release summary. For deeper release context, see `docs/release-notes/`.
Older releases may appear in `docs/release-notes/` before they are backfilled here.

## Unreleased

- None yet.

## v0.5.0 - 2026-03-14

### Added

- `common_coding_ux` benchmark expansion on the eval path, including broader task coverage, frozen baseline artifacts, helper runbooks, and focused comparison harnesses for closeout and validation-discipline follow-up work.
- TypeScript LSP context provider support and supporting server/runtime harness coverage for tool-assisted coding workflows.

### Changed

- Runtime completion policy and coding-task contracts are stricter and more explicit:
  - required validation commands and exact final-answer contracts now flow through clearer runtime-owned paths
  - validation-only shell handoff is more resilient when the model emits prose-only or wrong-tool turns after a successful edit
  - inline `reply with exactly \`...\`` closeout contracts are inferred more reliably in one-shot `run` prompts
- One-shot `run` / `exec` behavior now defaults to isolated ephemeral state plus sessionless execution unless state/session settings are explicitly requested.
- Common coding UX benchmark work is now treated as the active decision surface for later local-model/runtime improvement branches.

### Fixed

- Recovered malformed single wrapped tool calls more consistently.
- Tightened tool-repair helpers and clippy cleanliness around runtime/tool protocol paths.
- Sanitized stray `</think>` closers from visible output.

### Docs

- Aligned release metadata, release-notes indexing, and README release links for the `v0.5.0` release cut.
- Updated coding UX benchmark documentation to preserve current baseline and follow-up evidence as explicit artifacts instead of ad hoc notes.

### Schema Notes

- No intentional breaking schema ID changes.
- Runtime/event/task metadata changes in this release remain additive; downstream consumers should continue ignoring unknown fields for forward compatibility.

## v0.4.0 - 2026-03-03

### Added

- Tool call accuracy expansion through PR12, including stricter runtime/tool protocol handling and deterministic behavior improvements.
- Native read-only builtins:
  - `glob` for scoped file discovery
  - `grep` for bounded regex search over text files
- Manual TUI testing pack and PR8-PR12 planning/spec docs (archived under `docs/archive/`).

### Changed

- TUI reasoning UX:
  - moved from inline/collapsible thinking text to a dedicated right-side Reasoning pane
  - global `Ctrl+4` toggle for pane visibility (with terminal control-character fallback)
  - pane is hidden on banner and auto-shows after first prompt submission
- Shell execution flow:
  - improved shell error classification and compatibility handling
  - one-shot auto-repair for common command-not-found cases
- Approval workflow UX:
  - improved auto-refresh behavior, pending indicators, and pane auto-open behavior

### Docs

- Archived historical Tool Call Accuracy specs (`PR2`-`PR12`) under `docs/archive/`.
- Aligned README, docs index, and release notes with current `Ctrl+4` reasoning-pane UX.

### Schema Notes

- No intentional breaking schema ID changes.
- Run artifact and event payload updates in this release remain additive; consumers should continue ignoring unknown fields for forward compatibility.

## v0.3.1 - 2026-02-28

### Added

- Learn documentation set for operator clarity:
  - `/learn` workflow reference (`docs/reference/LEARN_WORKFLOW_REFERENCE.md`)
  - `/learn` output contract (`docs/reference/LEARN_OUTPUT_CONTRACT.md`)
- TUI input ergonomics:
  - Blinking caret in text fields
  - Arrow-key cursor navigation in active text inputs

### Changed

- Learn Overlay UX simplified for beginner flow:
  - Removed write-arm step from overlay flow
  - `Enter` now executes Capture save and Promote publish directly when required fields are valid
  - Promote tab remains intentionally focused on `target` + `force`
- Clarified command boundary:
  - Advanced promote flags (`--check-run`, `--replay-verify*`) remain typed-only via `/learn promote ...` or CLI
- Learn overlay render/status messaging streamlined:
  - More form-focused layout
  - Concise inline next-step/status text

### Fixed

- TUI learn overlay stability and input handling:
  - Prevented capture-row index panic
  - Removed letter shortcuts that blocked normal typing in overlay fields
  - Improved slash handling while active runs are in progress
  - Bounded and normalized overlay paste behavior
  - Improved category guidance and wrapped long overlay text
  - Added visible in-overlay progress feedback for assist-enhanced capture (`Enhancing summary...`) and aligned capture title/options presentation

### Docs

- Reorganized docs tree and archived historical scope docs under `docs/archive/`.
- Updated README and CLI reference to align with current `/learn` overlay behavior and command semantics.

## v0.3.0 - 2026-02-27

### Added

- Learning store foundation commands: `learn capture`, `learn list`, `learn show <id>`, and `learn archive <id>`
- Learning promotion flows:
  - `learn promote <id> --to check --slug <slug>`
  - `learn promote <id> --to pack --pack-id <pack_id>`
  - `learn promote <id> --to agents`
- Assisted capture mode with provenance metadata (`learn capture --assist [--write]`)
- Promote+validate one-shot chaining (`--check-run`, replay verify options)
- Chat TUI `/learn` command surface (Phase A + Phase B)

### Changed

- Promotion writes remain deterministic/idempotent with managed-section insertion for `AGENTS.md` and packs
- Learning event coverage expanded (`openagent.learning_captured.v1`, `openagent.learning_promoted.v1`)
- Runtime cancellation handling fixed so chat/TUI runs no longer exit immediately as `cancelled` due to dropped cancel sender

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

### Schema Notes

- Compatibility: additive-only schema changes across run artifacts and event streams in later `v0.2.1` patch work.
- New fields/events were added for operator-visible metadata (for example MCP/tooling diagnostics, guidance/repomap/profile/pack metadata, queue events, and Docker config summaries) without removing prior keys.
- Existing consumers should tolerate these additions if they ignore unknown fields; no intentional breaking schema ID replacements were introduced in this line.

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
