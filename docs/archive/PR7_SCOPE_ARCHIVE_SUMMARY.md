# PR7 Scope Archive Summary (`PR7` + `PR7B` + `PR7D`)

Status: Archived  
Owner: LocalAgent maintainers  
Last reviewed: 2026-02-27

## Purpose

This archive consolidates historical scope docs for the TUI `/learn` overlay track:

- `PR7_SCOPE_TUI_LEARN_OVERLAY_UX.md` (Finalized)
- `PR7B_SCOPE_TUI_LEARN_OVERLAY_REVIEW_PROMOTE.md` (Finalized)
- `PR7D_SCOPE_GUIDED_CAPTURE_AND_EVAL_PROOF.md` (Draft)

It preserves key scope decisions while removing redundant standalone planning files.

## Status Snapshot

- `PR7`: finalized base overlay shell and capture execution path through existing learn adapter.
- `PR7B`: finalized Review and Promote tab UX behavior and field requirements.
- `PR7D`: draft proposal for guided capture and eval-proof gating expansion.

Current canonical policy/research direction is documented in:

- `docs/research/LEARN_RESEARCH_MERGED.md`
- `docs/reference/LEARN_OUTPUT_CONTRACT.md`
- `docs/reference/LEARN_WORKFLOW_REFERENCE.md`

## Consolidated Scope Outcomes

### Overlay and lifecycle

- `/learn` opens overlay modal.
- `Esc` closes overlay.
- Tabs: Capture, Review, Promote.
- Overlay actions stay in learn-log context and do not append assistant transcript rows.

### Preview and write safety

- `PREVIEW` performs zero writes.
- `ARMED` is explicit operator-controlled write mode.
- Busy submit is rejected with deterministic code:
  - `ERR_TUI_BUSY_TRY_AGAIN`

### Backend parity rule

- Overlay dispatch must reuse existing slash adapter/backend logic.
- No duplicated promote/capture business logic inside TUI-specific code.

### Review/Promote behavior

- Review remains read-only (`list/show` style behavior).
- Promote requires target-specific required fields:
  - `check` requires `slug`
  - `pack` requires `pack_id`
  - `agents` has no extra target id field

### Guided capture and proof gating (from PR7D draft)

Proposed additions preserved from draft:

- Guided category templates with required fields.
- Capture quality gate with deterministic failure code:
  - `LEARN_CAPTURE_QUALITY_GATE_FAILED`
- Promotion eval-proof requirement with deterministic failure codes:
  - `LEARN_PROMOTE_EVAL_PROOF_REQUIRED`
  - `LEARN_PROMOTE_NO_EVAL_PROOF_REQUIRES_FORCE`
- Waiver path only with explicit force semantics.

## Acceptance Test Themes Preserved

1. Overlay open/close and tab behavior.
2. Strict preview no-write guarantees.
3. Busy-state deterministic rejection behavior.
4. Promote field validation by target.
5. Guided capture progression and quality-gate enforcement (draft stream).
6. Eval-proof metadata presence in entry/event for promote (draft stream).

## Supersession Note

This archive supersedes the three standalone PR7 scope docs listed above for navigation and maintenance.
Historical intent is preserved here; active requirements should be maintained in canonical learn docs.

