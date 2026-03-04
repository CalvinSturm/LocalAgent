# LocalAgent Docs

Status: Active  
Owner: LocalAgent maintainers  
Last reviewed: 2026-03-03

This docs tree is organized by document type.

Normative docs (source of truth for users/operators):
- `docs/guides/`
- `docs/reference/`
- `docs/adr/`

Non-normative docs (contributor context/history only):
- `docs/research/`
- `docs/archive/`

## Doc Map

| Path | Status | Owner | Last reviewed | Notes |
|---|---|---|---|---|
| `docs/guides/*` | current | LocalAgent maintainers | 2026-02-27 | User/operator how-to docs |
| `docs/reference/*` | current | LocalAgent maintainers | 2026-02-27 | Runtime and CLI source-of-truth docs |
| `docs/adr/*` | current | LocalAgent maintainers | 2026-02-27 | Decision records |
| `docs/research/*` | historical | LocalAgent maintainers | 2026-02-27 | Non-normative research and exploration |
| `docs/archive/*` | historical | LocalAgent maintainers | 2026-03-03 | Historical planning/audit snapshots |
| `docs/ARCHITECTURE_AND_OPERATIONS.md` | historical redirect | LocalAgent maintainers | 2026-03-03 | Moved to `docs/archive/ARCHITECTURE_AND_OPERATIONS.md` |
| `docs/OOTB_AGENT_EFFECTIVENESS_AUDIT.md` | historical redirect | LocalAgent maintainers | 2026-03-03 | Moved to `docs/archive/OOTB_AGENT_EFFECTIVENESS_AUDIT.md` |
| `docs/TOOL_CALL_ACCURACY_SPEC.md` | historical redirect | LocalAgent maintainers | 2026-03-03 | Moved to `docs/archive/TOOL_CALL_ACCURACY_SPEC.md` |

## Guides

- [Install](guides/INSTALL.md)
- [LLM Provider Setup](guides/LLM_SETUP.md)
- [Templates](guides/TEMPLATES.md)

## Reference

- [CLI Reference](reference/CLI_REFERENCE.md)
- [Learn Workflow Reference](reference/LEARN_WORKFLOW_REFERENCE.md)
- [Learn Output Contract](reference/LEARN_OUTPUT_CONTRACT.md)
- [Runtime Architecture](reference/RUNTIME_ARCHITECTURE.md)
- [Instruction Profiles](reference/INSTRUCTION_PROFILES.md)
- [Safe Tool Tuning Profile](reference/SAFE_TOOL_TUNING_PROFILE.md)

## Research

- Status: Non-normative; for contributor roadmap/design context only.
- [Learn Research Brief](research/LEARN_RESEARCH_BRIEF.md)
- [Learn Research (Merged)](research/LEARN_RESEARCH_MERGED.md)

## ADR

- [ADR-0001: Learn Overlay Governance](adr/ADR-0001-learn-overlay-governance.md)

## Archive

- Status: Non-normative; historical planning snapshots.
- [Architecture and Operations Snapshot](archive/ARCHITECTURE_AND_OPERATIONS.md)
- [OOTB Agent Effectiveness Audit Snapshot](archive/OOTB_AGENT_EFFECTIVENESS_AUDIT.md)
- [Tool Call Accuracy Umbrella Spec Snapshot](archive/TOOL_CALL_ACCURACY_SPEC.md)
- [PR3/PR4 Handoff Notes](archive/CODEX_HANDOFF_PR3_PR4_AND_REMAINING_WORK.md)
- [PR4 Scope](archive/PR4_SCOPE_LEARN_PROMOTE_PACK_AND_AGENTS.md)
- [PR5 Scope](archive/PR5_SCOPE_ASSISTED_LEARN_CAPTURE.md)
- [PR6 Scope](archive/PR6_SCOPE_TUI_LEARN_COMMANDS.md)
- [PR7 Scope Archive Summary](archive/PR7_SCOPE_ARCHIVE_SUMMARY.md)

## Tool Call Accuracy PR Specs (Historical)

- [PR2 Spec](archive/TOOL_CALL_ACCURACY_PR2_SPEC.md)
- [PR3 Spec](archive/TOOL_CALL_ACCURACY_PR3_SPEC.md)
- [PR4 Spec](archive/TOOL_CALL_ACCURACY_PR4_SPEC.md)
- [PR5 Spec](archive/TOOL_CALL_ACCURACY_PR5_SPEC.md)
- [PR6 Spec](archive/TOOL_CALL_ACCURACY_PR6_SPEC.md)
- [PR7 Spec](archive/TOOL_CALL_ACCURACY_PR7_SPEC.md)
- [PR8 Spec](archive/TOOL_CALL_ACCURACY_PR8_SPEC.md)
- [PR9 Spec](archive/TOOL_CALL_ACCURACY_PR9_SPEC.md)
- [PR10 Spec](archive/TOOL_CALL_ACCURACY_PR10_SPEC.md)
- [PR11 Spec](archive/TOOL_CALL_ACCURACY_PR11_SPEC.md)
- [PR12 Spec](archive/TOOL_CALL_ACCURACY_PR12_SPEC.md)
- PR1 note: pre-PR2 baseline is captured in historical umbrella docs under `docs/archive/`.

## Manual Testing

- [Manual TUI Coding Test Pack](../manual-tui-testing/README.md)

## Releases

- [Release Notes Index](release-notes/README.md)
- [Changelog](../CHANGELOG.md)

## Project Docs

- [Contributing](../CONTRIBUTING.md)
- [Security Policy](../SECURITY.md)
- [Code of Conduct](../CODE_OF_CONDUCT.md)
