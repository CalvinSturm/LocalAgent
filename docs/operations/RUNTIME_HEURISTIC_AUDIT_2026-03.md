# Runtime Heuristic Audit (2026-03)

Status: Active audit note  
Owner: LocalAgent maintainers  
Last reviewed: 2026-03-14

## Purpose

This document audits the current codebase for eval-era and benchmark-shaped heuristic behavior in shared runtime paths.

The focus is the boundary between:

- explicit contract parsing that should remain
- fallback compatibility heuristics that should be isolated
- shared runtime decisions that should move onto explicit state

Priority surfaces reviewed first:

- `src/agent/tool_facts.rs`
- `src/agent/task_contract.rs`
- `src/agent_impl_guard.rs`
- `src/agent/completion_policy.rs`
- `src/agent/run_finalize.rs`

Related state/gating surface also reviewed:

- `src/agent_runtime/guard.rs`

## Executive Summary

The codebase still contains several prompt-driven runtime behaviors that look like compatibility shims from benchmark- and prompt-shape-driven hardening work.

The most important findings are:

- explicit contract parsing is present and should remain, especially exact-closeout and validation-command extraction
- prompt-driven coding and write expectations still influence shared runtime semantics
- post-write follow-on behavior still depends on closeout-style prompt wording
- shared runtime decisions are not yet fully centralized around explicit state and classification results
- heuristic logic is scattered across multiple files instead of flowing through one intent-classification seam

The highest-risk surfaces are:

- prompt-based effective-write enforcement in `tool_facts.rs`
- prompt-based task-kind inference in `task_contract.rs`
- prompt-based follow-on completion in `agent_impl_guard.rs` and `completion_policy.rs`

## 1. Separate Heuristics By Type

### A. Explicit Contract Parsing To Keep

These are explicit user-authored or task-authored contracts and are reasonable to keep:

- tool-only directives in [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs#L80)
  - `prompt_requires_tool_only(...)`
- required validation command extraction in [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs#L123)
  - detects explicit commands like `cargo test`, `npm test`, `pnpm test`, `node --test`
- exact final answer extraction in [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs#L143)
  - parses `reply with exactly ...` or `your final answer must be exactly: ...`

Assessment:

- These are still prompt-based, but they read as explicit contracts rather than benchmark hacks.
- They should remain, but be clearly documented as explicit-contract parsing rather than generic intent inference.

### B. Compatibility Heuristics To Isolate

These still act like runtime-semantic heuristics:

- prompt-based effective-write requirement in [tool_facts.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/tool_facts.rs#L475)
- prompt-based new-file exception in [tool_facts.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/tool_facts.rs#L463)
- prompt-based coding task inference in [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs#L153)
- prompt-based post-write follow-on detection in [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs#L104)

Assessment:

- These should be treated as fallback-only compatibility behavior.
- Today they still materially affect shared runtime law.

### C. Recommendation

- Keep explicit contract parsing in `agent_impl_guard.rs`.
- Move compatibility heuristics behind a clearly named fallback classification seam.
- Mark those heuristics as non-authoritative in code comments and architecture docs.

## 2. Move Shared Runtime Decisions Onto Explicit State First

### Current explicit-state improvements

[guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/guard.rs#L7) now gates implementation-guard activation on actual write capability:

- `enable_write_tools && allow_write`
- or `unsafe_bypass_allow_flags`

This is a strong improvement because chat-only or no-write runs no longer inherit coding-task expectations just from `Build` mode.

### Remaining gaps

Shared runtime decisions still depend on prompt scans in places where explicit state should win:

- task-kind fallback inference in [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs#L177)
- effective-write enforcement trigger in [tool_facts.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/tool_facts.rs#L397)
- post-write follow-on decision in [completion_policy.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/completion_policy.rs#L313)

Missing explicit-state concepts in current shared runtime:

- explicit `discussion/planning` state
- explicit `clarification_required` state
- explicit user override such as “do not write code yet”
- explicit help/stuck guidance state

### Recommendation

Shared runtime should prefer, in order:

1. explicit user instruction
2. explicit task kind or task-profile contract
3. capability/trust/write-tool state
4. fallback intent inference

## 3. Replace Phrase Hacks With One Intent-Classification Module

### Current state

Intent-related logic is scattered:

- coding-task inference in [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs#L153)
- write expectation inference in [tool_facts.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/tool_facts.rs#L475)
- follow-on closeout inference in [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs#L104)
- explicit contract parsing in [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs#L80)

### Findings

- There is no single module that owns intent classification.
- Runtime semantics currently emerge from multiple `contains(...)` scans across several files.
- This makes it difficult to reason about precedence and easy to accumulate benchmark-era compatibility behavior.

### Recommendation

Create one intent-classification module that produces structured results such as:

- `discussion`
- `clarification_required`
- `implementation`
- `explicit_contract_only`

That module should own:

- implementation intent
- planning intent
- no-code-yet intent
- help/stuck intent
- ambiguity detection

And it should use token and combination logic, not example-prompt matching.

## 4. Make Runtime Consume Classification Results, Not Raw Prompt Scans

### Current state

Raw prompt scanning still directly affects runtime behavior:

- [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs#L177)
  - inferred `task_kind`
- [tool_facts.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/tool_facts.rs#L397)
  - effective-write requirement enforcement
- [tool_facts.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/tool_facts.rs#L412)
  - new-file write exception
- [completion_policy.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/completion_policy.rs#L326)
  - follow-on turn requirement

### Findings

- There is no structured runtime input like `IntentClassification`.
- Shared runtime modules still inspect raw prompt text themselves.
- This keeps control flow coupled to phrasing and makes paraphrase behavior fragile.

### Recommendation

Refactor so runtime consumes a classification result instead of calling prompt scanners in each module.

Target shape:

- classification happens once
- runtime modules consume normalized fields
- prompt parsing remains only for explicit contracts

## 5. Restrict Fallback Heuristics To Non-Authoritative Behavior

### Current state

Some fallback heuristics still have authoritative consequences:

- [tool_facts.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/tool_facts.rs#L397)
  - can turn a run into an effective-write-required failure
- [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs#L177)
  - can promote a run into `coding`
- [completion_policy.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/completion_policy.rs#L326)
  - can force a follow-on turn

### Findings

- These heuristics do more than suggest; they change runtime obligations.
- That is the main architectural risk from benchmark-era compatibility behavior.

### Recommendation

Fallback heuristics should be allowed to:

- suggest likely intent
- ask a clarification question
- route the agent into a clarification/planning path

Fallback heuristics should not, by themselves:

- force an implementation guard failure
- force a write requirement
- force a follow-on completion phase

unless explicit capability state and explicit contract support that result.

## 6. Main Hack Surfaces Audit

### A. `src/agent/tool_facts.rs`

Current heuristic seams:

- [tool_facts.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/tool_facts.rs#L463)
  - `prompt_allows_new_file_without_read(...)`
- [tool_facts.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/tool_facts.rs#L475)
  - `prompt_requires_effective_write(...)`
- [tool_facts.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/tool_facts.rs#L368)
  - `implementation_integrity_violation_from_facts(...)`

Assessment:

- Highest-risk heuristic surface.
- Prompt wording can still determine whether the runtime treats the run as requiring an effective write.
- This is a strong runtime consequence and should move off raw prompt scans.

### B. `src/agent/task_contract.rs`

Current heuristic seam:

- [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs#L153)
  - `prompt_suggests_coding_task(...)`

Assessment:

- Better than the older example-noun version, but still fallback inference in shared runtime.
- It remains a compatibility heuristic and should be isolated behind a formal intent-classification layer.

### C. `src/agent_impl_guard.rs`

Current prompt parsers:

- [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs#L80)
  - tool-only directives
- [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs#L104)
  - post-write follow-on
- [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs#L123)
  - validation command
- [agent_impl_guard.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs#L143)
  - exact final answer

Assessment:

- Mixed surface.
- Validation-command and exact-final-answer parsing are legitimate explicit contract extraction.
- Post-write follow-on parsing is more heuristic and should probably migrate out of the shared guard layer.

### D. `src/agent/completion_policy.rs`

Current heuristic consumption:

- [completion_policy.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/completion_policy.rs#L313)
  - `decide_verified_write_completion(...)` consumes prompt-derived follow-on detection

Assessment:

- Completion policy still depends on prompt-shape heuristics.
- This should consume explicit state or classification results instead.

### E. `src/agent/run_finalize.rs`

Current heuristic-adjacent behavior:

- [run_finalize.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/run_finalize.rs#L35)
  - finalization path defers to verified-write completion policy
- [run_finalize.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/run_finalize.rs#L60)
  - runtime note wording still reflects post-write follow-on assumptions

Assessment:

- `run_finalize.rs` is less of a heuristic source and more a downstream consumer.
- It still participates in the heuristic chain via completion-policy and integrity-guard decisions.

## 7. Regression Matrix Audit

### Current coverage strengths

The codebase already has useful regressions around:

- explicit exact-closeout behavior
- required validation-command behavior
- validation repair and follow-on paths
- Build/general vs coding task-contract inference

Examples:

- [task_contract.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs#L371)
  - general chat prompt stays `general`
- [completion_policy.rs](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/completion_policy.rs#L354)
  - validation phase start

### Gaps

Missing or underdeveloped regression classes:

- plain chat in write-capable Build mode
- planning-only coding discussion
- explicit “do not write code yet”
- underspecified build request that should trigger clarification instead of implementation
- help/stuck guidance path
- paraphrase matrix instead of single wording tests

### Recommended regression matrix

- plain chat
- planning-only coding discussion
- underspecified build request
- explicit no-code-yet request
- concrete implementation request
- exact-closeout request
- validation-command request
- paraphrases for each category

## 8. Keep Eval Shaping In Eval/Config Surfaces

### Findings

I did not find strong evidence that pack names like `common_coding_ux` or `omnicoder` are directly hardcoded into shared runtime control flow.

The larger problem is more general:

- benchmark-era prompt-shape expectations have leaked into shared runtime heuristics
- shared runtime still carries compatibility behavior that should live in explicit shaping surfaces instead

### Recommended shaping location

If a benchmark needs special closeout behavior or task shaping, prefer:

- eval task metadata
- instruction profiles
- eval-only shaping/config

Avoid putting benchmark-specific shaping into shared runtime law.

## Recommended Cleanup Order

1. Keep explicit contract parsing in `agent_impl_guard.rs`.
2. Extract fallback intent heuristics into one intent-classification module.
3. Refactor `tool_facts.rs` to consume classification results instead of raw prompt scans.
4. Refactor `task_contract.rs` to consume explicit state plus classification results.
5. Remove prompt-driven follow-on behavior from `completion_policy.rs` unless backed by explicit contract or classification state.
6. Add regression coverage for planning, clarification, no-code-yet, and paraphrase cases.

## Bottom Line

The codebase is partially improved, but shared runtime still carries several prompt-shaped compatibility heuristics that influence:

- task-kind inference
- effective-write enforcement
- new-file write exceptions
- post-write follow-on completion

Those behaviors should be demoted from scattered raw prompt scans into:

- explicit contracts where the user states them clearly
- explicit capability/task state
- one bounded fallback intent-classification layer

That is the cleanest path to removing eval-era shaping from shared runtime law without losing useful explicit contract support.
