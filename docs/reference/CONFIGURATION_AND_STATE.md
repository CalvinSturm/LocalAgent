# Configuration and State

Status: Active  
Owner: LocalAgent maintainers  
Last reviewed: 2026-03-07

This document covers LocalAgent state layout, config inputs, and the path resolution model used by the runtime.

## State Layout

State layout is derived, not hardcoded elsewhere:

- `state_dir` default: `<workdir>/.localagent`
- `policy`: `policy.yaml`
- `approvals`: `approvals.json`
- `audit`: `audit.jsonl`
- run artifacts: `runs/*.json`
- sessions: `sessions/*.json`

Related runtime state commonly used during debugging:
- eval outputs under the resolved state dir
- task checkpoints and graph run artifacts under the resolved task paths

* `Evidence: src/store.rs#resolve_state_paths`
* `Evidence: src/store.rs#StatePaths`
* `Evidence: src/taskgraph.rs#checkpoint_default_path`

## Config Inputs

Primary config inputs:
- CLI flags (`RunArgs`, `EvalArgs`)
- optional files: `instructions.yaml`, `hooks.yaml`, `mcp_servers.json`, and policy file with includes
- reliability profile overlay (`--reliability-profile`) mutates run args before dispatch

* `Evidence: src/cli_args.rs#RunArgs`
* `Evidence: src/cli_args.rs#EvalArgs`
* `Evidence: src/instruction_runtime.rs#resolve_instruction_messages`
* `Evidence: src/runtime_paths.rs#resolved_hooks_config_path`
* `Evidence: src/runtime_paths.rs#resolved_mcp_config_path`
* `Evidence: src/trust/policy.rs#Policy::from_path`
* `Evidence: src/reliability_profile.rs#apply_builtin_profile_to_run_args`

## Config Precedence

LocalAgent resolves runtime behavior from a combination of:

1. CLI flags
2. reliability profile overlays where enabled
3. session settings where a run path adopts them
4. resolved config files for instructions, hooks, MCP, and policy

The exact precedence depends on the subsystem, so review the owning code before making claims about a specific flag/config interaction.

* `Evidence: src/runtime_flags.rs#apply_agent_mode_capability_baseline`
* `Evidence: src/session.rs#resolve_run_settings`
* `Evidence: src/runtime_paths.rs#resolved_hooks_config_path`
* `Evidence: src/runtime_paths.rs#resolved_mcp_config_path`

## Path Notes

Resolved config paths are managed through runtime path helpers.

- instructions config: runtime instruction resolution
- hooks config: `resolved_hooks_config_path`
- MCP config: `resolved_mcp_config_path`
- state and artifact roots: `resolve_state_paths`

* `Evidence: src/instruction_runtime.rs#resolve_instruction_messages`
* `Evidence: src/runtime_paths.rs#resolved_hooks_config_path`
* `Evidence: src/runtime_paths.rs#resolved_mcp_config_path`
* `Evidence: src/store.rs#resolve_state_paths`

## Policy, Approvals, and Audit Notes

Policy, approvals, and audit paths all derive from the resolved state layout unless overridden.

- policy file supports includes and evaluation logic in `src/trust/policy.rs`
- approvals persistence is managed through `src/trust/approvals.rs`
- audit log appends are managed through `src/trust/audit.rs`

* `Evidence: src/trust/policy.rs#Policy::from_path`
* `Evidence: src/trust/policy.rs#Policy::evaluate`
* `Evidence: src/trust/approvals.rs#ApprovalsStore`
* `Evidence: src/trust/audit.rs#AuditLog`

## State-dir Behavior

`--state-dir` changes where LocalAgent stores runtime state and artifacts. Isolating `--state-dir` is recommended for repeatable testing and incident reproduction.

`--workdir` controls tool/workspace scope. For clean repros, isolate both:
- `--workdir` to isolate files and relative-path context
- `--state-dir` to isolate prior runs, approvals, sessions, and artifacts

* `Evidence: src/store.rs#resolve_state_paths`
* `Evidence: src/target.rs#resolve_path_scoped`

## Unknowns

- No separate global config file outside the resolved state/config path model was confirmed in this crate.
- Confirm by searching for additional config loaders beyond runtime paths, instructions, hooks, MCP, policy, session, and reliability profile code.

## Related Docs

- Operations: [../operations/OPERATIONAL_RUNBOOK.md](../operations/OPERATIONAL_RUNBOOK.md)
- CLI reference: [CLI_REFERENCE.md](CLI_REFERENCE.md)
- File and symbol index: [FILE_AND_SYMBOL_INDEX.md](FILE_AND_SYMBOL_INDEX.md)
