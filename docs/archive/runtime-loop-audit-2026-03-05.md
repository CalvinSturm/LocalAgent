# Runtime Loop Audit (2026-03-05)

Scope: `src/agent.rs`, `src/agent_runtime.rs`, `src/agent_impl_guard.rs`, `src/providers/http.rs`, `src/eval/runner.rs`, `src/tui/state.rs`.

Audit target principle:

`model proposes actions, runtime owns control flow and stopping`

## Executive Summary

Current runtime is **mostly aligned** with the principle.

Notable improvements now present:
- one-tool-per-step is runtime-enforced
- runtime state machine governs execute/continue/finalize decisions
- runtime-owned post-write verification runs internally (timeboxed), without returning control to model
- deterministic stop exits emit `RunEnd` in most critical paths (ok/error/budget/max_steps/approval_required/cancelled)

Remaining misalignment is primarily in **completion coupling** and one **lifecycle consistency edge case**.

## Aligned Areas

1. Runtime enforces one tool call per assistant step.
- Contract text: `src/agent.rs:422`, `src/agent.rs:426`
- Hard guard on multiple calls: `src/agent.rs:1294` to `src/agent.rs:1316`
- Tool execution loop remains but is effectively single-call because of guard: `src/agent.rs:2117`

2. Runtime owns per-step control flow via explicit decision state machine.
- Decision function: `src/agent.rs:109`
- Finalize predicate wrapper: `src/agent.rs:127`
- Branch handling (`ExecuteTools` / `ContinuePendingPlan` / `Finalize`): `src/agent.rs:1791`

3. Runtime-owned post-write verification is internal and bounded.
- Internal verification start: `src/agent.rs:1893`
- Timeout-bounded read-back: `src/agent.rs:1911` to `src/agent.rs:1912`
- Deterministic timeout fail path + `RunEnd(planner_error)`: `src/agent.rs:1957` to `src/agent.rs:1960`
- Verification failure fail path + `RunEnd(planner_error)`: `src/agent.rs:2018` to `src/agent.rs:2021`
- Verification lifecycle events emitted (`PostWriteVerifyStart/End`): `src/agent.rs:1899`, `src/agent.rs:1923`, `src/agent.rs:1984`

4. Runtime enforces bounded tool execution with explicit timeout handling.
- Effective timeout config: `src/agent.rs:334`, `src/agent.rs:342`
- Timeout around tool execution: `src/agent.rs:3250` to `src/agent.rs:3251`, `src/agent.rs:3687` to `src/agent.rs:3688`

5. Runtime has deterministic hard exits for budget and max steps.
- Wall-time budget check: `src/agent.rs:604` to `src/agent.rs:629`
- Max-steps terminal exit: `src/agent.rs:4426` to `src/agent.rs:4430`

6. Runtime/orchestrator owns cancellation and emits cancellation `RunEnd`.
- Cancellation select ownership: `src/agent_runtime.rs:814` to `src/agent_runtime.rs:819`
- Cancelled `RunEnd` emission: `src/agent_runtime.rs:924` to `src/agent_runtime.rs:927`
- Replan-resume cancellation ownership: `src/agent_runtime.rs:1173` to `src/agent_runtime.rs:1178`

## Findings (Remaining Gaps)

### F1 (Medium): Finalize candidacy still originates from model returning no tool call.

Where:
- `src/agent.rs:1337` to `src/agent.rs:1338`
- `src/agent.rs:1791`
- `src/agent.rs:1800` onward (Finalize branch)

Details:
- Runtime state machine is present, but `has_actionable_tool_calls` is derived directly from model response shape.
- In non-plan/non-guard contexts, a "no tool call" response is still the primary trigger to enter finalize.

Impact:
- Runtime owns enforcement once in finalize, but finalize candidacy is still partially model-signaled.

### F2 (Medium): Implementation integrity guard is conditional, not universally active.

Where:
- Guard injection rule: `src/agent_runtime.rs:137`
- Guard activation point: `src/agent_runtime.rs:778`
- Runtime check gate in agent loop: `src/agent.rs:799` to `src/agent.rs:800`

Details:
- Integrity checks (placeholder detection, read-before-write enforcement, post-write verify) depend on injected internal flag.
- If task classification does not activate guard, finalize can proceed without those implementation checks.

Impact:
- Behavior is deterministic, but strict runtime verification for coding/edit tasks is policy-dependent rather than global.

### F3 (Low): One provider-error early return path still skips explicit `RunEnd`.

Where:
- Compaction error return: `src/agent.rs:883`
- Contrast with normal provider error path emitting `RunEnd`: `src/agent.rs:687`

Details:
- On `compact_messages_for_step` error, code returns `finalize_run_outcome(...)` directly without emitting `EventKind::RunEnd`.

Impact:
- Lifecycle/event consumers can see inconsistent close semantics in this edge case.

### F4 (Low): Wall-time budget is enforced at step boundaries; long model calls depend on provider timeouts.

Where:
- Step-boundary wall-time check: `src/agent.rs:604` to `src/agent.rs:608`
- HTTP request timeout can be disabled when set to 0: `src/providers/http.rs:35` to `src/providers/http.rs:40`
- Streaming idle timeout can be disabled when set to 0: `src/providers/http.rs:43` to `src/providers/http.rs:48`

Details:
- Runtime wall-time budget does not preempt while inside a single provider call; boundedness relies on provider timeout configuration.

Impact:
- If operator sets request/idle timeouts to 0, very long model calls can delay stop responsiveness.

## Overall Alignment Verdict

- **Aligned in core architecture**: model proposes, runtime executes tools, runtime validates and stops.
- **Not fully aligned yet** with strict interpretation because finalize candidacy remains partly model-signaled, and one lifecycle edge path still omits explicit `RunEnd`.

## Recommended Next Patches

1. Make finalize candidacy fully runtime-owned:
- Introduce explicit runtime completion predicates per mode/task-kind (not only `tool_calls.is_empty()`).

2. Normalize `RunEnd` emission for the compaction-error return:
- Emit `RunEnd(provider_error)` before returning at `src/agent.rs:883`.

3. Harden timeout invariants:
- Prevent `http_timeout_ms=0` / `http_stream_idle_timeout_ms=0` in strict/runtime-owned modes, or cap with safe minimums.

