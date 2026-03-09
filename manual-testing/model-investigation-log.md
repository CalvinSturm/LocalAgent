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

---

### 2026-03-09 - `qwen/qwen3.5-9b` - `qualification divergence`
- Commit baseline:
  - `e1cc450` Add OpenAI-compatible streaming trace artifact
  - `bbac69a` Add qualification trace artifacts
  - `a27f4cf` Accept textual fallback in qualification probe
  - `34a2422` Make qualification success sticky within a session
- Provider:
  - LM Studio via OpenAI-compatible path
- Mode:
  - stream vs non-stream qualification comparison
- Prompt/task:
  - orchestrator qualification probe for native `list_dir({"path":"."})`
- Outcome:
  - streamed qualification passed
  - non-stream qualification initially failed even though the model could later emit native write tools in the real task
- First exact divergence:
  - stream-on qualification returned a native tool call; stream-off qualification returned textual `name=list_dir` / `arguments={"path":"."}` with no native `tool_calls[]`
- Classification:
  - compatibility gap
- Decision:
  - fixed
- Evidence:
  - qualification traces:
    - stream-on: [.tmp/qualification-traces/stream-on/2026-03-09T06-41-39_2550854Z-lmstudio-qwen-qwen3-5-9b-stream-on-http---localhost-1234-v1-b45297807324](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/qualification-traces/stream-on/2026-03-09T06-41-39_2550854Z-lmstudio-qwen-qwen3-5-9b-stream-on-http---localhost-1234-v1-b45297807324)
    - stream-off: [.tmp/qualification-traces/stream-off/2026-03-09T06-42-31_6175761Z-lmstudio-qwen-qwen3-5-9b-stream-off-http---localhost-1234-v1-b45297807324](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/qualification-traces/stream-off/2026-03-09T06-42-31_6175761Z-lmstudio-qwen-qwen3-5-9b-stream-off-http---localhost-1234-v1-b45297807324)
  - cache records:
    - stream-on: [orchestrator_qualification_cache.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/repro-state/stream-on/orchestrator_qualification_cache.json)
    - stream-off: [orchestrator_qualification_cache.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/repro-state/stream-off/orchestrator_qualification_cache.json)
- Notes:
  - Qualification was too brittle for local-model compatibility.
  - The fix was intentionally qualification-only: accept a tightly scoped textual fallback and make success sticky within a qualification session.

---

### 2026-03-09 - `qwen/qwen3.5-9b` - `T1 viability milestone`
- Commit baseline:
  - `a27f4cf` Accept textual fallback in qualification probe
  - `34a2422` Make qualification success sticky within a session
- Provider:
  - LM Studio via OpenAI-compatible path
- Mode:
  - stream and non-stream
- Prompt/task:
  - T1 create-file task from the T pack
- Outcome:
  - after the qualification fixes, T1 succeeded in both modes
- First exact divergence:
  - no remaining qualification-driven divergence on T1 after sticky success landed
- Classification:
  - fixed
- Decision:
  - fixed
- Evidence:
  - stream-on success:
    - qualification cache: [orchestrator_qualification_cache.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/repro-state/qual-fix-stream-on/orchestrator_qualification_cache.json)
    - verdict: [verdict.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/qualification-traces/qual-fix-stream-on/2026-03-09T06-51-22_3477286Z-lmstudio-qwen-qwen3-5-9b-stream-on-http---localhost-1234-v1-b45297807324/verdict.json)
  - stream-off success:
    - qualification cache: [orchestrator_qualification_cache.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/repro-state/qual-fix-stream-off/orchestrator_qualification_cache.json)
    - verdict: [verdict.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/qualification-traces/qual-fix-stream-off/2026-03-09T06-51-22_3582452Z-lmstudio-qwen-qwen3-5-9b-stream-off-http---localhost-1234-v1-b45297807324/verdict.json)
- Notes:
  - This was the point where the broad “LocalAgent cuts off” narrative became too vague.
  - The narrower conclusion became: basic tool execution was viable once qualification stopped stripping write tools.

---

### 2026-03-09 - `qwen/qwen3.5-9b` - `post-write finalization gap`
- Commit baseline:
  - `34a2422` Make qualification success sticky within a session
  - `b17bbb2` Add bounded post-write follow-on turn
- Provider:
  - LM Studio via OpenAI-compatible path
- Mode:
  - observed in both stream and non-stream on contract-complete edit tasks
- Prompt/task:
  - Tool A and Tool B tasks from the T pack requiring verification/test plus exact final answer
- Outcome:
  - before `b17bbb2`, LocalAgent often terminalized immediately after successful verified write, without the requested follow-on verification/test/final response
  - after `b17bbb2`, Tool A passed in both modes and Tool B passed in streamed mode
- First exact divergence:
  - the runtime finalized at the verified-write seam instead of requesting one bounded follow-on turn when the prompt explicitly still required more work
- Classification:
  - runtime bug
- Decision:
  - fixed
- Evidence:
  - pre-fix runs:
    - Tool A stream-on: [d97c406f-0b91-4da5-b9e2-493913b49907.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/repro-state/toolA-on-clean/runs/d97c406f-0b91-4da5-b9e2-493913b49907.json)
    - Tool A stream-off: [f3acad97-4bd4-4fb1-8929-a20b696fc6a9.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/repro-state/toolA-off-clean/runs/f3acad97-4bd4-4fb1-8929-a20b696fc6a9.json)
    - Tool B stream-on: [2e02c615-1694-413f-81ef-b64017e763cb.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/repro-state/toolB-on-rerun/runs/2e02c615-1694-413f-81ef-b64017e763cb.json)
    - Tool B stream-off: [5753ef6a-10ff-434f-91d0-4deb91f609e5.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/repro-state/toolB-off/runs/5753ef6a-10ff-434f-91d0-4deb91f609e5.json)
  - post-fix runs:
    - Tool A stream-on: [5f513a0f-427d-40f4-801a-ef0eb006b502.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/repro-state/toolA-on-rerun/runs/5f513a0f-427d-40f4-801a-ef0eb006b502.json)
    - Tool A stream-off: [f9090bfb-df6e-444e-90d3-cec33607c9e0.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/repro-state/toolA-off-rerun/runs/f9090bfb-df6e-444e-90d3-cec33607c9e0.json)
    - Tool B stream-on: [843ab1a7-eee3-441c-a229-16ac172256dc.json](/C:/Users/Calvin/Software%20Projects/LocalAgent/.tmp/repro-state/toolB-on-rerun2/runs/843ab1a7-eee3-441c-a229-16ac172256dc.json)
- Notes:
  - The fix was intentionally narrow: one bounded post-write follow-on turn only when the task contract explicitly required more work.
  - This investigation closed the earlier “post-write terminalization is too eager” bug without reopening general runtime-loop semantics.
