# Coding Eval Closeout Observation (2026-03)

Status: Non-normative local research note  
Owner: LocalAgent maintainers  
Last reviewed: 2026-03-14

## Observation

For local coding-task benchmark runs, successful closeouts appear more useful when the final assistant message includes both:

- a diff of the file changes
- a short explanation of what was implemented

This came from interactive TUI observation during local model testing rather than from a finalized benchmark rule.

## Intended use

Treat this as an eval-target observation first.

It is reasonable to test instruction profiles, benchmark task wording, or eval-only shaping against this preference and measure whether it improves:

- task understandability
- reviewability of successful edits
- subjective usefulness of closeouts for local coding models

## Non-goal

This note does not establish a shared runtime invariant.

It should not, by itself, force all successful `write_file`, `edit`, `apply_patch`, or `str_replace` runs to end with a diff-shaped final answer.

Any promotion from local observation to shared runtime behavior should be backed by benchmark evidence and checked against existing exact-answer, validator-driven, and general chat task classes.
