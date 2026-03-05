# Runtime Loop Audit (2026-03-04)

Scope: `src/agent.rs`, `src/agent_impl_guard.rs`, `src/agent_runtime.rs`, `src/tui/state.rs`, and eval wiring in `src/eval/runner.rs`.

Audit target principle:

`model proposes actions, runtime owns control flow and stopping`

## Summary

The runtime is mostly aligned on guardrails and bounded execution, but there are still important gaps where model behavior influences control flow more than desired.

Top risks:

1. Runtime advertises one-tool-per-step, but executes multiple tool calls if returned in one model response.
2. Finalization is still keyed off "model produced no tool calls" rather than a fully runtime-owned completion state machine.
3. Some failure branches return without emitting `RunEnd`, causing event-lifecycle inconsistency.

## Findings

### F1 (High): Contract says one tool call per step, runtime executes many

Where:
- `src/agent.rs:321`
- `src/agent.rs:325`
- `src/agent.rs:2000`

Details:
- The system contract instructs: "Emit at most one tool call per assistant step."
- Runtime still loops over `for tc in &resp.tool_calls` and executes each call in order.
- This gives the model more control over action batching and ordering per turn than intended.

Impact:
- More model-driven control than desired.
- Larger side-effect surface in one response.
- Harder to isolate and reason about each tool action.

### F2 (High): Stop condition still model-signaled (`no tool calls`)

Where:
- `src/agent.rs:1151`
- `src/agent.rs:1739`
- `src/agent.rs:1968`

Details:
- Main termination branch is entered when `resp.tool_calls.is_empty()`.
- Runtime then performs checks/guards and can emit `RunEnd(ok)`.
- This means model output structure still gates loop termination.

Impact:
- Control-flow ownership is shared with model behavior.
- Premature non-tool responses can still drive toward finalize paths (with guard dependence).

### F3 (Medium): Implementation guard scope is heuristic

Where:
- `src/agent_impl_guard.rs:43`
- `src/agent_impl_guard.rs:149`
- `src/agent_impl_guard.rs:185`

Details:
- Post-edit integrity guard runs only when `is_implementation_task_prompt` returns true.
- Detection is keyword-based (`action && artifact` heuristic).

Impact:
- False negatives are possible for prompts that are implementation tasks but do not match heuristic phrasing.
- In those misses, runtime does not enforce edit verification constraints.

### F4 (Medium): Eval C2 injects a bypass flag for post-write verification

Where:
- `src/eval/runner.rs:924`
- `src/agent.rs:66`
- `src/agent.rs:1844`
- `src/agent.rs:1918`

Details:
- C2 run path injects `INTERNAL_FLAG:allow_skip_post_write_verification`.
- Runtime-owned post-write verification is skipped when this flag is active.

Impact:
- Useful for specific eval behavior, but weakens strict runtime ownership for that path.

### F5 (Medium): `RunEnd` emission is not uniform across all early-return branches

Where:
- `src/agent.rs:829`
- `src/agent.rs:956`
- `src/agent.rs:923`

Details:
- Some error branches return `AgentOutcome` directly without first emitting `EventKind::RunEnd`.
- Examples include pre-model payload serialization failure and post-hook compaction failure paths.

Impact:
- Event lifecycle can be inconsistent for observers/UI.
- Status panels may rely on outcome persistence rather than a guaranteed `RunEnd` event.

### F6 (Low): Wall-time budget check runs per step boundary, not inside a single blocking operation

Where:
- `src/agent.rs:497`
- `src/agent.rs:721`
- `src/providers/http.rs:6`
- `src/providers/http.rs:43`

Details:
- Budget check executes at step loop boundaries.
- Long model/tool operations are bounded primarily by provider/tool timeouts (request/idle/connect), not by a preemptive in-loop wall-time interrupt.

Impact:
- If provider/tool timeout settings are loose, perceived hangs can persist longer than desired.

## Aligned areas (what already matches the principle)

### A1: Runtime-bounded step loop and max-steps termination

Where:
- `src/agent.rs:721`
- `src/agent.rs:4328`

### A2: Runtime wall-time budget enforcement and explicit budget-exceeded exit

Where:
- `src/agent.rs:497`
- `src/agent.rs:511`
- `src/agent.rs:528`

### A3: Runtime protocol guards for malformed wrappers / tool-only violations / repeated invalid patch format

Where:
- `src/agent.rs:1164`
- `src/agent.rs:1244`
- `src/agent.rs:3281`
- `src/agent.rs:3312`

### A4: Runtime-owned post-write verification before finalize (newly added)

Where:
- `src/agent.rs:1846`
- `src/agent.rs:1864`
- `src/agent_impl_guard.rs:103`

### A5: Runtime cancellation ownership in orchestrator

Where:
- `src/agent_runtime.rs:767`
- `src/agent_runtime.rs:774`
- `src/agent_runtime.rs:1227`

### A6: UI closes any running tool rows on `RunEnd`

Where:
- `src/tui/state.rs:270`
- `src/tui/state.rs:282`

## Recommended next patches (priority order)

1. Enforce one tool call execution per model step in runtime (`resp.tool_calls.first()` policy + protocol error for >1).
2. Introduce explicit runtime completion state machine (not only `resp.tool_calls.is_empty()`), with deterministic "task done" predicates by mode/task-kind.
3. Normalize `RunEnd` emission by routing all exits through a single finalize helper to avoid lifecycle drift.
4. Replace heuristic implementation-task detection with explicit task-kind/config signal where available.
5. Keep strict post-write verification default in all production paths; scope bypass flags to explicit test harness modes only.
