# LOCALAGENT_CODING_UX_PR_STACK.md

## PR1 — Measurement first: common coding UX benchmark + frozen baseline

**Objective**
Land the measurement surface first so all later tuning has a defensible before/after comparison.

**Why first**
The repo already has the eval runner seam, and the repo guidance favors evidence-led change over speculative runtime edits. This PR should land on the existing eval path first, not broaden checks/runtime architecture before the benchmark exists. It creates the benchmark and frozen baseline that every later PR should be judged against. ([GitHub][1])

**In scope**

* add `common_coding_ux` benchmark pack on the eval path
* fixture-backed tasks for common coding workflows
* raw per-run UX metrics
* frozen baseline capture and report format

**Out of scope**

* runtime-loop changes
* weighted composite scoring
* new execution modes
* presumed checks-schema redesign
* checks-subsystem expansion unless a concrete eval reporting gap requires it

**Key files**

* `src/eval/tasks.rs`
* `src/eval/runner.rs`
* `src/eval/runner_artifacts.rs`
* `src/eval/fixtures_repo.rs`
* `src/store/io.rs` only if artifact/report persistence needs a narrow extension
* `tests/common_coding_ux_eval.rs`
* fixture directories under `tests/fixtures/common_coding_ux/...` or dedicated eval fixtures
* `src/checks/schema.rs` **only if** a concrete authored-contract/reporting gap is proven during the eval landing

**Hard rule**

* preserve `tests/tool_call_accuracy_ci.rs` as protocol/regression coverage, while allowing small adjacent assertions only if needed for compatibility

Working draft:
- [COMMON_CODING_UX_BENCHMARK_DRAFT.md](/C:/Users/Calvin/Software%20Projects/LocalAgent/docs/operations/COMMON_CODING_UX_BENCHMARK_DRAFT.md)

---

## PR2 — Profile/task-kind naming cleanup without semantic rollback

**Objective**
Separate human-facing overlay/profile naming from canonical runtime semantics without undoing the explicit task-profile contract path that vNext intentionally landed.

**Why second**
The current runtime intentionally allows explicit task-profile selection to act as an explicit `task_kind` source. The problem to fix is narrower: arbitrary overlay naming and display/preset labels should not silently change semantics unless they are explicitly mapped to canonical task kinds. ([GitHub][2])

**In scope**

* canonical `task_kind` remains authoritative
* preserve intentionally authored explicit task-profile to canonical task-kind mapping
* add `coding_subkind` only if a downstream phase truly needs it
* keep overlay/display naming prompt-only unless explicitly mapped
* unknown overlays must not affect write/validation/guard semantics

**Out of scope**

* broad persisted taxonomy expansion
* runtime-loop changes
* planner policy changes
* removal of explicit `instruction_task_profile` as a semantic source where intentionally authored

**Key files**

* `src/agent/task_contract.rs`
* `src/instruction_runtime.rs`
* `src/instructions.rs`
* `src/project_guidance.rs`
* `docs/guides/INSTRUCTION_PROFILES.md`

**Hard rule**

* prefer resolution-time metadata first
* persist extra fields only if planner/eval/artifacts must observe them
* do not reopen the vNext decision that explicit task-profile metadata can drive canonical task-kind resolution

---

## PR3 — Coding UX shaping via instruction profiles and evaluation criteria

**Objective**
Improve coding-task UX through instruction/task shaping and explicit benchmark expectations before changing shared completion semantics.

**Why third**
Post-vNext guidance says not to reopen shared runtime semantics without a proven blocker. The next highest-leverage seam is instruction/task shaping plus benchmark scoring for coding-task closeout quality. ([GitHub][3])

**In scope**

* add concise coding-oriented instruction/task overlays
* make active instruction/profile/guidance layers more visible in artifacts or reporting
* score coding-task closeout quality in the benchmark, including changed files, validation result, and unresolved risk when those are part of the task expectation

**Out of scope**

* new validation subsystem
* runtime-loop redesign
* repo-wide mandatory coding final-answer schema in shared completion policy
* silent fallback paths for required validation

**Key files**

* `src/instruction_runtime.rs`
* `src/instructions.rs`
* `src/project_guidance.rs`
* `src/eval/runner_artifacts.rs`
* `docs/guides/INSTRUCTION_PROFILES.md`

---

## PR4a — Small context-shaping PR: bounded structural grounding

**Objective**
Improve bounded structural grounding using the existing LSP/repo context path.

**Why separate from PR3**
This keeps context plumbing reviewable and avoids turning a safe tuning PR into a broad runtime-shaping pass.

**In scope**

* bounded structural summaries
* ranked likely files/symbols
* explicit clean fallback when LSP is absent
* context-size budget enforcement

**Out of scope**

* persistent indexing project
* parallel repo-map subsystem
* transcript replay redesign

**Gate**

* do not start PR4a unless PR1 and PR3 still show repeated file-targeting or repo-navigation weakness that instruction shaping alone did not already fix

**Key files**

* `src/lsp_context.rs`
* `src/lsp_context_provider.rs`
* `src/repo_map.rs` if already used as part of the existing context path
* `src/compaction.rs`
* `docs/reference/FILE_AND_SYMBOL_INDEX.md`

**Hard rule**

* any changes to compaction or chat/runtime-facing context assembly must be limited to bounded injection, visibility, and trace/reporting behavior
* no completion/finalization semantic changes

---

## PR4b — Explicit pack/task metadata for coding contracts

**Objective**
Extend the next explicit authoring surface for coding-task semantics so common workflows rely less on prompt wording and ad hoc CLI combinations.

**Why here**
The heuristic reconciliation closeout explicitly calls out manual-pack/task metadata as the next follow-on authoring surface for validator/output/task semantics. This is a better next step than reopening runtime-loop behavior.

**In scope**

* explicit pack/task metadata for `task_kind`, validation expectations, and exact final-answer requirements where common coding workflows need them
* preserve current CLI override behavior
* add targeted tests and docs for authored contract precedence

**Out of scope**

* broad pack-system redesign
* new runtime-loop behavior
* planner policy expansion

**Key files**

* `src/packs.rs`
* `src/taskgraph.rs`
* `src/task_apply.rs`
* relevant eval/check metadata loaders where authored contracts are consumed
* `docs/reference/CLI_REFERENCE.md`

**Hard rule**

* prefer explicit authored metadata over new prompt heuristics
* do not broaden shared runtime semantics unless the authored-surface landing proves a real gap

---

## PR5 — Planner routing for selected coding cases

**Objective**
Add planner-first routing for selected coding tasks without changing canonical semantics.

**Why later and only if needed**
Planner work is safer once semantic coupling is fixed and success criteria are already tightened, but it should only move forward if PR1-PR4 still show planning/routing as a repeated LocalAgent-side blocker on common coding tasks.

**In scope**

* planner routing based on canonical `task_kind == "coding"`
* optional `coding_subkind`
* ambiguity/scope rules
* preserve trivial-task bypass

**Out of scope**

* new canonical task kinds
* overlay-driven semantics
* planner-everywhere policy
* new execution mode system

**Gate**

* do not start PR5 unless benchmark evidence after PR1-PR4 shows a repeated routing/planning failure that instruction shaping, explicit authored metadata, and bounded grounding did not already fix

**Key files**

* `src/planner_runtime.rs`
* `src/planner.rs`
* `src/agent/task_contract.rs`
* `src/cli_args.rs`
* `src/agent_runtime/planner_phase.rs`

---

## PR6 — Experimental tool-result normalization hook path

**Objective**
Add an experimental, opt-in, repo-scoped normalization path by extending the verified **`tool_result` hook/reporting surface**, with results emitted as structured observations through existing checks/reporting.

**Why late**
This is still the most speculative non-runtime-core surface, and it should stay clearly behind config and off by default. The verified seam today is the hook runner’s `tool_result` path, so this PR should extend that path rather than imply a separate first-class post-write lifecycle stage. ([GitHub][5])

**In scope**

* extend the existing `tool_result` hook/reporting surface in an opt-in way
* structured observations only
* integration with existing checks/reporting surfaces
* off by default

**Out of scope**

* does not govern completion semantics
* does not introduce mandatory lifecycle orchestration
* failures surface as structured observations only
* no hidden side-effect governance
* no presumed distinct `post_write` lifecycle subsystem

**Gate**

* do not start PR6 unless benchmark evidence after PR1-PR5 shows that structured post-tool observations would solve a repeated coding-task blocker that eval shaping, explicit authored contracts, and planner/routing work did not already address

**Key files**

* `src/hooks/mod.rs`
* `src/hooks/runner.rs`
* `src/checks/mod.rs`
* `src/checks/schema.rs`
* `src/agent_runtime.rs`
* `docs/guides/SAFE_TOOL_TUNING_BASELINE.md`

---

## PR7 — Conditional runtime/tool follow-on only if benchmark evidence still shows a repeated LocalAgent-side blocker

**Objective**
Only after PR1–PR6, and only if benchmark evidence still shows a repeated LocalAgent-side blocker, apply the smallest necessary runtime/tool follow-on.

**This is not a planned phase.**
It is a gated option.

**Default priority order**

1. verification closure
2. edit-surface tightening
3. permission/policy clarification
4. execution-mode work only if specifically proven

**Out of scope by default**

* broad runtime rewrite
* speculative loop cleanup
* predeclared runtime-core file sweep
* multi-agent orchestration
* top-level Plan/Build split unless benchmark evidence specifically proves it is needed

**Hard rule**

* modify only the smallest set of files required by the proven blocker
* no “just in case” runtime edits

The repo guidance explicitly says not to justify runtime-loop architecture changes from one weak local-model eval alone and to require proving evidence before changing shared runtime-loop semantics. ([GitHub][1])

# Recommended order

1. **PR1** measurement + frozen baseline
2. **PR2** profile/task-kind naming cleanup without semantic rollback
3. **PR3** coding UX shaping via instruction profiles and evaluation criteria
4. **PR4a only if PR1 and PR3 still show file-targeting or repo-navigation weakness**
5. **PR4b** explicit pack/task metadata for coding contracts
6. **PR5 only if PR1-PR4 still show routing/planning as a blocker**
7. **PR6 only if PR1-PR5 still show a structured post-tool observation gap**
8. **PR7 only if benchmark evidence still justifies it**

# One-line architecture summary

**Eval-first measurement defines the benchmark, canonical kind defines runtime semantics, explicit task-profile mapping stays intentional, overlays shape instructions, bounded structural context improves targeting, authored pack/task metadata reduces prompt fragility, hook work stays on the verified `tool_result` path, and runtime-core changes happen only when benchmark evidence proves a repeated LocalAgent-side blocker remains.**

[1]: https://raw.githubusercontent.com/CalvinSturm/LocalAgent/main/AGENTS.md "raw.githubusercontent.com"
[2]: https://github.com/CalvinSturm/LocalAgent/blob/main/src/agent/task_contract.rs "LocalAgent/src/agent/task_contract.rs at main · CalvinSturm/LocalAgent · GitHub"
[3]: https://github.com/CalvinSturm/LocalAgent/blob/main/src/agent/completion_policy.rs "LocalAgent/src/agent/completion_policy.rs at main · CalvinSturm/LocalAgent · GitHub"
[4]: https://github.com/CalvinSturm/LocalAgent/blob/main/src/instruction_runtime.rs "LocalAgent/src/instruction_runtime.rs at main · CalvinSturm/LocalAgent · GitHub"
[5]: https://github.com/CalvinSturm/LocalAgent/blob/main/src/hooks/runner.rs "LocalAgent/src/hooks/runner.rs at main · CalvinSturm/LocalAgent · GitHub"
