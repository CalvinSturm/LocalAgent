# LocalAgent

LocalAgent is a local-first agent runtime for connecting on-machine LLMs to MCP tools with explicit safety controls, replayable artifacts, and a guided path to first success.

It is built for the hard part of local agents: getting from curiosity to a working workflow without fighting provider setup, unsafe defaults, or opaque failures.

Use it to inspect a repo, summarize files, call MCP tools with approvals, and review the resulting artifacts locally.

<img width="1093" height="581" alt="LocalAgent chat TUI showing Code mode, connected LM Studio provider, command hints, and cwd footer (C:\demo)." src="https://github.com/user-attachments/assets/1b2c6f7e-9869-46bc-8ec8-24b70ae23268" />

### Why use LocalAgent

* Get a real local agent workflow running without fighting provider setup, unsafe defaults, or unclear tool permissions.
* Connect local models to MCP tools with explicit trust controls, audit trails, and replayable run artifacts.
* Learn and iterate faster with guided startup, provider auto-detection, evals, and inspectable runs.

## Why LocalAgent exists

Most friction in local agents is operational.

People are curious about local models, tool calling, MCP, coding workflows, and agent loops, but the path from interest to a successful run is still brittle. Provider setup is inconsistent. Tool permissions are easy to misconfigure. Trust boundaries are often unclear. When runs fail, it is often hard to tell what happened or why.

Smaller local models already operate under tighter reasoning and context budgets. Making them and their operators fight environment friction only reduces reliability further.

LocalAgent narrows that gap with a guided local-first runtime that keeps side effects explicit, runtime behavior visible, and runs inspectable, while still supporting serious MCP-based workflows.

## First success

Start a supported local provider first, then run LocalAgent in the project directory you want to work in.

```bash
# 1) Install from the repo root
cargo install --path . --force

# 2) Launch LocalAgent in the workspace you want to work in
localagent
```

On first run, LocalAgent initializes `.localagent/` in the current directory if it does not already exist.

If your provider starts after LocalAgent is already open, press `R` in the startup screen to refresh provider detection.

### Supported providers

* Ollama
* LM Studio
* llama.cpp server

### Important CLI rule

Global flags come before subcommands.

```bash
localagent --provider ollama --model llama3.2 --prompt "Summarize src/main.rs" run
localagent --provider ollama --model llama3.2 chat --tui
```

## What you get

* Guided startup with provider auto-detection
* Interactive TUI chat for local agent workflows
* MCP stdio integration for custom tool workflows
* Safe defaults with shell and write access disabled unless explicitly enabled
* Explicit trust controls with policy, approvals, and audit trails
* Replayable artifacts and inspectable event logs
* Built-in eval workflows and reviewable run outputs
* A clear beginner path without hiding advanced controls

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

## What makes LocalAgent different

### Guided startup for local agents

LocalAgent is built around local providers and on-machine workflows, with provider detection and a startup flow designed to get users to a successful run faster.

### Explicit trust boundaries

Shell and write capabilities are not casually exposed. You enable them intentionally, keep approvals visible, and make side effects a conscious choice.

### Inspectable, replayable runs

LocalAgent produces artifacts, logs, and traces so behavior can be reviewed, debugged, compared, and improved over time.

### MCP workflows without hiding the runtime

LocalAgent supports serious MCP-based workflows while keeping provider state, tool permissions, and trust controls visible to the operator.

### Reproducibility as a runtime feature

Policies, approvals, evals, and artifacts are part of the runtime itself, not bolted on later.

## Who it is for

### First-time local agent users

You want a safe, guided way to learn how local providers, tools, MCP, approvals, and runtime loops fit together.

### Builders

You want to prototype MCP-powered workflows on your own machine without starting in a large framework.

### Advanced users

You want explicit trust controls, replayable runs, evals, and operational clarity while iterating on serious agent workflows.

## Safety model

LocalAgent is designed to make side effects explicit.

* Shell and write access are disabled unless explicitly enabled
* Trust mode can enforce policy and approvals
* Runs remain inspectable through artifacts and logs

The goal is not to remove every restriction. It is to make local agents usable without hiding risk.

## Learn faster, not just run faster

LocalAgent is also meant to be a strong entry point for understanding how local agent systems actually work.

It helps users learn:

* agent runtime loops
* MCP integration
* trust and approvals
* tool-calling workflows
* local model operational limits
* how to move from a toy agent to a more reliable one

A lot of users quit before they reach the interesting part. LocalAgent is built to shorten that path.

## Provider prerequisites

Before running LocalAgent, start your provider and make sure a model is available.

### Ollama

* Start Ollama
* Ensure the model is present locally
* Default endpoint: `http://localhost:11434`

### LM Studio

* Start LM Studio
* Load a model
* Enable the OpenAI-compatible API
* Default endpoint: `http://localhost:1234/v1`

### llama.cpp

* Start `llama-server` with a loaded model
* Default endpoint: `http://localhost:8080/v1`

## Installation

### Build from source

```bash
cargo build --release
```

Binary output:

* Windows: `target/release/localagent.exe`
* Linux/macOS: `target/release/localagent`

### Install globally from source

```bash
cargo install --path . --force
```

### Releases

Prebuilt binaries are available in GitHub Releases.

For full install, updates, Windows troubleshooting, and verification steps, see:

* `docs/guides/INSTALL.md`

## Docs

Quick docs map:

* [Install guide](docs/guides/INSTALL.md)
* [Provider setup](docs/guides/LLM_SETUP.md)
* [Repo entry guide](AGENTS.md)
* [Runtime architecture](docs/architecture/RUNTIME_ARCHITECTURE.md)
* [Operational runbook](docs/operations/OPERATIONAL_RUNBOOK.md)
* [Configuration and state](docs/reference/CONFIGURATION_AND_STATE.md)
* [CLI reference](docs/reference/CLI_REFERENCE.md)
* [Runtime loop policy](docs/policy/AGENT_RUNTIME_PRINCIPLES_2026.md)
* [Runtime change review template](docs/policy/AGENT_RUNTIME_CHANGE_REVIEW_TEMPLATE.md)
* [Templates](docs/guides/TEMPLATES.md)
* [Changelog](CHANGELOG.md)

## Contributing

Issues, feedback, and contributions are welcome.

If you are interested in local-first agent runtimes, MCP workflows, trust controls, and reproducible agent systems, you are in the right repo.

## License

MIT
