# ADR-0001: Learn Overlay Governance

Status: Accepted  
Date: 2026-02-27  
Owners: LocalAgent maintainers

## Context

`/learn` introduces behavior-affecting memory publication. Without strict controls, captured notes can become ambiguous or unsafe runtime guidance.

## Decision

1. `/learn` uses staged lifecycle boundaries:
   - capture/review are draft-only
   - promote is the only behavior-changing action
2. Promotion defaults to proof-required:
   - accepted proof: `check_run_id` or `replay_verify_run_id`
   - waiver allowed only with explicit force semantics
3. Overlay and CLI remain preview-first:
   - preview must perform zero writes
   - arming is explicit and operator-controlled
4. Promotion must be auditable with deterministic metadata:
   - source entry hash
   - target hash
   - proof/waiver state
   - append-only event record

## Consequences

Positive:

- Reduces accidental low-quality or unverified behavior changes.
- Keeps operator intent explicit and auditable.
- Aligns with deterministic execution and replay-verification design.

Tradeoffs:

- Higher promotion friction than freeform note publishing.
- Requires stronger schema and validation logic in learn handlers/UI.

## Related Documents

- `docs/reference/LEARN_OUTPUT_CONTRACT.md`
- `docs/reference/LEARN_WORKFLOW_REFERENCE.md`
- `docs/research/LEARN_RESEARCH_MERGED.md`
