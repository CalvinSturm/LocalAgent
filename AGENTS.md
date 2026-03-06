# AGENTS.md

## TL;DR
LocalAgent is a single-crate Rust CLI/runtime for local-first agent execution with tool calling, trust/approval gates, MCP integration, reproducibility artifacts, evals, checks, and an interactive chat TUI.

- Main entrypoint: `main -> cli_dispatch::run_cli -> run_agent/run_agent_with_ui`.
- Default safety posture is conservative (`trust off`, shell/write disabled) unless flags or mode changes enable them.
- Runtime state is under `.localagent/` (runs, sessions, approvals, policy, audit, eval, tasks).
- Most operational debugging starts with run artifacts (`.localagent/runs/*.json`), event stream (`--events`), and audit (`audit.jsonl`).

* `Evidence: Cargo.toml#<config:package.name>`
* `Evidence: src/main.rs#main`
* `Evidence: src/cli_dispatch.rs#run_cli`
* `Evidence: src/agent_runtime.rs#run_agent_with_ui`
* `Evidence: src/store.rs#resolve_state_paths`
* `Evidence: src/runtime_wiring.rs#build_event_sink`
* `Evidence: src/runtime_wiring.rs#build_gate`

## How to Use This Document
Read top-down once, then jump by task:

1. New contributor: `Agent Start Checklist`, then `Level 0`, then `Level 3`.
2. On-call/operator: `Operational Playbook`, `Observability and Debugging`, then `Level 3` flow matching your incident.
3. Agentic coding assistant: follow `Agent Start Checklist`, then `Rules for AI Agents Working in This Repo`.

All non-trivial claims are anchored to file/symbol or config keys in evidence bullets.

* `Evidence: src/lib.rs#agent`
* `Evidence: src/lib.rs#checks`
* `Evidence: src/lib.rs#mcp`
* `Evidence: src/lib.rs#trust`
* `Evidence: src/lib.rs#tui`

## Agent Start Checklist
1. Read root crate manifest to lock package/bin assumptions.
- Why: defines binary target, edition, dependency surface.
- * `Evidence: Cargo.toml#<config:package.name>`
- * `Evidence: Cargo.toml#<config:package.edition>`
- * `Evidence: Cargo.toml#<config:package.default-run>`
- * `Evidence: Cargo.toml#<config:[[bin]].path>`

2. Read CLI schema before runtime code.
- Why: command and flag topology drives all dispatch.
- * `Evidence: src/cli_args.rs#Cli`
- * `Evidence: src/cli_args.rs#Commands`
- * `Evidence: src/cli_args.rs#RunArgs`

3. Read command dispatcher.
- Why: authoritative call routing for run/chat/eval/check/tasks/replay.
- * `Evidence: src/cli_dispatch.rs#run_cli`
- * `Evidence: src/cli_dispatch.rs#apply_run_command_defaults`

4. Read runtime orchestrator.
- Why: core execution assembly (gate, tools, hooks, MCP, session, artifacts).
- * `Evidence: src/agent_runtime.rs#run_agent_with_ui`
- * `Evidence: src/agent_runtime.rs#build_gate_context`
- * `Evidence: src/agent_runtime.rs#build_hook_and_tool_setup`

5. Read trust/gate enforcement.
- Why: safety and approval behavior.
- * `Evidence: src/runtime_wiring.rs#build_gate`
- * `Evidence: src/gate.rs#TrustGate`
- * `Evidence: src/trust/policy.rs#Policy`

6. Read tool runtime and side-effect model.
- Why: actual filesystem/shell/write behavior and argument validation.
- * `Evidence: src/tools.rs#ToolRuntime`
- * `Evidence: src/tools.rs#builtin_tools_enabled`
- * `Evidence: src/tools.rs#tool_side_effects`
- * `Evidence: src/tools.rs#execute_tool`

7. Read state/artifact layer.
- Why: reproducibility and debugging artifacts.
- * `Evidence: src/store.rs#StatePaths`
- * `Evidence: src/store/io.rs#write_run_record`
- * `Evidence: src/store/io.rs#load_run_record`

8. Read MCP registry/client.
- Why: external tool loading/calls and hash pinning behavior.
- * `Evidence: src/mcp/registry.rs#McpRegistry`
- * `Evidence: src/mcp/registry.rs#call_namespaced_tool`
- * `Evidence: src/mcp/client.rs#McpClient`

9. Read startup/chat paths.
- Why: operator UX path differs from one-shot run.
- * `Evidence: src/startup_bootstrap.rs#run_startup_bootstrap`
- * `Evidence: src/startup_detect.rs#detect_startup_provider`
- * `Evidence: src/chat_tui_runtime.rs#run_chat_tui`

10. Read CI and integration tests.
- Why: merge gates and regression surface.
- * `Evidence: .github/workflows/ci.yml#<config:jobs.ci.steps>`
- * `Evidence: tests/tool_call_accuracy_ci.rs#ci_unknown_tool_self_corrects_or_fails_clear`
- * `Evidence: tests/mcp_integration.rs#mcp_call_routing_returns_wrapped_result`

## Level 0: One-Screen Overview
What this repo does:
- Provides `localagent` CLI/runtime for local model providers (`lmstudio`, `llamacpp`, `ollama`, plus `mock`) with tool calling, trust/approval policy, MCP tools, replay/repro artifacts, eval/check workflows, and TUI chat.
- * `Evidence: src/cli_args.rs#ProviderKind`
- * `Evidence: src/cli_args.rs#Commands`
- * `Evidence: src/provider_runtime.rs#default_base_url`

Who uses it:
- Operators: run/chat, approvals, policy doctor/test, replay/eval/tasks/check commands.
- Developers: extend runtime modules, tools, providers, policies, hooks, tests.
- * `Evidence: src/cli_args.rs#Commands`
- * `Evidence: src/approvals_ops.rs#handle_approvals_command`
- * `Evidence: src/cli_dispatch_eval_replay.rs#handle_eval_command`

Main runtime modes:
- Single-run agent mode (`--mode single`).
- Planner-worker orchestration (`--mode planner-worker`).
- Agent behavior mode (`--agent-mode build|plan`).
- * `Evidence: src/cli_args.rs#RunArgs`
- * `Evidence: src/planner.rs#RunMode`
- * `Evidence: src/cli_args.rs#AgentMode`

Core modules/crates (single crate, module-heavy):
- `cli_dispatch`/`cli_args`: command parsing and routing.
- `agent_runtime`/`agent`: orchestration and loop.
- `tools`/`target`: tool exec and host/docker target.
- `trust`/`gate`: policy/approvals/audit decisions.
- `mcp`: external tool registry and RPC client.
- `store`/`repro`/`session`: state, artifacts, replay, reproducibility, session memory.
- * `Evidence: src/lib.rs#agent`
- * `Evidence: src/lib.rs#mcp`
- * `Evidence: src/lib.rs#store`
- * `Evidence: src/lib.rs#trust`
- * `Evidence: src/lib.rs#session`

Start-here path for new contributors:
- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, then trace `main -> run_cli -> run_agent_with_ui`.
- * `Evidence: CONTRIBUTING.md#<config:Development Setup>`
- * `Evidence: src/main.rs#main`
- * `Evidence: src/cli_dispatch.rs#run_cli`
- * `Evidence: src/agent_runtime.rs#run_agent_with_ui`

## Level 1: Architecture Map
Components and responsibilities:
1. CLI front door (`main`, `cli_args`, `cli_dispatch`).
2. Runtime orchestrator (`agent_runtime`, `runtime_wiring`, `run_prep`).
3. Execution kernel (`agent`, `tools`, `target`).
4. Trust plane (`gate`, `trust/policy`, `trust/approvals`, `trust/audit`).
5. Integrations (`providers/*`, `mcp/*`, `hooks/*`).
6. State/observability (`store`, `events`, `runtime_events`, `repro`, `session`).
7. UX surfaces (`startup_*`, `chat_*`, `tui/*`).
- * `Evidence: src/main.rs#main`
- * `Evidence: src/agent_runtime.rs#run_agent_with_ui`
- * `Evidence: src/tools.rs#execute_tool`
- * `Evidence: src/gate.rs#ToolGate`
- * `Evidence: src/providers/mod.rs#ModelProvider`
- * `Evidence: src/mcp/registry.rs#McpRegistry`
- * `Evidence: src/store/io.rs#write_run_record`
- * `Evidence: src/chat_tui_runtime.rs#run_chat_tui`

Data/control flow (primary run path):
`main -> run_cli -> provider selection -> run_agent -> run_agent_with_ui -> build_gate/build_event_sink -> prepare_tools_and_qualification -> Agent::run -> write_run_record`
- * `Evidence: src/main.rs#main`
- * `Evidence: src/cli_dispatch.rs#run_cli`
- * `Evidence: src/agent_runtime.rs#run_agent`
- * `Evidence: src/runtime_wiring.rs#build_gate`
- * `Evidence: src/runtime_wiring.rs#build_event_sink`
- * `Evidence: src/run_prep.rs#prepare_tools_and_qualification`
- * `Evidence: src/agent.rs#run`
- * `Evidence: src/store/io.rs#write_run_record`

External integrations:
- HTTP providers via `reqwest` (OpenAI-compatible + Ollama).
- MCP servers via spawned stdio process and JSON-RPC.
- Optional Docker exec target for tool execution.
- * `Evidence: Cargo.toml#<config:dependencies.reqwest>`
- * `Evidence: src/providers/openai_compat.rs#OpenAiCompatProvider`
- * `Evidence: src/providers/ollama.rs#OllamaProvider`
- * `Evidence: src/mcp/client.rs#McpClient::spawn`
- * `Evidence: src/target.rs#DockerTarget`

Side effects model:
- Filesystem read/write via built-in tools and state artifact writes.
- Process spawn for shell tools, hooks, MCP servers, and optional docker.
- Network via provider HTTP and MCP remote endpoints behind MCP servers.
- * `Evidence: src/tools.rs#tool_side_effects`
- * `Evidence: src/tools.rs#execute_tool`
- * `Evidence: src/store/io.rs#write_run_record`
- * `Evidence: src/hooks/runner.rs#invoke_hook`
- * `Evidence: src/mcp/client.rs#spawn_mcp_process`
- * `Evidence: src/provider_runtime.rs#doctor_check`

Observability surfaces:
- Event sinks: stdout JSON projection, JSONL file sink, TUI sink.
- Run artifacts in `.localagent/runs` with config hash/fingerprint/tool decisions.
- Audit log appends (`audit.jsonl`) for trust decisions.
- * `Evidence: src/runtime_wiring.rs#build_event_sink`
- * `Evidence: src/events.rs#JsonlFileSink`
- * `Evidence: src/store/io.rs#write_run_record`
- * `Evidence: src/trust/audit.rs#AuditLog::append`

## Level 2: Subsystems
### CLI and Dispatch
Purpose: parse command surface and route execution.

- Key symbols: `Cli`, `Commands`, `RunArgs`, `run_cli`.
- Inputs/outputs: argv -> command handlers, process exit codes/stdout/stderr.
- Error strategy: `anyhow::Result`, explicit `std::process::exit` for command fail semantics.
- Extension point: add new subcommand in `Commands`, wire match arm in `run_cli`.
- * `Evidence: src/cli_args.rs#Cli`
- * `Evidence: src/cli_args.rs#Commands`
- * `Evidence: src/cli_dispatch.rs#run_cli`

### Agent Runtime and Loop
Purpose: assemble full run context and execute agent loop.

- Key symbols: `run_agent_with_ui`, `build_session_bootstrap`, `build_context_augmentations`.
- Inputs/outputs: provider + model + prompt + `RunArgs` -> `RunExecutionResult` and optional artifact path.
- Error strategy: bubbles `anyhow`, maps provider failures to user hints; strict planner modes can transform failures.
- Invariants: runtime-owned modes require non-zero request and stream idle timeouts.
- Common failures: provider unavailable, policy parse failures, planner strict validation failures.
- Extension points: add injected context, runtime events, new pre/post phases in `run_agent_with_ui`.
- * `Evidence: src/agent_runtime.rs#run_agent_with_ui`
- * `Evidence: src/agent_runtime.rs#validate_runtime_owned_http_timeouts`
- * `Evidence: src/agent_runtime.rs#build_session_bootstrap`
- * `Evidence: src/agent_runtime.rs#build_context_augmentations`

### Tooling and Execution Targets
Purpose: expose built-in tools and execute them safely under host/docker target.

- Key symbols: `builtin_tools_enabled`, `execute_tool`, `ToolRuntime`, `ExecTarget`.
- Inputs/outputs: `ToolCall` -> tool result envelope message (`openagent.tool_result.v1`).
- Error strategy: structured `ToolErrorCode` in envelope.
- Invariants: scoped path checks, shell/write gating, strict schema arg validation when enabled.
- Failure modes: invalid args, out-of-scope path, shell unavailable/not found, non-zero exits.
- Extension points: add tool definition in `builtin_tools_enabled` and execution branch in `execute_tool`.
- * `Evidence: src/tools.rs#builtin_tools_enabled`
- * `Evidence: src/tools.rs#execute_tool`
- * `Evidence: src/tools.rs#ToolErrorCode`
- * `Evidence: src/target.rs#ExecTarget`
- * `Evidence: src/target.rs#HostTarget`
- * `Evidence: src/target.rs#DockerTarget`

### Trust, Policy, Approvals, Audit
Purpose: decide allow/deny/approval and persist approvals/audit trails.

- Key symbols: `TrustGate::decide`, `Policy::evaluate`, `ApprovalsStore`, `AuditLog`.
- Inputs/outputs: gate context + tool call -> `GateDecision` and audit event append.
- Error strategy: gate returns deny on approvals-store failures.
- Invariants: hard gates for shell/write flags, policy include resolution and version checks.
- Failure modes: approval key mismatch (v1/v2), expired/exhausted approvals, malformed policy.
- Extension points: policy condition ops, approval provenance/keying updates, audit schema additions.
- * `Evidence: src/gate.rs#TrustGate`
- * `Evidence: src/gate.rs#GateDecision`
- * `Evidence: src/trust/policy.rs#Policy::evaluate`
- * `Evidence: src/trust/policy.rs#Policy::from_path`
- * `Evidence: src/trust/approvals.rs#ApprovalsStore`
- * `Evidence: src/trust/audit.rs#AuditLog`

### MCP Integration
Purpose: load MCP server config, spawn clients, namespace tools, execute MCP tool calls.

- Key symbols: `McpRegistry::from_config_path`, `call_namespaced_tool`, `McpClient::call`.
- Inputs/outputs: enabled server names + config path -> tool defs; tool call -> tool envelope.
- Error strategy: schema validation before call; timeout cancellation notification.
- Invariants: namespaced tool naming (`mcp.<server>.<tool>`), docs/catalog hash generation.
- Failure modes: server spawn errors, timed-out calls, invalid config JSON.
- Extension points: additional MCP metadata capture, docs rendering, pinning checks.
- * `Evidence: src/mcp/registry.rs#McpRegistry::from_config_path`
- * `Evidence: src/mcp/registry.rs#McpRegistry::call_namespaced_tool`
- * `Evidence: src/mcp/registry.rs#default_config`
- * `Evidence: src/mcp/client.rs#McpClient::call`

### State, Replay, Repro, Session
Purpose: state path resolution, artifact persistence, replay/verify, and session memory/settings.

- Key symbols: `resolve_state_paths`, `write_run_record`, `verify_run_record`, `SessionStore`.
- Inputs/outputs: workdir + overrides -> state paths; run outcome -> persisted JSON artifact.
- Error strategy: atomic writes with temp files.
- Invariants: session v2 schema; deterministic config hash and stable path rendering.
- Failure modes: corrupted state JSON, missing run id artifact.
- Extension points: run record schema fields, session settings precedence.
- * `Evidence: src/store.rs#resolve_state_paths`
- * `Evidence: src/store/io.rs#write_run_record`
- * `Evidence: src/repro.rs#verify_run_record`
- * `Evidence: src/session.rs#SessionStore`
- * `Evidence: src/session.rs#resolve_run_settings`

### Eval, Checks, Task Graph
Purpose: deterministic evaluation runs, check runner orchestration, DAG task execution.

- Key symbols: `run_eval`, `run_check_command`, `run_tasks_graph`.
- Inputs/outputs: eval/check/task configs -> reports, artifacts, and exit codes.
- Error strategy: explicit non-zero exit classes for checks/tasks/eval thresholds.
- Invariants: check runner forces `approval_mode=fail` and `no_session=true`; task checkpoint hash match required.
- Failure modes: verifier failures/timeouts, checkpoint mismatch, missing required capabilities.
- Extension points: new eval tasks/verifiers, check frontmatter criteria, taskfile defaults/overrides.
- * `Evidence: src/eval/runner.rs#run_eval`
- * `Evidence: src/cli_dispatch_checks.rs#run_check_command`
- * `Evidence: src/checks/runner.rs#CheckRunExit`
- * `Evidence: src/tasks_graph_runtime.rs#run_tasks_graph`
- * `Evidence: src/taskgraph.rs#load_or_init_checkpoint`

## Level 3: Critical Execution Paths
### Flow 1: `localagent --provider ... --model ... --prompt ... run`
1. Parse args and enforce top-level invariants (`--no-limits` requires `--unsafe`, sampling checks).
2. Resolve workdir/state paths and optional auto-init.
3. Select provider implementation and base URL.
4. Call `run_agent`, then `run_agent_with_ui`.
5. Build gate/event sink/session/context/MCP/tools/hooks, run agent loop, write artifact.
6. Map provider failures to user-facing hints.

Validation points:
- Sampling and output mode compatibility checks.
- Gate hard checks for shell/write flags and policy.

Side effects:
- Reads/writes `.localagent/*`; may write events JSONL and audit JSONL.
- May execute tools/processes depending on flags and decisions.

Error mapping:
- Provider errors become CLI error with doctor hint and non-zero exit.

* `Evidence: src/cli_dispatch.rs#validate_sampling_args`
* `Evidence: src/cli_dispatch.rs#validate_run_output_mode`
* `Evidence: src/cli_dispatch.rs#run_cli`
* `Evidence: src/agent_runtime.rs#run_agent_with_ui`
* `Evidence: src/runtime_wiring.rs#build_gate`
* `Evidence: src/store/io.rs#write_run_record`

### Flow 2: Config and state resolution before runtime
1. `run_cli` canonicalizes workdir and applies ephemeral run defaults for run/exec when session/state-dir not explicit.
2. `resolve_state_paths` derives state, policy, approvals, audit, runs, sessions locations.
3. `startup_init::maybe_auto_init_state` ensures `.localagent` scaffolding when needed.
4. `scaffold::run_init` can materialize templates (`policy.yaml`, `hooks.yaml`, `instructions.yaml`, `mcp_servers.json`, eval/task templates).

* `Evidence: src/cli_dispatch.rs#apply_run_command_defaults`
* `Evidence: src/store.rs#resolve_state_paths`
* `Evidence: src/startup_init.rs#maybe_auto_init_state`
* `Evidence: src/scaffold.rs#run_init`

### Flow 3: Tool decision and execution path
1. Agent produces `ToolCall`.
2. Gate decides allow/deny/require-approval (`TrustGate::decide`).
3. If allowed: execute built-in tool (`execute_tool`) or MCP namespaced tool (`call_namespaced_tool`).
4. Emit tool decision/execution events and audit entry.
5. Tool result envelopes are appended to transcript; retries/guards may trigger protocol or repeat-block behavior.

Validation points:
- Tool arg schema checks.
- Path scope checks.
- Approval-key/version matching.

Side effects:
- Filesystem/shell/network/browser based on tool type.

* `Evidence: src/agent.rs#run`
* `Evidence: src/gate.rs#TrustGate::decide`
* `Evidence: src/tools.rs#execute_tool`
* `Evidence: src/mcp/registry.rs#McpRegistry::call_namespaced_tool`
* `Evidence: src/trust/audit.rs#AuditLog::append`

### Flow 4: `localagent eval ...`
1. CLI routes to `handle_eval_command`.
2. Profile overrides applied; config validated and `EvalConfig` built.
3. `run_eval` executes task matrix (models x tasks x runs), creates per-run workdirs, runs agent, applies assertions/verifiers.
4. Writes eval results JSON (+ optional junit/summary markdown), optional baseline compare and bundle generation.

Validation points:
- Required `--models` and non-empty split.
- Capability skip gates for write/shell/MCP requirements.

Side effects:
- Creates run artifacts and eval outputs under state dir.

* `Evidence: src/cli_dispatch_eval_replay.rs#handle_eval_command`
* `Evidence: src/eval/runner.rs#run_eval`
* `Evidence: src/eval/runner.rs#missing_capability_reason`
* `Evidence: src/eval/report.rs#write_results`

### Flow 5: `localagent tasks run --taskfile ...`
1. Load taskfile and compute hash.
2. Topologically order nodes and initialize/load checkpoint.
3. For each runnable node: merge defaults+overrides into `RunArgs`, resolve node workdir, optionally propagate summaries.
4. Execute node via `run_agent`; persist checkpoint after status transitions.
5. Emit taskgraph events and write taskgraph run artifact.

Validation points:
- Taskfile schema version and non-empty nodes.
- Checkpoint taskfile hash consistency.

Side effects:
- Writes checkpoint and graph-run artifacts; runs nested agent executions.

* `Evidence: src/tasks_graph_runtime.rs#run_tasks_graph`
* `Evidence: src/taskgraph.rs#load_taskfile`
* `Evidence: src/taskgraph.rs#topo_order`
* `Evidence: src/taskgraph.rs#write_checkpoint`
* `Evidence: src/taskgraph.rs#write_graph_run_artifact`

## Build, Run, Test
Build and run:
- Build binary: `cargo build --release`.
- Source install: `cargo install --path . --force`.
- Run one-shot: `localagent --provider <...> --model <...> --prompt "..." run`.

Primary test/lint gates:
- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`
- CI also runs `scripts/ci_release_readiness.py` and `cargo test --test tool_call_accuracy_ci`.

Manifest/toolchain notes:
- Single package manifest with one explicit default bin (`src/main.rs`).
- Additional helper test binaries under `src/bin/*` (`hook_stub`, `mcp_stub`) for integration tests.
- Build script injects git sha/target/build time env vars.

* `Evidence: README.md#Quick Start`
* `Evidence: CONTRIBUTING.md#Development Setup`
* `Evidence: .github/workflows/ci.yml#<config:jobs.ci.steps>`
* `Evidence: Cargo.toml#<config:[[bin]].name>`
* `Evidence: src/bin/hook_stub.rs#main`
* `Evidence: src/bin/mcp_stub.rs#main`
* `Evidence: build.rs#main`

## Configuration and State
State layout (derived, not hardcoded elsewhere):
- `state_dir` default: `<workdir>/.localagent`
- `policy`: `policy.yaml`
- `approvals`: `approvals.json`
- `audit`: `audit.jsonl`
- run artifacts: `runs/*.json`
- sessions: `sessions/*.json`

Config inputs:
- CLI flags (`RunArgs`, `EvalArgs`)
- optional files: `instructions.yaml`, `hooks.yaml`, `mcp_servers.json`, policy file with includes.
- reliability profile overlay (`--reliability-profile`) mutates run args before dispatch.

Unknowns:
- No separate global config file outside state dir found in this crate.
- Confirm by searching for additional config loaders beyond `runtime_paths`/`instructions`/`hooks`/`mcp`/`policy`.

* `Evidence: src/store.rs#resolve_state_paths`
* `Evidence: src/runtime_paths.rs#resolved_mcp_config_path`
* `Evidence: src/runtime_paths.rs#resolved_hooks_config_path`
* `Evidence: src/instruction_runtime.rs#resolve_instruction_messages`
* `Evidence: src/trust/policy.rs#Policy::from_path`
* `Evidence: src/reliability_profile.rs#apply_builtin_profile_to_run_args`

## Observability and Debugging
Primary debugging surfaces:
- Event stream to stdout (`--output json`) and/or file (`--events <path>`).
- Run artifact JSON with transcript/tool decisions/config fingerprints.
- Trust audit JSONL.
- Replay and replay verify commands.
- TUI tail command for event files.

Useful operator commands:
- `localagent replay <run_id>`
- `localagent replay verify <run_id> [--strict] [--json]`
- `localagent policy doctor|print-effective|test`
- `localagent approvals list|prune`, `approve`, `deny`
- `localagent tui tail --events ...`

* `Evidence: src/runtime_wiring.rs#build_event_sink`
* `Evidence: src/store/io.rs#write_run_record`
* `Evidence: src/trust/audit.rs#AuditLog::append`
* `Evidence: src/cli_dispatch_eval_replay.rs#handle_replay_command`
* `Evidence: src/cli_args.rs#TuiSubcommand`

## Operational Playbook
Repro steps:
1. Run with explicit provider/model/base-url and `--events` path.
2. Enable trust mode intentionally (`--trust on`) if validating policy/approval behavior.
3. Enable repro snapshots (`--repro on --repro-env safe`), or `all` only when needed.

State locations:
- `.localagent/runs` for per-run records.
- `.localagent/audit.jsonl` for trust audit.
- `.localagent/approvals.json` for approval lifecycle.
- `.localagent/tasks/checkpoint.json` and `.localagent/tasks/runs/*.json` for taskgraph state.

Safe cleanup:
- Delete specific run artifacts/checkpoints/sessions as needed.
- Avoid deleting policy/approvals blindly during incidents unless you intend to reset trust state.

Performance hotspots (evidence-based):
- Large TUI loop/event handling and rendering paths.
- Repo-map scan bounded by file/byte caps but potentially expensive near caps.
- Eval matrix loops (`models x tasks x runs`) and verifier subprocesses.

Safety/security notes:
- Path traversal guarded for tools/targets (`no absolute`, no `..`).
- Shell/write tools blocked unless allow flags or unsafe bypass.
- Docker target validates daemon/image and constrains workdir mounts.
- Hooks and MCP spawn subprocesses; hook output size/timeouts bounded.

Unknowns:
- No explicit secret redaction pipeline beyond hook-based customization and repro-env safeguards. Confirm by auditing hook defaults and repro env filter behavior.

* `Evidence: src/repro.rs#build_repro_record`
* `Evidence: src/store.rs#StatePaths`
* `Evidence: src/taskgraph.rs#checkpoint_default_path`
* `Evidence: src/chat_tui_runtime.rs#drive_tui_active_turn_loop`
* `Evidence: src/repo_map.rs#resolve_repo_map`
* `Evidence: src/eval/runner.rs#run_eval`
* `Evidence: src/target.rs#resolve_path_scoped`
* `Evidence: src/gate.rs#TrustGate::decide`
* `Evidence: src/target.rs#DockerTarget::validate_available`
* `Evidence: src/hooks/runner.rs#invoke_hook`

## Contribution Workflow
Repository conventions from config/docs/CI:
- Format gate: `cargo fmt --check`.
- Lint gate: `cargo clippy -- -D warnings`.
- Test gate: `cargo test` and dedicated `tool_call_accuracy_ci` test in CI.
- Release readiness script validates changelog/release-notes/schema-note coupling.

Test surfaces:
- Unit tests in many `src/*` modules.
- Integration tests under `tests/` (MCP, hooks, diagnostics, policy golden, artifact golden).
- Stub helper bins in `src/bin` for deterministic integration behavior.

* `Evidence: CONTRIBUTING.md#Development Setup`
* `Evidence: .github/workflows/ci.yml#<config:jobs.ci.steps>`
* `Evidence: scripts/ci_release_readiness.py#main`
* `Evidence: tests/policy_golden.rs#policy_golden_cases_are_stable`
* `Evidence: tests/artifact_golden.rs#run_artifact_schema_and_layout_golden_is_stable`
* `Evidence: src/bin/mcp_stub.rs#main`

### Rules for AI Agents Working in This Repo
- Always inventory and read entrypoints first, then follow the `Agent Start Checklist`.
- Always cite evidence bullets for non-trivial claims in docs/PR notes.
- Do not perform side effects without explicit operator approval when the action changes system/repo state beyond requested edits.
- Prefer small PR-sized changes; add or update tests near touched behavior; update docs for CLI/behavior changes.
- Keep builds reproducible; avoid changing `Cargo.lock` unless dependency changes require it and explain why.
- Avoid broad refactors unless requested; preserve public CLI behavior and compatibility unless change is required.

No explicit global operator-approval gating for source-code edits was found inside this crate beyond runtime tool/trust gates; side-effect governance for coding agents should be treated as external policy/process.

* `Evidence: src/gate.rs#TrustGate::decide`
* `Evidence: src/tools.rs#execute_tool`
* `Evidence: CONTRIBUTING.md#Project Principles`

## Risks and Tech Debt (Evidence-Based)
1. Monolithic files increase change risk and review complexity.
- The runtime-heavy files were split, but large test files still concentrate review risk and make targeted changes slower.
- * `Evidence: src/agent_tests.rs`
- * `Evidence: tests/mcp_impl_regression.rs`

2. Behavior duplication between eval/runtime gate-building paths.
- Similar trust gate construction exists in runtime and eval modules.
- * `Evidence: src/runtime_wiring.rs#build_gate`
- * `Evidence: src/eval/runner.rs#build_gate`

3. Operational complexity from many mode/flag interactions.
- Planner mode, agent mode, trust mode, approval mode, caps/hooks/session settings all interact.
- * `Evidence: src/cli_args.rs#RunArgs`
- * `Evidence: src/runtime_flags.rs#apply_agent_mode_capability_baseline`
- * `Evidence: src/session.rs#resolve_run_settings`

4. External process dependencies can fail in environment-specific ways.
- MCP servers, hooks, docker target, and shell tools rely on local executable/runtime availability.
- * `Evidence: src/mcp/client.rs#spawn_mcp_process`
- * `Evidence: src/hooks/runner.rs#invoke_hook`
- * `Evidence: src/target.rs#DockerTarget::validate_available`

## Glossary
- Agent mode: high-level behavior profile (`build` or `plan`) that can baseline capabilities.
- Planner-worker mode: two-phase planner + worker run orchestration.
- Trust gate: policy + approvals + audit decision engine for tool calls.
- MCP: Model Context Protocol server/tool integration over stdio JSON-RPC.
- Run artifact: persisted JSON record of a run under `.localagent/runs`.
- Repro snapshot: reproducibility fingerprint of runtime/environment config.
- Check runner: deterministic pass/fail runner for markdown-defined checks.
- Task graph: DAG taskfile execution with checkpoints and per-node runs.

* `Evidence: src/cli_args.rs#AgentMode`
* `Evidence: src/planner.rs#RunMode`
* `Evidence: src/gate.rs#TrustGate`
* `Evidence: src/mcp/mod.rs#registry`
* `Evidence: src/store/types.rs#RunRecord`
* `Evidence: src/repro.rs#RunReproRecord`
* `Evidence: src/checks/runner.rs#CheckRunExit`
* `Evidence: src/taskgraph.rs#TaskFile`

## Appendix: File and Symbol Index
Curated file index:
- `Cargo.toml`: package/bin/dependency manifest.
- `src/main.rs`: process entrypoint and tokio runtime bootstrap.
- `src/cli_args.rs`: clap command/flag schema.
- `src/cli_dispatch.rs`: top-level command routing.
- `src/agent_runtime.rs`: run orchestration facade and artifact finalization entrypoints.
- `src/agent_runtime/*`: setup/launch/planner/finalize helper modules.
- `src/agent.rs`: core agent loop and tool-call lifecycle.
- `src/tools.rs`: built-in tools facade and `execute_tool` dispatcher.
- `src/tools/*`: tool catalog/schema/envelope/exec helper modules.
- `src/gate.rs`: trust/no-gate decision implementations and public approval-key surface.
- `src/gate/helpers.rs`: internal approval-key and exec-target helper functions.
- `src/trust/policy.rs`: policy parse/eval/includes/allowlists.
- `src/mcp/registry.rs`: MCP config load, tool import, tool calls.
- `src/store.rs` + `src/store/io.rs`: state path resolution and run record IO.
- `src/eval/runner.rs`: eval matrix execution.
- `src/cli_dispatch_checks.rs`: check run orchestration.
- `src/tasks_graph_runtime.rs`: DAG task run executor.
- `src/chat_tui_runtime.rs`: interactive TUI chat runtime.
- `src/chat_ui.rs`: chat screen rendering facade.
- `src/chat_ui/overlay.rs`: learn overlay types and overlay rendering.

Key symbol index:
- `main` in `src/main.rs`
- `run_cli` in `src/cli_dispatch.rs`
- `Cli`, `RunArgs`, `Commands` in `src/cli_args.rs`
- `run_agent_with_ui` in `src/agent_runtime.rs`
- `Agent::run` in `src/agent.rs`
- `execute_tool`, `builtin_tools_enabled`, `ToolRuntime` in `src/tools.rs`
- `TrustGate::decide` in `src/gate.rs`
- `Policy::from_path`, `Policy::evaluate` in `src/trust/policy.rs`
- `ApprovalsStore::consume_matching_approved` in `src/trust/approvals.rs`
- `McpRegistry::from_config_path`, `McpRegistry::call_namespaced_tool` in `src/mcp/registry.rs`
- `write_run_record` in `src/store/io.rs`
- `run_eval` in `src/eval/runner.rs`
- `run_check_command` in `src/cli_dispatch_checks.rs`
- `run_tasks_graph` in `src/tasks_graph_runtime.rs`
- `run_chat_tui` in `src/chat_tui_runtime.rs`

* `Evidence: src/main.rs#main`
* `Evidence: src/cli_dispatch.rs#run_cli`
* `Evidence: src/cli_args.rs#RunArgs`
* `Evidence: src/agent_runtime.rs#run_agent_with_ui`
* `Evidence: src/agent.rs#run`
* `Evidence: src/tools.rs#execute_tool`
* `Evidence: src/gate.rs#TrustGate::decide`
* `Evidence: src/trust/policy.rs#Policy::evaluate`
* `Evidence: src/mcp/registry.rs#McpRegistry::call_namespaced_tool`
* `Evidence: src/store/io.rs#write_run_record`
* `Evidence: src/eval/runner.rs#run_eval`
* `Evidence: src/tasks_graph_runtime.rs#run_tasks_graph`
* `Evidence: src/chat_tui_runtime.rs#run_chat_tui`
