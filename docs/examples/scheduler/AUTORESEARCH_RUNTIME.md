# Autoresearch Runtime

This document defines the runtime services, lifecycle, and integration points
for the `autoresearch` workflow on ROCode.

It sits between:

- the workflow config contract in `workflow-autoresearch.schema.json`
- the owned runtime state contract in `AUTORESEARCH_STATE_MODEL.md`
- the existing scheduler execution kernel in `rocode-orchestrator`

The purpose of this document is to make one thing non-negotiable:

- scheduler owns generic execution orchestration
- autoresearch owns objective-driven iteration semantics

Neither side should absorb the other's domain.

## Runtime Design Rules

The runtime follows the same constitutional constraints used elsewhere in ROCode.

- Article 1, Unique Execution Kernel: scheduler remains the only generic execution loop.
- Article 5, Unique State Ownership: every runtime state field has exactly one write owner.
- Article 7, Lifecycle Symmetry: every started resource must be closed, released, restored, or orphaned deliberately.
- Article 8, Observability Rights: every active iteration, checkpoint, verifier run, and domain artifact must be inspectable.
- Article 9, Single Side-Effect Path: side effects run through explicit runtime services, not prompt conventions.

## Runtime Layers

Autoresearch runtime is split into four layers.

### 1. Scheduler Layer

Owned by ROCode scheduler.

Responsibilities:

- select execution posture from scheduler profile
- run execution stage dispatch (`SinglePass`, `CoordinationLoop`, `AutonomousLoop`)
- surface generic gate decisions: `Done`, `Continue`, `Blocked`
- drive stage transitions and finalization

Scheduler does not know:

- whether an iteration was kept or discarded
- why a guard failed
- how checkpoints are stored
- how "stuck" is defined

### 2. Workflow Layer

Owned by autoresearch runtime.

Responsibilities:

- validate workflow config against schema
- choose the mode controller
- drive baseline capture and iteration lifecycle
- own `IterationDecision`
- map domain decisions into scheduler gate decisions

This layer is the domain brain.

### 3. Service Layer

Owned by autoresearch runtime.

Responsibilities:

- metric evaluation
- verify and guard execution
- checkpoint and revert
- ledger persistence
- artifact generation

These services are deterministic helpers. They do not decide policy on their own.

### 4. Presentation Layer

Consumed by CLI, TUI, and web UI.

Responsibilities:

- show run objective, iteration count, metric trajectory, and stop reason
- link artifacts and reports
- display current checkpoint and verification status

Presentation reads state; it never mutates domain state directly.

## Runtime Service Inventory

The following services are required for a full implementation.

## WorkflowController

The WorkflowController is the top-level owner of a single autoresearch run.

Write ownership:

- `RunState.status`
- `RunState.current_iteration`
- `RunState.stuck_counter`
- mode-specific run state objects
- `IterationDecision -> SchedulerExecutionGateDecision` mapping

Responsibilities:

- construct the run from validated config
- initialize all other services
- capture baseline before first mutation
- drive the iteration state machine
- invoke decision policy with evaluator results
- call snapshot release or restore after a decision
- synthesize a `RunSummary`
- clean up or mark orphaned resources on exit

The WorkflowController is the only component allowed to coordinate across all runtime services.

## ObjectiveEvaluator

The ObjectiveEvaluator owns metric history.

Write ownership:

- `RunState.baseline`
- `RunState.metric_history`

Responsibilities:

- run metric extraction over verify output
- create `MetricSample`
- compare current sample against baseline and best-so-far
- evaluate `satisfiedWhen`
- expose directional comparison helpers

It does not:

- run guard commands
- decide keep or discard
- mutate scheduler gate state

## VerificationRunner

The VerificationRunner executes verify and guard commands.

Write ownership:

- no long-lived top-level fields; returns `VerificationResult` values

Responsibilities:

- execute verify command with timeout and environment policy
- execute guard command with timeout and environment policy
- capture stdout, stderr, exit code, and duration
- classify timeout versus normal failure
- pass verify stdout to ObjectiveEvaluator for metric extraction

It does not:

- compare metric values
- choose whether to revert

## DecisionPolicy

The DecisionPolicy is the single owner of iteration outcome semantics.

Write ownership:

- no persistent state fields; returns `IterationDecision`

Responsibilities:

- decide `Keep`, `Discard`, `Rework`, `RetryCrash`, or stop conditions
- enforce `maxReworkAttempts`
- enforce `maxCrashRetries`
- apply simplicity thresholds and tie-break rules
- emit reasoned discard categories

It does not:

- perform side effects
- write logs
- release checkpoints

## SnapshotEngine

The SnapshotEngine owns workspace checkpoints.

Write ownership:

- `RunState.active_checkpoint`
- `WorkspaceCheckpoint.status`

Responsibilities:

- create checkpoint before an iteration mutates the workspace
- restore checkpoint when an iteration is discarded
- release checkpoint when an iteration is kept
- mark checkpoint orphaned on abnormal termination
- recover orphaned checkpoints during startup cleanup

It does not:

- decide when to restore or release
- infer business semantics from metric output

## IterationLedger

The IterationLedger owns append-only per-iteration records.

Write ownership:

- `RunState.ledger`

Responsibilities:

- create `IterationRecord`
- append records in memory
- expose query helpers: counts, tail windows, best iteration
- serialize records for persistence

It does not:

- define artifact paths
- decide stop reasons

## ArtifactWriter

The ArtifactWriter owns materialized files.

Write ownership:

- no in-memory domain fields; writes files declared by config

Responsibilities:

- persist iteration ledger to TSV, JSONL, or both
- write run summary
- write mode-specific reports
- guarantee idempotent final write on completion or interruption

It does not:

- construct domain decisions
- mutate run state except by reporting write failures

## Mode Controllers

Each workflow mode has a dedicated controller. These are not optional prompt wrappers.

### PlanController

Responsibilities:

- gather codebase context
- validate scope globs
- propose metric definitions
- dry-run verify command
- emit launch-ready config

### SecurityController

Responsibilities:

- build asset inventory
- map trust boundaries
- track OWASP and STRIDE coverage
- maintain findings registry and evidence requirements

### DebugController

Responsibilities:

- capture symptom statement
- maintain hypothesis registry
- run experiments and classify outcomes
- produce root-cause oriented debug outputs

### FixController

Responsibilities:

- discover broken-state categories
- count errors by category
- prioritize repair order
- stop automatically when broken-state count reaches zero

### ShipController

Responsibilities:

- determine shipment type
- generate checklist
- run dry-run phase
- request approval for side-effecting ship actions
- monitor and optionally roll back

## Service Collaboration Contract

Runtime service interactions are one-directional and orchestrated by WorkflowController.

The canonical call graph for one iteration is:

```text
WorkflowController
  -> SnapshotEngine.capture()
  -> ModeController.prepare_iteration_context()
  -> Scheduler execution round mutates workspace
  -> VerificationRunner.run_verify()
  -> ObjectiveEvaluator.record_verify_result()
  -> VerificationRunner.run_guard()            // optional
  -> DecisionPolicy.decide(...)
  -> SnapshotEngine.release() or restore()
  -> IterationLedger.append()
  -> ArtifactWriter.append_iteration()
  -> WorkflowController.map_to_gate_decision()
```

Rules:

- services do not call each other laterally
- services communicate through return values and controller-owned state
- only WorkflowController decides the order of side effects

## Canonical Iteration State Machine

The runtime state machine below is the normative execution contract.

```text
Initializing
  -> CapturingBaseline
  -> Iterating
     -> Snapshot
     -> Execute
     -> Verify
     -> Guard? 
     -> Decide
        -> Keep
        -> Discard
        -> Rework
        -> RetryCrash
        -> StopSatisfied
        -> StopStalled
        -> StopBlocked
  -> Completing
  -> Completed
```

### Initializing

Entry actions:

- load and validate workflow config
- instantiate services
- recover orphaned checkpoints if configured
- allocate run id and artifact root

Exit conditions:

- success -> `CapturingBaseline`
- fatal config or recovery failure -> `Failed`

### CapturingBaseline

Entry actions:

- run verify command against initial workspace
- extract baseline metric if objective mode requires it
- initialize metric history

Exit conditions:

- success -> `Iterating`
- verify failure with no recovery path -> `Failed`
- operator interrupt -> `Interrupted`

### Iterating

Each iteration consists of the phases below.

#### Snapshot

Entry actions:

- create one active checkpoint for the current iteration
- persist checkpoint metadata if required by workspace policy

Failure path:

- checkpoint creation failure -> `StopBlocked`

#### Execute

Entry actions:

- hand execution context to scheduler execution round
- allow the chosen scheduler preset to perform mutations

Failure path:

- execution crash -> `RetryCrash` or `StopBlocked` depending on decision policy

#### Verify

Entry actions:

- run verify command
- parse metric
- update current metric sample

Failure path:

- verify failure or timeout becomes a decision input, not an immediate runtime crash

#### Guard

Executed only when:

- guard command exists
- verify passed well enough to evaluate the candidate state

Failure path:

- guard failure does not immediately discard
- it enters decision policy as a possible `Rework`

#### Decide

DecisionPolicy consumes:

- metric history
- verify result
- guard result
- rework attempt count
- crash retry count
- stuck counter
- satisfiedWhen threshold

Possible outputs:

- `Keep`
- `Discard`
- `Rework`
- `RetryCrash`
- `StopSatisfied`
- `StopStalled`
- `StopBlocked`

#### Keep

Entry actions:

- release active checkpoint
- append kept iteration record
- reset stuck counter to zero
- update best metric if applicable

Scheduler projection:

- `Continue`, unless iteration policy or objective threshold says to finish now

#### Discard

Entry actions:

- restore active checkpoint
- append discarded iteration record
- increment stuck counter

Scheduler projection:

- usually `Continue`
- may become `Blocked` if iteration policy or stuck protocol says stop

#### Rework

Entry actions:

- keep current checkpoint active
- preserve guard output as rework context
- re-enter execution with tightened next input

Scheduler projection:

- `Continue`

#### RetryCrash

Entry actions:

- restore checkpoint if needed
- append crash context
- retry with crash-repair context

Scheduler projection:

- `Continue`

#### StopSatisfied

Entry actions:

- release checkpoint if active
- finalize summary with satisfied stop reason

Scheduler projection:

- `Done`

#### StopStalled

Entry actions:

- restore or preserve checkpoint according to workspace policy
- emit stuck report and exhausted escalation path

Scheduler projection:

- `Blocked`

#### StopBlocked

Entry actions:

- stop further execution
- preserve best-effort recovery artifacts
- emit operator-facing block report

Scheduler projection:

- `Blocked`

### Completing

Entry actions:

- finalize run summary
- write all configured artifacts
- mark unresolved checkpoints as released or orphaned explicitly

Exit conditions:

- success -> `Completed`
- artifact write failure -> `Failed` with partial artifact report

## Snapshot Strategy Contract

Snapshot strategy is chosen by `workspacePolicy.revertStrategy`.

Supported strategies should include the following.

### GitBranchPerIteration

Behavior:

- create a branch or ref per iteration
- restore by hard reset within the isolated iteration ref, not within user branch context

Use when:

- repo is clean enough for branch-based isolation
- preserving per-iteration history is valuable

Avoid when:

- workspace has high unrelated churn

### GitStashStack

Behavior:

- stash tracked and optionally untracked changes with a run-scoped message
- restore by applying or popping the matching stash entry

Use when:

- changes are local and branch churn is undesirable

Risk:

- stash stacks are fragile without strong namespacing and cleanup

### PatchFile

Behavior:

- persist reverse patch and base metadata under artifact root
- restore by applying reverse patch against the expected base

Use when:

- file-level auditability matters more than git object manipulation

Risk:

- patch replay can fail after external drift

### WorktreeFork

Behavior:

- create a disposable worktree for the run or iteration
- restore by switching authoritative pointer back to the preserved source

Use when:

- strongest isolation is required
- ship or security workflows need clean separation

Risk:

- more expensive in disk and lifecycle complexity

## Snapshot Selection Policy

Recommended default selection order:

1. `WorktreeFork` for `ship` and high-risk `security` flows
2. `GitBranchPerIteration` for clean repos with explicit commit policy
3. `PatchFile` for dirty repos where destructive git actions are unacceptable
4. `GitStashStack` only when explicitly requested or operationally simpler

The runtime must never silently fall back to `git reset --hard`.

## Stuck Protocol

The stuck protocol is activated when `stuck_counter >= stuckThreshold`.

The protocol is owned by WorkflowController but uses mode-specific guidance.

Recommended escalation ladder:

1. Tighten execution prompt around objective and accepted scope
2. Force narrower change set or smaller experiment size
3. Switch from broad modification to targeted diagnostic or measurement pass
4. Require stronger evidence before allowing another discard-prone change
5. Abort as `StopStalled` if the escalation budget is exhausted

The stuck protocol must produce:

- a summary of repeated failure modes
- the last N discard reasons
- the best metric achieved
- the final escalation action attempted

This output feeds both the final report and scheduler block summary.

## Crash Recovery Contract

Crash recovery is distinct from ordinary discard.

### Recoverable crash

Examples:

- transient command failure
- syntax error introduced in iteration
- timeout in an exploratory command

Handling:

- restore checkpoint if workspace is compromised
- increment crash retry count
- emit `RetryCrash`
- continue until `maxCrashRetries`

### Unrecoverable crash

Examples:

- checkpoint restore failed
- repository permissions denied
- worktree creation failed repeatedly
- artifact root unavailable

Handling:

- emit `StopBlocked`
- preserve diagnostic context
- mark active checkpoint orphaned if cleanup cannot complete safely

## Verification Contract

Verify and guard execution follow hard rules.

### Verify

- may produce metric
- failure is a decision input
- timeout is distinguishable from non-zero exit
- stdout must be persisted when metric extraction depends on it

### Guard

- runs only after a candidate state is worth evaluating
- never overwrites verify result
- failure triggers rework first, then discard if rework budget is exhausted

### Mechanical metric extraction

Metric extraction belongs to ObjectiveEvaluator, not the model.

The model may explain the result, but only ObjectiveEvaluator decides:

- improved
- unchanged
- regressed
- satisfied
- invalid metric

## Decision Policy Contract

Decision policy must be deterministic for the same inputs.

Inputs:

- `MetricHistory`
- `VerificationResult` for verify
- optional `VerificationResult` for guard
- iteration counts
- rework attempts
- crash retry attempts
- thresholds from config

Outputs:

- exactly one `IterationDecision`

DecisionPolicy is not allowed to:

- run commands
- modify files
- write artifacts

## Artifact Writing Contract

ArtifactWriter runs in two modes.

### Incremental mode

After each iteration:

- append iteration record to ledger artifact
- update quick status artifact if configured

This mode improves recoverability and observability.

### Final mode

At completion:

- write `RunSummary`
- write mode-specific reports
- write checkpoint cleanup report if needed

Mode-specific artifacts should include:

- `run`: summary, ledger, best-iteration report
- `security`: findings, threat model, coverage matrix, recommendations
- `debug`: hypotheses, experiments, root cause, repro notes
- `fix`: broken-state trajectory, per-category deltas
- `ship`: checklist, dry-run log, ship log, monitor log, rollback metadata

## Scheduler Integration Points

Autoresearch should integrate with scheduler at explicit seams only.

### 1. Profile Selection

Scheduler profile selects orchestration posture:

- `prometheus` for `plan`
- `hephaestus` for `run`, `debug`, `fix`
- `atlas` for `security` and parts of `ship`

### 2. Execution Stage Body

The actual mutation work still runs through scheduler execution dispatch.

Autoresearch does not replace:

- `run_autonomous_loop`
- `run_coordination_loop`

It wraps them with:

- checkpointing
- verify and guard
- decision policy
- artifact persistence

### 3. Verification and Gate Boundary

Scheduler already has execution -> verification -> gate structure.
Autoresearch should hook here:

- after execution round completes
- before final gate status is projected upward

At this seam, WorkflowController maps `IterationDecision` into `SchedulerExecutionGateDecision`.

### 4. Effect Protocol

New effect kinds may be useful, but effect protocol is not the domain model.

Reasonable effect additions:

- `RunMechanicalVerification`
- `AppendIterationLedger`
- `CreateWorkspaceCheckpoint`
- `RestoreWorkspaceCheckpoint`
- `ReleaseWorkspaceCheckpoint`
- `WriteWorkflowArtifact`

These effects are acceptable only as dispatch surfaces for already-defined domain services.

They must not become a replacement for:

- `DecisionPolicy`
- `ObjectiveEvaluator`
- `SnapshotEngine`

### 5. Cancellation

Scheduler cancellation remains authoritative at the orchestration layer.

Autoresearch observes cancellation through scheduler runtime state and maps it to:

- `Interrupted` run status
- final stop reason `ManualInterrupt`

The scheduler should not attempt to synthesize autoresearch-specific stop reasons.

## Public Runtime Interfaces

The runtime should eventually expose stable interfaces equivalent to:

```text
AutoresearchRuntime::start(config, scheduler_context) -> RunHandle
AutoresearchRuntime::resume(run_id) -> RunHandle
AutoresearchRuntime::cancel(run_id) -> Result
AutoresearchRuntime::status(run_id) -> RunStateView
AutoresearchRuntime::artifacts(run_id) -> ArtifactIndex
```

Supporting interfaces:

```text
ObjectiveEvaluator::capture_baseline(...)
ObjectiveEvaluator::evaluate_verify_result(...)
VerificationRunner::run_verify(...)
VerificationRunner::run_guard(...)
DecisionPolicy::decide(...)
SnapshotEngine::capture(...)
SnapshotEngine::restore(...)
SnapshotEngine::release(...)
IterationLedger::append(...)
ArtifactWriter::append_iteration(...)
ArtifactWriter::write_final_summary(...)
```

These names are illustrative; the ownership boundaries are the important part.

## Non-Goals

This runtime intentionally does not:

- move scheduler gate semantics into autoresearch domain
- let models decide metric improvement without mechanical evaluation
- use destructive git reset as the normal revert path
- rely on prompt-only conventions for crash recovery
- let multiple services mutate the same run-state field

## Recommended Implementation Order

The implementation order should minimize semantic drift.

1. Implement typed config loading for `workflow-autoresearch.schema.json`
2. Implement runtime state structs matching `AUTORESEARCH_STATE_MODEL.md`
3. Implement ObjectiveEvaluator and VerificationRunner
4. Implement DecisionPolicy
5. Implement SnapshotEngine
6. Implement IterationLedger and ArtifactWriter
7. Implement WorkflowController
8. Integrate mode controllers
9. Hook WorkflowController into scheduler execution and gate seams
10. Add CLI, TUI, and web observability

This order keeps config, state, and behavior aligned from the start.
