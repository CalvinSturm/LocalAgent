# Tool Call Accuracy PR6 Spec

## Purpose
Add a deterministic CI eval harness that validates tool-call reliability behaviors without depending on real local models.

PR6 converts the highest-value failure modes from the audit into stable, script-driven gates so regressions are caught on every PR.

## Scope

### In scope
- Scripted/stub model execution path for eval-style tool-call scenarios
- Deterministic gate suite for the 5 core behaviors
- CI job wiring for pass/fail enforcement
- Artifacts/log output for debugging failed gates

### Out of scope
- Real-model matrix as required CI gate
- New repair heuristics or prompt-contract changes (covered by earlier PRs)
- Provider-specific latency/quality benchmarking

## Goals
- Catch regressions in tool-call correctness before merge.
- Keep CI deterministic and fast.
- Preserve existing behavior contracts from PR1-PR5.

## Non-goals
- Measuring absolute model quality across hardware/provider setups.
- Replacing nightly/manual real-model smoke tests.

## Determinism contract

### Stub model requirements
- Responses are pre-scripted and index-driven.
- No randomness, no time-based branching.
- Stable tool-call IDs and argument payloads.
- Stable failure modes for malformed calls.

### Test harness requirements
- Fixed prompts and expected event/result assertions.
- No external network dependency.
- Temporary workdirs only; no mutation outside test sandbox.

## Architecture changes

### Harness layer
- Add a scripted-model harness under eval/test infrastructure that can emit:
  - valid tool call
  - unknown tool call
  - invalid args tool call
  - malformed tool call payload
  - repeated identical failing calls
  - final assistant completion

Suggested locations:
- `src/eval/` for reusable harness pieces
- `tests/` for regression tests that assert run outcome + metrics + events

### Scenario fixtures
- Add scenario definitions (small JSON or Rust fixtures) that encode scripted response steps and expected outcomes.
- Keep fixtures minimal and human-reviewable.

### CI wiring
- Add a dedicated job/stage (for example `tool-call-accuracy`) running only deterministic scenario tests.
- Ensure output includes failed scenario name and mismatch details.

## Required deterministic gates

1. Hallucinated tool handling
- Script emits unknown tool first.
- Expected:
  - repair path attempted (bounded) or deterministic fail reason
  - `unknown_tool_count` incremented
  - clear terminal status

2. Invalid args repair
- Script emits known tool with invalid args, then corrected args.
- Expected:
  - repair attempt count increments
  - successful execution on bounded retry
  - reliability metrics reflect repaired success

3. Multi-step chain completion
- Script emits a valid sequence of tool calls followed by final answer.
- Expected:
  - no protocol/loop violations
  - expected tool sequence recorded
  - run exits `ok`

4. Repeat-loop breaker
- Script emits same failing call repeatedly.
- Expected:
  - repeat guard blocks at configured threshold
  - deterministic failure token/reason
  - `repeat_block_count` incremented

5. Trust gate behavior (shell/write)
- Script attempts side-effectful call without permissive flags/approval.
- Expected:
  - gate decision emitted and auditable
  - no side-effect execution when not allowed
  - explicit blocked/approval-required outcome

## Assertions and evidence

Each scenario must assert all of:
- Exit reason
- Tool reliability counters (as applicable)
- Presence/ordering of key events (decision/retry/block/end)
- Tool execution side-effect constraints

For failing scenarios, print:
- scenario id
- expected vs actual exit reason
- expected vs actual key counters
- key event projection for quick diagnosis

## CI command contract

Primary gate command (example):
```bash
cargo test eval::tool_call_accuracy::
```

If implemented as integration tests:
```bash
cargo test --test tool_call_accuracy_ci
```

The command chosen must:
- complete without external services
- be stable across runs
- fail on any scenario mismatch

## Performance targets
- Total PR6 gate runtime target: under 2 minutes on CI baseline runners.
- Per-scenario runtime target: under 10 seconds.

## Risks and mitigations

- Risk: flaky assertions due to event ordering drift
  - Mitigation: assert only stable subsequences and deterministic counters.

- Risk: harness diverges from production execution path
  - Mitigation: reuse real agent loop + gate + tool execution path; only stub provider output.

- Risk: fixture bloat and maintenance cost
  - Mitigation: keep fixtures minimal and focused on one behavior each.

## Rollout plan

1. Add harness primitives and one scenario.
2. Add remaining four required scenarios.
3. Add CI job and make it required.
4. Document how to run locally in contributor docs.

## Verification plan

Local:
```bash
cargo test eval::tool_call_accuracy::
cargo test
cargo fmt -- --check
```

CI:
- Run deterministic tool-call-accuracy job on every PR.
- Fail fast with scenario-level diagnostics.

## Exit criteria
- All 5 required deterministic gates implemented and passing.
- CI enforces the gate on every PR.
- Failures provide actionable diagnostics without reruns.
- No dependency on local model availability for required CI pass.
