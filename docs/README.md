# ROCode Docs

文档基线：`v2026.4.12`（更新日期：`2026-04-12`）

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

## Web Frontend Entry

当前默认 Web 前端是 `crates/rocode-server/web`（React 版本）：

- `/` 是正式 Web 入口
- `/web/*` 是正式静态资源前缀
- `crates/rocode-server/web-ui` 已从主线构建中清理

## Skill Hub CLI

远程 skill distribution / artifact cache / managed lifecycle 的正式 CLI 入口现在是：

```bash
rocode skill hub status
rocode skill hub distributions
rocode skill hub artifact-cache
rocode skill hub policy
rocode skill hub lifecycle
rocode skill hub install-plan --source-id <id> --source-kind registry --locator <locator> --skill-name <name>
rocode skill hub install-apply --session-id <session> --source-id <id> --source-kind registry --locator <locator> --skill-name <name>
rocode skill hub update-apply --session-id <session> --source-id <id> --source-kind registry --locator <locator> --skill-name <name>
rocode skill hub detach --session-id <session> --source-id <id> --source-kind registry --locator <locator> --skill-name <name>
rocode skill hub remove --session-id <session> --source-id <id> --source-kind registry --locator <locator> --skill-name <name>
```

所有读写命令都通过 `rocode-server` 的 `/skill/hub/*` 路由进入 authority，不在 CLI 侧直接执行副作用。

## Skill Hub Policy

第三卷 phase 7 的 artifact policy 通过唯一配置真相 `skills.hub` 提供，authority 会把当前生效值暴露到 `/skill/hub/policy`，CLI/TUI/Web 都应读取这一正式读面，而不是各端自己解析配置文件。

`rocode.jsonc` 示例：

```jsonc
{
  "skills": {
    "hub": {
      "artifactCacheRetentionSeconds": 604800,
      "fetchTimeoutMs": 30000,
      "maxDownloadBytes": 8388608,
      "maxExtractBytes": 8388608
    }
  }
}
```
