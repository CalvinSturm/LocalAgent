# Clippy Runtime Cleanup Assessment (2026-03)

Status: assessment only  
Scope: current `cargo clippy -- -D warnings` failures after `832e523`  
Decision target: whether these findings deserve a separate cleanup PR

## Summary

Yes. The current clippy failures deserve a separate cleanup PR, but not as one undifferentiated "make clippy green" change.

They fall into two categories:

- low-risk mechanical cleanups that are reasonable in an isolated lint PR
- runtime-signature findings that touch core control-flow/result types and should not be changed casually

The important conclusion is that the repo should not treat the current clippy state as blocked on the just-completed runtime artifact/checkpoint hardening slice. These are a distinct follow-on task.

## Evidence

Observed by running:

```powershell
cargo clippy -- -D warnings
```

Current failures:

1. `clippy::result_large_err`
   Affects:
   - `src/agent/phase_transitions.rs:31`
   - `src/agent.rs:292`
   - `src/agent.rs:425`
   - `src/agent.rs:531`
   - `src/agent.rs:1358`
   - `src/agent.rs:1394`
   - `src/agent.rs:1430`
   - `src/agent.rs:1467`
   - `src/agent.rs:1504`
   - `src/agent.rs:1542`
   - `src/agent.rs:2174`

2. `clippy::too_many_arguments`
   Affects:
   - `src/agent/planner_phase.rs:51`

3. `clippy::match_single_binding`
   Affects:
   - `src/agent.rs:318`
   - `src/agent.rs:426`

## Classification

### 1. `result_large_err`

This is the most consequential lint family in the current set.

It fires because multiple runtime coordinator/helper functions return:

```rust
Result<..., AgentOutcome>
```

and `AgentOutcome` is large.

This is not merely formatting or local style. Fixing it cleanly would require one of:

- boxing `AgentOutcome` in many signatures
- introducing a narrower runtime error/finalization carrier type
- adding targeted `#[allow(clippy::result_large_err)]`

Assessment:

- This is a real issue in the sense that clippy is correctly identifying a large error payload.
- It is not automatically a good candidate for immediate structural change, because these functions sit in the shared runtime coordinator path.
- A broad "box everything" edit would cause signature churn across sensitive runtime code without clear user-visible benefit.

Recommendation:

- Handle this in its own PR.
- Default recommendation is a narrowly justified allow on the affected runtime control-flow helpers unless there is a strong architectural reason to introduce a smaller carrier type.
- Do not change the core runtime result shape opportunistically under a lint-only banner.

### 2. `too_many_arguments`

This currently appears on:

- `src/agent/planner_phase.rs:51`

`evaluate_planner_response(...)` takes eight arguments and is a good candidate for a small refactor into a compact input struct or a more cohesive context object.

Assessment:

- This is a legitimate maintainability cleanup.
- It is low enough risk to be its own small code-quality PR.
- It does not require semantic change.

Recommendation:

- Include this in a separate clippy cleanup PR.
- Preferred fix is an input struct rather than a blanket allow, because the function is already logically operating over one decision context.

### 3. `match_single_binding`

This appears in two spots in `src/agent.rs`.

These are purely mechanical and clippy already provides the suggested transformation:

- bind the decision to a local `let`
- keep the real `match` on the decision application result

Assessment:

- Very low risk
- No meaningful semantic impact
- Suitable for a cleanup PR

Recommendation:

- Fix directly in the same separate cleanup PR as the `too_many_arguments` item.

## Recommended PR Structure

Do not open one PR that mixes all lint families blindly.

Preferred structure:

1. PR A: `runtime-clippy-mechanical-cleanups`
   Scope:
   - fix `match_single_binding`
   - reduce `too_many_arguments` for `evaluate_planner_response`
   - keep behavior unchanged

2. PR B: `runtime-clippy-result-large-err-policy`
   Scope:
   - decide whether runtime coordinator functions should:
     - keep returning `AgentOutcome` and gain targeted allows, or
     - move to a narrower boxed/error carrier
   - document the reasoning in the PR description because this touches shared runtime control flow

If only one PR is desired, it should still preserve this distinction internally and avoid disguising a runtime API/semantics change as a simple lint sweep.

## Recommendation

These findings do deserve their own cleanup work, but as a separate, explicitly scoped follow-on from the runtime artifact/checkpoint hardening that just landed.

The best next action is:

- first, do a narrow cleanup PR for `match_single_binding` and `too_many_arguments`
- then decide separately whether `result_large_err` should be solved structurally or accepted with targeted allows in the runtime coordinator layer

## Non-Recommendations

Do not:

- reopen the just-finished runtime artifact/checkpoint PR for this
- broadly refactor `AgentOutcome` without a runtime-focused design reason
- bundle structural runtime signature churn into a "make clippy green" patch
