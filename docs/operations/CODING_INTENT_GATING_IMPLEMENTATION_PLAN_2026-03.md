# Coding Intent And Gating Implementation Plan (2026-03)

Status: Active implementation plan  
Owner: LocalAgent maintainers  
Last reviewed: 2026-03-14

## Goal

Replace brittle example-phrase behavior with a structured intent model that:

- respects explicit user requests like "do not write code yet"
- distinguishes discussion, clarification, and implementation states
- only infers coding intent when mode and capability gates allow it
- asks clarification questions when coding intent is present but the request is underspecified
- avoids forcing tool expectations in chat-only or no-write runs

## Non-goals

- Do not add more hardcoded example prompts to shared runtime logic.
- Do not force implementation behavior in sessions that are clearly planning-only.
- Do not treat write-capable mode alone as proof that the user wants code written immediately.

## Current Problems

- Shared runtime logic has historically used example-driven prompt heuristics.
- Build/code-oriented sessions can collapse planning, clarification, and implementation into one path.
- Chat-only or no-write paths can still drift into coding-task expectations.
- Underspecified build requests do not consistently trigger clarification before execution.

## Desired Precedence

1. Explicit user instruction
2. Explicit task kind or task-profile contract
3. Capability and trust gating
4. Inferred intent from prompt wording

## Target States

### 1. Discussion / Planning

Examples:

- "Do not write code yet"
- "Help me think through the architecture"
- "Let's discuss options first"

Expected behavior:

- no write-task expectation
- no forced tool usage
- help the user reason, compare, and scope

### 2. Clarification Before Implementation

Examples:

- "Make me a website"
- "Build me a game"
- "Create an app"

Expected behavior:

- recognize coding-oriented intent only when write-capable mode allows it
- ask short scoped clarification questions
- offer concrete options for less technical users
- avoid guessing stack, platform, or deliverable shape

### 3. Implementation

Examples:

- "Fix `src/main.rs` so the parser trims whitespace"
- "Create `index.html` and `styles.css` for a small landing page"
- "Implement a React component for the dashboard header"

Expected behavior:

- permit coding-task inference when sufficiently specified
- enforce write-task expectations only when write capability is actually enabled
- proceed directly when requirements are concrete enough

## Implementation Workstreams

### A. Runtime Gating

- [x] Remove prompt-example-specific guard changes introduced during debugging.
- [x] Gate implementation-guard activation on actual write capability, not only agent mode.
- [ ] Audit remaining runtime-owned completion paths for hidden assumptions that write-capable mode implies implementation intent.
- [ ] Add regression coverage for plain chat in Build mode with no write capability.

### B. Intent Classification

- [x] Remove example-noun inference such as `landing page`, `homepage`, `index.html`, and `current directory` from shared runtime inference.
- [ ] Define a small controlled vocabulary for:
  - implementation intent words
  - planning/discussion words
  - no-code-yet / defer words
  - help-seeking / stuck words
  - technical target nouns
  - technical anchors such as files, extensions, languages, and paths
- [ ] Implement combination-based inference instead of example-phrase matching.
- [ ] Require mode/capability gating before coding-intent inference can affect runtime behavior.

### C. Clarification Path

- [ ] Add a distinct "coding intent but underspecified" classification result.
- [ ] Teach the agent to ask clarification questions instead of guessing implementation details.
- [ ] Keep clarification prompts short and concrete.
- [ ] Support guided options for less technical users.
- [ ] Ensure explicit "do not write code yet" overrides this path and stays in planning.

### D. Respecting User Overrides

- [ ] Detect explicit no-code / discuss-first requests reliably.
- [ ] Ensure these requests override write-capable mode defaults.
- [ ] Add regressions for contradictory prompts like:
  - "Build this, but don't write code yet"
  - "Help me choose a stack before implementing"

### E. Follow-up Audit

- [ ] Audit related heuristic surfaces beyond the first fix area:
  - task-kind inference
  - write expectation inference
  - clarification handling
  - closeout expectations
- [ ] Record any deferred findings in the local observations note instead of expanding scope mid-change.

## Proposed Test Matrix

- [ ] Plain chat prompt in Build mode with no write capability
- [ ] Planning-only coding discussion with write capability enabled
- [ ] Underspecified build request that should trigger clarification
- [ ] Concrete implementation request that should proceed directly
- [ ] Explicit no-code-yet request in code-capable mode
- [ ] Help-seeking user request that should produce guided options
- [ ] Contradictory request where explicit user override beats inferred coding intent

## Progress Notes

- 2026-03-14: Created plan doc to track the intent/gating cleanup as a structured implementation effort.
- 2026-03-14: Removed example-noun heuristics from shared runtime task-kind and new-file inference.
- 2026-03-14: Gated implementation-guard activation on actual write capability instead of Build mode alone.

## Open Questions

- Where should clarification live architecturally: runtime classification, agent prompt contract, or both?
- How much of the clarification UX should be TUI-specific versus shared across CLI/TUI/server runs?
- Should planning/discussion classification produce an explicit task kind distinct from `general`?
