# Tool Call Accuracy PR9 Spec

## Purpose
Integrate `glob`/`grep` into trust/policy defaults and documentation so safe search workflows do not require shell access.

## Scope

### In scope
- Update safe default trust policy to allow `glob` and `grep`
- Ensure gate decisions/reasons are clear for search tools
- Pin trust-mode and policy-source precedence for deterministic behavior
- Update operator docs to prefer `glob`/`grep` over shell for searching
- Add trust-policy tests for new defaults

### Out of scope
- New tools (already in PR8)
- TUI approval panel behavior (PR10)
- Agent mode split (PR11)
- JSON run-event output (PR12)

## File-level changes

- `src/trust/policy.rs`
  - Include `glob` and `grep` in safe default allow rules
  - Update `safe_default_policy_repr()` to match exact new rule ordering
  - Ensure matching/diagnostics preserve deterministic behavior
- `src/runtime_wiring.rs`
  - Validate no unexpected regression in trust gate selection paths and policy hash source
- `src/gate.rs` (tests only if needed)
  - Assert decision `source`/`reason` behavior remains stable
- `docs/reference/CLI_REFERENCE.md`
  - Add `glob`/`grep` usage references and safe-search guidance
- `README.md` (optional command examples, if currently shell-first)

## Behavioral contract

### Trust-mode policy source precedence
- `--trust off`:
  - No trust gate policy enforcement (`NoGate` path).
- `--trust auto`:
  - If `policy.yaml` exists: load file policy.
  - If `policy.yaml` is missing: no trust gate policy enforcement (`NoGate` path).
- `--trust on`:
  - If `policy.yaml` exists: load file policy.
  - If `policy.yaml` is missing: use in-code safe default policy.

### Safe default policy (no policy file, `--trust on`)
- Default decision remains `deny`.
- Exact rule set and order:
  1. `allow list_dir`
  2. `allow read_file`
  3. `allow glob`
  4. `allow grep`
  5. `require_approval shell`
  6. `require_approval write_file`
  7. `require_approval apply_patch`
- `safe_default_policy_repr()` must match that exact ordered rule list (used for deterministic policy hash input).

### Gate decision/source semantics
- For `glob`/`grep` allowed via safe default:
  - decision: `allow`
  - `source: "safe_default"`
  - `reason: null` (unless explicitly configured by file policy rule)
- Existing hard-gate denials remain unchanged:
  - `shell` denied without `--allow-shell` (source `hard_gate`)
  - write tools denied without `--allow-write` (source `hard_gate`)

### Approval behavior expectations
- With `--trust on` and safe default policy:
  - `glob`/`grep` do not require approval.
  - `shell`/write tools remain approval-gated by policy and separately constrained by allow flags.

## Test plan

### Policy tests
- `safe_default_allows_glob`
- `safe_default_allows_grep`
- `safe_default_rule_order_is_deterministic_for_repr`
- `safe_default_policy_repr_includes_glob_and_grep_in_order`
- existing shell/write approval requirements remain unchanged

### Integration
- trust-gated run invoking `glob`/`grep` succeeds without approval prompts
- `trust_on_without_policy_uses_safe_default_and_allows_glob_grep`
- `trust_auto_without_policy_uses_no_gate` (existing behavior preserved)
- `glob_grep_allow_decision_source_is_safe_default`
- `hard_gate_shell_denial_precedes_policy_allow_without_allow_shell`

### Documentation checks
- CLI/reference docs include `glob`/`grep` as first-line search tools.
- Shell examples for search are clearly marked as fallback/legacy.

## Determinism requirements
- Stable safe-default rule order in both compiled policy and repr string.
- Stable decision metadata for safe-default search tool allows (`source`, `reason`).
- Stable policy hash input for safe-default mode (`safe_default_policy_repr()`).
- No behavior change for non-search tool trust outcomes.

## Verification commands
```bash
cargo test trust::policy::
cargo test runtime_wiring::
cargo test
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
```

## Implementation checklist
- [ ] Update safe default policy rules for `glob`/`grep`
- [ ] Update safe-default repr string to include `glob`/`grep` in exact ordered list
- [ ] Confirm trust-mode/policy-source precedence remains unchanged
- [ ] Confirm gate decisions remain deterministic (`decision`, `source`, `reason`)
- [ ] Add policy/integration tests
- [ ] Update CLI/reference docs with safe-search guidance
- [ ] Run verification commands

## Exit criteria
- safe trust defaults allow read-only search tools
- no regression in shell/write approval semantics
- docs reflect new recommended search workflow
