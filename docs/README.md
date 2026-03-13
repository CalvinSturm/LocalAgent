# LocalAgent Docs

Status: Active  
Owner: LocalAgent maintainers  
Last reviewed: 2026-03-13

This docs tree is organized by document type.

Normative docs (source of truth for users/operators):
- `docs/architecture/`
- `docs/operations/`
- `docs/guides/`
- `docs/reference/`
- `docs/policy/`
- `docs/adr/`

Non-normative docs (contributor context/history only):
- `docs/research/`
- `docs/archive/`

## Doc Map

| Path | Status | Owner | Last reviewed | Notes |
|---|---|---|---|---|
| `docs/architecture/*` | current | LocalAgent maintainers | 2026-03-13 | Deep system maps, runtime targets, and execution-flow handoffs |
| `docs/operations/*` | current | LocalAgent maintainers | 2026-03-13 | Operational debugging, repro, and incident docs |
| `docs/guides/*` | current | LocalAgent maintainers | 2026-03-07 | User/operator how-to docs |
| `docs/reference/*` | current | LocalAgent maintainers | 2026-03-07 | Runtime, state, and CLI reference docs |
| `docs/policy/*` | current | LocalAgent maintainers | 2026-03-07 | Repo-local runtime policy and review requirements |
| `docs/adr/*` | current | LocalAgent maintainers | 2026-02-27 | Decision records |
| `docs/research/*` | historical | LocalAgent maintainers | 2026-02-27 | Non-normative research and exploration |
| `docs/archive/*` | historical | LocalAgent maintainers | 2026-03-13 | Historical planning/audit snapshots |
| `docs/ARCHITECTURE_AND_OPERATIONS.md` | historical redirect | LocalAgent maintainers | 2026-03-03 | Moved to `docs/archive/ARCHITECTURE_AND_OPERATIONS.md` |
| `docs/OOTB_AGENT_EFFECTIVENESS_AUDIT.md` | historical redirect | LocalAgent maintainers | 2026-03-03 | Moved to `docs/archive/OOTB_AGENT_EFFECTIVENESS_AUDIT.md` |
| `docs/TOOL_CALL_ACCURACY_SPEC.md` | historical redirect | LocalAgent maintainers | 2026-03-03 | Moved to `docs/archive/TOOL_CALL_ACCURACY_SPEC.md` |

Root-level redirect stubs in `docs/` are compatibility shims only. They should not receive new substantive edits; new or maintained content belongs in the current canonical doc under `architecture/`, `operations/`, `reference/`, `policy/`, `guides/`, `adr/`, `research/`, or `archive/` as appropriate.

## Architecture

- [Runtime Architecture](architecture/RUNTIME_ARCHITECTURE.md)
- [vNext Runtime Target](architecture/LOCALAGENT_VNEXT_RUNTIME_TARGET.md)
- [vNext Runtime Handoff](architecture/LOCALAGENT_VNEXT_RUNTIME_HANDOFF.md)

## Operations

- [Operational Runbook](operations/OPERATIONAL_RUNBOOK.md)

## Guides

- [Install](guides/INSTALL.md)
- [LLM Provider Setup](guides/LLM_SETUP.md)
- [Templates](guides/TEMPLATES.md)
- [Instruction Profiles](guides/INSTRUCTION_PROFILES.md)
- [Safe Tool Tuning Baseline](guides/SAFE_TOOL_TUNING_BASELINE.md)
- [Human-in-the-Loop Checklist](guides/HUMAN_IN_THE_LOOP_CHECKLIST.md)

## Reference

- [CLI Reference](reference/CLI_REFERENCE.md)
- [Configuration and State](reference/CONFIGURATION_AND_STATE.md)
- [File and Symbol Index](reference/FILE_AND_SYMBOL_INDEX.md)
- [Learn Workflow Reference](reference/LEARN_WORKFLOW_REFERENCE.md)
- [Learn Output Contract](reference/LEARN_OUTPUT_CONTRACT.md)

## Policy

- [Runtime Loop Policy](policy/AGENT_RUNTIME_PRINCIPLES_2026.md)
- [Runtime Change Review Template](policy/AGENT_RUNTIME_CHANGE_REVIEW_TEMPLATE.md)

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
- [vNext Runtime Next-Agent Handoff](archive/LOCALAGENT_VNEXT_RUNTIME_NEXT_AGENT_HANDOFF_2026-03.md)
- [Runtime Artifact/Checkpoint Consistency Hardening Plan](archive/RUNTIME_ARTIFACT_CHECKPOINT_CONSISTENCY_HARDENING_PLAN_2026-03.md)
- [Agent Runtime Audit Implementation Plan](archive/AGENT_RUNTIME_AUDIT_IMPLEMENTATION_PLAN_2026-03.md)
- [Runtime Improvement Harness PR1 Plan](archive/RUNTIME_IMPROVEMENT_HARNESS_PR1_PLAN_2026-03.md)
- [Clippy Runtime Cleanup Assessment](archive/CLIPPY_RUNTIME_CLEANUP_ASSESSMENT_2026-03.md)

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

- [Repo Entry Guide](../AGENTS.md)
- [Contributing](../CONTRIBUTING.md)
- [Security Policy](../SECURITY.md)
- [Code of Conduct](../CODE_OF_CONDUCT.md)
