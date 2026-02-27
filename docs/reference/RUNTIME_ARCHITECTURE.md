# Runtime Architecture

Status: Active  
Owner: LocalAgent maintainers  
Last reviewed: 2026-02-27

This document maps LocalAgent runtime flow after the CLI/runtime modularization refactor.

Use it to answer:

- Where does a run start?
- Where are provider detection and startup UX handled?
- Where are trust/policy/approvals enforced?
- Where do MCP tools get loaded/called?
- Where are artifacts written?
- Where should I change behavior X?

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
  - Main runtime orchestration for provider + tools + session + run artifact writing
  - Integrates gate, hooks, MCP registry, TUI/event sinks, planner/worker paths
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
  - `ToolGate` trait, `NoGate`, `TrustGate`, gate decisions, approval keying
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
  - Built-in tool schemas, exposure, validation, execution
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
  - Chat screen rendering composition
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
4. Renderers (`src/chat_ui.rs`, `src/chat_view_utils.rs`) draw panels/footer/banner/status

## Where To Change X

| Change you want | Primary file(s) |
|---|---|
| Add or change CLI flags/subcommands | `src/cli_args.rs`, `src/cli_dispatch.rs` |
| Change startup setup screen UX / keybindings / warnings | `src/startup_bootstrap.rs` |
| Change provider auto-detection behavior/messages | `src/startup_detect.rs` |
| Change provider resolution/runtime creation | `src/provider_runtime.rs`, `src/providers/*` |
| Change built-in tool schemas or execution | `src/tools.rs` |
| Change MCP server loading/tool import/call routing | `src/mcp/registry.rs`, `src/mcp/client.rs` |
| Change trust decisions (allow/deny/approval) | `src/gate.rs`, `src/trust/policy.rs` |
| Change approval storage/TTL/max-uses behavior | `src/trust/approvals.rs` |
| Change audit logging | `src/trust/audit.rs` |
| Change event streaming/log sinks | `src/runtime_wiring.rs`, `src/events.rs`, `src/runtime_events.rs` |
| Change run artifact schema/layout | `src/store.rs` |
| Change config fingerprint contents | `src/runtime_paths.rs`, `src/store.rs` |
| Change orchestrator qualification/read-only fallback | `src/qualification.rs`, `src/run_prep.rs` |
| Change planner/worker mode orchestration | `src/agent_runtime.rs`, `src/planner_runtime.rs`, `src/agent.rs` |
| Change task graph execution/artifacts | `src/tasks_graph_runtime.rs`, `src/taskgraph.rs` |
| Change TUI chat screen rendering/footer/banner | `src/chat_ui.rs`, `src/chat_view_utils.rs`, `src/tui/*` |
| Change session persistence/settings precedence | `src/session.rs`, `src/runtime_flags.rs` |
| Change repro record/bundle behavior | `src/repro.rs` |

## Contributor Notes

- Prefer adding tests at runtime seams when changing orchestration helpers (`runtime_*`, `run_prep`, `startup_*`).
- Artifact schema changes should update the golden fixture in `tests/fixtures/artifacts/` intentionally.
- Keep `src/main.rs` thin; route new CLI behavior through `src/cli_dispatch.rs` and `src/cli_args.rs`.
