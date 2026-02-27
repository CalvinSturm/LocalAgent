# PR6 Scope: `feat: add TUI /learn commands` (Finalized)

## Goal

Add TUI slash-command support for learning workflows by exposing existing `learn` command capabilities inside chat TUI:

- `/learn ...`

while preserving:

- explicit operator control
- deterministic command parsing and output
- existing learning/store/promotion safety invariants
- thin-wrapper architecture (reuse existing learn command logic, avoid duplicate behavior)

PR6 is a TUI command-surface feature, not a new learning semantics feature.

---

## In scope (PR6A only, default)

### 1. Add `/learn` slash commands in chat TUI

PR6A supports:

- `/learn help`
- `/learn list`
- `/learn show <id>`
- `/learn archive <id>`

PR6B (follow-up) adds:

- `/learn capture ...`
- `/learn promote ...`

#### Command-shape principle

- Keep syntax close to CLI (`localagent learn ...`) where practical.
- Prefer explicit flags over shorthand aliases.

#### Required behavior

- Parse slash input deterministically.
- Route each command to existing `learn` implementation paths.
- Render command results into TUI logs pane (bounded output).

Out of scope:

- new learning operations
- changing CLI semantics of `learn` commands
- TUI command palette redesign
- interactive forms/modals for learning commands

---

### 2. Slash-command discovery updates

Update slash command lists/help text to include `/learn` examples.

Minimum additions:

- `/learn help`
- `/learn list`
- `/learn show <id>`
- `/learn capture --category ... --summary ...`
- `/learn promote <id> --to ...`
- `/learn archive <id>`

#### UX note

- Keep help concise; defer full usage details to `/learn help`.

---

### 3. Parsing and routing strategy (recommended)

Preferred strategy (lowest drift):

- tokenize `/learn ...` into argv
- synthesize CLI-like argv: `["localagent", "learn", ...]`
- parse using the existing clap command model
- dispatch through existing learn handlers (or shared adapter that builds the same `LearnArgs`)

Fallback strategy (if direct clap reuse is blocked):

- parse to a typed TUI command
- convert to the same internal `LearnArgs` structs used by CLI
- do not re-encode semantics separately in chat TUI

Recommended enum:

```rust
enum TuiLearnCommand {
    Help,
    List { /* minimal supported filters */ },
    Show { id: String },
    Capture { /* CLI-aligned args subset */ },
    Promote { /* id, target, target args, force, chain flags */ },
    Archive { id: String },
}
```

#### Reuse requirement

- Reuse existing `learning` and `cli_dispatch_learn` logic where possible.
- Do not reimplement promotion/archive logic in TUI.

#### Quoting/tokenization contract (must be explicit)

- quoted strings with spaces are supported (example: `--summary "text with spaces"`)
- escaped quotes inside quoted strings are supported
- malformed quoting returns deterministic parse error text
- parser behavior must be covered by tests

---

### 4. Output/rendering contract in TUI

TUI `/learn` command outputs must:

- go to logs pane (not transcript as assistant reply)
- preserve existing bounded/redacted rendering guarantees
- show deterministic success/failure lines

#### Hard invariant

- `/learn ...` output must not append to assistant transcript buffer.

#### Error display

- Show deterministic error code/message text from existing learn paths.
- Do not swallow underlying learning errors.

---

### 5. Safety and invariants

All existing learning invariants remain unchanged:

- capture writes under `.localagent/learn/**`
- promotion path/sensitivity/atomicity rules remain as implemented
- archive behavior remains idempotent
- assisted capture preview/write semantics remain unchanged
- `/learn` commands do not execute model tool-calling flows themselves and do not mutate policy/approvals state

#### Hard rule (PR6)

- TUI `/learn` must be a surface adapter only; behavior is delegated to existing command logic.

---

### 6. Supported argument subset (initial PR6)

PR6A argument support:

- `/learn list`:
  - optional `--status ...`, `--category ...`, `--limit ...`, `--show-archived`, `--format ...`
- `/learn show <id>`:
  - optional `--format ...`, `--show-evidence ...`, `--show-proposed ...`
- `/learn archive <id>`

PR6B argument support:

- `/learn capture` (including `--assist` / `--write`)
- `/learn promote` (including current target and validation flags)

#### Busy behavior (v1)

- reject `/learn` command execution while agent run/tool execution is active
- deterministic log message: `ERR_TUI_BUSY_TRY_AGAIN`

---

## Out of scope (do not implement in PR6A)

- new backend learning features
- learn TUI screen/view separate from chat logs
- autocompletion beyond existing slash dropdown behavior
- batch learn command execution
- altering issue/CLI docs beyond slash-help additions
- capture/promote TUI command execution (PR6B)

---

## Proposed function/module boundaries (recommended)

### `src/chat_commands.rs`

- add `/learn` slash entries for overlay/help discoverability

### `src/chat_tui_learn_adapter.rs`

Add adapter helpers:

- `parse_and_dispatch_learn_slash(line: &str, ctx: &TuiLearnAdapterCtx) -> anyhow::Result<String>`
- tokenizer with deterministic quoting/escape behavior
- conversion into shared learn args/dispatch path

### `src/chat_tui_runtime.rs`

Route in `handle_tui_slash_command(...)`:

- branch for lines beginning with `/learn`
- call `tui::learn_adapter::parse_and_dispatch_learn_slash(...)`
- append output/error strings to logs

### `src/cli_dispatch_learn.rs` / `src/learning.rs`

- reuse existing functions; avoid moving semantics unless needed for reuse.

---

## Invariants (must not change)

- Existing non-TUI CLI behavior is unchanged.
- Learning storage/event/promotion semantics are unchanged.
- `/learn` commands remain explicit; no hidden auto-promotion/archive.
- Assisted capture still requires `--assist` and honors preview-only/no-write behavior.

---

## Acceptance Criteria

1. TUI `/learn` help and discovery

- Slash overlay/help includes `/learn` entries
- `/learn help` prints concise usage in logs

2. TUI `/learn list/show/archive` work

- Commands execute and log deterministic output/error

3. Busy-state behavior is deterministic

- `/learn ...` while run/tool execution active logs `ERR_TUI_BUSY_TRY_AGAIN`

4. Logs-only contract

- `/learn` output appears in logs pane only
- no assistant transcript mutation

5. No behavior regressions

- existing CLI `learn` commands unchanged
- chat TUI non-learn slash commands unchanged

6. Quality gate

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test --quiet`

---

## PR6A Tests (minimum)

1. Parser tests

- quoted args with spaces
- escaped quote behavior
- malformed quote handling -> deterministic error text

2. Routing tests

- `/learn` branch in slash handler invokes learn adapter
- non-`/learn` slash commands unaffected

3. Behavior parity smoke tests (PR6A commands)

- `/learn list` and `/learn show` in TUI produce expected logs
- `/learn archive` updates status through existing backend path

4. Logs-only invariant

- `/learn` command execution does not append assistant transcript rows

5. Busy-state invariant

- busy run/tool state rejects `/learn` execution with `ERR_TUI_BUSY_TRY_AGAIN`

---

## PR6A size guardrails

- Keep PR6A focused on `/learn help|list|show|archive`
- Avoid parser over-engineering
- Reuse existing learn backend functions
- Defer capture/promote to PR6B

---

## Suggested implementation order (PR6A)

1. Add `/learn` entries to slash/help overlays
2. Add `chat_tui_learn_adapter` with tokenizer + argv synthesis
3. Add `/learn help/list/show/archive` routing
4. Add busy-state rejection
5. Add parser/routing/invariant tests
6. Run validation and commit

---

## PR6B follow-up scope (not in PR6A)

- `/learn capture` routing (including `--assist` / `--write`)
- `/learn promote` routing (all current promote flags)
- parity tests for capture/promote TUI behavior

### PR6B Goal

Complete TUI `/learn` write-heavy workflows while preserving CLI parity and safety:

- `/learn capture ...`
- `/learn promote ...`

PR6B must remain a TUI surface adapter and must not introduce new learning semantics.

### PR6B In scope

#### 1. `/learn capture` in TUI

Support CLI-aligned capture args (minimum):

- `--category <workflow-hint|prompt-guidance|check-candidate>`
- `--summary <text>`
- optional existing capture args (`--run`, `--task-summary`, `--profile`, `--guidance-text`, `--check-text`, `--tag`, `--evidence`, `--evidence-note`)
- assisted flags:
  - `--assist`
  - `--write`

Behavior must match existing CLI:

- `--assist` without `--write` => preview only, no writes
- `--assist --write` => persist learning entry + emit capture event
- `--write` without `--assist` => deterministic validation error

#### 2. `/learn promote` in TUI

Support existing promote targets/flags:

- `--to check --slug <slug>`
- `--to pack --pack-id <pack_id>`
- `--to agents`
- optional:
  - `--force`
  - `--check-run`
  - `--replay-verify`
  - `--replay-verify-run-id <id>`
  - `--replay-verify-strict`

Behavior must match existing CLI promotion semantics and deterministic errors.

#### 3. Adapter reuse contract

- Reuse `learning` + `cli_dispatch_learn` semantics (or shared helpers) rather than duplicating logic in TUI.
- For PR6B, use tokenize-only adapter behavior:
  - tokenizer produces argv tokens
  - synthesize CLI argv: `["localagent", "learn", ...]`
  - run existing clap parsing and dispatch path for `learn capture|promote`
- Do not maintain a parallel flag parser for capture/promote semantics in TUI.

### PR6B Out of scope

- new learn backend capabilities
- changing promotion/capture/assist semantics
- TUI modal/forms
- batch `/learn` commands
- automatic queueing/retry of `/learn` commands

### PR6B deterministic error/message contract

Busy state:

- while run/tool execution is active, reject with:
  - `ERR_TUI_BUSY_TRY_AGAIN`

Capture/assist parity errors (from existing backend):

- `LEARN_ASSIST_WRITE_REQUIRES_ASSIST`
- `LEARN_ASSIST_PROVIDER_REQUIRED`
- `LEARN_ASSIST_MODEL_REQUIRED`

Promote parity errors (from existing backend):

- `LEARN_PROMOTE_SENSITIVE_REQUIRES_FORCE`
- `LEARN_PROMOTE_TARGET_EXISTS_REQUIRES_FORCE`
- `LEARN_PROMOTE_INVALID_SLUG`
- `LEARN_PROMOTE_INVALID_PACK_ID`

TUI parse errors:

- deterministic `learn parse error: ...` messages
- include offending flag/token where possible

### PR6B busy behavior (v1, unchanged)

- reject `/learn capture` and `/learn promote` while run/tool execution is active
- do not queue
- output to logs pane only:
  - `ERR_TUI_BUSY_TRY_AGAIN`

### PR6B quoting/tokenization contract

Tokenizer rules (same as PR6A contract):

- whitespace separates args unless quoted
- single and double quotes supported
- backslash escapes supported in normal/double-quote contexts
- malformed quoting yields deterministic parse error

Required quote coverage examples:

- `/learn capture --summary "text with spaces"`
- `/learn capture --guidance-text "contains \\\"quoted\\\" text"`
- `/learn promote 01J... --to check --slug "bad slug"` (then backend validation)
- malformed:
  - `/learn capture --summary "unterminated`

### PR6B output contract

- All `/learn` results/errors go to logs pane.
- No assistant transcript append from `/learn` command handling.
- Existing bounded/redacted rendering guarantees remain.

### PR6B acceptance criteria

1. `/learn capture` parity

- non-assist capture path matches CLI results
- `--assist` preview and `--assist --write` behaviors match CLI semantics
- deterministic assist validation errors match CLI

2. `/learn promote` parity

- check/pack/agents routing and flags behave as in CLI
- deterministic promote errors match CLI
- promote chaining flags (`--check-run`, `--replay-verify*`) behave as in CLI

3. Busy-state behavior

- busy execution rejects capture/promote with `ERR_TUI_BUSY_TRY_AGAIN`

4. Logs-only invariant

- `/learn capture|promote` handling does not append assistant transcript rows

5. Quoting behavior

- quoted/escaped args parse correctly
- malformed quotes return deterministic parse errors

6. No regressions

- PR6A commands (`help/list/show/archive`) unchanged
- non-`/learn` slash commands unchanged
- CLI `learn` commands unchanged

7. No partial writes / no TUI artifacts on failure

- failed `/learn capture|promote` execution does not create TUI-only temp/cache artifacts
- filesystem writes on success/failure match CLI behavior exactly for the same command

### PR6B tests (minimum)

Parser tests:

- capture and promote with quoted args
- escaped quotes inside capture text
- malformed quote errors
- unknown/invalid flag errors

Routing/parity tests:

- `/learn capture` success path writes expected entry
- `/learn capture --assist` preview path no-write
- `/learn capture --assist --write` write path persists entry
- `/learn promote ...` check target path
- `/learn promote ...` pack/agents target paths

No-write preview tests:

- `/learn capture --assist` asserts no `.localagent/learn/**` changes:
  - no new entry file
  - no events append
  - no status changes

Error parity tests:

- verify deterministic error-code text surfaces in logs for key assist/promote failures

Invariant tests:

- busy-state rejection for capture/promote
- logs-only output (no transcript append)

### PR6B implementation order (suggested)

1. Extend `tui::learn_adapter` parser for capture/promote args
2. Add capture/promote dispatch wiring via existing backend paths
3. Add busy-state checks for capture/promote (same `ERR_TUI_BUSY_TRY_AGAIN`)
4. Add quoting and parity tests for capture/promote
5. Run validation (`fmt`, `clippy`, `test`) and ship
