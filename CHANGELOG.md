# Changelog

## 2026.4.17

- 完成 TUI reratui 迁移主线收口：Phase 0-5 已按当前 hybrid app shell 边界结束，session subtree、消息渲染与热点交互已进入稳定态。
- 大幅更新 Web 界面：统一消息阅读节奏、收紧 sidebar / composer / header 密度、补齐更轻的 copy/footer 语法，并把 tool / status / structured block 纳入统一显示体系。
- Web composer 新增可检索 model picker，按 provider 分组展示模型、上下文窗口与能力 badge；输入框改为单行起始、最多 10 行增长。
- Web sidebar 新增 session 多选、批量删除与确认弹层，减少误删并提升会话管理效率。
- provider 模型读面补齐 capabilities，下游 Web/TUI 可直接消费视觉、音频、PDF、附件、tool-call、reasoning 等能力信息。
- 文档与计划同步到 `v2026.4.17`，移除旧的假入口，并把当前 TUI/Web 状态改为与实现一致的描述。
