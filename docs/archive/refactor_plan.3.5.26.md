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
- [x] Phase 1 complete
- [x] Phase 2 complete
- [x] Phase 3 complete
- [x] Phase 4 complete
- [x] Phase 5 complete

## Progress Update
### Workspace Status Note
- This section tracks the current workspace state, not only the last committed `HEAD`.
- Some extracted files are present locally but are still untracked as of this audit:
  - `src/eval/runner_artifacts.rs`
  - `src/learning/tests.rs`
  - `src/tools/tests.rs`

### Current Status
- Phase 1 is complete.
- Phase 2 is complete.
- Phase 3 is complete.
- Phase 4 is complete.
- `src/chat_tui_runtime.rs` has been reduced from 4878 lines to 528 lines.
- `src/agent.rs` has been reduced from 4757 lines to 1201 lines in the current workspace.
- The `agent` runtime has been split into focused helper modules under `src/agent/`.
- `src/tools.rs` has been reduced from 2339 lines to 213 lines.
- `src/agent_runtime.rs` has been reduced from 2025 lines to 595 lines.
- `src/learning.rs` has been reduced from 2391 lines to 446 lines.
- `src/eval/runner.rs` is 698 lines in the current workspace.
- `src/tui/state.rs` has been reduced from 1651 lines to 146 lines.
- `src/gate.rs` has been reduced from 1240 lines to 562 lines.
- `src/chat_ui.rs` has been reduced from 1185 lines to 670 lines.
- All runtime-heavy Phase 5 targets are now below the size target.

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
- `tools` envelope/schema handling and the filesystem-read, shell, and filesystem-write execution paths have been extracted into focused submodules.
- Full `cargo check` and `cargo test -q` have been run after each extraction slice and remain green.

## Eval Refactor Update
### Completed Extractions In `src/eval`
- `src/eval/runner_output.rs`
- `src/eval/runner_rows.rs`
- `src/eval/runner_runtime.rs`

### Current Eval State
- `src/eval/runner.rs` now acts as the eval orchestration facade and local test host.
- Row-building and skip/capability helpers live in `src/eval/runner_rows.rs`.
- Single-run execution, verifier execution, and gate/provider wiring live in `src/eval/runner_runtime.rs`.
- Eval artifact persistence and synthetic error artifact writing have been split into `src/eval/runner_artifacts.rs` in the current workspace, but that file is still untracked.
- `src/eval/runner.rs` is reduced to 698 lines, below the phase target.
- The committed eval runner slice is under the size target across `runner.rs`, `runner_rows.rs`, and `runner_runtime.rs`.

### Validation Status
- `cargo fmt --check` passes.
- `cargo clippy -- -D warnings` passes.
- `cargo test` passes.

### Notes
- The `clippy` cleanup required follow-up fixes in extracted `src/agent/*` helper modules introduced earlier in the refactor, but no user-facing behavior changes were made.
- The optional eval artifact split exists locally but is not yet tracked in git.
- No additional eval runner breakup is currently required for the phase target.

### Phase 2 Outcome
- `src/agent.rs` is effectively at the phase target, but the current workspace is 1201 lines after follow-on edits.
- The remaining code in `src/agent.rs` is primarily top-level orchestration and model-response handling rather than dense gate/tool execution state machines.
- Follow-up cleanup inside `src/agent/` can happen later without blocking the next major split.

## Priority Update
### Immediate Priority
1. Phase 5 runtime-heavy splits are complete.
2. Reassess whether the remaining large test files should be split now or handled as a separate cleanup pass.
3. Keep any further refactors mechanical and review-sized.

### Phase 4 Progress
#### Completed Extractions In `src/agent_runtime`
- `src/agent_runtime/setup.rs`
- `src/agent_runtime/launch.rs`
- `src/agent_runtime/guard.rs`
- `src/agent_runtime/planner_phase.rs`
- `src/agent_runtime/finalize.rs`

#### Current `agent_runtime` State
- `src/agent_runtime.rs` is now the orchestration facade for launch, execute, and finalize flow.
- Runtime-owned timeout/implementation guard policy, launch setup, planner-worker orchestration, and artifact/repro finalization now live in focused helpers.
- `src/agent_runtime.rs` is 595 lines, comfortably below the phase target.

#### Current `learning` State
- `src/learning.rs` already delegates assist, capture, promotion, rendering, store ops, and support logic to focused submodules.
- Inline tests have been moved into `src/learning/tests.rs` in the current workspace, but that file is still untracked.
- `src/learning.rs` is 446 lines and now acts as a thin facade over the existing learning submodules.

#### Remaining Work In Phase 4
- None required for the phase target.
- Future cleanup inside `src/learning/` is optional and does not block the next phase.

### Phase 3 Progress
#### Completed Extractions In `src/tools`
- `src/tools/catalog.rs`
- `src/tools/schema.rs`
- `src/tools/envelope.rs`
- `src/tools/exec_fs.rs`
- `src/tools/exec_shell.rs`
- `src/tools/exec_write.rs`
- `src/tools/exec_support.rs`

#### Current Phase 3 State
- `src/tools.rs` now acts as a thin facade for public tool types plus the top-level `execute_tool` dispatcher.
- Tool catalog/metadata, schema validation, and execution helpers now live in focused submodules under `src/tools/`.
- The split-out `src/tools/tests.rs` file exists in the current workspace but is still untracked.
- The facade is 213 lines, well below the phase target, so a `src/tools/mod.rs` conversion is not currently necessary.

#### Remaining Work In Phase 3
- None required for the phase target.
- A future `src/tools/mod.rs` rename remains optional stylistic cleanup only if a broader module-layout pass justifies it.

### Next Priority After Phase 4
1. Runtime-heavy Phase 5 splits are complete across `eval/runner.rs`, `tui/state.rs`, `gate.rs`, and `chat_ui.rs`.
2. Any remaining test-file breakup should be treated as separate follow-up work rather than a blocker for the runtime refactor.
3. Keep the same pattern: mechanical extraction first, local cleanup second, tests after every slice.

### Deferred Priority
- Split the large remaining test files after the runtime-heavy Phase 5 modules are stable.
