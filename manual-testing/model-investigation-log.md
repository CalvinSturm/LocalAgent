# Model Investigation Log

Use this file to record local-model investigations that materially affect LocalAgent runtime, provider, or compatibility decisions.

Keep entries append-only and lightweight.

## Entry Template

### YYYY-MM-DD - `<model>` - `<scenario>`
- Commit baseline:
- Provider:
- Mode:
- Prompt/task:
- Outcome:
- First exact divergence:
- Classification:
  - provider bug
  - runtime bug
  - compatibility gap
  - pure model-choice
- Decision:
  - fixed
  - accepted limitation
  - follow-up needed
- Evidence:
  - run record:
  - provider trace:
  - qualification trace:
  - external/control transcript:
- Notes:

---

### 2026-03-09 - `qwen/qwen3.5-9b` - `Tool B parser fix`
- Commit baseline:
  - `1daaddd` Improve TypeScript LSP diagnostics robustness
  - `e1cc450` Add OpenAI-compatible streaming trace artifact
  - `bbac69a` Add qualification trace artifacts
  - `a27f4cf` Accept textual fallback in qualification probe
  - `34a2422` Make qualification success sticky within a session
  - `b17bbb2` Add bounded post-write follow-on turn
- Provider:
  - LM Studio via OpenAI-compatible path
- Mode:
  - compared OpenCode control run vs LocalAgent stream and non-stream
- Prompt/task:
  - [PROMPT.txt](/C:/Users/Calvin/Software%20Projects/LocalAgent/manual-testing/T-tests/T3/PROMPT.txt)
- Outcome:
  - OpenCode succeeded
  - LocalAgent stream succeeded
  - LocalAgent non-stream failed with `TOOL_REPEAT_BLOCKED`
- First exact divergence:
  - inside LocalAgent, after equivalent failed `str_replace` recovery, streamed execution switched strategy to `apply_patch`, while non-stream execution continued repeating `str_replace` until the repeat guard terminated the run
- Classification:
  - pure model-choice
- Decision:
  - accepted limitation
- Evidence:
  - run record:
    - non-stream fail: [94995f06-bb71-4b90-88cc-ac0c04d72129.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/repro-state/toolB-off-tracefix/runs/94995f06-bb71-4b90-88cc-ac0c04d72129.json)
    - stream success: [843ab1a7-eee3-441c-a229-16ac172256dc.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/repro-state/toolB-on-rerun2/runs/843ab1a7-eee3-441c-a229-16ac172256dc.json)
  - provider trace:
    - non-stream: [.tmp/openai-traces/toolB-off-tracefix](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/openai-traces/toolB-off-tracefix)
    - stream: [.tmp/openai-traces/toolB-on-rerun2](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/openai-traces/toolB-on-rerun2)
  - qualification trace:
    - non-stream: [.tmp/qualification-traces/toolB-off-tracefix](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/qualification-traces/toolB-off-tracefix)
    - stream: [.tmp/qualification-traces/toolB-on-rerun2](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/qualification-traces/toolB-on-rerun2)
  - external/control transcript:
    - OpenCode: [opencode-run.jsonl](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/manual-testing/control/T-tests/20260308-195759-832-7fdd84/T3/opencode-run.jsonl)
    - OpenCode config: [opencode.jsonc](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/manual-testing/control/T-tests/20260308-195759-832-7fdd84/opencode.jsonc)
- Notes:
  - This is not explained by provider transport, response parsing, qualification, or post-write finalization.
  - LocalAgent already exposed the needed edit tools and surfaced the same `str_replace` failure plus recovery hint in both stream modes.
  - OpenCode succeeded earlier by using a different edit affordance (`edit`), but LocalAgent stream proves no LocalAgent patch is justified from current evidence.
