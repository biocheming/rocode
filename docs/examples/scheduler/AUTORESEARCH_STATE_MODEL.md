# Autoresearch State Model

This document defines every runtime state type that the autoresearch workflow owns.
These types are the single source of truth for what lives in memory during a run.
Runtime services (defined in AUTORESEARCH_RUNTIME.md) read and write these types;
no other module may mutate them directly.

Design constraints from the ROCode Constitution:

- Article 5 (Unique State Ownership): each state domain has exactly one owner.
- Article 7 (Lifecycle Symmetry): creation implies destruction; open implies close.
- Article 8 (Observability Rights): every active entity must be observable.

## Type Index

| Type | Owner | Purpose |
|------|-------|---------|
| RunState | WorkflowController | Top-level run lifecycle |
| RunConfig | WorkflowController | Frozen snapshot of validated config |
| IterationRecord | IterationLedger | Per-iteration decision record |
| IterationDecision | DecisionPolicy | Outcome of a single iteration |
| IterationDecision-to-GateDecision Mapping | WorkflowController | Bridge to scheduler gate |
| MetricSample | ObjectiveEvaluator | Single metric measurement |
| MetricHistory | ObjectiveEvaluator | Baseline + best + trajectory |
| VerificationResult | VerificationRunner | Verify/guard outcome |
| WorkspaceCheckpoint | SnapshotEngine | Restorable workspace state |
| RunSummary | WorkflowController | Final run report |

---

## RunState

The top-level lifecycle state of a single autoresearch run.
Exactly one RunState exists per run. It is created when the workflow starts
and destroyed when the workflow completes or is abandoned.

```
RunState {
    run_id:              RunId,           // unique, generated at creation
    config:              RunConfig,       // frozen at init, never mutated
    status:              RunStatus,       // lifecycle phase
    baseline:            Option<MetricSample>,  // captured before first iteration
    metric_history:      MetricHistory,   // updated after every evaluation
    current_iteration:   u32,            // 1-indexed, 0 means not started
    ledger:              IterationLedger, // append-only iteration records
    active_checkpoint:   Option<WorkspaceCheckpoint>, // current snapshot, if any
    stuck_counter:       u32,            // consecutive discards since last keep
    started_at:          Timestamp,
    finished_at:         Option<Timestamp>,
}
```

### RunStatus

```
enum RunStatus {
    Initializing,       // config validated, services starting
    CapturingBaseline,  // running verify command to capture baseline metric
    Iterating,          // main loop active
    Stuck,              // stuck protocol engaged (>= stuckThreshold consecutive discards)
    Completing,         // final synthesis and artifact write
    Completed,          // run finished normally
    Interrupted,        // manual stop (Ctrl+C or operator signal)
    Failed,             // unrecoverable error
}
```

**Lifecycle transitions:**

```
Initializing -> CapturingBaseline -> Iterating -> Completing -> Completed
                                  -> Stuck -> Iterating  (stuck protocol may re-enter)
                                  -> Interrupted
                                  -> Failed
```

Symmetry (Article 7): `Initializing` and `Completing` are paired entry/exit phases.
Every resource acquired in `Initializing` must be released in `Completing`,
`Interrupted`, or `Failed`.

---

## RunConfig

A frozen, validated snapshot of the workflow configuration.
Created once during `Initializing` from the parsed JSON config.
Never mutated after creation. All runtime services reference RunConfig by
shared immutable borrow, never by copy.

```
RunConfig {
    workflow:           WorkflowDescriptor,
    objective:          Option<ObjectiveDefinition>,
    iteration_policy:   Option<IterationPolicyDefinition>,
    decision_policy:    Option<DecisionPolicyDefinition>,
    workspace_policy:   Option<WorkspacePolicyDefinition>,
    artifacts:          Option<ArtifactDefinition>,
    approval_policy:    Option<ApprovalPolicyDefinition>,

    // Domain-specific, at most one populated per mode
    security:           Option<SecurityConfig>,
    debug:              Option<DebugConfig>,
    fix:                Option<FixConfig>,
    ship:               Option<ShipConfig>,
}
```

Fields map 1:1 to the workflow-autoresearch schema definitions.
Option fields are required or absent based on mode, validated at init.

---

## IterationRecord

A single entry in the iteration ledger. Created at the end of each iteration,
after the decision is made. Append-only; never modified after creation.

```
IterationRecord {
    iteration:          u32,             // 1-indexed
    decision:           IterationDecision,
    metric_before:      Option<f64>,     // metric value before this iteration
    metric_after:       Option<f64>,     // metric value after verify
    delta:              Option<f64>,     // metric_after - metric_before
    verify_result:      VerificationResult,
    guard_result:       Option<VerificationResult>,
    rework_attempts:    u32,             // 0 if no rework was needed
    crash_attempts:     u32,             // 0 if no crash recovery
    checkpoint_ref:     CheckpointRef,   // reference to the snapshot used
    changed_files:      Vec<FilePath>,   // files modified in this iteration
    summary:            String,          // one-sentence description of the change
    duration_ms:        u64,
    timestamp:          Timestamp,
}
```

---

## IterationDecision

The seven-variant decision enum. This is the autoresearch domain's authoritative
decision type. It carries the semantic reason for the outcome, not just the
scheduling signal.

```
enum IterationDecision {
    Keep,               // metric improved, verify passed, guard passed (or no guard)
    Discard {           // iteration did not improve or verify failed
        reason: DiscardReason,
    },
    Rework {            // metric improved but guard failed; trying different impl
        attempt: u32,           // which rework attempt (1..=maxAttempts)
        guard_output: String,   // stderr/stdout from guard for context
    },
    RetryCrash {        // runtime error during iteration; attempting fix
        attempt: u32,           // which crash retry attempt (1..=maxAttempts)
        error: String,          // error message
    },
    StopSatisfied,      // objective threshold met
    StopStalled,        // stuck_counter >= stuckThreshold, stuck protocol exhausted
    StopBlocked {       // unrecoverable: resource exhaustion, permission denied, etc.
        reason: String,
    },
}
```

### DiscardReason

```
enum DiscardReason {
    MetricRegressed,           // metric got worse
    MetricUnchanged,           // metric did not improve
    VerifyFailed,              // verify command returned non-zero
    VerifyTimeout,             // verify command exceeded timeout
    GuardFailedAfterRework,    // guard still failing after max rework attempts
    CrashUnrecoverable,        // crash recovery exhausted
    SimplicityOverride,        // marginal improvement below threshold
}
```

---

## IterationDecision to SchedulerExecutionGateDecision Mapping

The autoresearch domain produces IterationDecision.
The scheduler execution loop consumes SchedulerExecutionGateDecision
(Done / Continue / Blocked).

The mapping is performed by the WorkflowController at the boundary between
domain logic and scheduler plumbing. The scheduler never sees IterationDecision
directly. The domain layer never constructs GateDecision directly.

```
fn map_to_gate_decision(
    decision: &IterationDecision,
    iteration: u32,
    policy: &IterationPolicyDefinition,
) -> SchedulerExecutionGateDecision {
    match decision {
        // Loop continues: the iteration outcome does not end the run
        Keep => GateDecision {
            status: Continue,
            summary: "iteration kept, continuing",
            next_input: None,
            final_response: None,
        },
        Discard { .. } => GateDecision {
            status: Continue,
            summary: format!("iteration discarded: {reason}"),
            next_input: Some(compose_retry_context(decision)),
            final_response: None,
        },
        Rework { attempt, .. } => GateDecision {
            status: Continue,
            summary: format!("rework attempt {attempt}"),
            next_input: Some(compose_rework_context(decision)),
            final_response: None,
        },
        RetryCrash { attempt, .. } => GateDecision {
            status: Continue,
            summary: format!("crash recovery attempt {attempt}"),
            next_input: Some(compose_crash_context(decision)),
            final_response: None,
        },

        // Loop terminates normally
        StopSatisfied => GateDecision {
            status: Done,
            summary: "objective satisfied",
            next_input: None,
            final_response: Some(compose_run_summary()),
        },

        // Loop terminates due to blockage
        StopStalled => GateDecision {
            status: Blocked,
            summary: "progress stalled",
            next_input: None,
            final_response: Some(compose_stall_report()),
        },
        StopBlocked { reason } => GateDecision {
            status: Blocked,
            summary: format!("blocked: {reason}"),
            next_input: None,
            final_response: Some(compose_block_report()),
        },
    }
}
```

**Boundary rule:** IterationDecision is the domain's complete truth.
GateDecision is a lossy projection for the scheduler. Information flows
one way: domain -> scheduler, never scheduler -> domain. If the scheduler
needs to communicate back (e.g., cancellation), it uses the existing
`is_cancelled` flag on SchedulerProfileState, which the WorkflowController
observes.

**Bounded mode additional check:** When `iteration >= maxIterations` and
the decision is Continue-class, the WorkflowController overrides the
gate to Done with a "max iterations reached" summary. This check happens
in the mapping function, not in the scheduler.

---

## MetricSample

A single metric measurement with provenance.

```
MetricSample {
    value:              f64,
    iteration:          u32,         // 0 for baseline
    captured_at:        Timestamp,
    command_output:     String,      // raw stdout from verify command
    exit_code:          i32,
}
```

---

## MetricHistory

Tracks the metric trajectory across the entire run.
Owned by ObjectiveEvaluator. Updated after every verify execution.

```
MetricHistory {
    baseline:           Option<MetricSample>,   // captured once before first iteration
    best:               Option<MetricSample>,   // best value seen so far
    current:            Option<MetricSample>,   // most recent measurement
    samples:            Vec<MetricSample>,      // all measurements in order
    direction:          MetricDirection,        // from config: higher-is-better or lower-is-better
}
```

### MetricDirection

```
enum MetricDirection {
    HigherIsBetter,
    LowerIsBetter,
}
```

### Comparison methods

```
impl MetricHistory {
    /// Is the current sample better than baseline?
    fn improved_over_baseline(&self) -> bool;

    /// Is the current sample better than best-so-far?
    fn is_new_best(&self) -> bool;

    /// Absolute delta: current - baseline
    fn delta_from_baseline(&self) -> Option<f64>;

    /// Absolute delta: current - previous
    fn delta_from_previous(&self) -> Option<f64>;

    /// Is the objective satisfied per satisfiedWhen config?
    fn objective_satisfied(&self, threshold: &SatisfiedWhen) -> bool;
}
```

---

## VerificationResult

Outcome of running a verify or guard command.

```
VerificationResult {
    kind:               VerificationKind,   // Verify or Guard
    passed:             bool,               // exit code == 0
    exit_code:          i32,
    stdout:             String,
    stderr:             String,
    duration_ms:        u64,
    timed_out:          bool,
    metric_extracted:   Option<f64>,         // only for Verify, not Guard
}
```

### VerificationKind

```
enum VerificationKind {
    Verify,     // produces metric, exit code indicates success
    Guard,      // pass/fail only, no metric extraction
}
```

---

## WorkspaceCheckpoint

A restorable snapshot of the workspace state at a point in time.
Created by the SnapshotEngine before each iteration begins.

```
WorkspaceCheckpoint {
    checkpoint_id:      CheckpointId,       // unique, generated at creation
    iteration:          u32,                // which iteration this precedes
    strategy:           SnapshotStrategy,   // how the snapshot was taken
    ref_data:           CheckpointRefData,  // strategy-specific restoration data
    created_at:         Timestamp,
    status:             CheckpointStatus,
}
```

### SnapshotStrategy

```
enum SnapshotStrategy {
    GitBranchPerIteration,   // creates branch: autoresearch/{run_id}/iter-{n}
    GitStashStack,           // git stash with message
    PatchFile,               // git diff > patch file
    WorktreeFork,            // git worktree in .rocode/worktrees/
}
```

### CheckpointRefData

```
enum CheckpointRefData {
    GitBranch {
        branch_name: String,
        commit_sha: String,
    },
    GitStash {
        stash_index: u32,
        stash_ref: String,
    },
    PatchFile {
        patch_path: FilePath,
        base_commit: String,
    },
    Worktree {
        worktree_path: FilePath,
        branch_name: String,
    },
}
```

### CheckpointStatus

```
enum CheckpointStatus {
    Active,     // checkpoint exists and can be restored
    Restored,   // checkpoint was restored (revert happened)
    Released,   // checkpoint was released (iteration kept, no longer needed)
    Orphaned,   // checkpoint exists but run was interrupted; needs cleanup
}
```

**Lifecycle symmetry (Article 7):** every `Active` checkpoint must transition
to exactly one of `Restored`, `Released`, or `Orphaned`. `Orphaned` is handled
by the cleanup pass during `Initializing` (recover from previous interrupted run)
or during explicit cleanup.

---

## IterationLedger

The append-only collection of all IterationRecords for a run.
Owned by the IterationLedger service.

```
IterationLedger {
    records:            Vec<IterationRecord>,   // append-only
    run_id:             RunId,
}
```

### Query methods

```
impl IterationLedger {
    /// Total iterations attempted
    fn count(&self) -> u32;

    /// Count by decision kind
    fn count_kept(&self) -> u32;
    fn count_discarded(&self) -> u32;
    fn count_crashed(&self) -> u32;

    /// Last N records
    fn recent(&self, n: usize) -> &[IterationRecord];

    /// Consecutive discards from the tail
    fn consecutive_discards(&self) -> u32;

    /// All records with Keep decision
    fn kept_records(&self) -> Vec<&IterationRecord>;

    /// Best iteration by metric value
    fn best_iteration(&self) -> Option<&IterationRecord>;

    /// Serialize to TSV or JSONL for artifact persistence
    fn serialize(&self, format: LedgerFormat) -> String;
}
```

---

## RunSummary

Generated at the end of a run during the `Completing` phase.
Contains the final report suitable for display, artifact persistence,
and synthesis stage input.

```
RunSummary {
    run_id:             RunId,
    mode:               WorkflowMode,
    status:             RunStatus,       // Completed, Interrupted, or Failed
    total_iterations:   u32,
    kept:               u32,
    discarded:          u32,
    crashed:            u32,
    baseline_metric:    Option<f64>,
    final_metric:       Option<f64>,
    best_metric:        Option<f64>,
    best_iteration:     Option<u32>,
    total_duration_ms:  u64,
    artifact_paths:     Vec<FilePath>,
    stop_reason:        StopReason,
}
```

### StopReason

```
enum StopReason {
    ObjectiveSatisfied,
    MaxIterationsReached,
    ProgressStalled,
    Blocked { reason: String },
    ManualInterrupt,
    UnrecoverableError { error: String },
}
```

### Display format

```
=== Autoresearch Complete ({mode}, {kept+discarded+crashed}/{max or "unbounded"} iterations) ===
Baseline: {baseline} -> Final: {final} ({delta})
Best: {best} at iteration #{best_iter} -- {best_description}
Keeps: {kept} | Discards: {discarded} | Crashes: {crashed}
Stop reason: {stop_reason}
Duration: {duration}
Artifacts: {artifact_paths}
```

---

## Domain-Specific State Types

These types are populated only for their respective modes.
They extend RunState with mode-specific tracking.

### SecurityRunState

```
SecurityRunState {
    asset_inventory:    Vec<AssetEntry>,
    trust_boundaries:   Vec<TrustBoundary>,
    findings:           Vec<SecurityFinding>,
    coverage:           HashMap<CoverageTarget, CoverageStatus>,
}

SecurityFinding {
    id:                 FindingId,
    severity:           Severity,
    title:              String,
    description:        String,
    evidence:           FindingEvidence,
    status:             FindingStatus,   // Open, Accepted, Rejected, Fixed
}

FindingEvidence {
    file_path:          Option<FilePath>,
    line_range:         Option<(u32, u32)>,
    attack_scenario:    Option<String>,
    severity_justification: Option<String>,
    cwe_id:             Option<String>,
}
```

### DebugRunState

```
DebugRunState {
    symptom:            String,
    hypotheses:         Vec<Hypothesis>,
    experiments:        Vec<Experiment>,
    root_cause:         Option<RootCause>,
}

Hypothesis {
    id:                 HypothesisId,
    statement:          String,        // falsifiable claim
    status:             HypothesisStatus,  // Proposed, Testing, Confirmed, Disproven
    evidence_for:       Vec<String>,
    evidence_against:   Vec<String>,
}

Experiment {
    id:                 ExperimentId,
    hypothesis_id:      HypothesisId,
    description:        String,
    result:             ExperimentResult,
    files_examined:     Vec<FilePath>,
}
```

### FixRunState

```
FixRunState {
    broken_state:       HashMap<FixCategory, u32>,   // category -> error count
    initial_state:      HashMap<FixCategory, u32>,   // captured at baseline
    fix_history:        Vec<FixAttempt>,
}

FixAttempt {
    iteration:          u32,
    category:           FixCategory,
    errors_before:      u32,
    errors_after:       u32,
    delta:              i32,
    kept:               bool,
}
```

### ShipRunState

```
ShipRunState {
    shipment_type:      ShipmentType,
    checklist:          Vec<ChecklistItem>,
    dry_run_result:     Option<DryRunResult>,
    ship_result:        Option<ShipResult>,
    monitor_status:     Option<MonitorStatus>,
    rollback_available: bool,
}

ChecklistItem {
    id:                 String,
    description:        String,
    status:             ChecklistStatus,  // Pending, Passed, Failed, Blocked
    evidence:           Option<String>,
    blocking:           bool,
}
```

---

## State Ownership Map

Every field in RunState has exactly one service that may write to it (Article 5).

| Field | Write Owner | Read Access |
|-------|-------------|-------------|
| `status` | WorkflowController | all services |
| `baseline` | ObjectiveEvaluator | DecisionPolicy, IterationLedger |
| `metric_history` | ObjectiveEvaluator | DecisionPolicy, WorkflowController |
| `current_iteration` | WorkflowController | all services |
| `ledger` | IterationLedger | WorkflowController, ArtifactWriter |
| `active_checkpoint` | SnapshotEngine | WorkflowController |
| `stuck_counter` | WorkflowController | DecisionPolicy |
| `security / debug / fix / ship` | respective WorkflowController | ArtifactWriter |

No service writes to a field it does not own. Cross-service communication
happens through the WorkflowController, which orchestrates the iteration
lifecycle and passes owned references to each service at the appropriate phase.

---

## Persistence Contract

State is volatile (in-memory) during a run. Persistence is handled by the
ArtifactWriter service at two points:

1. **After each iteration:** append IterationRecord to the iteration log file.
2. **On run completion:** write RunSummary and domain-specific artifacts.

If the process is interrupted between persistence points, the IterationLedger
service recovers by:

1. Reading the last persisted iteration log.
2. Scanning for orphaned WorkspaceCheckpoints.
3. Reconstructing RunState from the log + current workspace state.

This is not full crash recovery (that requires the SnapshotEngine to have
clean checkpoints). It is best-effort reconstruction for resumability.
