# Tool Call Accuracy PR11 Spec

## Purpose
Introduce explicit runtime `agent mode` (`build` vs `plan`) to provide safer planning workflows and clearer operational intent.

## Scope

### In scope
- Add CLI/runtime `--agent-mode <build|plan>`
- Persist mode in run artifacts (`RunCliConfig`)
- Enforce read/search-only defaults in `plan` mode
- Show active mode in TUI status/header
- Add tests for mode behavior and persistence
- Preserve compatibility with existing planner `--mode` (`single|planner_worker`)

### Out of scope
- New tool additions (PR8)
- Policy default changes for search tools (PR9)
- JSON event stream output mode (PR12)

## File-level changes

- `src/cli_args.rs`
  - add `agent_mode` enum/field and clap option
  - keep `--mode` semantics unchanged (planner run mode)
- `src/agent_runtime.rs`
  - apply deterministic agent-mode defaults to capability flags
  - ensure provider/model resolution is unaffected by agent mode
- `src/chat_runtime.rs`
  - include agent mode in mode-label logic without breaking existing safe/code/web/custom labeling
- `src/chat_tui_runtime.rs` / `src/tui/render.rs`
  - display active agent mode clearly and deterministically
- `src/runtime_paths.rs`, `src/store/types.rs`
  - persist `agent_mode` in run CLI config
  - add backward-compatible serde default for older records
- `src/main_tests.rs`
  - CLI parse + validation tests
- `src/store/render.rs` (if run summary output includes mode)
  - display both planner `mode` and `agent_mode` clearly

## Mode semantics

### `build` (default)
- Existing behavior preserved for capability flags unless user sets explicit flags.

### `plan`
- Default capability baseline:
  - `enable_write_tools=false`
  - `allow_write=false`
  - `allow_shell=false`
  - `allow_shell_in_workdir=false`
- Read/search tools remain available: `list_dir`, `read_file`, `glob`, `grep`.
- Shell/write tool execution remains blocked unless explicitly overridden by user flags.
- Deterministic blocked-tool messages must reference the relevant allow flag.

## CLI and precedence contract

### Naming/non-conflict
- `--agent-mode` controls safety posture (`build|plan`).
- Existing `--mode` continues to control planner runtime shape (`single|planner_worker`).
- They are orthogonal and can be combined in any valid pair.

### Defaulting
- If `--agent-mode` is omitted, effective value is `build`.

### Precedence
- Precedence order for effective capability flags:
  1. hard-coded defaults
  2. `--agent-mode` baseline adjustments
  3. explicit CLI flags (`--allow-shell`, `--allow-write`, `--enable-write-tools`, `--allow-shell-in-workdir`)
  4. runtime `/params` edits (chat/TUI only, if applicable)
- `--agent-mode plan` must not make explicit user-enabling flags impossible.

### Compatibility with existing chat modes
- Chat labels `safe|coding|web|custom` remain derived from effective flags/MCP settings.
- `agent_mode` is additional metadata, not a replacement for chat mode.
- UI display should show both where space allows (for example `Mode: Safe · Agent: Plan`).

## Persistence contract

- `RunCliConfig` adds `agent_mode: String` with values `build|plan`.
- Backward compatibility:
  - deserializing older run records without `agent_mode` must default to `build`.
  - rendering/reporting should not fail on missing historical field.
- Artifact determinism:
  - stored `agent_mode` reflects effective CLI value after defaulting.
  - config fingerprint input must include `agent_mode` so mode changes alter fingerprint deterministically.

## Provider/model non-regression contract

- `agent_mode` does not alter:
  - provider selection
  - base URL resolution
  - model/planner_model/worker_model selection
  - transport/network settings
- Any change in these fields must come only from existing provider/model config paths.

## Test plan

### CLI/runtime tests
- `agent_mode_defaults_to_build`
- `agent_mode_plan_disables_shell_and_write_by_default`
- `agent_mode_build_preserves_current_behavior`
- `run_cli_config_persists_agent_mode`
- `planner_mode_and_agent_mode_can_be_set_together`
- `agent_mode_plan_respects_explicit_allow_shell_override`
- `agent_mode_plan_respects_explicit_allow_write_override`
- `agent_mode_plan_respects_explicit_enable_write_tools_override`
- `agent_mode_does_not_mutate_provider_or_model_resolution`
- `legacy_run_record_without_agent_mode_deserializes_as_build`
- `config_fingerprint_changes_when_agent_mode_changes`

### Integration
- `plan` mode run rejects write/shell calls without overrides
- `build` mode run unchanged from baseline
- `plan + planner_worker` path still works and persists both mode fields
- TUI/status output shows agent mode deterministically

## Determinism requirements
- Stable enum/string encoding for `agent_mode` (`build|plan` lowercase).
- Stable precedence behavior regardless of argument ordering.
- Stable persisted field values and render output for mode fields.
- No behavioral drift in non-mode config dimensions.

## Verification commands
```bash
cargo test main_tests::
cargo test agent_tests::
cargo test runtime_paths::
cargo test store::
cargo test artifact_golden -- --nocapture
cargo test
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Implementation checklist
- [ ] Add `--agent-mode` CLI option and parsing
- [ ] Preserve `--mode` planner semantics without breaking changes
- [ ] Wire agent mode into runtime capability defaults with explicit precedence
- [ ] Persist mode in run artifacts
- [ ] Add backward-compatible serde default for historical records
- [ ] Ensure config fingerprint includes `agent_mode`
- [ ] Surface mode in TUI/summary labels
- [ ] Add tests for build/plan behavior
- [ ] Run verification commands

## Exit criteria
- explicit `build|plan` mode is available and persisted without breaking planner `--mode`
- `plan` mode is safe by default
- explicit user overrides still work in `plan` mode
- tests/lints pass
