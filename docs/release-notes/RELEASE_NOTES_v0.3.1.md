# LocalAgent v0.3.1 Release Notes

Release date: 2026-02-28

## Highlights

- Simplified Learn Overlay flow for faster capture/review/promote operation.
- Improved TUI input ergonomics with caret visibility and arrow-key cursor navigation.
- Aligned docs to shipped `/learn` behavior with explicit workflow and output contracts.

## Included Changes

### Learn Overlay UX

- Removed the write-arm step from overlay submit flow.
- Capture now saves directly on `Enter`.
- Promote now publishes directly on `Enter` when required fields are present.
- Promote controls remain intentionally beginner-focused (`target` + `force`).
- Advanced promote options remain available through typed `/learn promote ...` or CLI:
  - `--check-run`
  - `--replay-verify`
  - `--replay-verify-run-id <RUN_ID>`
  - `--replay-verify-strict`

### TUI Reliability and Input Handling

- Added blinking caret for active text fields.
- Added arrow-key cursor navigation in overlay text inputs.
- Prevented Learn capture row-index panic.
- Removed overlay letter shortcuts that interfered with typing.
- Improved active-run slash handling and pane separation behavior.
- Bounded/normalized overlay paste and improved long-text wrapping.

### Documentation and Reference Alignment

- Added and aligned `/learn` reference docs:
  - `docs/reference/LEARN_WORKFLOW_REFERENCE.md`
  - `docs/reference/LEARN_OUTPUT_CONTRACT.md`
- Updated README and CLI reference for current Learn Overlay behavior.
- Reorganized docs and archived historical scope documents under `docs/archive/`.

## Compatibility Notes

- No intentional breaking CLI changes in this patch release.
- Learn artifacts/events remain additive and deterministic; consumers should continue to ignore unknown fields for forward compatibility.
