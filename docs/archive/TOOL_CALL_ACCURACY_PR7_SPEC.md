# Tool Call Accuracy PR7 Spec

## Purpose
Normalize remaining generation controls across providers and persist the effective settings used at runtime.

PR7 extends PR5 (`temperature`) with:
- `top_p`
- `max_tokens`
- `seed`

It also records resolved generation settings in run artifacts/diagnostics so runs are reproducible and provider comparisons are fair.

## Scope

### In scope
- New CLI flags: `--top-p`, `--max-tokens`, `--seed`
- Plumbing into runtime request model
- Provider request mapping for OpenAI-compatible and Ollama
- Persist effective generation settings in run records
- Diagnostics/effective-config output updates (if present)
- Deterministic tests for set/unset behavior and precedence

### Out of scope
- Provider-specific tuning heuristics
- Adaptive defaults by model family
- Changing retry/repair loop semantics

## Contract

### New flags
- `--top-p <f32>`
- `--max-tokens <u32>`
- `--seed <u64>`

All optional.

### Precedence
1. Explicit CLI flag
2. Profile/session-resolved value (if present)
3. Provider default behavior

### Determinism requirements
- No random fallback seed generation.
- If `seed` is unset, omit it (provider default).
- If a setting is set, persist exactly as used.
- Serialization order remains stable.

## Type and schema updates

### `src/types.rs`
- Extend `GenerateRequest` with:
  - `top_p: Option<f32>`
  - `max_tokens: Option<u32>`
  - `seed: Option<u64>`

### `src/cli_args.rs`
- Extend `RunArgs` with:
  - `top_p: Option<f32>`
  - `max_tokens: Option<u32>`
  - `seed: Option<u64>`

### `src/store/types.rs`
- Extend `RunCliConfig` with:
  - `top_p: Option<f32>`
  - `max_tokens: Option<u32>`
  - `seed: Option<u64>`
- Use `#[serde(skip_serializing_if = "Option::is_none")]`.

## Runtime wiring

### Request construction
Populate new fields anywhere `GenerateRequest` is built:
- agent runtime
- planner/qualification paths
- learn assistant path
- any direct provider test helpers

### Resolved config capture
Ensure run artifact capture includes effective values:
- `temperature`
- `top_p`
- `max_tokens`
- `seed`

## Provider mapping

### OpenAI-compatible
- Map directly when set:
  - `temperature`
  - `top_p`
  - `max_tokens`
  - `seed`
- Preserve current default only for temperature (`0.2`) unless explicitly changed by product decision.
- Omit other fields when unset.

### Ollama
- Place sampling values under `options` when set:
  - `temperature`
  - `top_p`
  - `seed`
- `max_tokens` maps to Ollama’s token-limit field (`num_predict`) in `options`.
- Omit each field when unset.

## Validation

### CLI validation
- `top_p` expected range: `(0, 1]`
- `max_tokens` must be `> 0`
- `seed` any valid `u64`

If validation fails, return deterministic CLI error text.

### Provider compatibility behavior
- If a provider cannot honor a field, behavior must be explicit:
  - Preferred: omit unsupported field and emit a deterministic warning event/log.
  - Do not silently rewrite to a different value.

## Tests

### Unit tests
- CLI parse tests for all new flags.
- Request builder tests per provider:
  - set values are passed through correctly
  - unset values are omitted/defaulted per contract

### Integration tests
- Run artifact includes resolved generation settings when set.
- Backward compatibility: older run records without these fields still deserialize.

### Regression tests
- Deterministic snapshot/assertions for request payload shape.

## CI verification commands
```bash
cargo test openai_compat::
cargo test ollama::
cargo test agent_tests::
cargo test --test artifact_golden run_artifact_schema_and_layout_golden_is_stable -- --nocapture
cargo test
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Risks and mitigations

- Risk: provider divergence in parameter semantics
  - Mitigation: provider-specific mapping tests with explicit docs.

- Risk: nondeterministic outputs from partial setting application
  - Mitigation: persist and display effective settings in artifacts.

- Risk: schema churn in artifacts
  - Mitigation: additive optional fields only + golden/snapshot updates.

## Rollout plan
1. Add CLI/type/schema fields.
2. Wire runtime and providers.
3. Add tests and update artifact golden expectations.
4. Update contributor docs for effective generation controls.

## Exit criteria
- All 4 generation controls (`temperature`, `top_p`, `max_tokens`, `seed`) normalized across supported providers.
- Effective settings are visible in run artifacts.
- Deterministic tests pass in CI for set/unset behavior and artifact compatibility.
