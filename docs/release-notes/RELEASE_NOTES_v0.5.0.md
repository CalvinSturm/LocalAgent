# LocalAgent v0.5.0 Release Notes

Release date: 2026-03-14

## Highlights

- Expanded `common_coding_ux` into the active eval decision surface, with broader task coverage, frozen baseline artifacts, and dedicated comparison harnesses for closeout and validation-discipline follow-up work.
- Hardened runtime-owned coding-task completion behavior so validation and exact-closeout contracts are handled more explicitly and recover better from local-model protocol drift.
- Switched one-shot `run` / `exec` defaults to isolated ephemeral state and sessionless execution unless the operator opts into persistent state/session settings.
- Added TypeScript LSP context support and supporting runtime/server harness coverage for tool-assisted coding workflows.

## Included Changes

### Runtime Contract and Validation Hardening

- Tightened required-validation and exact-final-answer runtime handling for coding tasks.
- Repaired validation-phase shell handoff when a model emits prose-only or wrong-tool turns after a verified edit.
- Improved inline exact-closeout inference for prompts that say `reply with exactly ...`.
- Continued runtime-loop extraction and hardening while preserving the shared completion-policy semantics.

### Eval and Benchmarking

- Promoted `common_coding_ux` as the current benchmark decision surface for LocalAgent coding-task improvement work.
- Expanded the pack with broader coding task families, UX metric rows, and frozen baseline artifacts.
- Added helper runbooks and focused harnesses for U3/U4 closeout follow-up and Omnicoder validation-discipline comparisons.

### Runtime Defaults and Operator Flow

- One-shot `run` / `exec` now default to ephemeral temp state when `--state-dir` is not provided.
- One-shot `run` / `exec` also default to `--no-session` unless session settings are explicitly requested.
- README and release docs now point more directly to the release-notes index for operator-facing release history.

### Tooling and Integration

- Added TypeScript LSP context provider support.
- Added server/runtime harness coverage and follow-on robustness fixes for tool-assisted coding flows.
- Improved malformed wrapped tool-call recovery and related helper rigor.

## Compatibility Notes

- No intentional breaking schema or artifact version removals were introduced in this release.
- Runtime/event/task metadata additions remain additive; downstream consumers should continue ignoring unknown fields for forward compatibility.
- Operators relying on persistent one-shot artifacts should now pass `--state-dir <path>` explicitly, because the default one-shot path is ephemeral in `v0.5.0`.
