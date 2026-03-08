# File and Symbol Index

Status: Active  
Owner: LocalAgent maintainers  
Last reviewed: 2026-03-07

## Glossary

- Agent mode: high-level behavior profile (`build` or `plan`) that can baseline capabilities.
- Planner-worker mode: two-phase planner + worker run orchestration.
- Trust gate: policy + approvals + audit decision engine for tool calls.
- MCP: Model Context Protocol server/tool integration over stdio JSON-RPC.
- Run artifact: persisted JSON record of a run under the resolved state dir; commonly `.localagent/runs` for persistent state, but one-shot `run` / `exec` default to an ephemeral temp state dir unless `--state-dir` is set.
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

## Curated File Index

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

## Key Symbol Index

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

## Related Docs

- Architecture: [../architecture/RUNTIME_ARCHITECTURE.md](../architecture/RUNTIME_ARCHITECTURE.md)
- Configuration and state: [CONFIGURATION_AND_STATE.md](CONFIGURATION_AND_STATE.md)
- CLI reference: [CLI_REFERENCE.md](CLI_REFERENCE.md)
