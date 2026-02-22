# Safe Tool Tuning Profile (V1)

This guide gives you a concrete, low-risk baseline for tuning tool usage with:

- `instructions.yaml`
- `policy.yaml`
- `hooks.yaml`
- session settings

It also includes a simple scorecard so you can measure whether each change helps.

## Goal

Improve tool usage quality by:

1. selecting the right tool,
2. using valid arguments,
3. reducing unnecessary tool calls,
4. preventing unsafe behavior.

## 1) Instructions Profile (`.localagent/instructions.yaml`)

Use this as your starting file:

```yaml
version: 1
base:
  - role: system
    content: "Be concise, deterministic, and tool-efficient."
  - role: developer
    content: "When files are involved, inspect before changing."

model_profiles:
  - name: qwen_tooling_v1
    selector: "qwen*"
    messages:
      - role: developer
        content: "Preferred workflow: list_dir -> read_file -> apply_patch -> optional verify."
      - role: developer
        content: "Do not call shell unless a required check cannot be done with built-in tools."

task_profiles:
  - name: coding_safe_v1
    selector: "coding"
    messages:
      - role: developer
        content: "For small edits, use apply_patch. Avoid write_file rewrites when possible."
      - role: developer
        content: "Keep edits scoped to requested files only."
```

Recommended run flags:

```powershell
localagent --instruction-task-profile coding_safe_v1 --instruction-model-profile qwen_tooling_v1 ...
```

## 2) Policy Profile (`.localagent/policy.yaml`)

Use this strict baseline:

```yaml
version: 2
default: deny
rules:
  - tool: "list_dir"
    decision: allow
  - tool: "read_file"
    decision: allow
  - tool: "shell"
    decision: require_approval
  - tool: "write_file"
    decision: require_approval
  - tool: "apply_patch"
    decision: require_approval
```

Optional MCP allowlist (only if needed):

```yaml
mcp:
  allow_servers: ["playwright"]
  allow_tools: ["mcp.playwright.*"]
```

## 3) Hooks Profile (`.localagent/hooks.yaml`)

Start with one post-tool sanitizer hook:

```yaml
version: 1
hooks:
  - name: redact_tool_noise_v1
    stages: ["tool_result"]
    command: "python"
    args: ["scripts/redact_tool_result.py"]
    timeout_ms: 2000
    match:
      tools: ["shell", "read_file"]
```

Hook intent:

- remove obvious secret-looking lines/tokens,
- strip binary-like garbage/noisy placeholders,
- keep deterministic output shape.

If you do not have a hook script yet, run with hooks off first.

## 4) Session Settings Profile (`.localagent/sessions/<name>.json`)

Use stable defaults:

```json
{
  "schema_version": "openagent.session.v2",
  "name": "tool_tuning_v1",
  "updated_at": "2026-02-22T00:00:00Z",
  "messages": [],
  "settings": {
    "compaction": {
      "max_context_chars": 0,
      "mode": "off",
      "keep_last": 20,
      "tool_result_persist": "digest"
    },
    "tool_args_strict": "on",
    "caps_mode": "off",
    "hooks_mode": "off"
  },
  "task_memory": []
}
```

Later, if context gets too long, set:

- `compaction.mode = "summary"`
- `max_context_chars = 16000` (or your preferred budget)

## 5) Recommended Runtime Flags

Baseline safe run:

```powershell
localagent `
  --provider ollama `
  --model qwen3:4b `
  --trust on `
  --session tool_tuning_v1 `
  --use-session-settings `
  --instruction-model-profile qwen_tooling_v1 `
  --instruction-task-profile coding_safe_v1 `
  run --prompt "..."
```

For tasks requiring edits/shell checks:

```powershell
localagent `
  --provider ollama `
  --model qwen3:4b `
  --trust on `
  --enable-write-tools `
  --allow-write `
  --allow-shell-in-workdir `
  --session tool_tuning_v1 `
  --use-session-settings `
  run --prompt "..."
```

## 6) Evaluation Scorecard (Use This Every Iteration)

Use a fixed set of 10-20 prompts. For each run, record:

| Metric | Definition | Target |
|---|---|---|
| Tool Selection Accuracy | Correct tool family chosen for the task | >= 90% |
| First-Call Success | First attempt uses valid args and succeeds | >= 85% |
| Unnecessary Calls | Calls not needed for task completion | <= 10% |
| Unsafe Attempt Rate | Attempts blocked by policy/scope errors | trend down over time |
| Edit Precision | Only requested files/lines changed | >= 90% |
| Completion Quality | Task solved as requested | >= 85% |

Simple per-task record template:

```markdown
Task ID:
Expected tools:
Actual tools:
First-call success: yes/no
Unsafe/denied attempts:
Output quality (0-2):
Notes:
```

## 7) Tuning Loop

1. Run baseline on fixed tasks.
2. Change exactly one thing (instructions or policy or hooks or session setting).
3. Re-run the same tasks.
4. Keep the change only if scorecard improves.
5. Repeat.

Avoid changing multiple layers at once; you will not know what helped.

## 8) Practical Change Order

1. Instructions (cheapest, fastest gain)
2. Policy constraints (safety hardening)
3. Hooks (noise cleanup)
4. Session compaction settings (stability on longer runs)

This order gives the most signal with the least risk.
