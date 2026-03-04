# Tool Call Accuracy PR2 Spec

## Purpose
Define the PR2 implementation slice for LocalAgent tool-call accuracy after PR1 structured error payloads.

PR2 focuses on:
- bounded schema/tool repair behavior
- deterministic retry gating by error code
- repeat-loop blocking for failed identical calls
- run-level reliability counters in memory (persistence is PR3)

## Scope

### In scope
- `src/agent.rs`
- `src/agent_tool_exec.rs`
- `src/agent_tests.rs`
- optional small helper additions in `src/tools.rs` if needed for error-code extraction

### Out of scope
- run record schema persistence (`RunRecord.tool_reliability`) -> PR3
- provider sampling (`--temperature`) -> PR5
- TUI presentation updates
- new event kinds (reuse existing `ToolRetry`, `ToolExecEnd`, `StepBlocked`, `Error`)

## Existing baseline (must preserve)
- schema repair path already exists via `schema_repair_instruction_message(...)`
- `ToolRetry` events already emitted for repair/retry/stop
- malformed tool-protocol guards already exist and should remain unchanged

## Deterministic constants
Add constants in `src/agent.rs`:

```rust
const MAX_SCHEMA_REPAIR_ATTEMPTS: u32 = 2;
const MAX_FAILED_REPEAT_PER_KEY: u32 = 3;
```

Behavior must not depend on wall clock randomness or non-deterministic ids.

## Repairability policy

Repair attempts are allowed only for tool failures with codes:
- `tool_args_invalid`
- `tool_unknown`
- `tool_args_malformed_json`
- optional single-shot for `tool_path_denied` (configurable via constant)

Repair attempts are NOT allowed for:
- `tool_disabled`
- trust/policy/approval-required/denied failures
- runtime budget failures

If no explicit `error.code` exists in tool envelope, fallback classification may still infer repairability using existing text-based logic, but explicit codes take precedence.

## Event contract (existing EventKind)

### `ToolRetry`
Required payload fields:
- `tool_call_id`
- `name`
- `attempt`
- `max_retries`
- `failure_class`
- `action` (`repair` | `retry` | `stop`)

Add optional:
- `error_code` (snake_case)
- `max_attempts` for repair actions

### `ToolExecEnd`
For repaired attempts include:
- `repair_attempted` (bool)
- `repair_succeeded` (bool)
- `error_code` when known

### `StepBlocked`
When repeat guard triggers:
- `source: "tool_repeat_guard"`
- `code: "TOOL_REPEAT_BLOCKED"`
- `tool_call_id`
- `name`
- `repeat_count`
- `repeat_limit`
- `repeat_key_sha256`

### `Error`
On repair exhaustion:
- `source: "schema_repair"`
- `code: "TOOL_SCHEMA_REPAIR_EXHAUSTED"`
- `tool_call_id`
- `name`
- `attempt`
- `max_attempts`

## Repeat-loop guard

### Key definition
Repeat key = `sha256(tool_name + "|" + canonical_json(arguments))`

Use existing deterministic canonicalization:
- `trust::approvals::canonical_json(...)`
- fallback string `"null"` on serialization failure

### Counting rule (avoid false positives)
Increment repeat counter only when previous execution for same key failed.

Do not block successful repeated reads.

Trigger block when `failed_repeat_count >= MAX_FAILED_REPEAT_PER_KEY`.

## In-memory reliability counters (AgentOutcome prep)
Add to `AgentOutcome` in-memory fields (persist in PR3):

```rust
pub struct ToolReliabilityRecord {
  pub tool_calls_total: u32,
  pub tool_calls_valid_first_try: u32,
  pub tool_calls_repaired: u32,
  pub tool_calls_repair_failed: u32,
  pub unknown_tool_count: u32,
  pub repeat_block_count: u32,
  pub malformed_tool_call_count: u32,
  pub by_tool: BTreeMap<String, ToolReliabilityByTool>,
}
```

Update counters during run loop only at deterministic decision points.

## Implementation plan

1. Add helper in `agent_tool_exec.rs` to extract `error.code` from tool envelope content.
2. In `agent.rs`, gate schema repair by `error.code` allowlist.
3. Cap schema repair attempts using `MAX_SCHEMA_REPAIR_ATTEMPTS`.
4. Emit `ToolRetry` with `max_attempts` + `error_code`.
5. Add failed-repeat key tracking and `StepBlocked` emission.
6. Increment in-memory reliability counters for:
   - total tool calls
   - first-try valid
   - repaired success/failure
   - unknown tool
   - malformed tool protocol
   - repeat block

## Tests

Add to `src/agent_tests.rs`:

1. `schema_repair_allowed_for_tool_args_invalid`
- model emits invalid args then corrected args
- assert success
- assert `ToolRetry(action=repair)` emitted once

2. `schema_repair_skipped_for_tool_disabled`
- model triggers disabled write/shell path
- assert no repair loop
- assert immediate stop/continue behavior per existing policy

3. `schema_repair_exhaustion_is_deterministic`
- model repeatedly emits invalid args
- assert exhaustion error payload and deterministic exit reason path

4. `repeat_guard_blocks_failed_identical_calls`
- model emits same failing call repeatedly
- assert `StepBlocked` with `TOOL_REPEAT_BLOCKED`

5. `repeat_guard_does_not_block_successful_repeats`
- model calls same read tool with successful response twice
- assert no repeat block

6. `tool_retry_event_includes_error_code_when_present`
- assert emitted event payload contains `error_code`

## Verification commands
```bash
cargo test agent_tests::
cargo test tools::
cargo test
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Risks and mitigations
- Risk: over-blocking legitimate repeated reads.
  - Mitigation: count repeats only when prior attempt failed.

- Risk: repair loop retries non-repairable failures.
  - Mitigation: strict allowlist by `error.code`.

- Risk: event payload drift breaking downstream parsing.
  - Mitigation: additive payload fields only; no renames.

## Exit criteria
PR2 is complete when:
- repair attempts are code-gated and bounded deterministically
- failed-repeat guard blocks loops without breaking successful repeats
- in-memory reliability counters are updated and test-covered
- test suite additions pass
