# Runtime Architecture

Status: Active  
Owner: LocalAgent maintainers  
Last reviewed: 2026-03-06

This document maps LocalAgent runtime flow after the CLI/runtime modularization refactor.

Use it to answer:

- Where does a run start?
- Where are provider detection and startup UX handled?
- Where are trust/policy/approvals enforced?
- Where do MCP tools get loaded/called?
- Where are artifacts written?
- Where should I change behavior X?

## One-Screen Overview

What this repo does:
- Provides `localagent` CLI/runtime for local model providers (`lmstudio`, `llamacpp`, `ollama`, plus `mock`) with tool calling, trust/approval policy, MCP tools, replay/repro artifacts, eval/check workflows, and TUI chat.

Who uses it:
- Operators: run/chat, approvals, policy doctor/test, replay/eval/tasks/check commands.
- Developers: extend runtime modules, tools, providers, policies, hooks, tests.

Main runtime modes:
- Single-run agent mode (`--mode single`).
- Planner-worker orchestration (`--mode planner-worker`).
- Agent behavior mode (`--agent-mode build|plan`).

Core modules/crates:
- `cli_dispatch` / `cli_args`: command parsing and routing.
- `agent_runtime` / `agent`: orchestration and loop.
- `tools` / `target`: tool execution and host/docker target.
- `trust` / `gate`: policy, approvals, and audit decisions.
- `mcp`: external tool registry and RPC client.
- `store` / `repro` / `session`: state, artifacts, replay, reproducibility, session memory.

* `Evidence: src/cli_args.rs#ProviderKind`
* `Evidence: src/cli_args.rs#Commands`
* `Evidence: src/provider_runtime.rs#default_base_url`
* `Evidence: src/planner.rs#RunMode`
* `Evidence: src/cli_args.rs#AgentMode`
* `Evidence: src/lib.rs#agent`
* `Evidence: src/lib.rs#mcp`
* `Evidence: src/lib.rs#store`
* `Evidence: src/lib.rs#trust`
* `Evidence: src/lib.rs#session`

## Entry Points

### CLI bootstrap

- `src/main.rs`
  - Thin executable bootstrap
  - Declares modules and delegates to CLI dispatch
- `src/cli_args.rs`
  - Clap command/flag definitions (`Commands`, `RunArgs`, eval args, etc.)
- `src/cli_dispatch.rs`
  - Command routing and top-level execution dispatch

### Library surface

- `src/lib.rs`
  - Exposes reusable subsystems (`agent`, `gate`, `store`, `mcp`, `tui`, etc.)

## High-Level Runtime Flow

## Architecture Map

Components and responsibilities:
1. CLI front door (`main`, `cli_args`, `cli_dispatch`).
2. Runtime orchestrator (`agent_runtime`, `runtime_wiring`, `run_prep`).
3. Execution kernel (`agent`, `tools`, `target`).
4. Trust plane (`gate`, `trust/policy`, `trust/approvals`, `trust/audit`).
5. Integrations (`providers/*`, `mcp/*`, `hooks/*`).
6. State and observability (`store`, `events`, `runtime_events`, `repro`, `session`).
7. UX surfaces (`startup_*`, `chat_*`, `tui/*`).

Data/control flow, primary run path:
`main -> run_cli -> provider selection -> run_agent -> run_agent_with_ui -> build_gate/build_event_sink -> prepare_tools_and_qualification -> Agent::run -> write_run_record`

External integrations:
- HTTP providers via `reqwest` (OpenAI-compatible plus Ollama).
- MCP servers via spawned stdio process and JSON-RPC.
- Optional Docker exec target for tool execution.

Side effects model:
- Filesystem read/write via built-in tools and state artifact writes.
- Process spawn for shell tools, hooks, MCP servers, and optional Docker.
- Network via provider HTTP and MCP remote endpoints behind MCP servers.

Observability surfaces:
- Event sinks: stdout JSON projection, JSONL file sink, TUI sink.
- Run artifacts in `.localagent/runs` with config hash, fingerprint, and tool decisions.
- Audit log appends (`audit.jsonl`) for trust decisions.

* `Evidence: src/main.rs#main`
* `Evidence: src/agent_runtime.rs#run_agent_with_ui`
* `Evidence: src/tools.rs#execute_tool`
* `Evidence: src/gate.rs#ToolGate`
* `Evidence: src/providers/mod.rs#ModelProvider`
* `Evidence: src/mcp/registry.rs#McpRegistry`
* `Evidence: src/store/io.rs#write_run_record`
* `Evidence: src/chat_tui_runtime.rs#run_chat_tui`
* `Evidence: Cargo.toml#<config:dependencies.reqwest>`
* `Evidence: src/providers/openai_compat.rs#OpenAiCompatProvider`
* `Evidence: src/providers/ollama.rs#OllamaProvider`
* `Evidence: src/mcp/client.rs#McpClient::spawn`
* `Evidence: src/target.rs#DockerTarget`
* `Evidence: src/hooks/runner.rs#invoke_hook`
* `Evidence: src/trust/audit.rs#AuditLog::append`

### Standard run (`localagent ...`)

1. `src/main.rs` -> `cli_dispatch::run_cli()`
2. CLI args parsed from `src/cli_args.rs`
3. Command path resolves runtime config + state paths
4. Provider setup and model selection (or startup UX path)
5. `src/agent_runtime.rs` orchestrates the run
6. Runtime wiring builds:
   - event sink (`src/runtime_wiring.rs`)
   - gate/policy/approvals (`src/runtime_wiring.rs`)
7. Tool preparation + qualification (`src/run_prep.rs`)
8. Agent loop executes (`src/agent.rs`)
9. Run artifacts written (`src/store.rs`)
10. Optional TUI updates/rendering (`src/chat_tui_runtime.rs`, `src/tui/*`)

### Startup setup UX path (`localagent` with no immediate run task)

1. `src/cli_dispatch.rs` routes into startup bootstrap flow
2. `src/startup_bootstrap.rs` drives the interactive startup screen
3. `src/startup_detect.rs` probes local providers (LM Studio / Ollama / llama.cpp)
4. User selects mode/provider/model and launches chat/run
5. Runtime transitions into chat/TUI execution (`src/chat_tui_runtime.rs` / `src/agent_runtime.rs`)

## Module Responsibilities

### Runtime orchestration

- `src/agent_runtime.rs`
  - Main runtime orchestration facade for provider + tools + session + run artifact writing
  - Integrates gate, hooks, MCP registry, TUI/event sinks, planner/worker paths
- `src/agent_runtime/*`
  - Focused runtime helpers for setup, launch, planner phase, guard logic, and finalize paths
- `src/agent.rs`
  - Core agent loop behavior, tool-call execution lifecycle, protocol enforcement, retries
- `src/chat_runtime.rs`
  - Shared chat-mode orchestration utilities
- `src/chat_repl_runtime.rs`
  - Non-TUI chat runtime flow
- `src/chat_tui_runtime.rs`
  - TUI chat runtime orchestration and UI loop integration
- `src/planner_runtime.rs`
  - Planner-focused runtime path orchestration
- `src/tasks_graph_runtime.rs`
  - Task graph execution runtime and graph artifact wiring

### Startup / onboarding UX

- `src/startup_bootstrap.rs`
  - Interactive startup/setup screen, mode selection, refresh controls, provider readiness
- `src/startup_detect.rs`
  - Local provider detection and provider/model probe status
- `src/startup_init.rs`
  - Startup initialization helpers and state/bootstrap setup

### Runtime seams and shared wiring

- `src/runtime_wiring.rs`
  - Builds event sinks and trust gate/policy wiring
- `src/runtime_paths.rs`
  - Resolved config path helpers and `RunCliConfig` / fingerprint builders
- `src/runtime_config.rs`
  - Runtime config shaping/defaults helpers
- `src/runtime_events.rs`
  - Event emission helpers
- `src/runtime_flags.rs`
  - Explicit flag parsing and settings precedence helpers
- `src/run_prep.rs`
  - Tool prep seam: built-ins + MCP exposure + orchestrator qualification

### Providers and model integration

- `src/provider_runtime.rs`
  - Provider creation/selection helpers used by startup/runtime paths
- `src/providers/*`
  - Provider implementations (`ollama`, `openai_compat`, etc.)
- `src/qualification.rs`
  - Qualification probing and readonly fallback logic

### Trust / policy / approvals / audit

- `src/gate.rs`
  - `ToolGate` trait, `NoGate`, `TrustGate`, gate decisions, and public approval-key helpers
- `src/gate/helpers.rs`
  - Approval-key hashing, workdir normalization, and exec-target argument shaping
- `src/trust/policy.rs`
  - Policy parsing/evaluation/includes/MCP allowlist logic
- `src/trust/approvals.rs`
  - Approval persistence and approval decision transitions
- `src/trust/audit.rs`
  - Audit log persistence
- `src/approvals_ops.rs`
  - User-facing approval management operations

### Tools / MCP

- `src/tools.rs`
  - Thin built-in tools facade and top-level `execute_tool` dispatcher
- `src/tools/*`
  - Tool catalog, schema handling, envelopes, exec support, and per-side-effect execution helpers
- `src/mcp/registry.rs`
  - MCP config loading, client startup, tool import, tool invocation, catalog hashing
- `src/mcp/client.rs`
  - MCP protocol request/response client and timeout handling

### Persistence / artifacts / replay

- `src/store.rs`
  - `.localagent` state path resolution
  - run artifact write/read (`write_run_record`, `load_run_record`)
  - config hashing helpers
  - replay rendering helpers
- `src/session.rs`
  - Session persistence and settings resolution
- `src/repro.rs`
  - Repro bundle/record construction
- `src/taskgraph.rs`
  - Task graph file model, checkpoints, graph artifact writing

### TUI / view layer

- `src/tui/*`
  - TUI state, tail parsing, rendering/event sink integration
- `src/chat_ui.rs`
  - Chat screen rendering facade and side-pane composition
- `src/chat_ui/overlay.rs`
  - Learn overlay rendering model, overlay tabs, and overlay-specific rendering helpers
- `src/chat_view_utils.rs`
  - Shared banner/header/footer view helpers (including version/cwd display)

## Call Flow (Detailed)

### Core run path (`agent_runtime`)

`src/agent_runtime.rs` orchestrates the runtime in this approximate order:

1. Resolve session/settings/instruction messages
2. Resolve MCP and hooks config paths (`src/runtime_paths.rs`)
3. Build MCP registry (if enabled) (`src/mcp/registry.rs`)
4. Build gate (`src/runtime_wiring.rs::build_gate`)
5. Prepare tools + qualification (`src/run_prep.rs`)
6. Build hook manager (`src/hooks/runner.rs`)
7. Build event sink (`src/runtime_wiring.rs::build_event_sink`)
8. Run planner/worker or single-agent path (`src/agent.rs`, `src/planner.rs`)
9. Emit events + collect tool decisions / hook report / MCP runtime trace
10. Build CLI config + config fingerprint (`src/runtime_paths.rs`)
11. Write run artifact (`src/store.rs::write_run_record`)

### Tool gating / approvals decision path

1. Agent wants to execute a tool (`src/agent.rs`)
2. `ToolGate::decide(...)` called (`src/gate.rs`)
3. `TrustGate` evaluates policy (`src/trust/policy.rs`)
4. If required, checks approvals store (`src/trust/approvals.rs`)
5. Returns one of:
   - `Allow`
   - `Deny`
   - `RequireApproval`
6. Decision recorded to audit/events/tool decision records

### MCP tool lifecycle path

1. MCP servers configured via CLI/profile/config
2. `McpRegistry::from_config_path(...)` loads config and starts clients
3. Registry imports MCP tools as namespaced definitions (`mcp.<server>.<tool>`)
4. `run_prep` merges MCP tools into the exposed tool catalog (subject to policy exposure filter)
5. Runtime tool execution routes MCP calls through registry/client
6. MCP catalog snapshots + hashes recorded in run artifacts (`src/store.rs`)

### TUI rendering path

1. Runtime builds optional UI event sink (`src/runtime_wiring.rs`)
2. Events stream into TUI state (`src/tui/*`)
3. Chat TUI runtime loop (`src/chat_tui_runtime.rs`) drives input + refresh
4. Renderers (`src/chat_ui.rs`, `src/chat_ui/overlay.rs`, `src/chat_view_utils.rs`) draw panels/footer/banner/status

## Critical Execution Flows

### Flow 1: `localagent --provider ... --model ... --prompt ... run`
1. Parse args and enforce top-level invariants (`--no-limits` requires `--unsafe`, sampling checks).
2. Resolve workdir/state paths and optional auto-init.
3. Select provider implementation and base URL.
4. Call `run_agent`, then `run_agent_with_ui`.
5. Build gate, event sink, session, context, MCP, tools, and hooks; run the agent loop; write artifact.
6. Map provider failures to user-facing hints.

Validation points:
- Sampling and output-mode compatibility checks.
- Gate hard checks for shell/write flags and policy.

Side effects:
- Reads and writes `.localagent/*`; may write events JSONL and audit JSONL.
- May execute tools and processes depending on flags and decisions.

* `Evidence: src/cli_dispatch.rs#validate_sampling_args`
* `Evidence: src/cli_dispatch.rs#validate_run_output_mode`
* `Evidence: src/cli_dispatch.rs#run_cli`
* `Evidence: src/agent_runtime.rs#run_agent_with_ui`
* `Evidence: src/runtime_wiring.rs#build_gate`
* `Evidence: src/store/io.rs#write_run_record`

### Flow 2: Config and state resolution before runtime
1. `run_cli` canonicalizes workdir and applies ephemeral run defaults for run/exec when session/state-dir are not explicit.
2. `resolve_state_paths` derives state, policy, approvals, audit, runs, and sessions locations.
3. `startup_init::maybe_auto_init_state` ensures `.localagent` scaffolding when needed.
4. `scaffold::run_init` can materialize templates (`policy.yaml`, `hooks.yaml`, `instructions.yaml`, `mcp_servers.json`, eval/task templates).

* `Evidence: src/cli_dispatch.rs#apply_run_command_defaults`
* `Evidence: src/store.rs#resolve_state_paths`
* `Evidence: src/startup_init.rs#maybe_auto_init_state`
* `Evidence: src/scaffold.rs#run_init`

### Flow 3: Tool decision and execution path
1. Agent produces `ToolCall`.
2. Gate decides allow, deny, or require-approval (`TrustGate::decide`).
3. If allowed: execute built-in tool (`execute_tool`) or MCP namespaced tool (`call_namespaced_tool`).
4. Emit tool decision/execution events and audit entry.
5. Tool result envelopes are appended to transcript; retries/guards may trigger protocol or repeat-block behavior.

Validation points:
- Tool arg schema checks.
- Path scope checks.
- Approval-key/version matching.

* `Evidence: src/agent.rs#run`
* `Evidence: src/gate.rs#TrustGate::decide`
* `Evidence: src/tools.rs#execute_tool`
* `Evidence: src/mcp/registry.rs#McpRegistry::call_namespaced_tool`
* `Evidence: src/trust/audit.rs#AuditLog::append`

### Flow 4: `localagent eval ...`
1. CLI routes to `handle_eval_command`.
2. Profile overrides applied; config validated and `EvalConfig` built.
3. `run_eval` executes the task matrix (`models x tasks x runs`), creates per-run workdirs, runs the agent, and applies assertions/verifiers.
4. Writes eval results JSON, optional JUnit/summary markdown, and optional baseline compare or bundle artifacts.

Validation points:
- Required `--models` and non-empty split.
- Capability skip gates for write/shell/MCP requirements.

* `Evidence: src/cli_dispatch_eval_replay.rs#handle_eval_command`
* `Evidence: src/eval/runner.rs#run_eval`
* `Evidence: src/eval/runner.rs#missing_capability_reason`
* `Evidence: src/eval/report.rs#write_results`

### Flow 5: `localagent tasks run --taskfile ...`
1. Load taskfile and compute hash.
2. Topologically order nodes and initialize or load checkpoint.
3. For each runnable node: merge defaults/overrides into `RunArgs`, resolve node workdir, and optionally propagate summaries.
4. Execute node via `run_agent`; persist checkpoint after status transitions.
5. Emit taskgraph events and write taskgraph run artifact.

Validation points:
- Taskfile schema version and non-empty nodes.
- Checkpoint taskfile hash consistency.

* `Evidence: src/tasks_graph_runtime.rs#run_tasks_graph`
* `Evidence: src/taskgraph.rs#load_taskfile`
* `Evidence: src/taskgraph.rs#topo_order`
* `Evidence: src/taskgraph.rs#write_checkpoint`
* `Evidence: src/taskgraph.rs#write_graph_run_artifact`

## Where To Change X

| Change you want | Primary file(s) |
|---|---|
| Add or change CLI flags/subcommands | `src/cli_args.rs`, `src/cli_dispatch.rs` |
| Change startup setup screen UX / keybindings / warnings | `src/startup_bootstrap.rs` |
| Change provider auto-detection behavior/messages | `src/startup_detect.rs` |
| Change provider resolution/runtime creation | `src/provider_runtime.rs`, `src/providers/*` |
| Change built-in tool schemas or execution | `src/tools.rs`, `src/tools/*` |
| Change MCP server loading/tool import/call routing | `src/mcp/registry.rs`, `src/mcp/client.rs` |
| Change trust decisions (allow/deny/approval) | `src/gate.rs`, `src/gate/helpers.rs`, `src/trust/policy.rs` |
| Change approval storage/TTL/max-uses behavior | `src/trust/approvals.rs` |
| Change audit logging | `src/trust/audit.rs` |
| Change event streaming/log sinks | `src/runtime_wiring.rs`, `src/events.rs`, `src/runtime_events.rs` |
| Change run artifact schema/layout | `src/store.rs` |
| Change config fingerprint contents | `src/runtime_paths.rs`, `src/store.rs` |
| Change orchestrator qualification/read-only fallback | `src/qualification.rs`, `src/run_prep.rs` |
| Change planner/worker mode orchestration | `src/agent_runtime.rs`, `src/agent_runtime/*`, `src/planner_runtime.rs`, `src/agent.rs` |
| Change task graph execution/artifacts | `src/tasks_graph_runtime.rs`, `src/taskgraph.rs` |
| Change TUI chat screen rendering/footer/banner | `src/chat_ui.rs`, `src/chat_ui/overlay.rs`, `src/chat_view_utils.rs`, `src/tui/*` |
| Change session persistence/settings precedence | `src/session.rs`, `src/runtime_flags.rs` |
| Change repro record/bundle behavior | `src/repro.rs` |

## Contributor Notes

- Prefer adding tests at runtime seams when changing orchestration helpers (`runtime_*`, `run_prep`, `startup_*`).
- Artifact schema changes should update the golden fixture in `tests/fixtures/artifacts/` intentionally.
- Keep `src/main.rs` thin; route new CLI behavior through `src/cli_dispatch.rs` and `src/cli_args.rs`.

## Related Docs

- Operations: [../operations/OPERATIONAL_RUNBOOK.md](../operations/OPERATIONAL_RUNBOOK.md)
- Configuration and state: [../reference/CONFIGURATION_AND_STATE.md](../reference/CONFIGURATION_AND_STATE.md)
- CLI reference: [../reference/CLI_REFERENCE.md](../reference/CLI_REFERENCE.md)
- Runtime policy: [../policy/AGENT_RUNTIME_PRINCIPLES_2026.md](../policy/AGENT_RUNTIME_PRINCIPLES_2026.md)
