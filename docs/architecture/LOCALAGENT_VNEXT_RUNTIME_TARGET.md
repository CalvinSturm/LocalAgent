# LocalAgent vNext Runtime Target

## Progress Status

Status as of current worktree after the behavior-repair and checkpoint-authoritative runtime slices:

- `TaskContractV1` exists and is resolved at launch time
- contract provenance is persisted in run artifacts
- `ToolFactV1` and `ToolFactEnvelopeV1` exist and are persisted in artifacts/checkpoints
- approval and operator-interrupt boundaries have explicit runtime-owned transition events
- validation and final-answer collection now have explicit runtime transition helpers
- `RunCheckpointV1`, execution tier, interrupt history, phase summary, and completion decisions are persisted
- `cargo test --quiet` is green again after repairing validation / exact-final-answer / post-write regressions
- validation / exact-final-answer / post-write behavior now runs on top of checkpoint-backed phase and retry state instead of parallel loop-local booleans
- tool-protocol loop state is now carried in `RunCheckpointV1` instead of parallel loop-local counters/flags
- resume restores richer checkpoint-backed runtime state and can resume back into validating / verifying-changes / collecting-final-answer paths
- `cargo clippy -- -D warnings` passes

Still incomplete:

- the main loop is not yet fully decomposed into explicit per-phase handlers
- some completion/transition logic still lives inline in `src/agent.rs` rather than entirely in checkpoint-driven phase helpers and `completion_policy.rs`
- interrupt/checkpoint coverage is stronger, but the explicit phase loop is still only partially consolidated

## Goal

Define a concrete target architecture for LocalAgent's agent runtime when the primary operating environment is local LLMs.

This target keeps LocalAgent's current strengths:

- conservative side-effect defaults
- explicit trust/approval controls
- artifact-heavy execution
- post-write verification
- bounded retries
- MCP/tool/runtime visibility

It changes the runtime in one major way:

- move from a guarded freeform loop toward a runtime-owned, checkpointed state machine with explicit contracts, typed tool facts, and interrupt/resume boundaries

## Design Summary

LocalAgent vNext should be:

- runtime-owned
- checkpointed
- phase/state-machine driven
- contract-driven
- event-log centric
- conservative about execution tiers
- evidence-based for completion

The model can propose work inside bounded phases, but the runtime owns:

- state transitions
- tool exposure
- execution permission checks
- approval interrupts
- verification
- validation
- finalization

## Current Strengths To Preserve

Preserve these existing properties:

- tool exposure is already distinct from tool execution permission
- trust/policy gating is already separated from the model loop
- post-write verification exists and is runtime-owned
- bounded retry logic already exists
- run artifacts and event logs are already first-class
- execution targets already distinguish host vs docker

Relevant current modules:

- [`src/agent.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent.rs)
- [`src/agent/runtime_completion.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/runtime_completion.rs)
- [`src/agent_impl_guard.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs)
- [`src/runtime_wiring.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/runtime_wiring.rs)
- [`src/target.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/target.rs)
- [`src/events.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/events.rs)
- [`src/store/types.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/store/types.rs)

## Runtime State Layers

LocalAgent should treat runtime data as three explicit layers.

### 1. Session State

Long-lived operator and conversational context.

Owns:

- message history
- session settings
- task memory

Current home:

- [`src/session.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/session.rs)

This should remain separate from run execution state.

### 2. Run State

Mutable execution state for a single run.

Owns:

- current phase
- active plan step
- retry counters
- pending approvals / interrupts
- validation status
- recent tool facts
- completion gating state

This is the main missing explicit state layer today.

### 3. Artifact / Evidence State

Immutable or append-only execution evidence.

Owns:

- run record
- event log
- tool decisions
- config fingerprint
- MCP pinning evidence
- traces and summaries

Current home:

- [`src/store/types.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/store/types.rs)
- [`src/events.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/events.rs)

## Target Runtime State Machine

Use a small explicit run phase model.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RunPhase {
    Setup,
    Planning,
    Executing,
    WaitingForApproval,
    WaitingForOperatorInput,
    VerifyingChanges,
    Validating,
    CollectingFinalAnswer,
    Finalizing,
    Done,
    Failed,
    Cancelled,
}
```

## Phase Invariants

Each phase should eventually define:

- allowed inputs
- allowed outputs
- allowed transitions
- checkpoint requirements

This is necessary to keep the runtime from drifting back into branch-heavy phase logic.

### `Setup`

Allowed inputs:

- CLI/runtime launch inputs
- resolved session settings
- provider/tool/gate bootstrap results

Allowed outputs:

- resolved `TaskContractV1`
- resolved `ExecutionTier`
- initial `RunCheckpointV1`

Allowed transitions:

- `Planning`
- `Executing`
- `Failed`
- `Cancelled`

Checkpoint requirements:

- required on successful exit from `Setup`

### `Planning`

Allowed inputs:

- planner-enabled run configuration
- planner model output
- planner step constraints

Allowed outputs:

- persisted planner summary / planner step constraints
- worker handoff state

Allowed transitions:

- `Executing`
- `Failed`
- `Cancelled`

Checkpoint requirements:

- required before leaving `Planning`

### `Executing`

Allowed inputs:

- current transcript/messages for the run
- current task contract
- current checkpoint
- tool availability and gate state

Allowed outputs:

- assistant content
- zero or more tool calls
- runtime block / continue decision
- transition trigger to verification, approval, validation, or final-answer phases

Allowed transitions:

- `Executing`
- `WaitingForApproval`
- `WaitingForOperatorInput`
- `VerifyingChanges`
- `Validating`
- `CollectingFinalAnswer`
- `Finalizing`
- `Failed`
- `Cancelled`

Checkpoint requirements:

- required before entering any non-`Executing` phase
- required after any runtime interrupt is raised

### `WaitingForApproval`

Allowed inputs:

- pending approval interrupt
- operator approval/deny result

Allowed outputs:

- approval resolution record
- resumed tool execution intent or terminal denial

Allowed transitions:

- `Executing`
- `Failed`
- `Cancelled`

Checkpoint requirements:

- required on entry
- required on resolution

### `WaitingForOperatorInput`

Allowed inputs:

- operator queue messages
- explicit operator intervention

Allowed outputs:

- injected operator message or control action

Allowed transitions:

- `Executing`
- `Failed`
- `Cancelled`

Checkpoint requirements:

- required on entry
- required on resolution

### `VerifyingChanges`

Allowed inputs:

- successful write facts
- post-write readback requests/results

Allowed outputs:

- verification facts
- completion block or progression decision

Allowed transitions:

- `Executing`
- `Validating`
- `CollectingFinalAnswer`
- `Failed`
- `Cancelled`

Checkpoint requirements:

- required on entry
- required after verification results are recorded

### `Validating`

Allowed inputs:

- required validation command
- validation shell execution result

Allowed outputs:

- validation facts
- validation satisfied / failed status

Allowed transitions:

- `Executing`
- `CollectingFinalAnswer`
- `Failed`
- `Cancelled`

Checkpoint requirements:

- required on entry
- required after validation attempt

### `CollectingFinalAnswer`

Allowed inputs:

- final-answer-only model turn
- exact-answer contract if present

Allowed outputs:

- final output candidate
- exact-answer retry or completion decision

Allowed transitions:

- `CollectingFinalAnswer`
- `Finalizing`
- `Failed`
- `Cancelled`

Checkpoint requirements:

- required before retrying exact final-answer-only mode

### `Finalizing`

Allowed inputs:

- satisfied completion policy
- final transcript / facts / artifacts

Allowed outputs:

- terminal run outcome
- persisted run record
- final event stream entries

Allowed transitions:

- `Done`
- `Failed`
- `Cancelled`

Checkpoint requirements:

- required before terminal outcome emission

Phase meanings:

- `Setup`: resolve runtime inputs, contract, tools, target, gate, and initial checkpoint
- `Planning`: planner-worker planning phase when enabled
- `Executing`: normal tool-capable execution loop
- `WaitingForApproval`: persisted interrupt because a gate requires approval
- `WaitingForOperatorInput`: persisted interrupt for explicit operator interaction
- `VerifyingChanges`: runtime-owned readback / post-write verification
- `Validating`: runtime-owned validation phase such as `cargo test`
- `CollectingFinalAnswer`: work is done, model owes only the closeout
- `Finalizing`: runtime assembling outcome/artifacts
- `Done`, `Failed`, `Cancelled`: terminal states

## Terminal-State Invariants

Terminal states must be stronger than "the loop stopped."

### `Done`

Required invariants:

- completion policy evaluated to `FinalizeOk`
- any required validation is satisfied
- any required exact final answer is satisfied
- no unresolved approval interrupt exists
- no unresolved operator interrupt exists
- run record is persisted
- terminal `RunEnd` event is emitted

### `Failed`

Required invariants:

- a terminal failure reason is recorded
- failure class/source is recoverable from artifacts or events
- no unresolved interrupt remains marked active
- run record is persisted
- terminal `RunEnd` event is emitted

### `Cancelled`

Required invariants:

- cancellation source is recorded
- no further model/tool execution is attempted after cancellation is committed
- run record is persisted
- terminal `RunEnd` event is emitted

### General terminal guarantees

For all terminal phases:

- terminal state is write-once for the run
- no transition back to non-terminal phases is allowed
- checkpoint and run record must agree on terminal outcome

Suggested helpers:

```rust
pub fn assert_terminal_invariants(
    checkpoint: &RunCheckpointV1,
    outcome: &AgentOutcome,
) -> anyhow::Result<()>;

pub fn is_terminal_phase(phase: &RunPhase) -> bool;
```

## Minimal v1 Task Contract

Do not encode every possible workflow rule immediately. Start with the minimal schema the runtime will actually consume.

Suggested new module:

- [`src/agent/task_contract.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs)

Suggested types:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WriteRequirement {
    None,
    Optional,
    Required,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ValidationRequirement {
    None,
    Command {
        command: String,
        required_phase: RunPhase,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FinalAnswerMode {
    Freeform,
    Exact { required_text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionPolicyV1 {
    pub require_pre_write_read: bool,
    pub require_post_write_readback: bool,
    pub require_effective_write: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicyV1 {
    pub max_schema_repairs: u32,
    pub max_repeat_failures_per_key: u32,
    pub max_runtime_blocked_completions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContractV1 {
    pub task_kind: String,
    pub write_requirement: WriteRequirement,
    pub validation_requirement: ValidationRequirement,
    pub allowed_tools: Option<Vec<String>>,
    pub completion_policy: CompletionPolicyV1,
    pub retry_policy: RetryPolicyV1,
    pub final_answer_mode: FinalAnswerMode,
}
```

### `task_kind` rigor

`task_kind` should not remain a freeform string forever. For v1 it may still serialize as a string for compatibility, but the runtime should converge on a bounded enum plus an optional custom tag.

Recommended direction:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskKindV1 {
    ReadOnlyAnalysis,
    CodeModification,
    ValidationOnly,
    PlanningOnly,
    OperatorMediated,
    Custom(String),
}
```

If `task_kind` remains string-backed temporarily, contract resolution must still normalize it into a canonical internal value before the run starts.

Required properties:

- canonicalized once during setup
- persisted in checkpoint and artifacts
- not mutated by planner output
- used as an input to contract validation, completion policy, and execution-tier selection

Examples of invalid behavior to avoid:

- inferring different `task_kind` values mid-run from later prompt text
- letting planner output silently change a read-only task into a write task
- using raw prompt text as the authoritative `task_kind`

### Contract Field Rigor

Every contract field should have:

- a clear owner
- a clear semantic meaning
- a clear merge rule
- a clear invalid-state policy

#### Field ownership

- `task_kind`
  - owned by runtime contract resolution
  - planner may specialize within runtime-allowed bounds, but may not redefine the run class arbitrarily
- `write_requirement`
  - owned by runtime
  - planner may narrow `Optional -> Required` for a step only if the runtime allows writes at all
  - planner may not widen `None -> Optional/Required`
- `validation_requirement`
  - owned by runtime
  - planner may propose validation steps, but runtime decides whether a validation command is mandatory for finalization
- `allowed_tools`
  - owned by runtime
  - planner may narrow for the active step
  - planner may never widen beyond runtime/tool/gate exposure
- `completion_policy`
  - owned by runtime only
  - planner may not mutate finalization semantics
- `retry_policy`
  - owned by runtime only
  - planner may not mutate retry budgets
- `final_answer_mode`
  - owned by runtime
  - planner may not change output contract

#### Validation requirement modeling

Validation should be modeled as an explicit runtime requirement, not just as "a shell command we hope got run."

For v1:

- `ValidationRequirement::None` means validation is not part of completion semantics
- `ValidationRequirement::Command` means the runtime must observe a successful validation fact before `Done`

Rules:

- validation requirement is part of completion semantics, not planner advice
- validation requirement must identify the phase where it is satisfied
- validation failure is not automatically terminal if runtime policy allows repair/retry
- validation requirement may exist even when planner is disabled

Recommended follow-on direction if validation broadens:

```rust
pub enum ValidationRequirementV2 {
    None,
    Command {
        command: String,
        required_phase: RunPhase,
    },
    ToolBacked {
        tool_name: String,
        arguments_fingerprint: String,
        required_phase: RunPhase,
    },
}
```

#### Merge rule

Contract resolution should follow a single monotonic merge order:

1. explicit runtime/operator inputs
2. trusted instruction/profile metadata
3. planner narrowing metadata
4. backward-compatible inference
5. defaults

The merge must be monotonic in the safe direction:

- planner and inference may narrow
- planner and inference may not widen permissions or weaken completion requirements

#### Invalid-state policy

Reject or normalize invalid contracts at resolution time, not in the main loop.

Examples:

- `write_requirement = Required` with no write-capable execution tier available
- `validation_requirement = Command` when shell execution can never be permitted
- `final_answer_mode = Exact` with empty `required_text`
- `allowed_tools = Some([])` while `write_requirement = Required`

Suggested helper:

```rust
pub fn validate_task_contract(
    contract: &TaskContractV1,
    execution_tier: &ExecutionTier,
    exposed_tools: &[String],
) -> anyhow::Result<()>;
```

### Contract Provenance

Each contract field should eventually record where it came from.

Suggested type:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContractValueSource {
    Explicit,
    Inferred,
    Defaulted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskContractProvenanceV1 {
    pub task_kind: ContractValueSource,
    pub write_requirement: ContractValueSource,
    pub validation_requirement: ContractValueSource,
    pub allowed_tools: ContractValueSource,
    pub completion_policy: ContractValueSource,
    pub retry_policy: ContractValueSource,
    pub final_answer_mode: ContractValueSource,
}
```

Persist provenance in:

- run checkpoint
- run artifact / run record
- optional `TaskContractResolved` event data

This matters because migration will mix explicit settings, inferred values, and defaults for some time.

### v1 Contract Sources

The contract should be resolved once during setup from:

- explicit CLI/runtime settings when present
- planner metadata when present
- instruction/task profile metadata when present
- backward-compatible prompt heuristics as a fallback only

Prompt parsing should remain a compatibility layer, not the long-term policy source.

### Planner Authority Boundaries

The planner is allowed to propose structure, not redefine runtime law.

The planner may:

- decompose work into steps
- propose intended tools per step
- narrow active-step tool choice
- propose verification sequencing
- request replan when the runtime allows replanning

The planner may not:

- widen `allowed_tools`
- override execution tier
- override trust gate decisions
- weaken `completion_policy`
- weaken `retry_policy`
- suppress required validation
- alter exact final-answer requirements
- force finalization while required runtime conditions are unmet

Concretely:

- planner output is advisory for sequencing
- planner constraints are authoritative only after runtime validation and normalization
- runtime remains authoritative for permission, verification, and finalization

Suggested normalized planner handoff shape:

```rust
pub struct NormalizedPlanStepV1 {
    pub step_id: String,
    pub summary: String,
    pub intended_tools: Vec<String>,
    pub runtime_allowed_tools: Vec<String>,
    pub completion_notes: Vec<String>,
}
```

Runtime should derive `runtime_allowed_tools` by intersecting:

- contract allowance
- exposed tools
- execution-tier capabilities
- trust/policy constraints known at plan time

Planner should never directly supply the final allowed-tool set.

## Run Checkpoint Schema

Suggested new module:

- [`src/agent_runtime/state.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/state.rs)

Suggested types:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryState {
    pub blocked_runtime_completion_count: u32,
    pub exact_final_answer_retry_count: u32,
    pub required_validation_retry_count: u32,
    pub post_write_guard_retry_count: u32,
    pub post_write_follow_on_turn_count: u32,
    pub malformed_tool_call_attempts: u32,
    pub schema_repair_attempts: std::collections::BTreeMap<String, u32>,
    pub failed_repeat_counts: std::collections::BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationState {
    pub required_command: Option<String>,
    pub satisfied: bool,
    pub last_attempt_ok: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalState {
    pub pending_approval_id: Option<String>,
    pub pending_tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCheckpointV1 {
    pub schema_version: String,
    pub run_id: String,
    pub phase: RunPhase,
    pub step_index: u32,
    pub active_plan_step_id: Option<String>,
    pub task_contract: TaskContractV1,
    pub retry_state: RetryState,
    pub validation_state: ValidationState,
    pub approval_state: ApprovalState,
    pub last_tool_facts: Vec<ToolFactV1>,
}
```

### Checkpoint atomicity rules

Checkpointing should be treated as an atomic runtime boundary, not just best-effort persistence.

Rules:

- never mutate in-memory phase/interrupt state and then continue execution without either:
  - atomically persisting the new checkpoint, or
  - failing the run
- every interrupt-raising transition must be checkpointed before control is yielded
- every terminal transition must be checkpointed before `RunEnd` is emitted
- checkpoint writes must be atomic replace operations, not partial overwrite
- checkpoint schema version and run id must be validated on every load/resume

Recommended discipline:

1. build next checkpoint value in memory
2. write atomically to disk
3. emit `CheckpointSaved`
4. then emit interrupt/phase/terminal events that depend on that checkpoint

Suggested helpers:

```rust
pub fn write_checkpoint_atomic(
    path: &std::path::Path,
    checkpoint: &RunCheckpointV1,
) -> anyhow::Result<()>;

pub fn load_checkpoint_verified(
    path: &std::path::Path,
    expected_run_id: &str,
) -> anyhow::Result<RunCheckpointV1>;
```

Persist checkpoints:

- before raising an interrupt
- after resolving an interrupt
- before and after validation
- before and after post-write verification
- on cancellation
- before terminal finalize

These should be interpreted as required atomic boundaries, not advisory opportunities.

## Typed Tool Facts

Suggested new module:

- [`src/agent/tool_facts.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/tool_facts.rs)

Suggested types:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolFactV1 {
    Read {
        path: String,
        ok: bool,
    },
    Write {
        path: String,
        ok: bool,
        changed: bool,
    },
    Shell {
        command: String,
        ok: bool,
        scoped: bool,
    },
    Validation {
        command: String,
        ok: bool,
    },
    ApprovalRequired {
        tool_name: String,
        approval_id: String,
        tool_call_id: String,
    },
    ApprovalResolved {
        tool_name: String,
        approval_id: String,
        approved: bool,
    },
}
```

### Tool Fact Provenance

For replay/debugging strength, `ToolFactV1` should eventually carry provenance.

Suggested supporting shape:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFactEnvelopeV1 {
    pub sequence: u64,
    pub tool_call_id: Option<String>,
    pub phase: RunPhase,
    pub plan_step_id: Option<String>,
    pub fact: ToolFactV1,
}
```

Minimum provenance to add early:

- `sequence`
- `tool_call_id`

Recommended next provenance fields:

- `phase`
- `plan_step_id`

Policy can still consume plain `ToolFactV1` if needed, but artifacts and replay should retain the envelope form.

### Why This Matters

The runtime should reason over typed facts, not mostly over:

- tool name string matches
- prompt keyword matches
- ad hoc booleans

This is the main upgrade path away from stringly runtime policy.

## Interrupt / Resume Model

Suggested new module:

- [`src/agent/interrupts.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/interrupts.rs)

Suggested types:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeInterruptV1 {
    ApprovalRequired {
        approval_id: String,
        tool_call_id: String,
        tool_name: String,
    },
    OperatorInputRequired {
        reason: String,
    },
    CancellationRequested,
    ExternalQueueMessagePending,
}
```

### Required Runtime Boundaries

Persist explicit interrupt boundaries for:

- gate approval requirement
- operator queue interruption
- user/operator pause points
- validation-required checkpoints if resumable
- cancellation

This is the main place where vNext should improve beyond the current pause-like behavior.

## Execution Tiers

LocalAgent should treat execution tier as a runtime policy concept, not only as a transport detail.

Suggested new type:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionTier {
    NoSideEffects,
    ReadOnlyHost,
    ScopedHostWrite,
    ScopedHostShell,
    DockerIsolated,
    McpOnly,
}
```

### Mapping To Current Modules

Current implementation lives in:

- [`src/target.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/target.rs)

Current runtime policy inputs live in:

- [`src/run_prep.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/run_prep.rs)
- [`src/runtime_wiring.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/runtime_wiring.rs)

vNext should:

- continue using `ExecTargetKind` for host vs docker mechanics
- add `ExecutionTier` in runtime policy and artifacts
- record tier in checkpoint, event stream, and run record
- let completion policy and approvals reason over tier

### Execution Tier vs Trust Gate

Keep this distinction explicit:

- execution tier = capability context
- trust gate = permission decision

Concretely:

- `ExecutionTier` describes what the runtime environment can do
- the trust gate decides whether a specific tool action is allowed, denied, or requires approval

Do not merge these concepts into one policy object.

## Eventual Tool-Allowance Modeling

`allowed_tools: Option<Vec<String>>` is acceptable for v1, but it should be treated as a transitional representation.

The longer-term model should distinguish between:

- exposed tools
- contract-allowed tools
- planner-intended tools
- gate-permitted tools at execution time

### v1 semantics

For `TaskContractV1`, `allowed_tools` should mean:

- the maximum runtime-allowed tool set for this run before planner narrowing and before per-call gate decisions

It should not mean:

- currently exposed tools
- planner preference
- final gate decision

### Recommended v2 direction

Move toward an allowance model like:

```rust
pub struct ToolAllowanceV2 {
    pub runtime_maximum: Vec<String>,
    pub planner_step_subset: Option<Vec<String>>,
    pub execution_tier_compatible: Vec<String>,
    pub gate_requires_runtime_check: bool,
}
```

Or, if you want a more policy-shaped representation:

```rust
pub enum ToolAllowanceMode {
    AnyExposed,
    Subset(Vec<String>),
    ReadOnly,
    NoSideEffects,
}
```

### Required distinction at runtime

The runtime should always be able to explain four sets:

1. tools exposed to the model
2. tools contract-allowed for the run
3. tools allowed for the active planner step
4. tools actually permitted when the gate evaluates a specific call

This distinction should be reflected in:

- checkpoint state
- event data
- run artifacts

Suggested event fields:

- `exposed_tools`
- `contract_allowed_tools`
- `plan_allowed_tools`
- `gate_decision`

## Event Model vNext

Current event system is strong and should remain central.

Current home:

- [`src/events.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/events.rs)

Add these event kinds:

- `PhaseEntered`
- `PhaseExited`
- `CheckpointSaved`
- `InterruptRaised`
- `InterruptResolved`
- `CompletionCheck`
- `CompletionSatisfied`
- `CompletionBlocked`
- `TaskContractResolved`
- `ExecutionTierSelected`

### Event Philosophy

LocalAgent does not need full event sourcing.

It does need:

- event-log centric replay
- enough event coverage to explain runtime decisions
- enough event coverage to debug operator/approval behavior

The goal is that an operator can reconstruct:

- what phase the run was in
- why the runtime blocked progress
- why finalize was allowed
- which interrupt was pending
- which execution tier was active
- which contract was enforced

## Completion Policy Module

Move finalize eligibility into one explicit module.

Suggested new module:

- [`src/agent/completion_policy.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/completion_policy.rs)

Suggested surface:

```rust
pub enum CompletionBlockReason {
    PendingPlanStep,
    ToolOnlyRequiresToolCall,
    MissingRequiredWrite,
    MissingPreWriteRead,
    MissingPostWriteReadback,
    MissingValidation,
    ExactFinalAnswerMismatch,
    ProtocolArtifactEcho,
}

pub enum CompletionDecisionKind {
    FinalizeOk,
    Blocked,
}

pub struct CompletionDecisionV1 {
    pub kind: CompletionDecisionKind,
    pub block_reason: Option<CompletionBlockReason>,
    pub retryable: bool,
    pub next_phase: Option<RunPhase>,
    pub unmet_requirements: Vec<String>,
}

pub fn evaluate_completion(
    checkpoint: &RunCheckpointV1,
    tool_facts: &[ToolFactEnvelopeV1],
    final_output: &str,
) -> CompletionDecisionV1;

pub fn next_phase_after_write(
    checkpoint: &RunCheckpointV1,
    tool_facts: &[ToolFactEnvelopeV1],
) -> RunPhase;
```

### Current Logic To Migrate

Current finalize/completion logic is spread across:

- [`src/agent.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent.rs)
- [`src/agent/runtime_completion.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/runtime_completion.rs)
- [`src/agent_impl_guard.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs)

The vNext goal is:

- one completion policy module
- one task contract
- one typed fact model
- one phase model

### Why A Richer Completion Decision Matters

The completion module should eventually be able to report:

- why completion is blocked
- whether retry is allowed
- which phase should run next
- which requirements remain unsatisfied

This will improve:

- runtime correctness
- event output quality
- artifact explainability
- operator-facing debugging

## File-By-File Module Plan

### Keep With Narrower Responsibilities

- [`src/cli_dispatch.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/cli_dispatch.rs)
  - keep CLI entry and command-path defaults only
- [`src/agent_runtime/launch.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/launch.rs)
  - keep launch preparation, but also resolve task contract and initial checkpoint
- [`src/runtime_wiring.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/runtime_wiring.rs)
  - keep gate/event sink wiring
- [`src/target.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/target.rs)
  - keep execution target implementation
- [`src/session.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/session.rs)
  - keep session state only
- [`src/store/types.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/store/types.rs)
  - extend artifact schemas for checkpoint, contract, and interrupt history

### Refactor Heavily

- [`src/agent.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent.rs)
  - reduce to phase coordinator
  - stop carrying large policy surface inline
- [`src/agent/runtime_completion.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/runtime_completion.rs)
  - make it consume contract + checkpoint + tool facts
- [`src/agent_impl_guard.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs)
  - split into contract inference and completion-policy helpers
- [`src/run_prep.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/run_prep.rs)
  - allow contract-aware tool exposure filtering

### Add New Modules

- [`src/agent_runtime/state.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/state.rs)
- [`src/agent/task_contract.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs)
- [`src/agent/tool_facts.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/tool_facts.rs)
- [`src/agent/interrupts.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/interrupts.rs)
- [`src/agent/completion_policy.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/completion_policy.rs)

## Concrete Responsibility Split

### `src/agent_runtime/launch.rs`

Add:

- resolve `TaskContractV1`
- determine `ExecutionTier`
- create initial `RunCheckpointV1`
- persist checkpoint before entering main runtime
- emit `TaskContractResolved` and `ExecutionTierSelected`

### `src/agent.rs`

Target responsibilities:

- run the phase loop
- call model for current phase
- normalize assistant output
- dispatch tool execution
- persist checkpoint at boundaries
- delegate completion gating to `completion_policy`
- delegate interrupt handling to `interrupts`

### `src/agent/tool_facts.rs`

Own:

- translating tool decisions and tool execution results into `ToolFactV1`
- helper queries like:
  - `has_effective_write`
  - `has_read_before_write`
  - `pending_post_write_readback_paths`
  - `validation_satisfied`

### `src/agent/completion_policy.rs`

Own:

- finalization eligibility
- follow-on phase after successful write
- exact final answer gating
- validation gating
- retryable vs terminal completion blocks

### `src/agent/interrupts.rs`

Own:

- raising interrupts
- serializing interrupt payloads
- checkpointing around interrupts
- resuming after approval/operator response

### `src/store/types.rs`

Extend `RunRecord` with:

- `task_contract`
- `execution_tier`
- `final_checkpoint`
- `interrupt_history`
- `phase_summary`
- `completion_decisions`

## Proposed Run Loop Shape

Target pseudocode:

```rust
loop {
    load_or_resume_checkpoint();
    emit_phase_entered();

    match checkpoint.phase {
        RunPhase::Setup => transition_to_planning_or_executing(),
        RunPhase::Planning => run_planner_and_checkpoint(),
        RunPhase::Executing => run_worker_turn_and_collect_tool_calls(),
        RunPhase::WaitingForApproval => await_or_resume_approval(),
        RunPhase::WaitingForOperatorInput => await_or_resume_operator(),
        RunPhase::VerifyingChanges => run_post_write_readback(),
        RunPhase::Validating => run_required_validation(),
        RunPhase::CollectingFinalAnswer => collect_exact_or_freeform_closeout(),
        RunPhase::Finalizing => write_artifacts_and_finish(),
        RunPhase::Done | RunPhase::Failed | RunPhase::Cancelled => break,
    }
}
```

## Migration Plan

### Phase 1: Contract Introduction

Status: complete

Goal:

- add `TaskContractV1`
- derive it during runtime launch
- persist it in the run record

Changes:

- add [`src/agent/task_contract.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/task_contract.rs)
- update [`src/agent_runtime/launch.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/launch.rs)
- extend [`src/store/types.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/store/types.rs)

Behavior:

- no execution semantics change yet
- current prompt heuristics remain fallback

### Phase 2: Typed Tool Facts

Status: complete for v1 fact emission; still being expanded for more policy consumers

Goal:

- translate existing tool executions/decisions into `ToolFactV1`

Changes:

- add [`src/agent/tool_facts.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/tool_facts.rs)
- make [`src/agent.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent.rs) append facts

Behavior:

- keep current guard logic
- add fact generation in parallel for parity

### Phase 3: Checkpointed Interrupt Boundaries

Status: substantially complete, with explicit phase-loop consolidation still remaining

Goal:

- persist approval and operator interrupts

Changes:

- add [`src/agent_runtime/state.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_runtime/state.rs)
- add [`src/agent/interrupts.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/interrupts.rs)
- update gate approval paths and queue interruption paths

Behavior:

- no change to user-facing trust posture
- improved resumability/debuggability

Current state:

- approval checkpoints exist
- interrupted/operator boundaries exist
- approval resume is implemented
- operator-interrupt live transition events now mirror approval more closely
- resume is no longer boundary-only; checkpoint-backed validation / verification / final-answer state can be restored into the live loop
- checkpoint state now owns materially more of the live control surface, even though the main loop is not yet fully phase-dispatched

### Phase 4: Central Completion Policy

Status: substantially complete, with some inline transition logic still remaining

Goal:

- consolidate finalize eligibility

Changes:

- add [`src/agent/completion_policy.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/completion_policy.rs)
- move logic from:
  - [`src/agent/runtime_completion.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent/runtime_completion.rs)
  - [`src/agent_impl_guard.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent_impl_guard.rs)
  - [`src/agent.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent.rs)

Behavior:

- completion is decided from:
  - contract
  - checkpoint
  - tool facts
  - final output

Current state:

- verified-write completion has been centralized
- required-validation completion has been centralized
- validation phase transitions have been centralized
- approval/operator/final-answer transition helpers exist
- the validation / exact-final-answer / post-write phase family now uses checkpoint-backed state in the live loop
- the tool-protocol loop state previously carried as separate booleans/counters now lives in `RunCheckpointV1`
- some completion and transition logic still remains inline in `src/agent.rs`

### Phase 5: Explicit Phase Loop

Status: effectively complete for v1

Goal:

- make `RunPhase` first-class

Changes:

- simplify [`src/agent.rs`](/C:/Users/Calvin/Software%20Projects/LocalAgent/src/agent.rs)
- make runtime completion and post-write flow phase-driven

Behavior:

- same semantics where possible
- clearer runtime ownership

Current state:

- `RunPhase` exists
- approval, operator-interrupt, validation, and final-answer boundaries emit explicit phase transitions
- artifacts/checkpoints persist phase-oriented state
- the live loop now uses checkpoint-backed phase/retry/protocol state for more runtime decisions
- assistant tool-call normalization and planner-response evaluation now route through dedicated helper modules
- the active runtime loop now explicitly dispatches `Executing`, `Validating`, `VerifyingChanges`, and `CollectingFinalAnswer`
- the shared active-turn path is starting to split into smaller helpers for normalized-response handling and verified-write follow-on handling
- the shared active-turn path now also delegates completion/tool-execution handling to a dedicated helper
- the shared active-turn path now also delegates provider response acquisition and assistant protocol normalization
- `Validating` and `CollectingFinalAnswer` no longer enter through the same top-level phase function as `Executing`
- `VerifyingChanges` no longer enters through the same top-level phase function as `Executing`
- `Executing` no longer enters through a shared top-level active-phase function either
- the dispatcher now handles interrupt/pre-active/terminal non-active phases explicitly rather than through one generic fallback branch
- the dispatcher now also names `Setup`, `Planning`, `Finalizing`, interrupt, and terminal handling individually
- the repeated active-turn setup now routes through one lower-level helper for generation/normalization/response-processing instead of being duplicated across active phases
- the completion/tool coordinator path now also delegates runtime completion application, tool execution, and post-tool follow-on handling through smaller helpers
- runtime completion checkpoint transitions and post-tool/post-write checkpoint mutation policy now route through a dedicated `phase_transitions` helper module
- required-validation phase and post-response guard checkpoint mutation policy now route through a dedicated `response_guards` helper module
- the remaining guard/post-tool decision-to-effects translation now routes through a dedicated `runtime_effects` helper module
- the outer per-step runtime loop now routes through a dedicated coordinator helper, leaving `run_with_checkpoint` closer to setup -> iterate -> finalize
- the Phase 5 coordinator/phase-loop target is now effectively satisfied for v1; remaining cleanup can defer to later phases unless a concrete regression appears

### Phase 6: Execution Tier Integration

Status: complete for v1 visibility

Goal:

- make execution tier visible in policy and artifacts

Changes:

- add `ExecutionTier`
- emit tier events
- include tier in run record and config fingerprint

Behavior:

- clearer sandbox/host/docker semantics
- no need to rewrite execution targets

Current state:

- execution tier is resolved at launch
- execution tier is persisted in checkpoint and run artifacts
- execution tier is emitted in startup/runtime evidence
- further policy consumption can be expanded later without schema changes

## What Not To Do

Avoid these traps:

- do not replace all prompt heuristics at once
- do not introduce a huge task schema the runtime does not actually consume
- do not attempt full event sourcing as a first milestone
- do not merge session state and run state
- do not collapse trust, execution target, and completion policy into one module

## Success Criteria

The vNext runtime target is achieved when:

- a run has an explicit persisted phase
- approvals and operator pauses are explicit interrupts with resume points
- completion policy reads from task contract and typed tool facts
- post-write verification is phase-driven rather than branch-heavy
- event logs explain why progress was blocked or allowed
- artifacts show contract, tier, interrupts, and completion decisions
- `src/agent.rs` becomes materially smaller and more coordinator-like

## Immediate Next Work

Phase 5 should not be extended by default.

Recommended next work:

1. Treat explicit phase-loop consolidation as effectively closed unless a concrete runtime regression or clarity issue appears.
2. Move to the next runtime priorities that build on the checkpoint-backed phase model instead of reopening coordinator-shape cleanup.
3. Keep `cargo test --quiet` green and use targeted regressions if a future change touches the runtime loop again.

## Recommended First PR Sequence

1. Add `TaskContractV1` and persist it in artifacts.
2. Add `ToolFactV1` and generate facts alongside current logic.
3. Add `RunCheckpointV1` for approval interrupts only.
4. Add `completion_policy.rs` and switch one guard family at a time.
5. Introduce explicit `RunPhase` after parity is established.

This sequence minimizes semantic risk while steadily moving the runtime toward the target architecture.
