# OpenAgent

Local-runtime agent loop with tool calling, focused on safe defaults and pragmatic control.

## What It Does

- Runs an agent loop against:
  - LM Studio (OpenAI-compatible API)
  - llama.cpp server (OpenAI-compatible API)
  - Ollama
- Supports built-in tools:
  - `list_dir`
  - `read_file`
  - `shell` (gated)
  - `write_file` / `apply_patch` (optional + gated)
- Supports Trust-lite:
  - policy allow/deny/require_approval
  - approvals store + CLI
  - audit log
- Persists:
  - sessions
  - run artifacts
  - replayable runs

## Safety Defaults

Defaults are intentionally safe:

- `--trust off`
- `--enable-write-tools` is off
- `--allow-write` is off
- `--allow-shell` is off
- read/shell output truncation limits are on

## Build

```bash
cargo build
```

## Basic Usage

```bash
openagent --provider ollama --model llama3.2 --prompt "Summarize src/main.rs"
```

Provider defaults:

- `lmstudio`: `http://localhost:1234/v1`
- `llamacpp`: `http://localhost:8080/v1`
- `ollama`: `http://localhost:11434`

## Doctor

Check provider connectivity:

```bash
openagent doctor --provider ollama
openagent doctor --provider lmstudio --base-url http://localhost:1234/v1
```

## Tool Gating

Enable shell execution:

```bash
openagent ... --allow-shell
```

Expose and allow write tools:

```bash
openagent ... --enable-write-tools --allow-write
```

## Trust-Lite

Trust modes:

- `--trust off`
- `--trust auto`
- `--trust on`

Approval behavior:

- `--approval-mode interrupt` (default)
- `--approval-mode fail` (CI-friendly fail-fast)
- `--approval-mode auto`

Auto-approve scope:

- `--auto-approve-scope run`
- `--auto-approve-scope session`

Recommended non-interrupting safe flow:

```bash
openagent ... --trust on --approval-mode auto --auto-approve-scope run
```

## Approvals Commands

```bash
openagent approvals list
openagent approvals prune
openagent approve <id> [--ttl-hours 24] [--max-uses 10]
openagent deny <id>
```

## State Directory

Default state dir: `<workdir>/.openagent`

Files:

- `policy.yaml`
- `approvals.json`
- `audit.jsonl`
- `runs/<run_id>.json`
- `sessions/<name>.json`

Compatibility:

- If `.openagent` does not exist but `.agentloop` does, OpenAgent uses `.agentloop` and prints a warning.
- Override explicitly with:

```bash
openagent ... --state-dir /path/to/state
```

## Sessions

```bash
openagent ... --session default
openagent ... --reset-session
openagent ... --no-session
openagent ... --max-session-messages 40
```

## Runs and Replay

Each run is written to:

- `<state_dir>/runs/<run_id>.json`

Replay:

```bash
openagent replay <run_id>
```

Replay header includes:

- run id, provider, model, exit reason
- policy hash
- config hash
- approval/unsafe mode fields

## VM / Lab Mode

Disable output limits (unsafe):

```bash
openagent ... --unsafe --no-limits
```

Optionally bypass shell/write allow flags (still does not auto-expose write tools):

```bash
openagent ... --unsafe --unsafe-bypass-allow-flags
```

## Help

```bash
openagent --help
openagent approvals --help
```
