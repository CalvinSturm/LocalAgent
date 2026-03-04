# LocalAgent v0.4.0 Release Notes

Release date: 2026-03-03

## Highlights

- Completed Tool Call Accuracy delivery through PR12 with stricter runtime/tool contracts and deterministic behaviors.
- Added native `glob` and `grep` builtins for safe, non-shell file discovery/search workflows.
- Improved shell-call robustness with better compatibility handling and one-shot auto-repair for common `program not found` failures.
- Upgraded TUI runtime flow with improved approvals behavior and a dedicated Reasoning side pane (`Ctrl+4`).
- Added a dedicated manual TUI testing pack and archived historical planning/spec docs for cleaner ongoing documentation.

## Included Changes

### Tool Runtime and Contract Accuracy (PR2-PR12)

- Added tool-call repair gating and repeat-loop guardrails.
- Added runtime record persistence and versioned runtime tool contract prompting.
- Added deterministic tool-call-accuracy CI harness.
- Normalized provider sampling/controls (`temperature`, `top_p`, `max_tokens`, and `seed` handling).
- Implemented agent-mode build/plan runtime semantics.
- Implemented JSON run-output event projection mode.

### New Builtin Tools and Safe Defaults

- Added `glob` builtin with scoped path behavior and deterministic match handling.
- Added `grep` builtin with bounded, deterministic text-search behavior.
- Enabled safer default trust policy posture for tool execution.

### Shell and Approval Flow Improvements

- Improved shell tool compatibility and error classification for clearer fallback behavior.
- Added one-shot shell auto-repair for command-not-found style failures.
- Improved approvals UX with auto-refresh behavior, pending indicators, and auto-open refinements.

### TUI UX

- Added a dedicated right-side Reasoning pane for live/last-run model reasoning visibility.
- Added global `Ctrl+4` pane toggle handling (including terminal control-character fallback).
- Reasoning pane is hidden on banner and auto-shows after first prompt submission.

### Documentation and Testing Assets

- Added PR8-PR12 spec docs and a manual TUI testing pack.
- Archived historical tool-call accuracy specs (PR2-PR12) under `docs/archive/`.
- Aligned docs index and references after archive/reorg.

## Compatibility Notes

- `glob`/`grep` are additive builtins and do not remove existing tooling paths.
- Tool-call runtime behavior is stricter/more deterministic; integrations should continue ignoring unknown JSON fields for forward compatibility.
- Shell execution behavior may differ by trust mode and environment policy; blocked or unavailable shell runs now provide clearer error semantics.
