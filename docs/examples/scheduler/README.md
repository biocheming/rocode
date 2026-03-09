# Scheduler Examples

This directory contains formal external scheduler profile examples for ROCode.

## Files

### Public OMO examples

- `scheduler-profile.schema.json`
  - Formal schema for the generic scheduler profile file
  - Public orchestrator surface: `sisyphus`, `prometheus`, `atlas`, `hephaestus`
  - Supported orchestrators are limited to the public OMO-aligned presets
- `sisyphus.example.jsonc`
  - Public OMO-aligned delegation-first example
- `prometheus.example.jsonc`
  - Public OMO-aligned planning-first example
- `atlas.example.jsonc`
  - Public OMO-aligned coordination example
- `hephaestus.example.jsonc`
  - Public OMO-aligned autonomous execution example

## Current Scope

These examples reflect the current implementation scope:

- external JSON / JSONC config parsing exists in `rocode-orchestrator`
- public preset profiles are:
  - `sisyphus`
  - `prometheus`
  - `atlas`
  - `hephaestus`
- named orchestrators are presets over the shared scheduler profile kernel, not separate execution engines
- `Sisyphus` currently defaults to stages:
  - `request-analysis`
  - `route`
  - `execution-orchestration`
- `Prometheus` currently defaults to stages:
  - `request-analysis`
  - `route`
  - `interview`
  - `plan`
  - `review`
  - `handoff`
- `Atlas` currently defaults to stages:
  - `request-analysis`
  - `execution-orchestration`
  - `synthesis`
- `Hephaestus` currently defaults to stages:
  - `request-analysis`
  - `execution-orchestration`

## Current Behavioral Notes

These public examples now assume the following runtime semantics:

- `Prometheus`
  - planner-only workflow
  - blocking interview questions should use the formal `question` tool / question-card flow
  - review stays enabled by default before handoff
- `Atlas`
  - coordination / delegation / verification preset
  - QA `Gate Decision` YES/NO checks are Atlas internal rubric, not a user questionnaire
  - use the `question` tool only for real user decision blockers, not for Atlas's own QA responsibility
- `Hephaestus`
  - autonomous deep-worker preset
  - failure recovery follows a clearer `3-Level Escalation Protocol`
- `Sisyphus`
  - execution-oriented single-loop preset
  - favors bounded execution with final delivery normalization rather than planner-style interview flow

These examples do not yet cover the full future scheduler system described in long-form plans.

## Intended Usage

These files are intended to be referenced externally by a future `schedulerPath` field in `rocode.json` / `rocode.jsonc`.

Example:

```jsonc
{
  "schedulerPath": "./docs/examples/scheduler/sisyphus.example.jsonc"
}
```

## Validation

The checked-in public examples should stay aligned with the scheduler runtime authority:

- they should parse through `SchedulerConfig::load_from_file(...)`
- their default profile should resolve successfully
- their `orchestrator` and `stages` should match the corresponding public preset defaults in code
