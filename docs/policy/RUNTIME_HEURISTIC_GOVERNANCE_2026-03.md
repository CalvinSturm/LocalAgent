# Runtime Heuristic Governance (2026-03)

Status: Active repo-local governance note  
Owner: LocalAgent maintainers  
Last reviewed: 2026-03-14

## Purpose

This document defines hard rules for adding, keeping, reviewing, or removing runtime heuristics in LocalAgent.

These rules apply especially to shared runtime behavior that affects:

- task-kind inference
- write expectations
- follow-on completion behavior
- clarification behavior
- prompt-derived execution semantics

## Core Law

A heuristic in shared runtime code must represent a reusable semantic class, not a benchmark workaround.

If a change only solves one eval row, one prompt shape, one model quirk, or one example wording, it does not qualify as shared runtime law.

## Hard Rules

### 1. Explicit contracts are allowed to be authoritative

Prompt parsing is acceptable when it extracts an explicit user or task contract, for example:

- `reply with exactly ...`
- `your final answer must be exactly ...`
- `run cargo test before finishing`
- tool-only directives

These are explicit instructions, not heuristic guesses.

### 2. Heuristics must be general, not example-shaped

Any heuristic that remains in shared runtime behavior must work across multiple materially different instances.

It must be abstractable into a reusable semantic rule such as:

- intent class
- ambiguity class
- clarification class
- capability/state rule
- completion class

It must not be anchored to:

- one exact prompt
- one benchmark row
- one eval pack task
- one model-specific wording quirk

### 3. Heuristics must be non-blocking unless backed by explicit contract and explicit state

Heuristic logic may:

- suggest likely intent
- route into clarification
- route into planning/discussion
- influence safe defaults

Heuristic logic must not, by itself:

- create a hard planner/runtime failure
- force an implementation-guard violation
- force a write requirement
- force a follow-on completion phase

unless explicit capability state and explicit contract support that outcome.

### 4. Shared runtime must depend on explicit state first

Runtime decisions should prefer, in order:

1. explicit user instruction
2. explicit task kind or task-profile contract
3. capability/trust/write-tool state
4. fallback inference

Prompt heuristics must not outrank explicit user intent.

### 5. Write-capable mode does not imply immediate implementation intent

Even when write tools are enabled, the runtime must still distinguish:

- discussion/planning
- clarification before implementation
- implementation

Explicit user requests such as:

- `do not write code yet`
- `let's discuss first`
- `help me plan this`

must override write-capable defaults.

### 6. Good implementation requests should be abstractable

A good runtime behavior change should be explainable as a general rule across multiple instances.

If the only explanation is “this fixed a benchmark prompt,” the change is not ready for shared runtime behavior.

### 7. Any heuristic change requires detailed human review

Any new heuristic or compatibility rule in shared runtime code must be reviewed by the user or maintainer in detail before merge.

That review should ask:

- What general failure class does this solve?
- Why is this a semantic rule instead of a benchmark workaround?
- Why can this not be handled by explicit metadata, task config, or instruction shaping?
- What are the false-positive and false-negative risks?
- Why is this safe for normal interactive use, not just evals?

### 8. Evidence must cover multiple distinct instances

Proof for a heuristic change must include multiple instances, not a single eval row.

Minimum standard:

- multiple materially different phrasings
- multiple task shapes when relevant
- evidence that the rule is useful outside one benchmark wording

Paraphrases with only one noun changed are not enough.

### 9. Benchmark shaping belongs outside shared runtime law

If behavior is mainly needed to improve a benchmark or local-model evaluation workflow, it should prefer:

- eval task metadata
- instruction profiles
- eval-only shaping/config

Shared runtime law should not be the first place for benchmark shaping.

### 10. If it cannot be abstracted, it should not be merged as runtime law

A runtime heuristic must be expressible as a reusable semantic rule.

If it cannot be abstracted at least one level above the example that inspired it, it does not belong in shared runtime code.

## Review Checklist

Before merging a heuristic-related runtime change, confirm:

- [ ] This is not just an eval workaround.
- [ ] The rule is abstractable beyond the motivating prompt.
- [ ] The rule is supported by multiple distinct instances.
- [ ] Explicit state and explicit contracts were considered first.
- [ ] The heuristic is non-blocking unless explicit contract/state justify stronger behavior.
- [ ] The user reviewed the change in detail.

## Practical Guidance

- Prefer one centralized intent-classification layer over scattered `contains(...)` checks.
- Prefer clarification over guessing when coding intent is real but underspecified.
- Prefer planning/discussion behavior over forced tool use when the user explicitly asks not to write code yet.
- Prefer eval/config shaping over runtime-law changes when the need is benchmark-specific.
