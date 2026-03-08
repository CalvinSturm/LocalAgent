# Contributing to LocalAgent

Thanks for contributing.

## Development Setup

1. Install Rust stable.
2. Clone repo and enter root.
3. Build and validate:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

Or use the local coding harness:

```bash
# Fast local iteration (default)
python scripts/dev_harness.py

# Quick profile + only changed integration tests
python scripts/dev_harness.py --changed-tests

# CI-like local validation
python scripts/dev_harness.py --profile full
```

## Project Principles

- Keep default safety posture unchanged unless explicitly discussed.
- Prefer additive changes over breaking changes.
- Keep behavior deterministic and replayable.
- Do not weaken trust/approval/tool-gate invariants.

## Coding Guidelines

- Use concise, readable code and minimal diffs.
- Add tests for new behavior.
- Update docs when flags/commands/workflows change.
- Keep platform behavior cross-compatible (Windows/Linux/macOS) when possible.

## Runtime Behavior Changes

If your change affects shared runtime-loop behavior, finalize semantics, retry behavior, continuation policy, or related guard/validator behavior:

- Read [AGENTS.md](AGENTS.md) first for the repo entry guidance.
- Read [docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md](docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md) before changing runtime-loop semantics.
- For substantial runtime-behavior PRs, use [docs/policy/AGENT_RUNTIME_CHANGE_REVIEW_TEMPLATE.md](docs/policy/AGENT_RUNTIME_CHANGE_REVIEW_TEMPLATE.md) in the PR description.
- Preserve the repo default that verified successful write is terminal by default unless continuation is explicitly authorized by repo-local policy.

## Pull Request Checklist

- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo test` passes
- [ ] New/changed behavior covered by tests
- [ ] README/docs updated if user-facing changes were made

## Commit Guidance

- Use clear commit messages (e.g., `feat: ...`, `fix: ...`, `docs: ...`, `chore: ...`).
- Keep commits focused and reviewable.

## Reporting Issues

When filing issues, include:

- OS and version
- `localagent version --json` output
- Command used
- Expected vs actual behavior
- Relevant logs/artifacts (`.localagent/runs`, events JSONL) if available
