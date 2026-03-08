# Runtime Improvement Harness Reset (2026-03)

Status: updated review  
Scope: retrospective review of the harness reset work completed so far  
Goal: record what was actually completed, what it enabled, and what remains before later effectiveness phases

## Executive Summary

The original reset proposal is no longer the right description of the work. The important parts of the reset were implemented.

What is now true:
- Phase 1 closed the baseline-truth and manual-effectiveness-harness gaps.
- Phase 2 established a real backend-owned server/session/run model with attach and live event/control transport.
- Phase 3 has started with a bounded LSP/context seam, artifact visibility, and symbol/definition/reference context on top of that seam.

What this means:
- LocalAgent now has a materially stronger effectiveness harness than it had when this document was first drafted.
- The manual control-pack workflow is cleaner, more repeatable, and more attributable.
- Server-owned execution is no longer a plan; it exists.
- Context acquisition has a real contract and artifact surface, not just a future placeholder.

What is still not done:
- real language-server adapter hardening
- broader LSP/runtime context acquisition beyond the current bounded seam
- role-split orchestration
- extensibility/model-routing phases

This document should now be read as a completed-work review plus residual-gap note, not as a proposal-only reset memo.

## Original Reset Thesis

The original reset thesis was correct:
- do not use control tasks as the runtime benchmark
- separate model-effectiveness measurement from runtime-contract correctness
- enforce fresh-copy execution and cleaner result recording
- use the harness to support runtime improvement without confusing model weakness, fixture hygiene, and runtime defects

That thesis still stands. The difference is that much of it has now been implemented.

## What Was Completed

## 1. Phase 1: Baseline Truth and Manual Effectiveness Harness

Phase 1 is closed and established the baseline required before server-core work.

Completed surfaces:
- [prepare_manual_control_pack.ps1](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/scripts/prepare_manual_control_pack.ps1)
- [verify_manual_control_pack_hygiene.ps1](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/scripts/verify_manual_control_pack_hygiene.ps1)
- [append_manual_control_result.ps1](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/scripts/append_manual_control_result.ps1)
- [README.md](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/D-tests/README.md)
- [RESULTS_TEMPLATE_D.md](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/D-tests/results/RESULTS_TEMPLATE_D.md)

Operational outcomes:
- source control packs are no longer the default run target
- prepared copies under `.tmp/manual-testing/control/...` are the runnable targets
- prepared instances get explicit identity via `PREPARED_INSTANCE.json`
- source-pack hygiene is script-enforced
- control-pack results preserve exact runtime truth instead of collapsing everything into simple pass/fail

Docs/default guardrails added in tests:
- [main_tests.rs](C:/Users/Calvin/Software%20Projects/LocalAgent/src/main_tests.rs)

That guardrail layer now covers:
- parser-derived `RunArgs` defaults
- parser-derived `EvalArgs` defaults
- canonical CLI example parsing

Validation outcome:
- `cargo test --workspace` passed when Phase 1 was closed

## 2. Phase 2: Server-Core, Sessions, Runs, Attach, Events, Control

Phase 2 moved LocalAgent from a process-local CLI-only runtime toward a backend-owned runtime model.

Completed server-core and identity surfaces:
- [server.rs](C:/Users/Calvin/Software%20Projects/LocalAgent/src/server.rs)
- [cli_args.rs](C:/Users/Calvin/Software%20Projects/LocalAgent/src/cli_args.rs)
- [cli_dispatch.rs](C:/Users/Calvin/Software%20Projects/LocalAgent/src/cli_dispatch.rs)

What now exists:
- `localagent serve`
- explicit backend instance identity
- loopback HTTP backend
- server capabilities endpoint
- backend-owned in-memory session registry
- session CRUD
- backend-owned run records
- session `active_run_id` / `last_run_id`

Run execution is no longer placeholder-only:
- `POST /v1/sessions/{session_id}/runs` creates backend-owned runs
- the backend now owns live execution state
- the server reuses the real runtime path instead of implementing a second loop

Attach/event/control surfaces now exist on top of backend-owned live runs:
- attach session handshake
- event replay
- SSE live event stream
- operator input routing
- explicit `interrupt`
- explicit `next`
- explicit `cancel`
- interactive `localagent attach`

Important runtime seam work:
- server-owned runs can pass external cancel/control into the real runtime path
- artifacts and live run state remain attributable

Deterministic proof added for cancelled-run artifact completeness:
- cancelled run persists stable artifact
- late cancel does not corrupt a completed artifact

The key files in this slice are:
- [server.rs](C:/Users/Calvin/Software%20Projects/LocalAgent/src/server.rs)
- [agent_runtime.rs](C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime.rs)
- [launch.rs](C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/launch.rs)
- [setup.rs](C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/setup.rs)

Validation outcome:
- focused server tests passed through each Phase 2 slice
- `cargo test --workspace` passed after Phase 2 implementation

## 3. Phase 3 So Far: LSP/Context Contract, Visibility, Symbol Context

Phase 3 has been started with the bounded context seam that the original effectiveness plan required.

Completed contract and seam surfaces:
- [lsp_context.rs](C:/Users/Calvin/Software%20Projects/LocalAgent/src/lsp_context.rs)
- [lsp_context_provider.rs](C:/Users/Calvin/Software%20Projects/LocalAgent/src/lsp_context_provider.rs)

What now exists:
- explicit bounded LSP context envelope
- diagnostics snapshot support
- bounded symbol context with:
  - symbols
  - definitions
  - references
- deterministic truncation and rendering
- provider seam for LSP-derived context
- setup-time LSP context injection adjacent to repo-map injection

Artifact/config visibility was added in:
- [runtime_paths.rs](C:/Users/Calvin/Software%20Projects/LocalAgent/src/runtime_paths.rs)
- [store/types.rs](C:/Users/Calvin/Software%20Projects/LocalAgent/src/store/types.rs)
- [finalize.rs](C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/finalize.rs)

That visibility now records bounded metadata such as:
- provider
- schema version
- truncation state
- bytes kept
- diagnostics included
- symbol query
- symbol/definition/reference counts

Important boundary preserved:
- no runtime-loop semantics changed
- no approval/trust changes were introduced
- no real language-server lifecycle management was added yet

Validation outcome:
- focused LSP/context tests passed
- `cargo test --workspace` passed after the current Phase 3 PR3 slice

## What The Reset Actually Achieved

The reset succeeded in the areas that mattered most for runtime improvement work.

### A. It made effectiveness data more trustworthy

Before:
- source packs could be stale or contaminated
- result recording was too lightweight
- manual runs were easy to misread

Now:
- prepared-copy execution is the default
- source-pack hygiene is enforceable
- result recording preserves exact runtime truth

### B. It separated control-task effectiveness from runtime-contract truth

Before:
- it was too easy to overread `D-tests` as shared-runtime evidence

Now:
- the control-pack workflow is clearly an effectiveness harness
- runtime-contract correctness still lives primarily in code tests and artifact-backed server/runtime verification

### C. It created the server/runtime foundation needed for later effectiveness work

Before:
- `serve`, attach, backend-owned sessions, and live event/control transport were future ideas

Now:
- they exist and are test-backed

### D. It created a real context-acquisition seam

Before:
- there was no explicit LSP-derived context contract

Now:
- there is a bounded structured seam for diagnostics and symbol context
- it is attributable in artifacts
- it can be expanded without rewriting runtime policy

## What The Reset Did Not Do

The reset did not turn LocalAgent into the final target product.

Still not done:
- real LSP adapter hardening and process management
- broader diagnostics/symbol acquisition from real servers
- planner/explorer/builder/reviewer role routing
- extensibility/plugin surface
- model-routing layer
- OpenCode-level client polish

It also did not change the shared runtime-loop contract in broad ways. That restraint was intentional and correct.

## Completed Invariants

The following invariants were materially preserved through the completed work:

1. Control packs remain effectiveness harnesses, not shared-runtime benchmarks.
2. Exact runtime truth still hangs off canonical runtime fields like `exit_reason`.
3. Server ownership changes transport and session continuity, not the fundamental runtime policy boundary.
4. LSP-derived context is context, not hidden policy authority.
5. Artifacts remain attributable and machine-readable.

## Residual Gaps

The main remaining gaps after the completed work are no longer baseline-harness problems. They are later-phase capability gaps.

### 1. Real LSP adapter hardening

What is missing:
- selected-language real adapter path
- process/lifecycle management
- stronger real-world diagnostics acquisition

### 2. Richer attached operator UX

What is missing:
- more polished attach interaction
- better multi-step operator ergonomics on top of live backend-owned sessions

### 3. Role-split orchestration

What is missing:
- explicit planner/explorer/builder/reviewer routing
- bounded handoff contracts between roles

### 4. Extensibility and model-routing

What is missing:
- plugin/extensibility guardrails
- small-model / big-model routing
- richer operator/platform surfaces

## Review Of The Original Proposal

The original proposal was directionally right, but several parts were overtaken by the implementation.

### What the proposal got right

- the need for prepared-copy execution
- the need for better result taxonomy
- the need to separate effectiveness measurement from runtime-contract reasoning
- the importance of fresh fixture hygiene

### What the implementation clarified

- the most important next move after Phase 1 was server-core, not more harness taxonomy
- server ownership and attachability were higher leverage than building more documentation around hypothetical harness layers
- the practical runtime-improvement path is:
  - trustworthy measurement
  - backend-owned execution
  - explicit context acquisition
  - later orchestration and extensibility

## Current Recommendation

Do not treat this document as an open proposal anymore.

Treat it as:
- a record that the reset largely succeeded
- a checkpoint on what was actually built
- a baseline for later effectiveness phases

For active planning, use the later phase-specific documents and private execution plans. This document should mainly answer:
- what the reset was for
- what was completed
- what remains outside the reset itself

## Bottom Line

The harness reset worked.

It did not solve the whole product roadmap, but it did solve the foundational problems that were blocking credible runtime-improvement work:
- manual control-pack hygiene
- prepared-copy repeatability
- result attribution
- server/session/run ownership
- attach/event/control transport
- bounded LSP/context seam and artifact visibility

LocalAgent is now past the stage where “runtime improvement harness reset” is mostly planning. It is in the stage where later effectiveness gains should build on this completed foundation rather than reopening the reset itself.
