# Operational Runbook

Status: Active  
Owner: LocalAgent maintainers  
Last reviewed: 2026-03-07

This runbook covers operational debugging, reproducibility, incident handling, and day-to-day operator workflows.

## Observability and Debugging

Primary debugging surfaces:
- Event stream to stdout (`--output json`) and/or file (`--events <path>`).
- Run artifact JSON with transcript, tool decisions, and config fingerprints.
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

### Repro steps
1. Run with explicit provider, model, base URL, and `--events` path.
2. Enable trust mode intentionally (`--trust on`) if validating policy or approval behavior.
3. Enable repro snapshots (`--repro on --repro-env safe`), or `all` only when needed.

### State locations
- `.localagent/runs` for per-run records.
- `.localagent/audit.jsonl` for trust audit.
- `.localagent/approvals.json` for approval lifecycle.
- `.localagent/tasks/checkpoint.json` and `.localagent/tasks/runs/*.json` for taskgraph state.

### Safe cleanup
- Delete specific run artifacts, checkpoints, or sessions as needed.
- Avoid deleting policy or approvals blindly during incidents unless you intend to reset trust state.

### Performance hotspots
- Large TUI loop, event handling, and rendering paths.
- Repo-map scan bounded by file and byte caps but potentially expensive near caps.
- Eval matrix loops (`models x tasks x runs`) and verifier subprocesses.

### Safety and security operational notes
- Path traversal is guarded for tools and targets (`no absolute`, no `..`).
- Shell and write tools are blocked unless allow flags or unsafe bypass enable them.
- Docker target validates daemon/image and constrains workdir mounts.
- Hooks and MCP spawn subprocesses; hook output sizes and timeouts are bounded.

### Unknowns
- No explicit secret-redaction pipeline beyond hook-based customization and repro-env safeguards was confirmed here. Audit hook defaults and repro env filtering before making stronger claims.

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

## Operator Command Baseline

Use explicit flags for reproducible investigation:

```bash
localagent --provider <provider> --model <model> --workdir <isolated-workdir> --state-dir <isolated-state-dir> --no-session --events run-events.jsonl --prompt "..." run
```

For interactive investigation:

```bash
localagent --provider <provider> --model <model> --workdir <isolated-workdir> --state-dir <isolated-state-dir> --no-session chat --tui
```

## Related Docs

- Architecture map: [../architecture/RUNTIME_ARCHITECTURE.md](../architecture/RUNTIME_ARCHITECTURE.md)
- Configuration and state: [../reference/CONFIGURATION_AND_STATE.md](../reference/CONFIGURATION_AND_STATE.md)
- CLI reference: [../reference/CLI_REFERENCE.md](../reference/CLI_REFERENCE.md)
- Runtime policy: [../policy/AGENT_RUNTIME_PRINCIPLES_2026.md](../policy/AGENT_RUNTIME_PRINCIPLES_2026.md)
