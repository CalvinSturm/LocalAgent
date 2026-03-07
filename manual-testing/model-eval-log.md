# Model Eval Log

## Purpose

Manual log for local coding-agent model checks against LocalAgent runtime behavior.

Current baseline settings:

- Provider: `lmstudio`
- Task shape: manual `C1`-style run
- Prompt: `First use read_file on src/hello.txt. If it does not exist, then create src/hello.txt containing exactly hello followed by a newline using write_file. Then verify it with read_file and reply with exactly done: src/hello.txt`
- Max concurrent predictions: `1`
- Context length: `8192`
- Temperature: `0.0`
- Evaluation batch size: `256`
- Seed: fixed
- Top K: conservative
- Top P: `1.0`
- Repeat penalty: `1.0`
- Min P: disabled
- Structured output: off

## Runtime caveat

`C1` is currently confounded by an implementation-integrity guard around file creation. Models that successfully do `read_file` on a missing file and then `write_file` the new file can still fail with:

- `implementation guard: write_file on 'src/hello.txt' requires prior read_file on the same path`

Interpret `C1` results accordingly.

## Investigation Notes

- Check runtime logic that appears to enforce `read_file` before `write_file`.
- Current hypothesis: the guard may not be treating a failed `read_file` on a missing target as sufficient prior-read evidence for a subsequent create-new-file write.
- Why this matters: several models have shown the same valid read-missing-then-create pattern and still fail `C1`, which makes the task less useful as a pure model-selection signal until this behavior is understood.

## Run Log

### qwen2.5-coder-14b

- Workdir/state: `.tmp/eval-c1`
- Artifacts:
- `d29949c2-2d0c-4f05-b5fc-a0735e005278.json`
- `7e45e6b0-9571-4966-801d-b453bb37589f.json`
- `330cea12-55fc-4dbd-8c01-c409679d0be8.json`
- Summary: capable of entering the real tool loop; inconsistent completion.
- Observed behavior:
- valid tool calls were produced
- one run created `src/hello.txt`
- one run kept reading existing state and failed the effective-write guard
- one run read missing file and never completed the write path
- Current interpretation: promising for runtime iteration, but not dependable on this task yet

### starcoder2-7b

- Workdir/state: `.tmp/eval-c1`
- Artifact:
- `d4aa8b7c-7ef3-4712-a8d8-a33863b2d9f1.json`
- Summary: failed earlier than the others at tool qualification.
- Observed behavior:
- orchestrator qualification failed
- runtime downgraded to read-only fallback
- no effective write
- Current interpretation: below the bar for LocalAgent coding-agent tool use

### qwen3-4b-instruct-2507-ud

- Workdir/state: `.tmp/eval-c1`
- Artifact:
- `4418d799-7053-4aa3-88a5-9de6f994dff8.json`
- Summary: clean read-then-create behavior, but still blocked by the guard.
- Observed behavior:
- attempted `read_file` on missing `src/hello.txt`
- called `write_file` to create `hello\n`
- run still failed with the implementation guard
- Current interpretation: better interop than most small models tested so far

### qwen2.5-coder-7b-instruct@q8_0

- Workdir/state: `.tmp/eval-c1`
- Artifact:
- `6ace0c92-80cd-4443-a01c-4080b18838e4.json`
- Summary: contaminated run due to existing `src/hello.txt`.
- Observed behavior:
- read existing file
- produced expected reply text
- failed effective-write guard because no new write occurred
- Current interpretation: inconclusive because the workdir was not clean

- Workdir/state: `.tmp/eval-c1-qwen25coder7bq8`
- Artifact:
- `bc77afff-c178-4db9-ab22-d9cef074c89c.json`
- Summary: fresh rerun matched the Qwen3 failure shape.
- Observed behavior:
- `read_file` on missing `src/hello.txt`
- `write_file` created `hello\n`
- run still failed with `implementation guard: write_file on 'src/hello.txt' requires prior read_file on the same path`
- Current interpretation: runtime-compatible enough to test against, but `C1` remains confounded

### deepseek-coder-v2-lite-instruct

- Workdir/state: `.tmp/eval-c1-qwen25coder7bq8`
- Artifact:
- `698a5e64-b317-4a9d-b49d-19f4dd04bdd7.json`
- Summary: failed native tool-call interop.
- Observed behavior:
- emitted inline fake tool-call markup in plain text
- LocalAgent recorded `tool_calls_total = 0`
- failed with `implementation guard: file-edit task finalized without any tool calls`
- Current interpretation: likely provider/template interop problem for this runtime path, not just raw model capability

### essentialai/rnj-1

- Workdir/state: `.tmp/eval-c1-qwen25coder7bq8`
- Artifact:
- `37e7b912-f1dc-425a-8cca-cdca7a0012c7.json`
- Summary: same useful failure shape as the better small-model candidates.
- Observed behavior:
- `read_file` on missing `src/hello.txt`
- `write_file` created `hello\n`
- `tool_calls_total = 2`
- `malformed_tool_call_count = 0`
- run still failed with `implementation guard: write_file on 'src/hello.txt' requires prior read_file on the same path`
- Current interpretation: runtime-compatible enough to test against; blocked by the same `C1` guard rather than by tool-protocol incapability

## Consistency Notes

- `qwen3-4b-instruct-2507-ud` and `qwen2.5-coder-7b-instruct@q8_0` both showed the same useful pattern:
- they entered the real tool loop
- they attempted the expected read-then-create flow
- they were blocked by the same runtime guard

- `essentialai/rnj-1` now joins that same pattern:
- real native tool calls
- successful file creation
- same implementation-guard failure after the write

- `deepseek-coder-v2-lite-instruct` showed a distinct incompatibility pattern:
- plain-text pseudo tool calls
- zero runtime-recognized tool calls

- `starcoder2-7b` showed a distinct qualification failure pattern:
- failed probe
- downgraded to read-only fallback

## Full Eval Runs (C1–C5)

### 2026-03-07 — qwen2.5-coder-14b — str_replace tool added

Three consecutive `eval --pack coding` runs after adding `str_replace` tool and updating eval assertions to accept `{apply_patch,str_replace}`.

Settings: `--provider lmstudio --enable-write-tools --allow-write --allow-shell --timeout-seconds 300 --max-steps 30`

#### Consistent patterns (3/3 runs)

| Task | Exit Reason | Tool Sequence | Notes |
|------|-------------|---------------|-------|
| C1 | planner_error | `write_file` | Impl guard: `write_file` without prior `read_file`. Prompt says "Create" but `prompt_allows_new_file_without_read` patterns don't match. |
| C2 | ok | `read_file → apply_patch` | Tools work correctly every run. Model doesn't emit exact output phrase `"patched answer()"`. |

#### Variable patterns

| Task | Run 1 | Run 2 | Run 3 |
|------|-------|-------|-------|
| C3 | denied (2 steps, `read_file → str_replace`) | denied (2 steps) | timeout (0 steps) |
| C4 | denied (2 steps, `grep → str_replace`) | denied (3 steps, `grep → read_file → str_replace`) | provider_error |
| C5 | denied (3 steps, `shell → read_file → str_replace`) | denied (3 steps) | denied (3 steps) |

#### Key observations

1. **Model naturally prefers `str_replace` over `apply_patch`** for C3/C4/C5 editing tasks — validates the tool addition.
2. **C1 is a known guard bug**, not a model issue — `prompt_allows_new_file_without_read` pattern too narrow for C1's prompt phrasing.
3. **C2 tools work perfectly** — failure is only the exact output string match, not the edit itself.
4. **C3/C4/C5 `denied` exits**: model calls `str_replace` but gets blocked. `bytes_written: 0` in all cases. Root cause unclear — could be impl guard (post-write verification missing), trust gate, or `str_replace` execution failure. Needs investigation with bundle/session logs.
5. **C4**: model uses `grep` to find the file instead of `read_file`, then the impl guard blocks `str_replace` because no prior `read_file` on that path. This is a valid guard enforcement — model should `read_file` before editing.
6. **Glob assertion fix**: `matches_pattern` was not recognizing `{a,b}` brace expansion — fixed by adding `{` to the glob-trigger check.

#### Run 4 — after gate/trust fix for str_replace

`str_replace` was missing from `gate.rs` hard gate and `trust/policy.rs` default rules, causing all `str_replace` calls to exit `denied`. After adding `str_replace` alongside `apply_patch` in both:

| Task | Exit | Steps | Tool Sequence | bytes_written | Notes |
|------|------|-------|---------------|---------------|-------|
| C1 | planner_error | 1 | `write_file` | 6 | Same guard issue |
| C2 | ok | 4 | `read_file → apply_patch ×3` | 60 | Tools work, output miss |
| C3 | ok | 3 | `read_file → read_file → str_replace` | 419 | str_replace succeeded! Didn't run cargo test |
| C4 | planner_error | 6 | `grep → read_file → str_replace → grep` | 0 | str_replace ran but no match |
| C5 | planner_error | 7 | `shell → read_file → grep ×3` | 0 | Never used a write tool |

**Gate fix unblocked C3**: `denied` → `ok` with successful file edit (419 bytes written).

#### Artifacts

- `.localagent/eval/results_2026-03-07T03-02-06.4234611Z.json` (run 1)
- `.localagent/eval/results_2026-03-07T05-11-06.7849613Z.json` (run 2 after assertion fix)
- `.localagent/eval/results_2026-03-07T05-32-30.3361591Z.json` (run 3)
- `.localagent/eval/results_2026-03-07T05-47-47.3497653Z.json` (run 4 after gate/trust fix)

## Current Temp State

- Prepared next-run workdir: `.tmp/eval-c1-qwen25coder7bq8/workdir`
- `src/hello.txt`: absent
