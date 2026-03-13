# Runtime Improvement Harness PR1 Plan (2026-03)

Status: implementation plan  
Derived from: [RUNTIME_IMPROVEMENT_HARNESS_RESET_2026-03.md](C:/Users/Calvin/Software%20Projects/LocalAgent/docs/policy/RUNTIME_IMPROVEMENT_HARNESS_RESET_2026-03.md)

## Request Summary

This PR is the first implementation slice of the runtime-improvement harness reset.

It focuses only on harness hygiene and repeatable execution. The goal is to ensure manual control tasks are run from fresh isolated copies, source fixture packs stay clean, and the pack documentation reflects the actual LocalAgent CLI/runtime usage.

## Relevant Codebase Findings

The current `D-tests` pack lives under [manual-testing/D-tests](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/D-tests) and already provides a useful manual control pack, but it currently mixes source fixtures with generated artifacts such as `target/` and `Cargo.lock`.

The current pack README already documents the corrected flattened CLI invocation shape in [manual-testing/D-tests/README.md](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/D-tests/README.md), but execution still depends on operators remembering to reset fixtures manually.

The existing manual pack export pattern in [export_eval_coding_pack.ps1](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/scripts/export_eval_coding_pack.ps1) already gives the repo a precedent for generated manual task packs and lightweight PowerShell-based fixture setup.

The runtime policy explicitly requires evidence before changing shared-runtime semantics and explicitly separates runtime behavior from eval harness design in [AGENT_RUNTIME_PRINCIPLES_2026.md](C:/Users/Calvin/Software%20Projects/LocalAgent/docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md). So this PR should not change runtime loop behavior.

## Proposed Implementation Approach

This PR should make the control-pack workflow mechanically safe and reproducible.

Ordered steps:

1. Clean the source `D-tests` fixture pack so it contains only prompt files and intended fixtures.
2. Add a script that creates a fresh isolated runnable copy of `D1`-`D5` under `.tmp` or another generated location.
3. Update the `D-tests` README so the default documented workflow is:
   - prepare a fresh run copy
   - enter the copied task directory
   - run `localagent` from there
4. Keep all result files outside the source fixture task directories.
5. Add a light verification test or script check that the source fixture pack does not contain generated runtime/build artifacts.

## Ordered PR Breakdown

This document covers only one PR.

1. `runtime-harness-pr1-hygiene-and-fresh-copy-execution`
   Primary goal: make manual control runs reproducible and artifact-free without changing runtime behavior.
   Dependency: none.

## Per-PR Scope Details

### PR: `runtime-harness-pr1-hygiene-and-fresh-copy-execution`

In scope:
- clean [manual-testing/D-tests](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/D-tests) so the source pack contains only intended fixtures
- remove generated artifacts from source tasks
- add a script such as `manual-testing/scripts/reset_manual_fixture_pack.ps1` or `manual-testing/scripts/run_manual_control_pack.ps1`
- update [manual-testing/D-tests/README.md](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/D-tests/README.md) to make fresh-copy execution the default workflow
- ensure results continue to live under [manual-testing/D-tests/results](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/D-tests/results) or another explicit results location, not inside task fixtures

Out of scope:
- runtime guard or finalize behavior changes
- result taxonomy redesign
- diagnostic harness design
- movement to a new top-level directory layout like `manual-testing/control/`
- new runtime-native case packs

Key files or subsystems likely to change:
- [manual-testing/D-tests](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/D-tests)
- [manual-testing/D-tests/README.md](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/D-tests/README.md)
- [manual-testing/scripts/export_eval_coding_pack.ps1](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/scripts/export_eval_coding_pack.ps1) if shared helpers are worth reusing
- new script under [manual-testing/scripts](C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/scripts)

Acceptance criteria:
- `D1`-`D5` source directories contain no `target/`, `.state/`, or prior-run outputs
- one documented script creates a fresh runnable copy of the whole pack or a selected task
- the README documents the correct sequence and the correct flattened CLI invocation
- operators no longer need to manually remember fixture reset as part of the default workflow
- no shared runtime behavior changes are introduced

Test or verification expectations:
- run the new prep/reset script and verify it creates a clean isolated copy
- verify a prepared copy contains `PROMPT.txt` and fixture files but not stale artifacts from previous runs
- manually inspect the source pack after cleanup
- optionally add a lightweight script/test that fails if `manual-testing/D-tests` contains `target/` or `.state/`

Notes on why this PR boundary is correct:
- it solves the highest-noise harness problem first: stale fixture state
- it does not mix hygiene work with schema or semantics redesign
- it improves measurement reliability immediately without requiring runtime policy decisions
- it gives later PRs a clean base for unified results and runtime-native case packs

## Risks / Open Questions

Risk:
- deleting generated artifacts from the source pack is straightforward, but the repo should decide whether `Cargo.lock` belongs in manual control fixtures on a per-task basis. For this PR, the safe rule is: keep only files intentionally needed for the fixture and remove obviously generated directories like `target/` and `.state/`.

Open question:
- whether the fresh-copy script should prepare the whole pack at once or one named task at a time. The likely best default is to support both, with whole-pack copy as the simple path and single-task copy as the faster path for repeated manual runs.
