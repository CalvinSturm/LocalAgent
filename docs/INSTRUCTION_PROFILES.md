# Instruction Profiles (Per-Model Tuning)

LocalAgent supports per-model and per-task prompt tuning through an `instructions.yaml` file.

This is the recommended way to improve reliability for individual local models (especially smaller models) without changing code.

## Where the File Lives

Default path:

```text
.openagent/instructions.yaml
```

You can also pass a custom path:

```bash
localagent --instructions-config /path/to/instructions.yaml ...
```

## What the File Does

`instructions.yaml` has three main sections:

- `base`: messages always included
- `model_profiles`: messages added when a model name matches a selector
- `task_profiles`: messages added when a task kind matches a selector

Typical uses:

- improve tool-call formatting for a specific model
- reduce verbosity for a weak local model
- enforce deterministic response structure
- customize behavior for coding/summarize/browser tasks

## File Structure

```yaml
version: 1
base:
  - role: system
    content: "Base instruction for all runs."

model_profiles:
  - name: my_model_profile
    selector: "qwen3:*"
    messages:
      - role: developer
        content: "Tool-use guidance for this model family."

task_profiles:
  - name: coding
    selector: "coding"
    messages:
      - role: developer
        content: "Coding-specific output and verification rules."
```

## Selectors (How Matching Works)

Selectors are simple wildcard patterns.

- Exact match example: `"essentialai/rnj-1"`
- Family match example: `"qwen3:*"`
- Catch-all example: `"*"`

Practical pattern:

1. Start with a family selector (`"qwen3:*"`)
2. If one model behaves differently, add a more specific profile
3. Keep a catch-all profile for baseline tool discipline

## How To Add a New Model Profile

1. Open `.openagent/instructions.yaml`
2. Copy an existing `model_profiles` entry
3. Change `name` and `selector`
4. Add 1-3 short `developer` messages (start small)
5. Test the same task repeatedly
6. Keep only instructions that improve consistency

Example (small local model tool-call stability):

```yaml
model_profiles:
  - name: deepseek_r1_8b_tool_calling_v1
    selector: "deepseek-r1:8b*"
    messages:
      - role: developer
        content: "When using tools, emit exactly one valid tool call at a time."
      - role: developer
        content: "Use the tool JSON schema exactly. Do not wrap tool calls in markdown."
      - role: developer
        content: "If required tool arguments are missing, ask one short clarification question."
```

Example (Qwen family discipline):

```yaml
model_profiles:
  - name: qwen3_tool_discipline_v1
    selector: "qwen3:*"
    messages:
      - role: developer
        content: "Use tools before factual claims about local files or command outcomes."
      - role: developer
        content: "Keep responses concise. Do not output hidden reasoning tags."
```

## Task Profiles (Recommended)

Task profiles help when one model is fine in general but weak on a specific workflow.

Examples:

- `coding`: require minimal diffs and verification
- `summarize`: enforce evidence-first summaries
- `browser`: require browser MCP usage and unsafe-page prompt resistance

## How To Use Profiles at Runtime

Use these flags:

- `--instructions-config <PATH>`
- `--instruction-model-profile <NAME>`
- `--instruction-task-profile <NAME>`
- `--task-kind <NAME>`

Example:

```bash
localagent \
  --provider ollama \
  --model qwen3:8b \
  --instructions-config .openagent/instructions.yaml \
  --instruction-model-profile qwen3_tool_discipline_v1 \
  --instruction-task-profile coding \
  --task-kind coding \
  chat --tui
```

## How Users Should Add Learnings Over Time

Recommended workflow:

1. Run a task and note failure mode(s)
2. Add one small instruction change for that model
3. Re-run the same task
4. Compare behavior
5. Keep changes that improve reliability
6. Version your profile names (`*_v1`, `*_v2`)

Good things to encode:

- tool-call formatting rules
- "ask before guessing" behavior
- output structure / concision
- explicit tool usage expectations
- safety boundaries (no shell/write unless needed)

Avoid:

- very large prompt blocks
- overlapping/conflicting instructions across `base` and model profiles
- trying to force one prompt style across every model

## Context Length and Local Performance Notes

- Each model has a max context window, but practical context depends on VRAM/RAM and runtime settings.
- More context increases memory pressure (KV cache) and can reduce stability/performance.
- For many local agents, stronger tool discipline beats simply increasing context length.
- Inference is typically GPU-heavy; CPU-only runs are often too slow for interactive agent workflows.

