# LocalAgent Runtime Loop Policy (2026)

**Status:** Repo-local runtime policy memo
**Applies to:** LocalAgent shared runtime-loop behavior and closely related finalize, retry, guard, and validator semantics
**Primary governed surfaces:** `src/agent.rs`, `src/agent/runtime_completion.rs`, `src/agent/run_finalize.rs`, `src/agent/run_setup.rs`, `src/agent_impl_guard.rs`, and contract tests covering those paths
**Audience:** Maintainers and coding agents editing LocalAgent runtime behavior

---

## Purpose

This document defines the repo-local policy for changing the LocalAgent runtime loop.

It exists to keep the shared runtime narrow, auditable, and reproducible as the codebase evolves. The goal is to preserve a stable execution core and require evidence before changing loop semantics.

This document is normative only within this repo and only for the governed runtime surfaces listed above. It does not supersede `AGENTS.md`; it operationalizes the runtime-loop policy described there and should be interpreted consistently with it. If a future ADR or explicitly adopted design decision changes these defaults, that decision should be linked from this document.

---

## Conflict resolution

If current code or tests conflict with this policy, the conflict should be resolved explicitly in the same change set by either:

* updating code and tests to match policy
* updating this policy through an adopted ADR or linked design decision

---

## Scope

This policy applies to:

* continuation and terminal conditions in the shared runtime loop
* retry classification and retry bounds
* finalize/result shaping behavior
* protocol-artifact handling in assistant output
* closely related guard and validator semantics that can authorize another turn

This policy does not automatically govern every runtime-adjacent surface. In particular, it does not by itself define policy for:

* eval harness design
* future orchestration systems
* artifact-only changes
* logging-only or observability-only changes
* isolated wording changes that do not alter control flow or result semantics

Those areas may reference this document when relevant, but they are not governed by it unless explicitly adopted.

Changes outside the governed surfaces should reference this policy only when they materially affect the shared runtime-loop contract.

---

## Core thesis

In this repo, the shared runtime is not treated as an open-ended loop.

It is a policy-enforced execution state machine with:

* explicit permissions
* explicit tool boundaries
* explicit recovery classes
* explicit validation steps
* explicit terminal conditions
* explicit artifacts for replay and audit

Tool success alone is not a continuation reason.

The runtime may request another model turn only when a classified, bounded reason exists.

---

## Primary runtime invariant

**The runtime may only request an additional model turn when a classified, bounded continuation condition is present.**

In this repo, a verified successful write is terminal by default unless continuation is authorized by an explicit validator failure, classified retry condition, declared phase transition, or user-directed follow-on step.

This is a repo default, not a universal law. A PR may override it only with explicit local evidence, type-level clarity where applicable, and contract tests.

---

## Secondary invariants

### 1. Recovery and continuation are not the same thing

The runtime must not use one ambiguous path to represent both:

* deterministic corrective retry
* general workflow continuation

If both semantics exist, they should remain explicit in code, logs, and tests.

### 2. Side effects do not imply progress

A file write, shell command, or patch application is evidence that something happened. It is not, by itself, evidence that the task should continue.

### 3. Validation governs continuation

Continuation should be driven by explicit state such as:

* failed validator
* failed guard
* unmet acceptance check
* required next phase declared by the runtime contract
* user-requested decomposition step

Not by the mere fact that a tool succeeded or that the model sounded unfinished.

### 4. Finalization is a contract boundary

Finalize paths must preserve structurally valid assistant output where appropriate, reject protocol artifacts, and emit a coherent terminal result.

### 5. Heuristics require evidence

Heuristics are allowed only as narrow compatibility shims for observed failures. They are not a substitute for runtime architecture.

---

## Allowed continuation classes

The runtime may continue only for reasons that fit one of the following repo-local classes.

### A. Classified recovery

A known runtime or guard defect requires one bounded corrective retry.

Examples:

* guard misfinalization
* deterministic wrapper/protocol correction with a documented retry contract
* deterministic tool-result reconciliation failure with a bounded retry rule

### B. Validator-driven continuation

An explicit validator has failed and the runtime contract says another model turn is required to address it.

Examples:

* tests failed
* lint failed
* schema invalid
* acceptance criteria not met
* output artifact incomplete

### C. Declared phase transition

The runtime, task contract, or user request explicitly requires another phase.

A declared phase transition must come from explicit runtime state, task contract, or user instruction, not from inferred assistant intent alone.

Examples:

* plan approved, now execute
* implementation finished, now summarize or produce a declared artifact
* patch applied, now run the required validation phase

### D. User-directed follow-on step

The user explicitly wants iteration after an otherwise successful step.

User-directed follow-on steps must be explicit in the current request or adopted task plan, not inferred after a successful terminal action.

Examples:

* "make the change, then refactor"
* "write the file, then generate tests"
* "apply the patch, then draft the changelog"

---

## Disallowed continuation classes

The runtime must not continue for any of the following reasons.

### 1. Successful verified write alone

A verified write is terminal-ready by default unless some other explicit continuation condition is present.

### 2. Generic "keep going" behavior after tool success

Do not synthesize an extra model turn just because the last step succeeded.

### 3. Ambiguous assistant verbosity

A model sounding unfinished is not, by itself, a runtime continuation class.

### 4. Test-shape inertia

Do not preserve extra turns because a historical regression test happened to assert them.

### 5. Weak heuristic matching

Do not broaden loop behavior based on speculative pattern matching unless it is tied to a reproduced artifact.

---

## Recovery policy

### One retry maximum per offending turn

A retry-class recovery condition may trigger at most one corrective retry for the offending turn.

### Recovery must be machine-identifiable

The reason for recovery must be encoded in runtime state and, where available, logs as a classified cause, not buried only in free text.

### Recovery must be local

A corrective retry must preserve task state except for the minimal corrective message or structural repair required by the recovery path.

### Recovery must be auditable

The runtime must emit enough information to answer:

* why was a retry allowed?
* what condition triggered it?
* why was only one retry allowed?
* what changed between the original attempt and retry?

---

## Type and control-flow guidance

Do not collapse retry recovery and general workflow continuation into one ambiguous path.

Preferred patterns include separate result variants or separate branches such as:

* `TerminalSuccess`
* `TerminalFailure`
* `RetryAfterGuard`
* `RetryAfterValidation`
* `EnterNextPhase`

Equivalent designs are fine, but the semantics should be explicit.

If a single type currently carries multiple meanings, the burden is on the change author to justify that choice and show why the ambiguity is controlled.

---

## Finalization rules

Finalize paths must satisfy all of the following:

### Preserve valid assistant output

Do not drop the last valid assistant message in planner-error or near-terminal finalize paths if it remains structurally relevant.

### Reject protocol artifacts

Protocol wrappers, envelopes, or transport-shaped intermediate content must never become final user-visible output.

### Emit structurally coherent terminal results

Terminal output must conform to the runtime's result contract even when the model or tool layer behaved imperfectly.

### Prefer terminal correctness over loop continuation

If the runtime already has a valid terminal result, finalize it. Do not ask for another model turn just to polish the shape unless a validator or declared phase contract explicitly requires that.

---

## Heuristic admission policy

A runtime heuristic is allowed only if backed by at least one of:

* reproduced failing eval
* captured real-run artifact
* observed transcript that can be regression-tested
* precise bug report with exact offending output

Synthetic-only tests do not justify new runtime heuristics.

If a heuristic is kept, it should be:

* narrow
* named
* justified
* logged when activated when practical
* covered by a regression derived from the real failure
* removable once the underlying structural issue is fixed

---

## Guard and validator model

### Guards

Guards protect the runtime from unsafe or structurally invalid behavior.

Examples:

* permission boundary checks
* write policy enforcement
* protocol-artifact rejection
* malformed finalize protection

### Validators

Validators determine whether the task outcome is complete and correct enough to stop.

Examples:

* tests
* lint
* schema checks
* acceptance criteria checks
* artifact integrity checks

### Key rule

Guards and validators may authorize another turn. Tool success alone may not.

Validator failure must be machine-identifiable at the runtime boundary, not inferred only from assistant narration.

---

## Test philosophy

Tests should encode runtime contract, not incidental loop shape.

### Good runtime tests

* verified write produces a structurally valid terminal result
* protocol wrapper cannot become final output
* retryable guard failure allows exactly one retry
* no extra turn occurs without a classified reason
* finalize path preserves valid assistant content where required

### Bad runtime tests

* must continue to a third request because the current implementation happens to do so
* must emit a generic recovery message after every successful write
* must preserve a heuristic without a reproduced artifact

### Required negative testing

For every allowed continuation class, add negative tests proving nearby non-class cases do not continue.

---

## Evidence standard for runtime changes

Before changing the governed runtime surfaces, gather proving evidence from:

* code-path inspection
* run artifacts
* event streams
* focused transcripts
* targeted regressions derived from real failures

Separate confirmed facts from open hypotheses.

If the runtime is behaving correctly and the model still loops, ignores instructions, emits empty answers, or recovers poorly, treat that as model behavior unless stronger evidence shows otherwise.

---

## Code review checklist for runtime changes

A runtime PR touching the governed surfaces should answer:

1. What state transition is being added or changed?
2. What explicit condition authorizes this transition?
3. Is the transition terminal, retry, validator-driven, or next-phase?
4. How is the reason encoded in types, branches, or logs?
5. What regression proves the positive case?
6. What regression proves the nearby negative case?
7. Is any heuristic tied to a reproduced artifact?
8. Does this broaden loop behavior? If yes, why is that justified?
9. Can the same goal be met with a validator, hook, clearer phase boundary, or other extension point instead?
10. Does this improve or reduce auditability?

---

## Rules for coding agents in this repo

When editing the governed runtime surfaces:

### Do

* preserve explicit runtime invariants
* prefer smaller, typed state transitions
* add contract tests for each new transition
* require evidence before introducing heuristics
* separate retry paths from workflow continuation
* keep verified successful writes terminal by default unless an explicitly classified reason overrides that default

### Do not

* add "always continue" behavior after writes
* collapse recovery and continuation into one ambiguous path
* add open-ended retries
* bless architectural drift by writing broad loop-shape tests
* keep speculative heuristics without artifacts

---

## Default repo decision

Unless a future ADR or explicitly adopted design decision says otherwise, this repo uses the following default:

**A successful verified write is terminal by default.**

**A failed validator, failed guard, unmet declared phase boundary, or explicit user-directed next step is what authorizes continuation.**

---

## One-sentence summary

**In LocalAgent, the shared runtime loop is a policy-enforced state machine, and it may continue only when an explicit, classified, bounded reason exists.**
