# ROCode Docs

文档基线：`v2026.3.15`（更新日期：`2026-03-15`）

This directory contains product-facing examples and design references for ROCode features.

## 当前文档入口

- `README.md`
  - 项目总览、启动方式、当前公开能力范围
- `USER_GUIDE.md`
  - 面向使用者的命令、scheduler、TUI 交互说明
- `docs/examples/scheduler/README.md`
  - public scheduler presets、stage 默认值、当前行为说明
- `docs/examples/scheduler/SCHEDULER_GUIDE.md`
  - Scheduler 完整使用指南（Tutorial & User Guide）
- `docs/examples/context_docs/README.md`
  - `context_docs` schema、registry、index 示例
- `docs/plugins_example/README.md`
  - Skill / TS plugin / Rust 扩展示例

## Examples

- `examples/context_docs/`
  - Formal examples for `context_docs`
  - Includes minimal `rocode.json` / `rocode.jsonc` config samples
  - Includes `context-docs-registry` schema and example
  - Includes `context-docs-index` schema and example docs index
- `examples/scheduler/`
  - Formal external scheduler profile examples for the public OMO-aligned presets: `sisyphus`, `prometheus`, `atlas`, and `hephaestus`
  - Includes generic scheduler JSON Schema and current public example profiles
- `plugins_example/`
  - Skill / TS plugin / Rust extension examples

## Plans

- `plans/`
  - Design notes and architecture plans
  - Use these as implementation references, not as runtime config files
- `docs/plans/README.md`
  - 架构计划入口
- `docs/plans/rocode-app-blueprint.md`
  - `rocode-app` desktop-first 原生壳蓝图

## Context Docs Entry

The canonical entry for `context_docs` examples is:

- `docs/examples/context_docs/README.md`
- `docs/examples/context_docs/context-docs-registry.schema.json`
- `docs/examples/context_docs/context-docs-index.schema.json`
- `docs/examples/context_docs/context-docs-registry.example.json`
- `docs/examples/context_docs/react-router.docs-index.example.json`
- `docs/examples/context_docs/tokio.docs-index.example.json`

The canonical schema IDs are:

- `https://rocode.dev/schemas/context-docs-registry.schema.json`
- `https://rocode.dev/schemas/context-docs-index.schema.json`

Read-only validation entry:

```bash
rocode debug docs validate
rocode debug docs validate --registry ./docs/examples/context_docs/context-docs-registry.example.json
rocode debug docs validate --index ./docs/examples/context_docs/react-router.docs-index.example.json
```

## Scheduler Entry

The canonical scheduler example entry is:

- `docs/examples/scheduler/README.md`
- `docs/examples/scheduler/scheduler-profile.schema.json`
- `docs/examples/scheduler/sisyphus.example.jsonc`
- `docs/examples/scheduler/prometheus.example.jsonc`
- `docs/examples/scheduler/atlas.example.jsonc`
- `docs/examples/scheduler/hephaestus.example.jsonc`

The public scheduler presets are:

- `sisyphus`
- `prometheus`
- `atlas`
- `hephaestus`

The current schema IDs are:

- `https://rocode.dev/schemas/scheduler-profile.schema.json`
