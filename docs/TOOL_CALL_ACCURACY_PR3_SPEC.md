# Tool Call Accuracy PR3 Spec

## Purpose
Persist tool-call reliability metrics to run artifacts so regressions are measurable and replay/debug workflows can inspect reliability outcomes deterministically.

PR3 builds on PR1 and PR2.

## Scope

### In scope
- Persist reliability counters already tracked in agent runtime to `RunRecord`
- Keep schema backward-compatible for existing artifacts
- Update artifact golden fixtures and tests

### Out of scope
- New reliability metric computation logic beyond current counters
- UI rendering changes
- Replay verify rule-tightening
- Provider sampling controls

## Required behavior

### Persisted reliability section
Add `tool_reliability` to run record with deterministic ordering:

```rust
pub struct ToolReliabilityByTool {
    pub calls: u32,
    pub valid_first_try: u32,
    pub repaired: u32,
    pub repair_failed: u32,
    pub unknown_tool: u32,
}

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

### Backward compatibility
- `RunRecord.tool_reliability` must use `#[serde(default)]`.
- `ToolReliabilityRecord` and `ToolReliabilityByTool` must derive `Default`.
- Loading old run artifacts without `tool_reliability` must succeed.

### Determinism constraints
- `by_tool` is `BTreeMap` only.
- No timestamps or random IDs inside reliability payload.
- Values are integer counters only.

## File-level changes

### 1) Agent outcome schema
- `src/agent.rs`
  - Add `tool_reliability: ToolReliabilityRecord` to `AgentOutcome`
  - Ensure all run exits populate this field
  - Include reliability values already tracked in PR2 (including repeat guard and malformed counts)

### 2) Store schema and writer
- `src/store/types.rs`
  - Add `tool_reliability` field to `RunRecord` with `#[serde(default)]`
- `src/store/io.rs`
  - Persist `outcome.tool_reliability.clone()` into `RunRecord`

### 3) Runtime config/paths impact
- No new CLI flags required in PR3.
- No `RunCliConfig` changes required in PR3.

## Tests

### Unit/integration
- `src/agent_tests.rs`
  - Add assertions for reliability counters in at least one happy-path and one failure-path run

### Artifact schema golden
- `tests/artifact_golden.rs`
  - Include `tool_reliability` in projection assertions
- `tests/fixtures/artifacts/run_record_schema_golden.json`
  - Update fixture to include `tool_reliability` key and expected nested key set

### Compatibility test
Add one test (new or existing harness) that deserializes a legacy run record missing `tool_reliability` and verifies defaults are zeroed.

## Suggested projection update (artifact golden)
In projection object include:
- `tool_reliability_present` boolean
- `tool_reliability_keys` sorted key list
- optional sample counters from fixture run (`tool_calls_total`, `malformed_tool_call_count`)

## Migration and rollout
1. Add types and defaults.
2. Wire `AgentOutcome` population.
3. Wire `write_run_record` persistence.
4. Update artifact golden projection and fixture.
5. Run test suite and verify no replay/load regressions.

## Verification commands
```bash
cargo test artifact_golden -- --nocapture
cargo test agent_tests::
cargo test
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Risks and mitigations
- Risk: large compile break due to many explicit `AgentOutcome { ... }` constructors.
  - Mitigation: introduce helper builder/default where feasible; otherwise update all compile errors mechanically.

- Risk: golden fixture drift not intentional.
  - Mitigation: update only expected schema sections and keep deterministic ordering.

- Risk: older artifacts fail deserialization.
  - Mitigation: `#[serde(default)]` and explicit compatibility test.

## Exit criteria
PR3 is complete when:
- every run artifact includes `tool_reliability`
- old artifacts still load without migration steps
- artifact golden tests pass with intentional fixture updates
- new reliability fields are visible in run JSON and stable across repeated runs
