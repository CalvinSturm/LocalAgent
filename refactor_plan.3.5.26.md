# Refactor Plan (2026-03-05)

## Goal
Reduce maintenance risk from oversized modules while preserving behavior, CLI compatibility, and test outcomes.

## Non-Goals
- No feature additions.
- No behavior changes to trust/policy/tool semantics.
- No broad API redesign across the crate.

## Scope and Priorities

### P0 (Immediate)
1. `src/chat_tui_runtime.rs` (4878 lines)
2. `src/agent.rs` (4757 lines)
3. `src/tools.rs` (2339 lines)

### P1 (Next)
4. `src/agent_runtime.rs` (2025 lines)
5. `src/learning.rs` (2391 lines)
6. `src/eval/runner.rs` (1750 lines)

### P2 (Follow-up)
7. `src/tui/state.rs` (1651 lines)
8. `src/gate.rs` (1240 lines)
9. `src/chat_ui.rs` (1185 lines)
10. `src/agent_tests.rs` (4157 lines) and `tests/mcp_impl_regression.rs` (983 lines)

## Guardrails
- Keep each extracted module focused on one concern.
- Target max file size: `< 900` lines for runtime files, `< 1200` for test-heavy files.
- Keep symbols and call sites stable in first-pass extraction (move first, redesign second).
- After each phase: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`.

## Execution Strategy
Use a two-step pattern per area:
1. Mechanical extraction (minimal logic change, mostly moves + wiring).
2. Local cleanup (naming, small helper consolidation, dead code removal).

## Phase Plan

## Phase 0: Baseline and Safety Net
### Tasks
1. Capture baseline:
   - `cargo fmt --check`
   - `cargo clippy -- -D warnings`
   - `cargo test`
2. Add temporary tracking section in this file for progress and blockers.

### Exit Criteria
- Clean baseline on current `main`.

---

## Phase 1: Split `chat_tui_runtime.rs` (P0)
### Proposed module layout
- `src/chat_tui/runtime_loop.rs`
  - `drive_tui_active_turn_loop`, turn lifecycle orchestration.
- `src/chat_tui/input_dispatch.rs`
  - `handle_tui_outer_event_dispatch`, key/mouse/paste dispatch.
- `src/chat_tui/submit.rs`
  - `prepare_tui_normal_submit_state`, `build_tui_normal_submit_launch`, `handle_tui_enter_submit`.
- `src/chat_tui/slash_commands.rs`
  - `handle_tui_slash_command` and related command parsing.
- `src/chat_tui/overlay.rs`
  - learn overlay focus, state helpers, preview/submit line builders.
- `src/chat_tui/render_model.rs`
  - frame model builders and caret rendering helpers.
- Keep `src/chat_tui_runtime.rs` as thin facade/wiring entrypoint.

### Steps
1. Create `src/chat_tui/mod.rs` and move pure helpers first (`char_len`, cursor utils, paste normalization).
2. Move dispatch and submit paths.
3. Move slash-command handling.
4. Move runtime loop.
5. Move local tests into nearest module test blocks where possible.

### Exit Criteria
- `src/chat_tui_runtime.rs` reduced below 1000 lines.
- No TUI behavior regressions in existing tests.

---

## Phase 2: Split `agent.rs` (P0)
### Proposed module layout
- `src/agent/core_loop.rs`
  - `Agent::run` loop internals.
- `src/agent/model_io.rs`
  - `execute_model_request`, message compaction helpers.
- `src/agent/tool_lifecycle.rs`
  - tool detection, retries, execution glue.
- `src/agent/run_finalize.rs`
  - outcome finalization and run-end event emission.
- `src/agent/operator_queue.rs`
  - queue operator message/pending/clear logic.
- Keep top-level `src/agent.rs` for primary types and public surface.

### Steps
1. Extract pure helper functions (`sanitize_user_visible_output`, normalization helpers).
2. Extract operator queue methods.
3. Extract tool lifecycle helpers.
4. Extract model request path.
5. Extract finalize path and simplify `run`.

### Exit Criteria
- `src/agent.rs` reduced below 1200 lines.
- Tool-call and completion-flow tests remain green.

---

## Phase 3: Split `tools.rs` (P0)
### Proposed module layout
- `src/tools/defs.rs`
  - tool definitions and metadata (`builtin_tools_enabled`, side-effects mapping).
- `src/tools/schema.rs`
  - schema helpers and argument validation.
- `src/tools/exec_fs.rs`
  - list/read/glob/grep/write/apply_patch execution.
- `src/tools/exec_shell.rs`
  - shell execution, repair logic, shell error classification.
- `src/tools/envelope.rs`
  - tool result envelope/message conversion helpers.
- `src/tools/pathing.rs`
  - path resolution/scope checks and helpers.
- `src/tools/mod.rs` re-exporting existing public functions.

### Steps
1. Move envelope + schema logic first (least coupling).
2. Move FS execution handlers.
3. Move shell-specific execution/repair.
4. Keep `execute_tool` in `mod.rs` initially; optionally move after stabilization.

### Exit Criteria
- `src/tools.rs` replaced by `src/tools/mod.rs` and split modules.
- Unknown-tool, invalid-args, and shell-repair tests still pass.

---

## Phase 4: `agent_runtime.rs` and `learning.rs` (P1)
### `agent_runtime.rs` split targets
- planner-worker flow
- runtime wiring/builders
- artifact/repro/session finalization

### `learning.rs` split targets
- capture + persistence
- promotion targets (check/agents/pack)
- rendering/output formatters
- sensitivity/redaction utilities

### Exit Criteria
- Both files below 1200 lines.
- No regression in `cli_dispatch_learn` and planner-worker paths.

---

## Phase 5: `eval/runner.rs`, `tui/state.rs`, `gate.rs`, `chat_ui.rs` (P1/P2)
### Focus
- Isolate orchestration from helpers and serializers.
- Break event-handler clusters into smaller state transition modules.
- Keep existing command behavior and event schemas unchanged.

### Exit Criteria
- Each target file below 1000 lines, except where test blocks justify slightly larger.

---

## Test Refactor Plan
1. Split `src/agent_tests.rs` into:
   - `src/agent/tests/tool_flow.rs`
   - `src/agent/tests/retry_and_error.rs`
   - `src/agent/tests/completion.rs`
   - `src/agent/tests/operator_queue.rs`
2. Split `tests/mcp_impl_regression.rs` into:
   - `tests/mcp_registry_routing.rs`
   - `tests/mcp_timeout_and_cancel.rs`
   - `tests/mcp_docs_and_hash.rs`

## Acceptance Criteria (Repo-Level)
- No changes to CLI command signatures or flags.
- No changes to tool result envelope schema.
- CI-equivalent checks pass locally:
  - `cargo fmt --check`
  - `cargo clippy -- -D warnings`
  - `cargo test`
  - `cargo test --test tool_call_accuracy_ci`
- Net reduction in largest-file concentration:
  - No single runtime file > 2000 lines.
  - Top-3 largest files each reduced by at least 40%.

## Rollout Order (Concrete)
1. Phase 0 baseline.
2. Phase 1 TUI split PR.
3. Phase 2 agent split PR.
4. Phase 3 tools split PR.
5. Phase 4 runtime + learning split PR.
6. Phase 5 remaining runtime splits + test file splits.

## Risk Register
1. Hidden behavior drift during extraction.
   - Mitigation: move-only commits first; run tests after each extraction.
2. Circular dependencies from module breakup.
   - Mitigation: keep shared types in parent module; use narrow helper modules.
3. Long-lived branch merge pain.
   - Mitigation: ship as small sequential PRs by phase.

## Tracking
- [ ] Phase 0 complete
- [ ] Phase 1 complete
- [x] Phase 2 complete
- [ ] Phase 3 complete
- [ ] Phase 4 complete
- [ ] Phase 5 complete

## Progress Update
### Current Status
- Phase 2 is complete.
- `src/agent.rs` has been reduced from 4757 lines to 1198 lines.
- Phase 3 is now the active priority, starting with `src/tools.rs`.
- The `agent` runtime has been split into focused helper modules under `src/agent/`.

### Completed Extractions In `src/agent`
- `agent_types.rs`
- `budget_guard.rs`
- `gate_paths.rs`
- `mcp_drift.rs`
- `model_io.rs`
- `operator_queue.rs`
- `response_normalization.rs`
- `run_control.rs`
- `run_events.rs`
- `run_finalize.rs`
- `run_setup.rs`
- `runtime_completion.rs`
- `timeouts.rs`
- `tool_helpers.rs`

### What Has Been Stabilized
- Outcome finalization paths have been consolidated behind helper methods.
- Gate allow/deny/approval-required paths have been extracted from the main loop.
- MCP drift handling has been moved out of the core loop.
- Tool timeout, malformed-call repair, failed-repeat guard, tool-result hook processing, taint update, retry event emission, and post-write verification now have dedicated helpers.
- Full `cargo check` and `cargo test -q` have been run after each extraction slice and remain green.

### Phase 2 Outcome
- `src/agent.rs` is now below the phase target of 1200 lines.
- The remaining code in `src/agent.rs` is primarily top-level orchestration and model-response handling rather than dense gate/tool execution state machines.
- Follow-up cleanup inside `src/agent/` can happen later without blocking the next major split.

## Priority Update
### Immediate Priority
1. Start Phase 3 on `src/tools.rs`.
2. Prioritize envelope/schema extraction first, then split filesystem execution paths, then shell-specific execution.
3. Keep `execute_tool` stable during the first pass and move internals before reconsidering the top-level dispatch shape.

### Next Priority After Phase 3
1. Move to `src/agent_runtime.rs` and `src/learning.rs`.
2. Keep the same pattern: mechanical extraction first, local cleanup second, tests after every slice.

### Deferred Priority
- Phase 1 (`src/chat_tui_runtime.rs`) remains important, but the next highest-value move is to reduce `src/tools.rs` while `agent.rs` call sites are fresh and already stabilized.
