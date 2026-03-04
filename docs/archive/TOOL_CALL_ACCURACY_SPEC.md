# Tool Call Accuracy Spec

> **Status:** Historical umbrella spec for tranche planning.
> **Implementation status:** PR1-PR7 are completed. For authoritative per-PR behavior, use:
> - `docs/archive/TOOL_CALL_ACCURACY_PR2_SPEC.md`
> - `docs/archive/TOOL_CALL_ACCURACY_PR3_SPEC.md`
> - `docs/archive/TOOL_CALL_ACCURACY_PR4_SPEC.md`
> - `docs/archive/TOOL_CALL_ACCURACY_PR5_SPEC.md`
> - `docs/archive/TOOL_CALL_ACCURACY_PR6_SPEC.md`
> - `docs/archive/TOOL_CALL_ACCURACY_PR7_SPEC.md`

## Goal
Improve LocalAgent tool-call accuracy and OOTB effectiveness for local models with deterministic, auditable behavior.

## Scope
This spec defines the first implementation tranche and follow-on slices.

Tranche 1 (PR1):
- Structured deterministic tool error payloads
- Compact deterministic schema/example hints
- Unknown-tool payload improvements
- Path/disabled error coding
- Unit tests for payload stability

Follow-on (separate PRs):
- Bounded schema repair loop refinements and persistence
- Repeat-loop guard
- Run-record reliability metrics
- Sampling normalization (`--temperature`)
- Prompt contract tightening

## Non-goals (for PR1)
- No trust policy behavior changes
- No replay schema verification changes
- No provider sampling changes
- No TUI rendering changes

## Invariants
- No random IDs in tool error payloads
- Stable JSON payload shape
- Stable ordering of `available_tools`
- Existing message content remains human-readable
- Existing event kinds remain compatible

## Contracts

### Tool error code contract
Codes are treated as API contract values.

Use `ToolErrorCode` (serde snake_case):
- `tool_args_invalid`
- `tool_unknown`
- `tool_path_denied`
- `tool_disabled`
- `tool_args_malformed_json`

### Tool envelope contract
Extend `openagent.tool_result.v1` envelope with optional structured `error`:

```json
{
  "schema_version": "openagent.tool_result.v1",
  "tool_name": "read_file",
  "tool_call_id": "tc1",
  "ok": false,
  "content": "invalid tool arguments: missing required field: path",
  "truncated": false,
  "error": {
    "code": "tool_args_invalid",
    "message": "Invalid arguments.",
    "expected_schema": {"type":"object","required":["path"],"properties":{"path":{"type":"string"}}},
    "received_args": {},
    "minimal_example": {"path":"src/main.rs"}
  },
  "meta": {"side_effects":"filesystem_read","source":"builtin","execution_target":"host"}
}
```

`tool_call_id` fallback for parse-level failures is the sentinel `"unknown"`.

### Deterministic compact schema rules
For `expected_schema`:
- Include only `type`, `required`, `properties`, and critical constraints (`enum`, `minimum`, `maximum`, `items`, `additionalProperties`).
- Exclude descriptions and non-essential prose.
- Keep key ordering stable by constructing objects deterministically.

For `minimal_example`:
- Keep minimal valid object per tool.

For `available_tools`:
- Always sorted lexicographically.

## File-level implementation

### PR1
- `src/tools.rs`
  - Add `ToolErrorCode` enum and `ToolErrorDetail` struct.
  - Add optional `error` field to `ToolResultEnvelope`.
  - Add helpers for compact schema and minimal example per builtin tool.
  - Emit structured errors for:
    - invalid args (`tool_args_invalid`)
    - unknown tool (`tool_unknown`, with sorted `available_tools`)
    - path scope violations (`tool_path_denied`)
    - disabled shell/write (`tool_disabled`)
  - Keep legacy `content` strings for backwards readability.

- `src/agent_tool_exec.rs`
  - `make_invalid_args_tool_message` should emit structured error payload using the same contract.

### Follow-on PRs
- `src/agent.rs`
  - Repair attempt gating by repairable codes only.
  - Repeat-loop guard keyed by `(tool_name, canonical_args)` counting failed repeats only.
  - Persist reliability counters in `AgentOutcome`.

- `src/store/types.rs`, `src/store/io.rs`, `tests/artifact_golden.rs`
  - Add `tool_reliability` record with `#[serde(default)]` and deterministic `BTreeMap`.

- `src/cli_args.rs`, `src/types.rs`, `src/providers/openai_compat.rs`, `src/providers/ollama.rs`, `src/runtime_paths.rs`
  - Add/persist `--temperature` and resolved value.

- `src/agent.rs`
  - Add prompt marker `TOOL_CONTRACT_VERSION: v1` and strict tool-call contract text.

## Test plan

### PR1 tests
- `invalid_args_payload_is_structured_and_deterministic`
- `unknown_tool_payload_includes_sorted_available_tools`
- `path_denied_payload_uses_tool_path_denied_code`

### Follow-on tests
- repair succeeds within bounded attempts
- repair exhaustion exits deterministically
- repeat guard blocks repeated failed calls only
- run-record golden includes reliability fields
- provider request builders reflect temperature resolution

## Event compatibility
Use existing event kinds where possible:
- `tool_retry`
- `tool_exec_end`
- `step_blocked`
- `error`

No new event kind is required for PR1.

## Rollout plan
1. PR1: structured tool error payloads + tests.
2. PR2: repair-loop refinements + metrics counters.
3. PR3: run-record reliability persistence + golden update.
4. PR4: prompt contract marker and tightening.
5. PR5: sampling normalization (`--temperature`).
6. PR6: deterministic CI harness for tool-call accuracy scenarios.
7. PR7: normalize `top_p`, `max_tokens`, and `seed` across providers.

## Verification commands
```bash
cargo test tools::
cargo test agent_tests::
cargo test artifact_golden -- --nocapture
cargo test openai_compat::
cargo test ollama::
cargo test
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

