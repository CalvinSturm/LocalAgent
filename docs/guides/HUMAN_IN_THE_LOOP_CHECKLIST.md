# Human-In-The-Loop Checklist

Use this for every LocalAgent behavior change, model tune, and release patch.

## 1) Define Success First
- Write one concrete prompt to test.
- Define expected tool sequence and forbidden tools.
- Define pass/fail criteria before running.

Example:
- Prompt: `Improve chess.html so it works like proper chess.`
- Expected tools: `list_dir -> read_file -> apply_patch -> read_file`
- Forbidden tools: `shell` (unless explicit verification request)
- Pass: file changed in place, no tool arg errors, no user-content asks for already-known files.

## 2) Run Controlled Experiments
- Change one variable at a time:
- model, instructions profile, runtime flags, policy, or code gate.
- Keep a short run log:
- input prompt, model, flags, top tool outcomes.

## 3) Inspect Evidence, Not Narratives
- Review tool pane/results first.
- Check for:
- missing args
- repeated failed retries
- fallback to shell
- asking user for content that tools already provided

## 4) Escalation Path
- First: instructions/profile fix in project `.localagent/instructions.yaml`.
- Second: policy guard (`require_approval` / deny) for risky behavior.
- Third: runtime logic gate in code for deterministic enforcement.

Promote to code gate when the same failure repeats across runs/models.

## 5) Protect Writes
- Default expectation for existing files:
- `read_file` then `apply_patch`
- `write_file` only for new files or explicit full rewrite intent.
- Keep approvals on for high-risk operations in trust-on flows.

## 6) Regression Lock
- Add or update tests/fixtures after every confirmed fix.
- Prefer deterministic tests for:
- policy decisions
- overwrite behavior
- TUI behavior regressions

## 7) Release Readiness
- CI green.
- User-facing fixes validated against acceptance criteria.
- No known blocker regressions.
- Release notes summarize what changed and what was enforced.

## 8) Debrief Each Cycle
- What failed?
- What fixed it?
- Which layer fixed it (prompt/policy/code)?
- What test now prevents recurrence?

---

## Quick Command Baseline

Use a known-good baseline for coding tasks:

```bash
localagent --provider lmstudio --model <model> --caps strict --trust on --enable-write-tools --allow-write chat --tui
```

Then tighten as needed:
- disable shell: `/params allow_shell off`
- set timeout off/on: `/timeout off` or `/timeout 60`

## Related Docs

- Instruction profiles: [INSTRUCTION_PROFILES.md](INSTRUCTION_PROFILES.md)
- Safe tool tuning baseline: [SAFE_TOOL_TUNING_BASELINE.md](SAFE_TOOL_TUNING_BASELINE.md)
