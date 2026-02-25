# LocalAgent Install

## From Source

```bash
cargo install --path . --force
```

State/config bootstrap:

- Automatic: first `localagent` command in a project auto-creates `.localagent/` if missing.
- Explicit: run `init` yourself when you want scaffolding created immediately.

```bash
localagent init
```

Primary command is `localagent`.

## Updating / Reinstalling (Important)

When upgrading between versions, re-install the binary from the repo root:

```bash
cargo install --path . --force
```

Windows note:

- If you see a `failed to move ... localagent.exe` error, the old binary is usually still running.
- Close any `localagent` TUI/chat sessions and terminals using it, then re-run the install command.

You can verify which binary is being used:

```powershell
Get-Command localagent
localagent version
```

## From GitHub Releases

1. Download the archive for your OS from the Releases page.
2. Extract the binary and place it on your `PATH`.
3. Run:

```bash
localagent version
localagent init
```

## Verify

```bash
localagent --help
localagent doctor --provider ollama
localagent --provider ollama --model llama3.2 --prompt "hello" run
```

## Command Pattern

Global flags come before subcommands:

```bash
localagent --provider lmstudio --model essentialai/rnj-1 --prompt "hello" run
localagent --provider lmstudio --model essentialai/rnj-1 chat --tui
```
