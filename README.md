# LocalAgent

LocalAgent is a local-first agent runtime for connecting on-machine LLMs to MCP tools with explicit safety controls, replayable artifacts for persistent workflows, and a guided path to first success.

It is built for the hard part of local agents: getting from curiosity to a working workflow without fighting provider setup, unsafe defaults, or opaque failures.

Use it to inspect a repo, summarize files, call MCP tools with approvals, and review resulting artifacts locally when you use persistent state.

<img width="1093" height="581" alt="LocalAgent chat TUI showing Code mode, connected LM Studio provider, command hints, and cwd footer (C:\demo)." src="https://github.com/user-attachments/assets/1b2c6f7e-9869-46bc-8ec8-24b70ae23268" />

## Why LocalAgent

Most friction in local agents is operational.

People are curious about local models, tool calling, MCP, coding workflows, and agent loops, but the path from interest to a successful run is still brittle. Provider setup is inconsistent. Tool permissions are easy to misconfigure. Trust boundaries are often unclear. When runs fail, it is often hard to tell what happened or why.

LocalAgent narrows that gap with a guided local-first runtime that keeps side effects explicit, runtime behavior visible, and persistent runs inspectable, while still supporting serious MCP-based workflows.

As of `v0.5.0`, the core runtime is materially stronger for coding workflows: completion and validation behavior are more runtime-owned, one-shot runs default to isolated ephemeral state unless you opt into persistence, and the repo includes broader eval and local-model investigation surfaces for measuring coding-task reliability.

What you get:

- guided startup with provider auto-detection
- interactive TUI chat for local agent workflows
- MCP stdio integration for custom tool workflows
- stronger coding-task runtime contracts, validation handling, and recovery paths
- TypeScript/LSP-assisted coding support for richer code investigation workflows
- safe defaults with shell and write access disabled unless explicitly enabled
- explicit trust controls with policy, approvals, and audit trails
- replayable artifacts and inspectable event logs for persistent workflows
- built-in eval workflows, coding benchmarks, and reviewable run outputs
- a clear beginner path without hiding advanced controls

## First success

Start a supported local provider first, then run LocalAgent in the project directory you want to work in.

```bash
# 1) Install from the repo root
cargo install --path . --force

# 2) Launch LocalAgent in the workspace you want to work in
localagent
```

State behavior depends on the command path:

- bare startup and persistent project workflows use the resolved state dir, typically `.localagent/` under the workdir
- one-shot `run` / `exec` default to an ephemeral temp state dir whenever you do not set `--state-dir`
- one-shot `run` / `exec` also default to `--no-session` unless you pass session-related settings explicitly

If you want persistent artifacts for one-shot runs, pass `--state-dir <path>` explicitly.

If your provider starts after LocalAgent is already open, press `R` in the startup screen to refresh provider detection.

### Supported providers

- Ollama
- LM Studio
- llama.cpp server

### Important CLI rule

Global flags come before subcommands.

```bash
localagent --provider ollama --model llama3.2 --prompt "Summarize src/main.rs" run
localagent --provider ollama --model llama3.2 chat --tui
```

## Common paths

### One-shot task

```bash
localagent --provider ollama --model llama3.2 --prompt "Summarize src/main.rs" run
```

### Interactive TUI chat

```bash
localagent --provider ollama --model llama3.2 chat --tui
```

### Verify a provider

```bash
localagent doctor --provider ollama
localagent doctor --provider lmstudio
localagent doctor --provider llamacpp
```

### Enable trust controls

```bash
localagent --trust on --provider ollama --model llama3.2 chat --tui
```

Enable shell and write tools only when you intentionally want side effects.

## Safety model

LocalAgent is designed to make side effects explicit.

- shell and write access are disabled unless explicitly enabled
- `--allow-shell-in-workdir` is a narrower shell mode than `--allow-shell`: it allows shell only with a cwd that stays under the current workdir
- trust mode can enforce policy and approvals
- persistent runs remain inspectable through artifacts and logs; one-shot `run` / `exec` keep artifacts only when you pass `--state-dir`

The goal is not to remove every restriction. It is to make local agents usable without hiding risk.

## Who it is for

### First-time local agent users

You want a safe, guided way to learn how local providers, tools, MCP, approvals, and runtime loops fit together.

### Builders

You want to prototype MCP-powered workflows on your own machine without starting in a large framework.

### Advanced users

You want explicit trust controls, replayable runs, evals, and operational clarity while iterating on serious agent workflows.

## Provider setup

Before running LocalAgent, start your provider and make sure a model is available.

### Ollama

- Start Ollama
- Ensure the model is present locally
- Default endpoint: `http://localhost:11434`

### LM Studio

- Start LM Studio
- Load a model
- Enable the OpenAI-compatible API
- Default endpoint: `http://localhost:1234/v1`

### llama.cpp

- Start `llama-server` with a loaded model
- Default endpoint: `http://localhost:8080/v1`

## Installation

### Build from source

```bash
cargo build --release
```

Binary output:

- Windows: `target/release/localagent.exe`
- Linux/macOS: `target/release/localagent`

### Install globally from source

```bash
cargo install --path . --force
```

### Releases

Prebuilt binaries are available in GitHub Releases.

For full install, updates, Windows troubleshooting, and verification steps, see:

- [Install guide](docs/guides/INSTALL.md)

## Docs

### Getting started

- [Install guide](docs/guides/INSTALL.md)
- [Provider setup](docs/guides/LLM_SETUP.md)
- [Repo entry guide](AGENTS.md)

### Runtime internals

- [Runtime architecture](docs/architecture/RUNTIME_ARCHITECTURE.md)
- [Operational runbook](docs/operations/OPERATIONAL_RUNBOOK.md)
- [Configuration and state](docs/reference/CONFIGURATION_AND_STATE.md)
- [CLI reference](docs/reference/CLI_REFERENCE.md)

### Runtime policy

These are mainly for contributors changing shared runtime behavior.

- [Runtime loop policy](docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md)
- [Runtime change review template](docs/policy/AGENT_RUNTIME_CHANGE_REVIEW_TEMPLATE.md)

### Additional docs

- [Templates](docs/guides/TEMPLATES.md)
- [Release notes](docs/release-notes/README.md)
- [Changelog](CHANGELOG.md)

## Contributing

Issues, feedback, and contributions are welcome.

If you are interested in local-first agent runtimes, MCP workflows, trust controls, and reproducible agent systems, you are in the right repo.

Start here:

- [Contributing guide](CONTRIBUTING.md)

## License

MIT
