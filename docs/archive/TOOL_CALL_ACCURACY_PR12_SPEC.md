# Tool Call Accuracy PR12 Spec

## Purpose
Add a structured non-interactive run output mode for automation/CI, with deterministic event records that are easy to parse.

## Scope

### In scope
- Add run output format option (`human|json`) for non-TUI runs
- Emit deterministic structured event records in JSON mode via a stable projection schema
- Include key run/tool lifecycle events and terminal outcome
- Add integration tests for JSON shape stability

### Out of scope
- New tool capabilities (PR8)
- policy and mode semantics (PR9/PR11)
- changing internal `EventKind` taxonomy

## File-level changes

- `src/cli_args.rs`
  - add run output format option
- `src/cli_dispatch.rs`
  - route output mode behavior and validate incompatible flag combinations
- `src/agent_runtime.rs`
  - emit projected run events to stdout in JSON mode
- `src/events.rs`
  - add projection helper/type for stable external JSON contract (separate from internal event struct)
- `src/runtime_wiring.rs`
  - wire JSON stdout sink selection for non-interactive runs
- `src/runtime_paths.rs`, `src/store/types.rs`
  - persist `output_mode` in run artifacts for replay/debug visibility
- `docs/reference/CLI_REFERENCE.md`
  - document JSON mode usage and schema expectations

## CLI contract

- New run flag:
  - `--output <human|json>` (default `human`)
- Scope:
  - Applies to `run` command in non-TUI mode.
  - If `--tui` is enabled, `--output json` is rejected with deterministic error text.
- Interactions:
  - `--output json` implies machine-readable stdout lines; human prose/final text is suppressed on stdout.
  - `--events <path>` remains independent and still writes internal event JSONL file.
  - `--stream` does not print raw token deltas in JSON mode; deltas are emitted only as projected JSON events.

## Persistence and fingerprint contract

- `RunCliConfig` adds `output_mode: "human|json"`.
- `output_mode` is persisted for observability/replay context.
- `output_mode` is excluded from behavior/config fingerprint inputs (presentation-only, no model/tool behavior impact).

## JSON output contract (locked)

Each stdout line in JSON mode is one object with this envelope:

```json
{
  "schema_version": "openagent.run_event.v1",
  "sequence": 1,
  "ts": "2026-03-03T20:11:12Z",
  "run_id": "r_123",
  "step": 0,
  "type": "run_started",
  "data": {}
}
```

### Envelope fields
- `schema_version`:
  - literal `openagent.run_event.v1`.
- `sequence`:
  - 1-based monotonically increasing per process invocation.
- `ts`:
  - RFC3339 UTC timestamp string.
- `run_id`:
  - stable run identifier for the invocation.
- `step`:
  - numeric step counter from runtime event context.
- `type`:
  - projected external event type (see list below).
- `data`:
  - event-specific payload object.

### Projected external event types
- `run_started`
- `step_started`
- `tool_call_detected`
- `tool_decision`
- `tool_exec_started`
- `tool_exec_finished`
- `tool_retry`
- `step_blocked`
- `provider_retry`
- `provider_error`
- `run_finished` (terminal; exactly once)

Internal events without external value in CI pipelines may be dropped from projection.

### Required `run_finished` payload
`type = "run_finished"` must include:
- `exit_reason: string`
- `ok: bool`
- `final_output: string` (may be empty)
- `error: string|null`

### Field stability and evolution
- Unknown fields may be added in `data` only (additive evolution).
- Envelope field names and meanings are frozen for `v1`.
- Breaking changes require new schema version (`openagent.run_event.v2`).

### Optional-field encoding rule
- Envelope fields are always present.
- For optional projected `data` fields:
  - use `null` when value is unavailable
  - do not omit declared optional keys for a given `type`.

## Ordering and delivery semantics

- Events are emitted in runtime occurrence order.
- `sequence` strictly increases by 1 for each emitted line.
- Exactly one terminal `run_finished` record is emitted on all non-crash exit paths.
- If a fatal pre-run error prevents `run_id` allocation, emit one `run_finished` with:
  - `run_id: ""`
  - `sequence: 1`
  - `ok: false`
  - deterministic `exit_reason`/`error`.
- Boundary note:
  - JSON run-event emission is guaranteed once run execution starts.
  - CLI parse/argument errors before run execution are out of scope for `openagent.run_event.v1` and remain standard CLI stderr failures.

## Output hygiene contract

- JSON mode stdout is JSONL-only (one JSON object per line, no prefixes/suffixes).
- No ANSI color/control sequences in JSON mode stdout.
- Non-fatal logging/warnings go to stderr, never stdout.
- Serialized JSON must be UTF-8 valid.

## Projection mapping contract

- Mapping from internal `EventKind` to external `type` is deterministic and table-driven.
- Unmapped internal kinds are ignored unless explicitly added to projection table.
- `tool_decision` payload must include (when present internally):
  - `decision`
  - `reason`
  - `source`
  - `tool`
- `tool_exec_finished` payload must include:
  - `tool`
  - `ok`
  - `content_preview` (optional bounded preview)

## Size and truncation semantics

- Any potentially large string field in projected `data` is bounded:
  - `content_preview_max_bytes = 4096`
- If truncation occurs:
  - include `truncated: true`
  - include `original_bytes` when available
- Truncation is byte-based on UTF-8 boundary-safe slicing.

## Determinism requirements
- Stable envelope field names and semantics for `v1`.
- Stable projected type mapping and sequence generation.
- Stable handling of missing optional internal fields (`null`, never omitted for declared keys).
- No random IDs beyond existing run/tool IDs.
- Additive changes only within `data`; version bump for envelope or semantic breaks.

## Test plan

### Integration tests
- `run_json_mode_emits_parseable_event_stream`
- `run_json_mode_includes_final_record`
- `run_json_mode_is_stable_for_fixed_mock_inputs`
- `run_json_mode_stdout_contains_only_json_lines`
- `run_json_mode_emits_monotonic_sequence_numbers`
- `run_json_mode_emits_exactly_one_terminal_run_finished`
- `run_json_mode_suppresses_human_final_output_print`
- `run_json_mode_rejects_tui_with_clear_error`
- `run_json_projection_ignores_unmapped_internal_event_kinds`
- `run_json_projection_truncates_large_fields_with_metadata`

### Compatibility
- default output mode remains unchanged
- existing `--events` file behavior remains unchanged

## Verification commands
```bash
cargo test main_tests::
cargo test agent_tests::
cargo test runtime_wiring::
cargo test --test tool_call_accuracy_ci
cargo test
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Implementation checklist
- [ ] Add run output format CLI option
- [ ] Add deterministic projection schema (`openagent.run_event.v1`)
- [ ] Implement JSON stdout sink for projected events
- [ ] Enforce JSON-only stdout hygiene in JSON mode
- [ ] Persist `output_mode` in `RunCliConfig`
- [ ] Exclude `output_mode` from behavior/config fingerprint inputs
- [ ] Include exactly one terminal `run_finished` record
- [ ] Define and test mapping table from internal events to external types
- [ ] Add truncation metadata for bounded large fields
- [ ] Add integration tests for shape/stability
- [ ] Update CLI reference docs
- [ ] Run verification commands

## Exit criteria
- non-interactive runs can be consumed reliably by scripts/CI
- default human-readable output remains intact
- JSON mode contract is versioned and deterministic
- tests/lints pass
