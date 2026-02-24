# LocalAgent v0.1.3 Release Notes

Date: 2026-02-24

## Highlights

- Runtime control-loop hardening for planner/worker execution.
- MCP lifecycle visibility expanded with progress, cancellation, and pin enforcement telemetry.
- Deterministic boundedness tightened across runtime/tool/schema paths.
- Trace and eval gating strengthened to catch regressions earlier.

## What Shipped

### Runtime Enforcement

- Enforced planner control envelope and separated control-plane output from user-facing output.
- Enforced typed step-status transitions and stronger step invariants in planner-worker loops.
- Added controlled replan flow and deterministic retry behavior for tool failures.

### Boundedness and Reliability

- Added hard runtime budgets with explicit budget-exceeded exits.
- Added runtime tool-call budgets and bounded schema-repair retry before tool execution.
- Added eval retry/failure-class metrics and regression gates.

### MCP Lifecycle and Drift Controls

- Added MCP progress and cancellation lifecycle events.
- Added MCP runtime trace persistence and lifecycle continuity checks.
- Added MCP tool catalog snapshotting and live drift verification.
- Added MCP pin metadata, drift visibility, and configurable pin enforcement modes:
  - `hard`
  - `warn`
  - `off`
- Surfaced MCP enforcement mode and diagnostics in the TUI status views.

### TUI Operator Visibility

- Added MCP lifecycle/cancellation/progress badges and stall timers.
- Improved guardrail/reason taxonomy visibility and step-level runtime hints.
- Added two-phase cancel visibility and clearer cancellation completion signaling.

## Breaking / Behavior Changes

- Planner-worker completion is now runtime-gated; model outputs no longer imply completion on their own.
- Budget and policy failures terminate with deterministic, categorized exit reasons.
- MCP pin enforcement may block drifted tool catalogs when enforcement is set to `hard`.

## Upgrade

```bash
cargo install --path . --force
localagent --help
```

## Recommended Post-Upgrade Checks

1. Verify effective policy and approval behavior in your environment.
2. Validate MCP pin enforcement mode (`hard`, `warn`, or `off`) for your deployment.
3. Run your eval profile/baseline comparison before promoting to production.

## Notable Internal Tracking

Primary implementation landed across commits from `4a512f0` through `02cbb7f` on `main`, including runtime boundedness, planner envelope enforcement, MCP lifecycle instrumentation, and pin enforcement diagnostics.
