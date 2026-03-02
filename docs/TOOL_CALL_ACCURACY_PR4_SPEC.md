# Tool Call Accuracy PR4 Spec

## Purpose
Improve OOTB tool-call success by tightening the base prompt contract used by the runtime agent loop, while preserving deterministic behavior and avoiding prompt bloat.

PR4 introduces a stable, versioned tool-use contract in `Agent::build_initial_messages`.

## Scope

### In scope
- Update base system prompt in `src/agent.rs::build_initial_messages`
- Add explicit contract marker line for traceability
- Add prompt-focused tests to lock contract stability

### Out of scope
- Provider request/schema changes
- Tool execution behavior changes
- Event schema changes
- Run record schema changes

## Design constraints
- Keep prompt compact and deterministic
- No model-family branching in PR4
- No random/dynamic content in contract block
- Do not include chain-of-thought instructions

## Contract requirements

Add a stable contract block with marker:

```text
TOOL_CONTRACT_VERSION: v1

Tool use contract:
- Use only tools explicitly provided in this run.
- Emit at most one tool call per assistant step.
- Tool arguments must be a valid JSON object matching the tool schema.
- If a tool returns an error, read the tool error and retry with corrected arguments only when applicable.
- If no tool is needed, return a direct final answer.

Fallback when native tool calls are unavailable:
- Emit exactly one wrapper block:
  [TOOL_CALL]
  {"name":"<tool>","arguments":{...}}
  [END_TOOL_CALL]
- Emit no extra prose inside the wrapper.
```

## File-level changes

### `src/agent.rs`
- Modify `build_initial_messages(...)` system prompt string to include:
  - `TOOL_CONTRACT_VERSION: v1`
  - compact contract bullets above
- Keep existing behavior of injecting session + extra messages unchanged

## Test plan

### `src/agent_tests.rs`
Add tests:

1. `build_initial_messages_contains_tool_contract_version_marker`
- Assert system message contains `TOOL_CONTRACT_VERSION: v1`

2. `build_initial_messages_contains_single_tool_call_rule`
- Assert text includes one-tool-call-per-step rule

3. `build_initial_messages_contains_fallback_wrapper_contract`
- Assert text includes `[TOOL_CALL]` and `[END_TOOL_CALL]` instructions

4. `build_initial_messages_contract_is_stable_snapshot`
- Optional snapshot-style string equality assertion against expected prompt block

## Risk and mitigation

- Risk: prompt grows too large and hurts weaker local models
  - Mitigation: keep contract concise; no examples in PR4

- Risk: accidental contract drift in future edits
  - Mitigation: marker + stability test(s)

- Risk: conflicts with planner/worker prompts
  - Mitigation: this change applies only to runtime agent `build_initial_messages`; do not touch planner prompts

## Verification commands
```bash
cargo test agent_tests::
cargo test agent_tests::build_initial_messages_contains_tool_contract_version_marker
cargo test
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Exit criteria
PR4 is complete when:
- runtime system prompt includes `TOOL_CONTRACT_VERSION: v1`
- explicit one-tool-call and fallback-wrapper rules are present
- prompt contract tests pass
- no behavior regressions in existing agent tests
