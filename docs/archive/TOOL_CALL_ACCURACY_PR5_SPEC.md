# Tool Call Accuracy PR5 Spec

## Purpose
Normalize sampling controls across providers with deterministic precedence and artifact visibility, starting with `temperature`.

PR5 adds `--temperature` and plumbs it through runtime request construction for OpenAI-compatible and Ollama providers.

## Scope

### In scope
- Add `--temperature` CLI option
- Propagate resolved temperature into model requests
- Persist effective temperature in run CLI config artifacts
- Add provider request-builder tests

### Out of scope
- `top_p`, `max_tokens`, `seed` (future work)
- provider-specific adaptive defaults by model family
- profile schema expansion beyond optional temperature wiring

## Contract and precedence

### New flag
- `--temperature <f32>` on `RunArgs`
- Optional; if omitted, behavior stays provider-default-compatible

### Effective value precedence
1. CLI explicit `--temperature`
2. (future) profile/session injected value, if introduced later
3. provider default

For this PR, only (1) and (3) are active.

### Provider defaults (current behavior to preserve)
- OpenAI-compatible: default `0.2` when unset
- Ollama: omit temperature when unset (server default)

## Schema and type updates

### `src/cli_args.rs`
- Add:
```rust
pub(crate) temperature: Option<f32>
```
- validation: none beyond `f32` parse in PR5

### `src/types.rs`
- Extend `GenerateRequest`:
```rust
pub temperature: Option<f32>
```

### `src/agent.rs`
- Populate `GenerateRequest.temperature` from run args / agent config path

### `src/providers/openai_compat.rs`
- `to_request(...)` should set:
  - `temperature = req.temperature.unwrap_or(0.2)`

### `src/providers/ollama.rs`
- Extend request payload with optional options object:
```rust
#[serde(skip_serializing_if = "Option::is_none")]
options: Option<OllamaOptions>
```
```rust
struct OllamaOptions { temperature: f32 }
```
- Include `options` only when `req.temperature.is_some()`

### `src/runtime_paths.rs` and `src/store/types.rs`
- Persist effective temperature in `RunCliConfig`:
```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub temperature: Option<f32>
```

## Determinism requirements
- No randomization added
- Effective temperature must be captured in run artifact when provided
- Serialization shape must be stable (optional field omitted when `None`)

## Test plan

### CLI and config tests
- parse test for `--temperature 0.2`
- run cli config builder includes `temperature` when supplied

### Provider request tests
- `openai_compat::to_request_uses_cli_temperature`
- `openai_compat::to_request_defaults_to_point_two_when_unset`
- `ollama::to_request_includes_options_temperature_when_set`
- `ollama::to_request_omits_options_when_unset`

### Artifact tests
- update artifact golden projection if `RunCliConfig` key set changes
- verify deserialization compatibility for records without `temperature`

## Verification commands
```bash
cargo test openai_compat::
cargo test ollama::
cargo test agent_tests::
cargo test artifact_golden -- --nocapture
cargo test
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Risks and mitigations
- Risk: accidental behavior drift in existing runs when flag unset
  - Mitigation: preserve current defaults exactly

- Risk: inconsistent behavior between providers
  - Mitigation: explicit provider tests for set/unset behavior

- Risk: run artifacts missing effective sampling config
  - Mitigation: add `RunCliConfig.temperature` and test coverage

## Exit criteria
PR5 is complete when:
- `--temperature` is accepted and propagated
- OpenAI-compatible and Ollama set/omit behavior matches spec
- run artifacts persist temperature when provided
- all related tests pass
