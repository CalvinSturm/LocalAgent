# LocalAgent v0.5.0 Release Notes

Release date: 2026-03-14

`v0.5.0` is a major runtime and architecture hardening release. It refactors large parts of LocalAgent's core runtime/tooling surfaces into smaller focused modules, strengthens runtime-owned completion and validation behavior for coding tasks, and expands the coding/eval workflow with broader benchmark coverage, local-model investigation assets, and new tooling integrations.

## Highlights

- Reworked the runtime around checkpoint-backed phases, runtime-owned completion/finalization, explicit coding-task contracts, and tighter validation/exact-closeout enforcement.
- Completed a broad internal refactor across the runtime, tools, eval, learning, and TUI layers so the codebase is split into smaller focused modules instead of relying on a few oversized orchestration files.
- Expanded LocalAgent’s coding-task surface with stronger edit/repair paths (`str_replace`, edit-path normalization, wrapped tool-call recovery) and broader eval/benchmark coverage through `common_coding_ux`.
- Added TypeScript LSP context support plus server/runtime harness work, making the repo materially stronger for tool-assisted coding and backend-driven execution flows.
- Switched one-shot `run` / `exec` defaults to isolated ephemeral state and sessionless execution unless the operator opts into persistent state/session settings.
- Added substantial manual evaluation and local-model investigation assets to support repeatable model/runtime comparisons.

## Release Scope

- Commits since `v0.4.0`: `239`
- Diff size since `v0.4.0`: `257` files changed, about `64,073` insertions and `19,884` deletions
- Primary themes:
  - runtime and architecture hardening
  - coding-task contract enforcement and validation recovery
  - coding/eval benchmark expansion
  - local-model investigation and tooling integration

## Included Changes

### Runtime Architecture and Completion Semantics

- Introduced checkpoint-backed runtime phases and stricter runtime-artifact/checkpoint consistency validation.
- Moved completion/finalization behavior further into runtime-owned control flow, with clearer decision points for:
  - validated completion
  - exact final-answer collection
  - post-write follow-on turns
  - repair/retry boundaries
  - approval/cancel/budget terminal states
- Formalized explicit runtime validation contracts and exact-final-answer/output contracts for authored tasks and checks.
- Tightened runtime invariants around verified writes, post-write verification, validation sequencing, and one-tool-call-per-step behavior.
- Repaired validation-phase shell handoff when a model emits prose-only or wrong-tool turns after a verified edit.
- Improved inline exact-closeout inference for prompts that say `reply with exactly ...`.
- Added stricter runtime timeout handling and nonzero timeout defaults in runtime-owned modes.

### Tooling, Editing, and Provider Robustness

- Added `str_replace` as a small-model-friendly file-edit path and aligned eval assertions/trust defaults around it.
- Tightened validation flow and edit alias normalization so existing-file coding tasks recover more reliably.
- Improved malformed wrapped tool-call recovery and LM Studio/OpenAI-compatible message normalization.
- Improved shell compatibility, shell error classification, and validation-only shell-shape repair behavior.
- Added TypeScript LSP context support and improved diagnostics robustness for TypeScript/JS coding tasks.

### Eval, Benchmarking, and Investigation Workflow

- Expanded `common_coding_ux` into the active benchmark decision surface for LocalAgent coding-task improvement work.
- Added broader task coverage, UX metric rows, baseline artifacts, bundle/report/compare improvements, and helper scripts for coding-task evaluation.
- Added focused comparison harnesses for closeout behavior and validation-discipline follow-up work.
- Added substantial manual testing/investigation assets under `manual-testing/`, including canonical `T`/`D` packs, runbooks, logs, and result templates for local-model sweeps.

### Runtime Defaults, TUI, and Operator Flow

- One-shot `run` / `exec` now default to ephemeral temp state when `--state-dir` is not provided.
- One-shot `run` / `exec` also default to `--no-session` unless session settings are explicitly requested.
- Added multiline input mode to the TUI chat flow.
- Improved TUI tool-row reconciliation, verified-write rendering, and post-run status cleanup for complex runs.
- Closed operator resume/replay gaps and improved runtime/tui coordination around terminal events.

### Server, Docs, and Repo Structure

- Added server runtime foundation and associated harness coverage.
- Reorganized the repo documentation into clearer architecture/reference/policy/operations/guides sections, with older material archived explicitly.
- Added and updated runtime-target, runbook, policy, and local-model-improvement docs to match the current codebase.
- README and release docs now point more directly to the release-notes index for operator-facing release history.

### Internal Refactor Scope

- Split large runtime orchestration paths into focused modules under `src/agent/` and `src/agent_runtime/`.
- Split tool execution/catalog/schema logic into focused modules under `src/tools/`.
- Split eval runner/reporting/output logic into smaller surfaces under `src/eval/`.
- Split learning capture/promotion/render/store helpers into focused modules under `src/learning/`.
- Continued TUI/chat decomposition so input, overlays, transcript, rendering, approvals, and runtime glue are easier to reason about and test.

## Compatibility Notes

- No intentional breaking schema or artifact version removals were introduced in this release.
- Runtime/event/task metadata additions remain additive; downstream consumers should continue ignoring unknown fields for forward compatibility.
- Operators relying on persistent one-shot artifacts should now pass `--state-dir <path>` explicitly, because the default one-shot path is ephemeral in `v0.5.0`.
- Coding-task behavior is stricter in `v0.5.0`: validation, exact-closeout, and write-verification flows are enforced more explicitly, so models that previously drifted through loosely enforced completion paths may now be asked to repair or continue instead of being accepted early.
