# Autoresearch On ROCode

This document describes what a serious `autoresearch` implementation on ROCode would require.

The paired example file, `autoresearch.example.jsonc`, is intentionally constrained to the
current scheduler schema so it can be parsed today. That file should be read as an
orchestration draft, not as feature parity.

## What ROCode Already Has

ROCode already has enough scheduler surface to express the outer orchestration shape:

- named profiles in one JSONC file
- preset-backed execution modes such as `prometheus`, `atlas`, and `hephaestus`
- configurable stage sequences
- per-stage `toolPolicy`, `loopBudget`, `childSession`, `agentTree`, `skillList`
- agent trees for role specialization
- skill-tree text for protocol framing

That is enough to sketch:

- a planning profile
- an autonomous execution profile
- a coordination-heavy security profile
- specialized debug, fix, and ship postures

It is not enough to encode the actual `autoresearch` contract.

## Current Implementation Mismatch

One concrete mismatch should be fixed before anyone treats these examples as authoritative:

- the public scheduler docs and JSON schema expose `childSession`
- the config loader's `SchedulerStageOverride` struct does not currently deserialize that field
- the stage-policy merge path only applies `toolPolicy`, `loopBudget`, and `sessionProjection`

That means a profile can mention `childSession` today and still parse, but the override is not
actually carried into the stage override config path. This is not an `autoresearch`-specific gap,
it is an existing scheduler implementation inconsistency that should be corrected independently.

## Full-Parity Target

For this design, "full parity" means ROCode can express and execute the core
`autoresearch` contract without burying important semantics inside free-form prompts.

That contract includes:

- objective-driven iteration
- mechanical verification
- baseline capture
- keep versus discard decisions
- reversible mutation handling
- structured logs and artifacts
- domain-specific workflows for `plan`, `security`, `debug`, `fix`, and `ship`

## Schema Additions

The current scheduler schema should be extended with first-class workflow fields.

### 1. Objective Contract

Need a structured way to define the thing being optimized.

Suggested shape:

```jsonc
"objective": {
  "goal": "Increase test coverage to 90%",
  "scope": {
    "include": ["src/**/*.ts", "tests/**/*.ts"],
    "exclude": ["generated/**"]
  },
  "direction": "higher-is-better",
  "metric": {
    "kind": "numeric-command",
    "command": "npm test -- --coverage | ./scripts/extract_coverage.sh",
    "parser": {
      "type": "regex-capture",
      "pattern": "All files[^0-9]+([0-9]+\\.[0-9]+)"
    },
    "unit": "percent"
  },
  "verify": {
    "command": "npm test -- --coverage",
    "timeoutMs": 600000
  },
  "guard": {
    "command": "npm run lint",
    "timeoutMs": 300000,
    "required": true
  }
}
```

Why it matters:

- `goal`, `metric`, `verify`, and `guard` are not scheduler trivia
- they are the control variables for the loop
- hiding them in prompt prose makes them non-authoritative and non-inspectable

### 2. Loop Semantics

`loopBudget` only caps the model loop. It does not define the workflow loop.

Need first-class workflow controls:

```jsonc
"iterationPolicy": {
  "mode": "unbounded",
  "maxIterations": null,
  "stopWhen": [
    "objective-satisfied",
    "no-progress-after:5",
    "error-count-zero"
  ],
  "progressReportEvery": 10
}
```

Why it matters:

- `autoresearch` loops over verified state transitions, not just token turns
- some workflows terminate on success, others run until interrupted
- this should not be inferred from prompt text

### 3. Keep Or Discard Policy

Need explicit commit and revert semantics.

Suggested shape:

```jsonc
"decisionPolicy": {
  "baselineStrategy": "capture-before-first-iteration",
  "keepWhen": [
    "metric-improved",
    "verify-passed",
    "guard-passed"
  ],
  "discardWhen": [
    "metric-regressed",
    "verify-failed",
    "guard-failed"
  ],
  "tieBreaker": "keep-if-non-regression",
  "maxReworkAttempts": 2
}
```

Why it matters:

- the core `autoresearch` promise is not just "try things"
- it is "keep only validated improvements"
- scheduler today has no structured decision layer for that

### 4. State Snapshot And Revert Contract

Need authoritative mutation boundaries.

Suggested shape:

```jsonc
"workspacePolicy": {
  "mutationMode": "tracked",
  "protectedPaths": ["tests/golden/**", "vendor/**"],
  "revertStrategy": "git-commit-or-patch-snapshot",
  "commitStrategy": {
    "enabled": true,
    "messageTemplate": "autoresearch(iteration:{iteration}): {summary}"
  }
}
```

Why it matters:

- full `autoresearch` depends on being able to discard bad iterations cleanly
- this needs a real runtime contract, not a best-effort prompt habit

### 5. Artifact And Logging Contract

Need a schema for reports, iteration ledgers, and workflow outputs.

Suggested shape:

```jsonc
"artifacts": {
  "rootDir": ".rocode/autoresearch",
  "iterationLog": {
    "format": "tsv",
    "path": "runs/{date}-{slug}/results.tsv"
  },
  "summary": {
    "format": "markdown",
    "path": "runs/{date}-{slug}/summary.md"
  }
}
```

For domain workflows:

- `security` needs threat model, findings, coverage, recommendations
- `debug` needs symptom log, hypothesis log, experiment log, findings
- `ship` needs checklist, dry-run log, ship log, monitor log
- `fix` needs broken-state counts and per-iteration deltas

### 6. Domain Workflow Kind

Need a first-class workflow discriminator.

Suggested shape:

```jsonc
"workflow": {
  "kind": "autoresearch",
  "mode": "debug"
}
```

Supported `mode` values should include:

- `run`
- `plan`
- `security`
- `debug`
- `fix`
- `ship`

Why it matters:

- each mode has different stop conditions, artifacts, prompts, and gate logic
- that distinction should live in structured config and runtime hooks

### 7. Domain-Specific Schema Blocks

Each mode needs more than prompts.

Suggested blocks:

```jsonc
"security": {
  "coverageTargets": ["owasp-top-10", "stride"],
  "failOnSeverity": "critical",
  "diffMode": false,
  "autoFix": false
},
"debug": {
  "symptom": "API returns 500 on POST /users",
  "minSeverity": "medium",
  "requiredEvidence": ["file-line", "repro-steps"]
},
"fix": {
  "target": "auto-detect",
  "categories": ["test", "type", "lint", "build"],
  "fromDebug": false
},
"ship": {
  "type": "deployment",
  "dryRun": true,
  "autoApprove": false,
  "rollbackEnabled": true,
  "monitorMinutes": 10
}
```

Why it matters:

- today these semantics would be smuggled in through comments or prompts
- they should be machine-visible and govern runtime behavior

### 8. Approval And Side-Effect Policy

Shipping and auto-fix workflows need stronger approval semantics.

Suggested shape:

```jsonc
"approvalPolicy": {
  "requireHumanApprovalFor": [
    "ship-action",
    "rollback-action",
    "external-write"
  ],
  "allowAutoApproveWhen": [
    "dry-run-passed",
    "no-blockers"
  ]
}
```

Why it matters:

- `autoresearch:ship` can cross from repo edits into real external side effects
- scheduler today does not distinguish "changed files" from "deployed to prod"

## Runtime Additions

Schema alone is not enough. ROCode also needs runtime machinery that enforces the contract.

### 1. Objective Evaluator

Need a runtime service that can:

- run metric commands
- parse numeric values from stdout or structured output
- compare current value against baseline and best-so-far
- classify results as improved, regressed, unchanged, invalid

This evaluator should be authoritative for keep/discard decisions.

### 2. Verification Runner

Need a dedicated verifier layer, not just ad hoc `bash`.

It should support:

- verify command execution
- guard command execution
- timeouts
- retry policy
- stdout and stderr capture
- structured pass or fail records

### 3. Baseline And Iteration Ledger

Need a persistent run state model with:

- run metadata
- baseline metric
- current best metric
- iteration count
- per-iteration decision
- changed files
- verify result
- guard result
- artifact paths

This should be queryable by CLI, TUI, and web UI.

### 4. Snapshot, Commit, And Revert Engine

Need authoritative mutation management:

- create baseline snapshot before first mutation
- create per-iteration snapshots or commits
- revert rejected iterations cleanly
- recover after crashes
- surface which files were reverted and why

This is central to `autoresearch`. Without it, "discard" is not real.

### 5. Workflow-Specific Controllers

Each subcommand needs its own runtime controller.

#### `plan`

Need:

- scope discovery
- scope validation
- metric construction help
- verify dry-run
- launchable config emission

#### `security`

Need:

- asset inventory builder
- trust-boundary mapper
- STRIDE coverage tracker
- OWASP coverage tracker
- severity model
- findings registry with file-line evidence

#### `debug`

Need:

- symptom intake
- hypothesis registry
- experiment runner
- finding classifier
- repro step capture

#### `fix`

Need:

- broken-state detector across tests, types, lint, build
- category prioritizer
- error-count delta comparator
- stop-on-zero controller

#### `ship`

Need:

- shipment type detection
- checklist generator
- dry-run adapter
- side-effect executor
- rollback adapter
- monitor loop

### 6. Artifact Writers

Need a consistent artifact subsystem for:

- TSV iteration logs
- markdown summaries
- threat-model docs
- findings reports
- checklist docs
- debug hypothesis and experiment logs

These artifacts should not be emitted as free-form assistant text only.

### 7. Runtime Gates

Need hard gates, not just polite instructions.

Examples:

- block `ship` if checklist has blockers
- block `keep` if guard failed
- block `security` finding acceptance without evidence fields
- block `debug` finding acceptance without reproduction notes

### 8. Crash Recovery

Need formal recovery semantics for:

- syntax errors introduced by the agent
- hung verify commands
- exhausted resources
- interrupted runs
- partially completed ship actions

Recovery needs stateful runtime support, not just prompt advice.

### 9. Session Projection And UI

Need first-class UX for run state:

- baseline and best metric
- iteration number
- current objective
- verify and guard status
- keep or discard decision
- run summary
- artifact links

Current stage projection is useful, but it is not the same thing as an `autoresearch` dashboard.

### 10. Public API Surface

Need stable API and CLI surfaces for:

- launching a run from objective config
- resuming an interrupted run
- inspecting iteration history
- exporting artifacts
- switching from `debug` output into `fix --from-debug`

This should be an explicit product surface, not a prompt convention.

## Recommended Implementation Shape

If ROCode wants a serious implementation, the clean split is:

1. Scheduler profile decides orchestration posture.
2. Workflow schema defines objective and mode-specific contracts.
3. Runtime controllers enforce loop semantics and write artifacts.
4. UI layers project run state and decisions.

In that model:

- `prometheus` is the best fit for `autoresearch:plan`
- `hephaestus` is the best fit for `autoresearch:run`, `debug`, and `fix`
- `atlas` is the best fit for `security` and parts of `ship`

## Suggested Follow-On Files

If this draft moves forward, the next serious documents should be:

- `workflow-autoresearch.schema.json`
- `AUTORESEARCH_RUNTIME.md`
- `AUTORESEARCH_API.md`
- `AUTORESEARCH_ARTIFACTS.md`

Those files would turn this from a prompt-level imitation into a real runtime feature.
