# LocalAgent Runtime Change Review Template

**Status:** PR review template
**Use for:** Substantial changes to shared runtime behavior
**Primary policy sources:** `AGENTS.md`, `docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md`
**Typical governed files:** `src/agent.rs`, `src/agent/runtime_completion.rs`, `src/agent/run_finalize.rs`, `src/agent/run_setup.rs`, `src/agent_impl_guard.rs`

---

## When this template is required

Use this template for PRs that materially change one or more of:

* runtime control flow
* finalize semantics
* retry behavior
* continuation policy
* protocol-artifact handling
* guard or validator behavior that can authorize another turn
* permissioned execution behavior with runtime-control implications

This template is usually not required for:

* logging-only changes
* artifact-only changes
* observability-only changes
* wording-only changes
* refactors with no behavior change outside governed runtime semantics

If in doubt, use it.

---

## Purpose

Use this template to prevent architectural drift in the shared runtime by forcing each substantial change to state:

* what state transition is changing
* what explicit condition authorizes it
* whether it is terminal, retry, validator-driven, or phase-driven
* what evidence justifies it
* how it is tested
* how it affects auditability and reproducibility

If a PR cannot be reviewed cleanly through this template, the change is probably underspecified.

---

## Instructions

* Copy this template into the PR description for qualifying runtime changes.
* Fill out every applicable section.
* If a section does not apply, say why.
* If this PR intentionally overrides a repo default from `AGENTS.md` or `docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md`, explain why and link the adopting ADR or design decision if one exists.
* Short answers are acceptable for narrow qualifying changes as long as the behavioral impact is still explicit.

---

## PR Metadata

**PR Title:**
**Author:**
**Date:**
**Related issue(s):**
**Related eval(s):**
**Related artifact(s):**
**Touches files:**

---

## 1. Change Summary

### What is changing?

Describe the runtime behavior being added, removed, narrowed, or refactored.

### Why is it changing?

Describe the user-visible or runtime-level defect, ambiguity, or missing capability.

### What category of change is this?

Mark all that apply.

- [ ] Terminal behavior
- [ ] Retry behavior
- [ ] Guard behavior
- [ ] Validator behavior
- [ ] Phase transition behavior
- [ ] Tool permission behavior
- [ ] Finalize/result shaping
- [ ] Heuristic addition
- [ ] Heuristic removal
- [ ] Logging / artifacts only
- [ ] Refactor with no behavior change

---

## 2. Runtime Contract Impact

### Which repo runtime default or invariant is affected?

Reference the exact principle from `AGENTS.md` or `docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md`.

- [ ] The runtime may continue only for a classified, bounded reason
- [ ] Verified successful write is terminal by default in this repo
- [ ] Recovery and continuation remain distinct
- [ ] Finalize paths preserve valid assistant output
- [ ] Protocol artifacts do not become final output
- [ ] Heuristics require evidence
- [ ] Retry is bounded to one corrective retry per offending turn
- [ ] Permissions remain runtime policy, not prompt-only behavior

### Is this change narrowing or broadening runtime behavior?

- [ ] Narrowing
- [ ] Broadening
- [ ] Neutral refactor

### If broadening, what explicit new continuation or recovery class is being introduced?

Be precise. If none, say none.

---

## 3. State Transition Definition

### What explicit state transition is being changed?

Describe it as a before/after transition.

**Before:**
**After:**

### What authorizes this transition?

Choose one primary class.

- [ ] Classified recovery
- [ ] Validator-driven continuation
- [ ] Declared phase transition
- [ ] User-directed follow-on step
- [ ] Terminal success
- [ ] Terminal failure

Declared phase transitions must come from explicit runtime state, task contract, or user instruction, not inferred assistant intent alone.

### Why is that classification correct?

Explain why this is not one of the other classes.

### Does this change add a new model turn?

- [ ] Yes
- [ ] No

### If yes, what exact condition permits that extra turn?

State the machine-identifiable reason.

---

## 4. Recovery vs Continuation Check

### Does this PR risk mixing retry recovery with workflow continuation?

- [ ] Yes
- [ ] No

### If no, explain how the distinction remains explicit in code.

Reference types, branches, or state fields.

### If yes, why is that unavoidable?

This should be rare. Explain the tradeoff and how ambiguity is controlled.

### Does a single enum or result type currently represent multiple meanings?

- [ ] Yes
- [ ] No

### If yes, what is the plan?

- [ ] Split the type
- [ ] Rename to narrower semantics
- [ ] Defer with justification
- [ ] Not applicable

---

## 5. Evidence Standard

### What evidence justifies this change?

Mark all that apply and link the source.

- [ ] Reproduced failing eval
- [ ] Captured real-run artifact
- [ ] Observed transcript
- [ ] Precise bug report with exact offending output
- [ ] Existing regression derived from a real failure
- [ ] Purely synthetic test
- [ ] Refactor only, no behavioral evidence required

### Evidence links / references

List exact files, logs, eval names, transcripts, or artifacts.

1.
2.
3.

### If any behavior change is justified only by synthetic tests, explain why that is acceptable.

This should be rare.

---

## 6. Heuristic Review

### Does this PR add or broaden a heuristic?

- [ ] Yes
- [ ] No

### If yes, fill out all of the following

**Heuristic name:**
**Exact pattern matched:**
**Why the structural fix is not sufficient right now:**
**Observed artifact that justifies it:**
**How activation is logged:**
**How the heuristic is kept narrow:**
**Conditions under which it can be removed:**

### If removing a heuristic

**Which heuristic is being removed:**
**Why removal is safe:**
**What evidence shows it is no longer justified:**

---

## 7. Finalization Review

### Does this PR affect finalize paths?

- [ ] Yes
- [ ] No

### If yes, confirm each applicable statement

- [ ] Valid assistant output is preserved where structurally appropriate
- [ ] Protocol artifacts are still rejected from final output
- [ ] Terminal result remains structurally coherent
- [ ] Finalization does not trigger another turn unless explicitly classified
- [ ] Planner-error or near-terminal paths were checked

### What terminal shape should result after this change?

Describe the expected terminal output contract.

---

## 8. Tool Success and Continuation Review

### Does this PR change behavior after successful tool execution?

- [ ] Yes
- [ ] No

### If yes, answer all of the following

**Does successful verified write remain terminal by default in this repo?**

- [ ] Yes
- [ ] No

**If no, what explicit classified reason overrides that default?**

Explain precisely.

**Does tool success alone authorize continuation?**

- [ ] Yes
- [ ] No

This should almost always be **No**.

*Note: tool success cannot be used as a proxy for an undeclared validator failure or phase boundary.*

If **Yes**, the PR must identify the explicit repo-local exception, evidence, and test coverage.

---

## 9. Permissions / Guards / Validators

### Does this PR change runtime permissions?

- [ ] Yes
- [ ] No

If yes, describe the policy impact.

### Does this PR change guard behavior?

- [ ] Yes
- [ ] No

If yes, describe:

* what guard changed
* what it blocks or permits
* whether it can trigger retry
* how that retry remains bounded

### Does this PR change validator behavior?

- [ ] Yes
- [ ] No

If yes, describe:

* what validator changed
* what failure means
* whether validator failure can authorize another turn
* how that continuation remains explicit

---

## 10. Test Plan

### What tests were added or changed?

List exact test names and files.

1.
2.
3.

### Positive contract tests

Mark all covered by this PR.

Mark only the cases relevant to this PR; if none apply, explain why.

- [ ] Verified write yields a structurally valid terminal result
- [ ] Protocol artifact cannot become final output
- [ ] Guard failure allows one corrective retry maximum
- [ ] Finalize path preserves valid assistant output
- [ ] Validator failure explicitly authorizes continuation
- [ ] Declared phase transition explicitly authorizes continuation

### Negative contract tests

Mark all covered by this PR.

Mark only the cases relevant to this PR; if none apply, explain why.

- [ ] Successful verified write does not add a turn by itself
- [ ] Non-retry paths do not increment retry state
- [ ] Ambiguous assistant verbosity does not authorize continuation
- [ ] Nearby non-class cases do not trigger the new transition
- [ ] Synthetic wrapper content does not leak into final output

### Does any test currently assert incidental loop shape rather than contract?

- [ ] Yes
- [ ] No

### If yes, what is the plan?

- [ ] Rewrite test to assert contract
- [ ] Delete test
- [ ] Keep temporarily with justification

---

## 11. Logging / Artifacts / Auditability

### What new runtime facts become inspectable after this change?

Examples:

* retry reason
* phase transition reason
* validator failure cause
* heuristic activation
* finalize path selection

### What logs or artifacts will show this?

List exact fields, files, or event types.

1.
2.
3.

### Does this change improve auditability?

- [ ] Yes
- [ ] No
- [ ] Neutral

### Explain

Focus on replay, reproducibility, and postmortem clarity.

---

## 12. Risk Review

### What can regress?

List the top risks.

1.
2.
3.

### What nearby behaviors were checked?

List adjacent paths reviewed to avoid silent drift.

1.
2.
3.

### Worst-case failure mode if this change is wrong

Describe the most important bad outcome.

Examples:

* infinite or inflated loops
* dropped valid assistant output
* invalid terminal results
* extra writes after success
* retry state corruption
* protocol wrapper leakage

---

## 13. Rollback Plan

### How can this be reverted safely?

Describe commit boundaries or isolated files.

### What signal would tell us to revert?

Examples:

* eval regression
* loop count inflation
* protocol artifact leak
* terminal result corruption
* false-positive heuristic activation

---

## 14. Reviewer Decision Matrix

Reviewers should answer the following before approving.

### Contract clarity

- [ ] The state transition is explicitly defined
- [ ] The authorizing condition is explicit and classified
- [ ] Recovery and continuation are not ambiguously mixed

### Evidence quality

- [ ] Change is backed by real evidence or clearly marked as refactor-only
- [ ] Any heuristic is tied to a reproduced artifact
- [ ] Synthetic-only reasoning is not carrying runtime policy

### Testing quality

- [ ] Positive contract tests exist
- [ ] Negative contract tests exist
- [ ] No unjustified loop-shape test is being blessed

### Runtime quality

- [ ] Verified successful write remains terminal by default in this repo, or the override is explicitly justified
- [ ] Retry remains bounded and machine-identifiable
- [ ] Finalization remains structurally correct
- [ ] Auditability is preserved or improved

---

## 15. Reviewer Outcome

### Decision

- [ ] Approve
- [ ] Approve with required edits
- [ ] Request changes
- [ ] Reject

### Reviewer notes

Summarize the main reason for the decision.

---

## 16. Required Reviewer Comment Format

Paste this into the review and fill it out.

### Runtime Review Verdict

**Decision:**
**Transition changed:**
**Authorizing condition:**
**Classification:**
**Evidence quality:**
**Test quality:**
**Auditability impact:**
**Biggest remaining risk:**

**Approval note:**
State whether the PR preserves repo runtime policy and, if not, what must change before approval.

---

## 17. Fast Reject Conditions

A runtime PR should be rejected immediately if any of the following are true:

* it adds continuation without an explicit classified reason
* it makes successful verified write non-terminal by default without explicit local justification, evidence, and contract tests
* it collapses retry and continuation into one ambiguous path
* it adds or broadens a heuristic without real evidence
* it preserves or adds loop-shape tests that encode architectural drift
* it changes finalize behavior without checking terminal structural correctness
* it mutates retry state from non-retry success paths
* it reduces auditability of state transitions

---

## 18. Minimal Example of a Good Runtime PR Summary

> This PR narrows post-write control flow so verified successful write returns terminal success instead of scheduling another turn. The only remaining extra-turn path is a classified guard-recovery retry, bounded to one retry per offending turn. Evidence comes from a reproduced eval and an observed transcript showing guard misfinalization. Tests add one positive retry-case regression and two negative regressions proving successful write and protocol artifact rejection do not continue the loop.

---

## 19. Minimal Example of a Bad Runtime PR Summary

> This PR keeps the agent moving after successful writes because the model often seems like it has more to say. A regression was added proving the runtime reaches a third request in the happy path.

That is not acceptable under repo runtime policy.

---

## 20. Final Reminder

This runtime is a policy-enforced state machine, not an open-ended loop.

Every extra turn must be justified.

Every retry must be classified.

Every heuristic must be earned.

Every finalize path must preserve terminal correctness.
