# LocalAgent `/learn` Overlay Research

## Document Control

- Status: `Draft (decision-ready)`
- Owner: LocalAgent maintainers
- Last updated: 2026-02-27
- Repo baseline: `8a47f2f`
- Scope: `/learn` capture, review, promote workflow and enforcement model
- Non-goals: final CLI/TUI API freeze, formal compliance certification

## Executive Summary

`/learn` should be implemented as a supervisory-control pipeline with strict stage boundaries:

1. Capture and review create draft artifacts only.
2. Promotion is the only behavior-changing operation.
3. Promotion must be proof-gated by default and auditable.

This pattern is supported by evidence from checklist reliability, staged evaluation workflows, and automation human factors [1][3][5][8]. It also matches existing LocalAgent architecture seams for deterministic hashing, replay verification, and JSONL event logging (`src/repro.rs`, `src/events.rs`) [R4][R5].

## Decision Summary

Recommended defaults:

1. Structured candidate format: `Trigger -> Action -> Verification`.
2. Quality gates:
   - Capture: SAT-min (`Specific`, `Actionable`, `Testable`).
   - Promotion: SATSS (`Specific`, `Actionable`, `Testable`, `Scoped`, `Safe`), unless waived with explicit force semantics.
3. Promotion proof requirement:
   - `check_run_id` or `replay_verify_run_id`, else explicit waiver via `--no-eval-proof --force`.
4. Preview-first UX with explicit ARMED write mode.
5. Atomic promotion order:
   - write target -> hash target -> update entry status -> append event.

## Methods and Source Quality

- Method: synthesis of two internal drafts plus repo seam mapping.
- Source policy:
  - Primary sources preferred for technical claims.
  - Secondary sources allowed for interpretation/context.
  - Internal repo docs/code used for implementation feasibility.

Source mix used in this document:

- Primary external: 8
- Secondary external: 4
- Internal repo references: 7

## Evidence-to-Decision Matrix

| Decision / Claim | Evidence | Quality | Notes |
| :--- | :--- | :--- | :--- |
| Structured capture outperforms freeform for reliability-critical tasks. | [3], [5], [8] | High | Checklist and staged-eval literature converge on constrained, explicit steps. |
| Explicit PREVIEW vs ARMED boundaries reduce automation mode confusion risk. | [7], [13] | High | Human-factors guidance supports explicit mode transitions. |
| Promotions should require verifiable proof artifacts or explicit waiver. | [10], [12], [11] | Medium | Inference from verification domains to product gating model. |
| Busy-state rejection during active runs is safer than interleaved writes. | [3], [6] | Medium-High | Inference from workload/checklist timing practices. |
| LocalAgent repo can support deterministic audit trail without new infrastructure. | [R4], [R5], [R6], [R7] | High | Existing hash/replay/event seams already present. |

## Recommended Policy Defaults

### Candidate schema

Required:

- `trigger`
- `action`
- `verification`

Recommended:

- `scope`
- `safety_flags`
- `provenance` (run id / command / workdir snapshot)

### Quality gates

- Capture blocking minimum: SAT-min.
- Promotion blocking minimum: SATSS plus eval-proof presence.

### Eval-proof enforcement

Accept one of:

- `check_run_id`
- `replay_verify_run_id`
- waiver (`no_eval_proof_waived=true`) with `--no-eval-proof --force`

Waiver requirements:

- Persist waiver flag in candidate metadata.
- Emit waiver flag in promotion event payload.
- Require short human reason string.

## UX Contract for TUI Overlay

Tabs:

1. Capture
2. Review
3. Promote

Mode semantics:

- `PREVIEW`: no writes
- `ARMED`: writes enabled after explicit toggle

Busy behavior:

- Reject ARMED write when runtime is active with `ERR_TUI_BUSY_TRY_AGAIN`.

Error message format:

1. Friendly sentence.
2. Stable error code.
3. Next-step remediation hint.

## Repo Implementation Mapping

Targeted files/modules:

- CLI surface: `src/cli_args.rs` [R1], `src/cli_dispatch.rs` [R2]
- Event append: `src/events.rs` [R4]
- Deterministic hashing and replay proof seam: `src/repro.rs` [R5]
- TUI integration seam: `src/chat_tui_runtime.rs` [R3]
- Deterministic scaffold/hash testing patterns: `src/scaffold.rs` [R6]
- Contract alignment doc: `docs/reference/LEARN_OUTPUT_CONTRACT.md` [R7]

Proposed module addition:

- `src/learn/` with entry schema, proof types, target writers, promote transaction, and event payload types.

Storage proposal:

- `.localagent/learn/entries/<id>.json`
- `.localagent/learn/events.jsonl`

## Acceptance Criteria

1. No-preview-write guarantee:
   - Preview operations do not modify target file hash or mtime.
2. Proof gating:
   - Promotion without proof fails unless waiver flags are present.
3. Atomicity:
   - Target-write failure prevents status/event updates.
4. Audit completeness:
   - Every promotion event includes proof mode or waiver metadata.
5. Idempotent managed insertion:
   - Re-promoting same entry to `AGENTS.md` does not duplicate managed block.

## Operational Metrics

Primary:

- >=80% first-try guided capture completion
- >=50% reduction in post-promotion rework/archive
- 100% promotion events include proof metadata or waiver state

Secondary:

- capture-to-promote latency
- warning rates by SATSS criterion
- repeated-error recurrence after promotion
- scope regression count

## Risks and Open Questions

1. Canonical lifecycle and validation for `check_run_id` is still undefined.
2. Event append failure policy needs explicit decision:
   - strict rollback vs. repair workflow.
3. Runtime precedence/conflict resolution across checks, packs, and `AGENTS.md` is not yet specified.
4. Minimum operator metadata for waiver accountability needs a concrete schema.

## Proposed Answers (2026-02-27)

1. `check_run_id` lifecycle and validation
   - Decision: treat `check_run_id` as immutable proof metadata tied to one promote action.
   - Validation rule:
     - must reference an existing run record in `.localagent/check_runs/` (or equivalent registry),
     - run status must be terminal (`passed` or `failed`, never `running`),
     - when `--to check`, prefer `passed` unless `--force` is set.
   - Rationale: preserves deterministic auditability and prevents dangling proof references.

2. Event append failure policy
   - Decision: strict rollback for promotion writes.
   - Rule:
     - if target write succeeds but event append fails, revert target + entry status update,
     - emit deterministic error code for operator retry (no partial promoted state).
   - Rationale: keeps entry status, target content, and event log causally consistent.

3. Runtime precedence across checks, packs, and `AGENTS.md`
   - Decision: explicit deterministic precedence order:
     - `check` (strictest, run-gated) > `pack` (team-scoped defaults) > `AGENTS.md` (global guidance fallback).
   - Conflict rule:
     - higher-precedence source wins on conflicting guidance/check semantics,
     - losing source logged in diagnostics for visibility.
   - Rationale: aligns enforcement with verification strength.

4. Minimum waiver accountability metadata
   - Decision: require the following fields when proof is waived:
     - `waived=true`,
     - `waiver_reason` (non-empty),
     - `operator_id` (or local user handle),
     - `timestamp_utc`,
     - `forced=true`.
   - Rule: missing any required field -> reject waiver path with deterministic code.
   - Rationale: supports post-hoc review and compliance without excessive operator burden.

## Implementation Plan (Near-Term)

1. Add `learn` CLI command surface and typed flags.
2. Add learn entry schema and draft lifecycle (`capture/list/show/archive`).
3. Add `promote` for `check` target with proof gating and events.
4. Add TUI overlay with PREVIEW/ARMED semantics and busy rejection.
5. Extend `promote` to `pack` and `agents` with idempotent managed insertion.

## References

External references:

- [1] Towards AI as Colleagues: Multi-Agent System Improves Structured Ideation Processes (arXiv v2). https://arxiv.org/html/2510.23904v2
- [2] Towards AI as Colleagues: Multi-Agent System Improves Structured Professional Ideation (arXiv v1). https://arxiv.org/html/2510.23904v1
- [3] Human Factors Issues of the Aircraft Checklist (Embry-Riddle). https://commons.erau.edu/cgi/viewcontent.cgi?article=1553&context=jaaer
- [4] Human Performance Considerations in the Use and Design of Aircraft Checklists (SKYbrary). https://skybrary.aero/sites/default/files/bookshelf/1566.pdf
- [5] Human factors of flight-deck checklists: The normal checklist (NASA NTRS). https://ntrs.nasa.gov/citations/19910017830
- [6] NASA Human Factors Handbook (NASA-HDBK-8709.25). https://standards.nasa.gov/sites/default/files/standards/NASA/Baseline/4/NASA-HDBK-870925-14.pdf
- [7] IJHCS UI Transition study (Fan et al.). https://www.mingmingfan.com/papers/IJHCS-UI-Transition.pdf
- [8] Designing Staged Evaluation Workflows for LLMs (arXiv). https://arxiv.org/abs/2410.02054
- [9] Comparing Criteria Development Across Experts/Lay Users/Models (arXiv). https://arxiv.org/html/2410.02054v1
- [10] ProofAug: Efficient Neural Theorem Proving (arXiv). https://arxiv.org/pdf/2501.18310
- [11] Active-security compiler thesis (University of Groningen). https://fse.studenttheses.ub.rug.nl/33067/1/mCS2024RotaLDF.pdf
- [12] HybridPlonk (IACR ePrint). https://eprint.iacr.org/2025/908.pdf
- [13] Jeffrey Emanuel Projects page (for context only). https://jeffreyemanuel.com/projects

Internal repo references:

- [R1] `src/cli_args.rs`
- [R2] `src/cli_dispatch.rs`
- [R3] `src/chat_tui_runtime.rs`
- [R4] `src/events.rs`
- [R5] `src/repro.rs`
- [R6] `src/scaffold.rs`
- [R7] `docs/reference/LEARN_OUTPUT_CONTRACT.md`

