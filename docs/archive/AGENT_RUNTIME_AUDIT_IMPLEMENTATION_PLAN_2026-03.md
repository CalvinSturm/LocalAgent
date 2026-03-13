# Agent Runtime Audit — Implementation Plan

Status: In Progress
Date: 2026-03-06

---

## Phase 1: Critical Trust Gate Fixes

### 1.1 Qualification fallback must disable `allow_write` on ToolRuntime and GateContext
- **Files:** `src/qualification.rs`, `src/run_prep.rs`, `src/agent_runtime.rs`, `src/agent_runtime/setup.rs`
- **Bug:** `qualify_or_enable_readonly_fallback()` removes write tools from the tool list (line 213) but never sets `allow_write = false`. ToolRuntime and GateContext are built from CLI args, so writes are still permitted at runtime.
- **Confirmed:** starcoder2-7b failed qualification, entered "read-only fallback", yet `write_file` still created a file on disk.
- **Fix:** When qualification fallback fires, propagate `allow_write = false` back to the caller so that both ToolRuntime and GateContext are built with writes disabled. Options:
  - Return a struct from `qualify_or_enable_readonly_fallback` that includes an `allow_write_override: Option<bool>`
  - Or accept `&mut args` and set `args.allow_write = false` directly
- [x] Implement fix — added `write_disabled_by_qualification` to `PreparedTools`, patched `launch.rs` to set `args.allow_write = false` and `gate_ctx.allow_write = false`
- [ ] Add test: qualification fallback → ToolRuntime.allow_write == false
- [ ] Add test: qualification fallback → GateContext.allow_write == false
- [ ] Manual verification: starcoder2-7b C1 no longer writes file

### 1.2 Content-extracted tool calls must respect filtered tool list
- **Files:** `src/agent_tool_exec.rs` (lines 323-398)
- **Bug:** `extract_content_tool_calls()` checks `allowed_tool_names`, but the gate hard-check in `src/gate.rs` uses `ctx.allow_write` independently. If #1.1 is fixed, this is resolved transitively. Verify that no extraction path bypasses the `allowed_tool_names` filter.
- [x] Verified: both `extract_inline_tool_call` (line 345) and `extract_wrapped_tool_calls` (line 379) check `allowed_tool_names`
- [ ] Add test: content-extracted write_file is rejected when write tools are removed from allowed set

### 1.3 `enable_write_tools` dead field on GateContext
- **Files:** `src/gate.rs` (lines 103, 109)
- **Bug:** `enable_write_tools` is stored on GateContext but never checked in any gate decision. Either use it or remove it.
- [ ] Deferred — field is used broadly in CLI/eval/chat runtime for tool catalog decisions, not a gate bypass risk

---

## Phase 2: Critical Blocking Fixes

### 2.1 Wrap `generate_streaming` in a timeout
- **Files:** `src/agent/model_io.rs` (line 30)
- **Bug:** `generate_streaming` is awaited without `tokio::time::timeout`. If the model provider hangs mid-stream, the agent loop deadlocks permanently.
- **Fix:** Wrap in `tokio::time::timeout(Duration::from_millis(http_timeout_ms), ...)` and return a timeout error on expiry.
- [x] Implemented 10-minute hard ceiling via `tokio::time::timeout` wrapping all model requests in `model_io.rs`
- [ ] Add test: simulated stream hang returns timeout error
- [x] Non-streaming path covered by same wrapper

### 2.2 Operator message infinite loop guard
- **Files:** `src/agent/runtime_completion.rs` (lines 216-219)
- **Bug:** Operator messages delivered at `FinalizeOk` cause `ContinueAgentStep` with `blocked_runtime_completion_count: 0`. If messages keep arriving, this loops forever on the same step.
- **Fix:** Add a per-step counter for operator message deliveries (e.g., `max_operator_deliveries_per_step = 3`). After the limit, finalize anyway or error.
- [x] Add operator delivery counter — `operator_delivery_count` tracked in agent loop and passed through `RuntimeCompletionAction::ContinueAgentStep`
- [x] Cap operator deliveries per step — `MAX_OPERATOR_DELIVERIES_PER_STEP = 3` in `runtime_completion.rs`; skips delivery when exceeded, allowing finalization to proceed
- [ ] Add test: operator messages exceeding cap → finalize or error

---

## Phase 3: Medium Trust Gate Fixes

### 3.1 NoGate bypasses all permission checks when trust=Off
- **Files:** `src/gate.rs` (lines 177-191), `src/agent_runtime/launch.rs` (lines 98-104)
- **Issue:** NoGate returns `Allow` for everything. When trust=Off (default), hard gates in TrustGate don't run. Only ToolRuntime exec checks protect writes/shell.
- **Fix:** Either:
  - Make NoGate still check `ctx.allow_write` and `ctx.allow_shell` before allowing
  - Or document this as intentional and ensure ToolRuntime checks are sufficient
- [x] Implemented: NoGate now checks `ctx.allow_write` and `ctx.allow_shell` before allowing, matching TrustGate hard gates
- [ ] Add test: NoGate with allow_write=false → deny write tools

### 3.2 Eval runner missing qualification fallback
- **Files:** `src/eval/runner_runtime.rs` (lines 316-391)
- **Issue:** Eval runner builds ToolRuntime with `allow_write` from config but never calls `qualify_or_enable_readonly_fallback`. A model that fails qualification in production could still write in eval.
- **Fix:** Add qualification check to eval runner, or document intentional omission.
- [x] Decision: intentional exemption. Eval runner deliberately tests model write behavior; adding qualification would prevent evaluating models that fail qualification probes. Eval runs in sandboxed fixture repos with controlled inputs.
- N/A — no implementation needed
- N/A — no test needed

---

## Phase 4: Medium Blocking Fixes

### 4.1 Corrective messages don't consume step budget
- **Files:** `src/agent.rs` (lines 932, 938)
- **Issue (original):** `ContinueStep` and `ContinueAgentStep` loop without incrementing the step counter.
- **Resolution:** Non-issue upon closer inspection. Both `continue` and `continue 'agent_steps` target the `for step in 0..self.max_steps` loop, which DOES advance the step counter. Each corrective re-attempt consumes one step from the budget.
- [x] Verified: no fix needed

### 4.2 Tool repeat guard bypassable via argument variation
- **Files:** `src/agent/tool_helpers.rs` (lines 468-510)
- **Issue:** `TOOL_REPEAT_BLOCKED` is keyed by hash of `name|canonical_args`. Model can bypass by trivially varying arguments (extra whitespace, etc.).
- **Fix:** Normalize arguments more aggressively before hashing, or also track a per-tool-name counter (not just per-args).
- [x] Add per-tool-name repeat counter — `name::` prefixed keys in `failed_repeat_counts` map, incremented at all 3 counter sites (gate_paths.rs, tool_helpers.rs schema repair, tool_helpers.rs invalid patch)
- [x] Set a global per-tool-name repeat limit — `MAX_FAILED_REPEAT_PER_TOOL_NAME = 5` checked alongside per-key limit in `handle_failed_repeat_guard`
- [ ] Add test: varying args still triggers tool-name-level block

### 4.3 Tool retry loop can exceed limits via alternating error paths
- **Files:** `src/agent/tool_helpers.rs` (lines 1009-1110)
- **Issue:** Schema repair attempts, invalid patch format attempts, and generic retry count are tracked separately. Alternating between error types can exceed the effective retry limit.
- **Fix:** Add a unified total-attempts counter that caps ALL retries for a single tool execution regardless of error type.
- [x] Add total retry cap — `MAX_TOTAL_RETRY_ATTEMPTS = 4` in `tool_retry_loop`, incremented every loop iteration regardless of error path
- [ ] Add test: alternating error types still hit total cap

---

## Phase 5: Hardening

### 5.1 Post-write verification path count limit
- **Files:** `src/agent/runtime_completion.rs` (lines 230-272)
- **Issue:** No limit on how many paths can be pending verification. Each takes up to `post_write_verify_timeout_ms` (5s default).
- [x] Add max pending verification paths cap — `MAX_POST_WRITE_VERIFY_PATHS = 10` applied via `.take()` in both `FinalizeOk` and `finalize_verified_write_step_or_error`
- [ ] Add test

### 5.2 Audit `nanbeige4.1-3b` false-pass (changed:false not caught)
- **Files:** `src/agent_impl_guard.rs`, `src/agent_tool_exec.rs` (line 229-234)
- **Issue:** nanbeige4.1-3b reported success on C2 but file was unchanged. The `changed:false` flag from apply_patch may not be checked by the implementation guard.
- [x] Added `changed: Option<bool>` field to `ToolExecutionRecord`
- [x] `tool_result_changed_flag` now read from apply_patch/write_file results and stored in record
- [x] Implementation guard now treats `changed:false` as non-effective write
- [x] Updated `runtime_noop_apply_patch_does_not_finalize_ok` test to expect PlannerError

---

## Completed Fixes

- [x] **Runtime completion retry on failed writes** — Changed `saw_write_attempt` to `saw_successful_write` in `runtime_completion.rs:283-285` so agent retries when apply_patch fails instead of immediately erroring
- [x] **Blocked count shadowing** — Removed `let blocked_runtime_completion_count = 0` shadowing in `runtime_completion.rs:214` so retry counter properly tracks across attempts
- [x] **Operator message loop guard (2.2)** — Added `operator_delivery_count` tracked across agent steps, capped at 3 deliveries per step via `MAX_OPERATOR_DELIVERIES_PER_STEP`
- [x] **Tool repeat name-level guard (4.2)** — Added `name::` prefixed keys in `failed_repeat_counts` with `MAX_FAILED_REPEAT_PER_TOOL_NAME = 5` limit
- [x] **Unified retry cap (4.3)** — Added `MAX_TOTAL_RETRY_ATTEMPTS = 4` in tool retry loop, caps all retries regardless of error path
- [x] **Post-write verification path cap (5.1)** — Added `MAX_POST_WRITE_VERIFY_PATHS = 10` via `.take()` in both verification sites
- [x] **Corrective message step budget (4.1)** — Verified non-issue: both `continue` and `continue 'agent_steps` advance the for-loop step counter
- [x] **Eval runner qualification (3.2)** — Documented as intentional exemption: evals test write behavior in sandboxed fixtures

---

## Test Matrix After All Fixes

| Scenario | Expected |
|---|---|
| Qualification fallback → write_file attempted | Denied by ToolRuntime |
| Qualification fallback → apply_patch attempted | Denied by ToolRuntime |
| NoGate + allow_write=false → write_file | Denied |
| Model hangs mid-stream | Timeout error after configured ms |
| Operator messages flood at FinalizeOk | Capped, finalize after limit |
| apply_patch returns changed:false | Not counted as effective write |
| Tool retries alternate error types | Hit unified cap |
| Tool repeat with varied args | Hit per-tool-name cap |
