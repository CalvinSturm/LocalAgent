# PR5 Scope: `feat: add assisted learn capture drafts with provenance` (Draft)

## Goal

Implement operator-assisted learning draft generation for capture:

- `localagent learn capture --assist ...`

while preserving:

- explicit operator control
- preview-before-write semantics
- bounded/redacted terminal output
- deterministic provenance metadata around nondeterministic LLM output
- existing learning-store invariants (capture writes only under `.localagent/learn/**`)

This PR is draft generation + provenance only. It is not autonomous self-modification.

---

## In scope (PR5 only)

### 1. CLI: `learn capture --assist`

Use the existing `learn capture` command with an assist mode.

#### Required flag

- `--assist`

#### Optional flags (PR5)

- `--write`
  - persist the assisted draft as a normal learning entry
  - without `--write`, assisted mode is preview-only

#### Existing capture args remain available

- `--run`
- `--category` (operator may supply/override category)
- `--summary`
- `--task-summary`
- `--profile`
- `--guidance-text`
- `--check-text`
- `--tag`
- `--evidence`
- `--evidence-note`

PR5 may use these as assist input and/or operator overrides.

#### Hard invariant (PR5)

- `--assist` without `--write`:
  - generates and prints a preview
  - exits success (`0`)
  - performs zero filesystem writes

#### Deterministic validation behavior

- `--write` without `--assist` should fail deterministically (preferred), not silently no-op

#### Deterministic error codes (new)

- `LEARN_ASSIST_WRITE_REQUIRES_ASSIST`
- `LEARN_ASSIST_PROVIDER_REQUIRED`
- `LEARN_ASSIST_MODEL_REQUIRED`

Out of scope:

- new standalone `learn assist` command
- interactive prompts/confirmation
- TUI `/learn` flows
- auto-promotion / auto-validation chaining

---

### 2. Assisted capture model contract (nondeterministic text, deterministic framing)

Assisted capture may use an LLM to draft:

- `summary`
- optional `proposed_memory.guidance_text`
- optional `proposed_memory.check_text`
- optional category suggestion (operator override wins if provided)

#### Determinism boundary (must be explicit)

- LLM output text is nondeterministic
- The following must be deterministic:
  - assist input builder (canonical field ordering)
  - `assist.input_hash_hex`
  - prompt version constant
  - trimming/capping/redaction post-processing
  - preview formatting/banner

---

### 3. Assist input surface (safety-first)

Default assist input should include only bounded, structured metadata:

- operator-provided capture fields
- source metadata (`run`, `task-summary`, `profile`)
- category (if supplied)
- evidence references (`kind`, `value`, optional `note`) only

#### Hard rule (PR5)

- Do not include raw tool outputs, artifact file contents, or replay payloads in assist prompts by default

Deferred / out of scope:

- `--include-evidence-snippets`
- artifact file reading for assist context

---

### 4. Preview UX (bounded + redacted + explicit)

Assisted draft preview output must:

- be clearly labeled as not persisted
- redact suspected secrets (`[REDACTED_SECRET]`)
- be byte-bounded (reuse current learn-show style cap or define explicit cap)
- use stable formatting for tests

#### Recommended preview banner (stable)

- `ASSIST DRAFT PREVIEW (not saved). Use --write to persist.`

---

### 5. Provenance metadata on learning entry (preferred design)

Add optional assist provenance metadata to `LearningEntryV1` as a fully optional field.

#### Schema compatibility contract

- Existing non-assisted entries remain valid
- Old entries without assist metadata remain valid
- PR5 should preserve `openagent.learning_entry.v1` if the new field is optional and readers tolerate unknown fields

#### Recommended schema block (optional on entry)

```yaml
assist:
  enabled: true
  provider: "ollama"
  model: "qwen2.5-coder:7b"
  prompt_version: "openagent.learn_assist_prompt.v1"
  input_hash_hex: "<sha256 hex>"
  source_run_id: "01J..."        # optional
  generated_at: "2026-02-26T..." # informational
  output_truncated: false
```

#### Rust shape (recommended)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssistCaptureMetaV1 {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub prompt_version: String,
    pub input_hash_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
    pub generated_at: String,
    #[serde(default)]
    pub output_truncated: bool,
}
```

```rust
// In LearningEntryV1
#[serde(skip_serializing_if = "Option::is_none")]
pub assist: Option<AssistCaptureMetaV1>,
```

If entry schema expansion is blocked, event-only provenance is acceptable as a temporary fallback, but must be documented as a deliberate compromise.

---

### 6. Entry hash contract (must be explicit)

`entry_hash_hex` behavior for assisted entries should be defined before implementation.

#### Recommended PR5 rule

Include deterministic assist metadata in `entry_hash_hex` input:

- `assist.enabled`
- `assist.provider`
- `assist.model`
- `assist.prompt_version`
- `assist.input_hash_hex`
- `assist.source_run_id` (if present)
- `assist.output_truncated`

Exclude informational assist metadata from `entry_hash_hex`:

- `assist.generated_at`

This preserves traceability while avoiding timestamp-induced hash churn.

---

### 7. Provider/model contract (reuse existing runtime config)

Assisted capture should reuse existing provider/model flags used by other LLM-backed commands.

#### Required config in assisted mode

- provider
- model

If missing, fail deterministically using:

- `LEARN_ASSIST_PROVIDER_REQUIRED`
- `LEARN_ASSIST_MODEL_REQUIRED`

PR5 should not introduce a separate provider stack/config path.

---

### 8. Write semantics + events

Assisted capture write path should preserve PR1 semantics when `--write` is present:

- write entry JSON to `.localagent/learn/entries/<id>.json`
- emit existing capture event: `openagent.learning_captured.v1`

#### Atomicity rule

On failed entry write:

- do not emit capture event

#### Provenance in events

If practical, include assist provenance fields in the `learning_captured` event payload (non-breaking additive fields).

---

## Out of scope (do not implement in PR5)

- promotion (`learn promote`)
- archive command changes
- replay/check auto-validation defaults
- TUI `/learn` commands
- multi-learning batch assist
- artifact-content ingestion for assist prompts
- multiple prompt templates / model-routing logic

---

## Proposed functions / module boundaries (recommended)

### `src/cli_dispatch_learn.rs`

- parse/validate `--assist` + `--write`
- route to assisted capture preview/persist path
- print deterministic preview or capture confirmation

### `src/learning.rs`

Add PR5 helpers:

- `capture_learning_entry_assisted_preview(...) -> anyhow::Result<AssistedCapturePreview>`
- `capture_learning_entry_assisted_write(...) -> anyhow::Result<CaptureLearningOutput>`
- `build_assist_capture_input_canonical(...) -> anyhow::Result<AssistCaptureInputCanonical>`
- `compute_assist_input_hash_hex(...) -> String`
- `render_assist_capture_preview(...) -> String`
- `build_assist_capture_meta(...) -> AssistCaptureMetaV1`
- `apply_assist_draft_to_capture_input(...) -> CaptureLearningInput`

Keep persistence delegated to the existing capture/write path where possible.

---

## Invariants (must not change)

- Assisted mode is explicit (`--assist`)
- `--assist` without `--write` performs zero filesystem writes
- Assisted capture entries are still status=`captured`
- Capture writes remain limited to `.localagent/learn/**`
- No auto-promotion or auto-validation in PR5
- Preview output is bounded and redacted

---

## Acceptance Criteria

1. Assisted preview works

- `learn capture --assist ...` prints a bounded/redacted draft preview
- exits success
- writes nothing

2. Explicit write gate enforced

- `learn capture --assist ... --write` persists a learning entry
- `--write` without `--assist` fails deterministically (`LEARN_ASSIST_WRITE_REQUIRES_ASSIST`)

3. Provider/model requirements enforced

- assisted mode without provider/model fails with deterministic error codes

4. Provenance metadata recorded

- persisted assisted entries include optional `assist` metadata block (preferred design)
- `assist.prompt_version` and `assist.input_hash_hex` are present

5. Hash contract respected

- `assist.generated_at` does not perturb `entry_hash_hex`
- deterministic assist fields do perturb `entry_hash_hex`

6. Atomicity preserved

- failed write => no `learning_captured` event emission

7. Quality gate

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test --quiet`

---

## PR5 Tests (minimum)

1. CLI validation

- `--write` without `--assist` -> deterministic error code
- missing provider/model in assisted mode -> deterministic error code(s)

2. Preview-only no writes

- `--assist` without `--write` creates/modifies no files under `.localagent/**`

3. Assist input canonicalization

- canonical input serialization and `input_hash_hex` are stable for fixed fixture

4. Preview safety contract

- preview is bounded
- suspected secrets are redacted
- preview banner is present

5. Persisted assisted entry

- write path creates `.localagent/learn/entries/<id>.json`
- status is `captured`
- optional `assist` metadata is present and populated

6. Hash semantics

- changing `assist.generated_at` only does not change `entry_hash_hex`
- changing `assist.input_hash_hex` does change `entry_hash_hex`

7. Event atomicity

- simulated write failure => no capture event emitted

---

## PR size guardrails

- Keep PR5 to assisted draft capture + provenance only
- No interactive prompts
- No TUI work
- No promotion/archive coupling
- No broad learning schema redesign beyond optional assist metadata

---

## Suggested implementation order

1. Lock CLI + write-gate semantics (`--assist` preview-only, `--write` persists)
2. Add optional assist metadata struct + hash-contract updates
3. Add canonical assist input builder + `input_hash_hex` + prompt version constant
4. Implement provider/model-backed draft generation with deterministic post-processing
5. Implement bounded/redacted preview renderer and banner
6. Reuse capture persistence path for `--write` + capture event emission
7. Add tests
8. Run validation and commit
