# AGENTS.md

## TL;DR
LocalAgent is a single-crate Rust CLI/runtime for local-first agent execution with tool calling, trust/approval gates, MCP integration, reproducibility artifacts, evals, checks, and an interactive chat TUI.

- Main entrypoint: `main -> cli_dispatch::run_cli -> run_agent/run_agent_with_ui`.
- Default safety posture is conservative: `trust off`, shell disabled, and write tools disabled unless the operator enables them.
- Runtime state resolves through `resolve_state_paths`, but one-shot `run` / `exec` default to an ephemeral temp state dir plus `--no-session` unless `--state-dir` or `--no-session` is set explicitly.

* `Evidence: Cargo.toml#<config:package.name>`
* `Evidence: src/main.rs#main`
* `Evidence: src/cli_dispatch.rs#run_cli`
* `Evidence: src/agent_runtime.rs#run_agent_with_ui`
* `Evidence: src/store.rs#resolve_state_paths`
* `Evidence: src/runtime_wiring.rs#build_gate`

## Before You Change Code
1. Read the crate and CLI entrypoints first: `Cargo.toml`, `src/cli_args.rs`, `src/cli_dispatch.rs`, `src/agent_runtime.rs`.
2. Identify whether your change touches shared runtime-loop behavior, tool/runtime policy, or only a narrower surface.
3. Read `docs/README.md` for the current docs layout, then open the deeper doc that matches the task instead of extending this file.
4. For CLI/defaults questions, treat `src/cli_args.rs` and `src/cli_dispatch.rs` as the source of truth over prose docs.

* `Evidence: Cargo.toml#<config:package.default-run>`
* `Evidence: src/cli_args.rs#Cli`
* `Evidence: src/cli_args.rs#Commands`
* `Evidence: src/cli_dispatch.rs#run_cli`
* `Evidence: src/agent_runtime.rs#run_agent_with_ui`

## Build and Test
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`
- CI also runs `scripts/ci_release_readiness.py` and `cargo test --test tool_call_accuracy_ci`

* `Evidence: CONTRIBUTING.md#Development Setup`
* `Evidence: .github/workflows/ci.yml#<config:jobs.ci.steps>`
* `Evidence: scripts/ci_release_readiness.py#main`
* `Evidence: tests/tool_call_accuracy_ci.rs#ci_unknown_tool_self_corrects_or_fails_clear`

## Rules for AI Agents Working in This Repo
- Read entrypoints first, then use the docs map below for deeper context.
- Cite evidence for non-trivial claims in docs, reviews, and design notes.
- Prefer small, reviewable changes; add or update tests near changed behavior.
- Do not perform side effects beyond requested edits without explicit operator approval.
- Preserve public CLI behavior unless the change explicitly requires otherwise.
- Treat source-code side-effect governance as external process unless enforced by repo runtime policy or tool/trust code.

* `Evidence: src/gate.rs#TrustGate::decide`
* `Evidence: src/tools.rs#execute_tool`
* `Evidence: CONTRIBUTING.md#Project Principles`

## Runtime Loop Change Summary
- Treat the shared runtime loop as stable core behavior.
- In this repo, verified successful write is terminal by default unless continuation is authorized by an explicit validator failure, classified retry condition, declared phase transition, or user-directed follow-on step.
- Do not justify runtime-loop architecture changes from one weak local-model eval alone.
- Require proving evidence from code-path inspection plus artifacts, events, transcripts, or targeted regressions before changing shared runtime-loop semantics.
- Runtime-loop changes require reading the runtime policy doc first; substantial runtime behavior changes must also use the runtime PR review template.

Runtime policy docs:
- [Runtime Loop Policy](docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md)
- [Runtime Change Review Template](docs/policy/AGENT_RUNTIME_CHANGE_REVIEW_TEMPLATE.md)

* `Evidence: src/agent.rs#run`
* `Evidence: src/agent/runtime_completion.rs`
* `Evidence: src/agent/run_finalize.rs`
* `Evidence: src/agent/run_setup.rs`
* `Evidence: src/agent_impl_guard.rs`

## Docs Map
- Docs index: [docs/README.md](docs/README.md)
- Architecture: [docs/architecture/RUNTIME_ARCHITECTURE.md](docs/architecture/RUNTIME_ARCHITECTURE.md)
- Runtime target: [docs/architecture/LOCALAGENT_VNEXT_RUNTIME_TARGET.md](docs/architecture/LOCALAGENT_VNEXT_RUNTIME_TARGET.md)
- Runtime handoff: [docs/architecture/LOCALAGENT_VNEXT_RUNTIME_HANDOFF.md](docs/architecture/LOCALAGENT_VNEXT_RUNTIME_HANDOFF.md)
- Operations: [docs/operations/OPERATIONAL_RUNBOOK.md](docs/operations/OPERATIONAL_RUNBOOK.md)
- Config/state: [docs/reference/CONFIGURATION_AND_STATE.md](docs/reference/CONFIGURATION_AND_STATE.md)
- CLI reference: [docs/reference/CLI_REFERENCE.md](docs/reference/CLI_REFERENCE.md)
- Runtime policy: [docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md](docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md)
- Runtime PR template: [docs/policy/AGENT_RUNTIME_CHANGE_REVIEW_TEMPLATE.md](docs/policy/AGENT_RUNTIME_CHANGE_REVIEW_TEMPLATE.md)
- File/symbol index: [docs/reference/FILE_AND_SYMBOL_INDEX.md](docs/reference/FILE_AND_SYMBOL_INDEX.md)
- Instruction tuning: [docs/guides/INSTRUCTION_PROFILES.md](docs/guides/INSTRUCTION_PROFILES.md)
- Safe tool tuning baseline: [docs/guides/SAFE_TOOL_TUNING_BASELINE.md](docs/guides/SAFE_TOOL_TUNING_BASELINE.md)
- Human-in-the-loop checklist: [docs/guides/HUMAN_IN_THE_LOOP_CHECKLIST.md](docs/guides/HUMAN_IN_THE_LOOP_CHECKLIST.md)

Archive note:
- Historical planning, audit, and superseded handoff docs live under [docs/archive](docs/archive/README.md). Do not treat archived docs as the current source of truth.
