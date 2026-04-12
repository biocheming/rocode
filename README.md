# RustingOpenCode (ROCode)

**A Rusted OpenCode Version**

RustingOpenCode（简称 ROCode）是 OpenCode 的 Rust 实现与演进版本，提供完整的 CLI / TUI / Server 工作流，用于本地 AI 编码代理、会话管理、工具调用、MCP/LSP 集成和插件扩展。

## 当前状态

- 软件名：`RustingOpenCode` / `ROCode`
- 版本标识：`v2026.4.12`
- 可执行命令：`rocode`
- 公共 scheduler presets：`sisyphus` / `prometheus` / `atlas` / `hephaestus`

## 功能概览

- 交互模式：TUI（默认）、CLI 单次运行、HTTP 服务、Web / ACP 模式
- 会话能力：创建 / 继续 / 分叉会话，导入导出会话
- 工具系统：内置读写编辑、Shell、补丁、提问等工具链
- 模型体系：多 Provider 适配、Agent 模式切换
- 扩展能力：插件桥接（含 TS 插件）、MCP、LSP
- Scheduler：共享执行骨架 + public OMO-aligned presets
- 终端体验：增强排版、可折叠侧栏、代码高亮、路径补全、question 卡片
- `/autoresearch`：CLI / TUI / Web 统一命令入口，支持补参与验证驱动的迭代优化

## Scheduler 快览

当前公开的 scheduler 预设都运行在同一套共享骨架上：

- `sisyphus`：delegation-first / execution-oriented / single-loop execution orchestration
- `prometheus`：planning-first / interview → plan → review → handoff
- `atlas`：coordination-heavy / task-ledger-driven execution + synthesis
- `hephaestus`：autonomous deep worker / explore → plan → decide → execute → verify

最近交互收口的重点包括：

- `Prometheus` 的阻塞性 interview 问题应走正式 `question` 工具，而不是只写在 transcript 里
- `Atlas` 的 QA `Gate Decision` 是内部 gate rubric，不是用户问卷；只有真实用户决策阻塞才应发 question
- `Hephaestus` 使用更明确的 `3-Level Escalation Protocol`
- TUI 会把 scheduler stage transcript 投影到主 session，并对 `route` 阶段做可读摘要渲染
- `/autoresearch` 命令走统一的 command + scheduler 配置链路，CLI / TUI / Web 都使用同一套参数补全与 question 流程
- verification / reasoning / scheduler stage 的流式输出在三端保持同构，Web 与 TUI 会做更稳定的历史回放与增量合并

## TUI 交互说明

- 首轮 user turn 会显示精简后的 system prompt preview，而不是完整长 prompt
- scheduler stage transcript 会作为 assistant stage 消息投影到主 session 中
- 当 session 处于 `busy` 时，普通输入会排队；若当前 workflow 需要用户回答，应通过 `question` 卡片完成
- question 卡片在有选项时会自动追加 `Other (type your own)`，用于自定义答案
- slash command 弹窗不再要求所有命令都必须预注册；已注册命令继续提供提示，`/autoresearch` 这类仓库命令会进入统一补参与执行链路

## `/autoresearch` 快览

`/autoresearch` 不是一个单独的 mode，而是一个命令入口。它的职责是把“目标 + 参数 + scheduler 配置 + verify/guard”组织成一次迭代优化运行。

- CLI：`rocode run "/autoresearch ..."`
- TUI：输入 `/autoresearch`，按 question 卡片补齐缺失参数
- Web：在输入框直接发送 `/autoresearch`，缺失参数会走同一套 question UI

适用场景：

- 持续改进文档、教案、书稿、报告、代码质量基线
- 本地有明确 `verify` 指标与 `guard` 约束的优化回路
- 希望让 scheduler 自动执行“修改 → 验证 → 保留/丢弃”循环

不适用场景：

- 只有开放式聊天目标、没有可检验输出
- 尚未定义 `verify` / `guard` / metric 提取规则

## 快速开始

### 1. 环境要求

- Rust stable
- Cargo
- Git（建议）

### 2. 构建

```bash
cargo build -p rocode-cli
```

### 3. 查看帮助

```bash
./target/debug/rocode --help
```

或

```bash
cargo run -p rocode-cli -- --help
```

### 4. 启动方式

- 默认进入 TUI：

```bash
cargo run -p rocode-cli --
```

- 显式进入 TUI：

```bash
cargo run -p rocode-cli -- tui
```

- 非交互运行：

```bash
cargo run -p rocode-cli -- run "请检查这个仓库中的风险点"
```

- 启动 HTTP 服务：

```bash
cargo run -p rocode-cli -- serve --port 3000 --hostname 127.0.0.1
```

Web 前端入口说明：

- `/`：默认 React Web 首页
- `/web/*`：React Web 静态资源前缀

当前 Web 交互约定：

- 首页左侧为 workspace-scoped session tree，按 workspace 聚合 session
- 右侧为 workspace 文件树侧栏，可显隐
- session 列表只展示当前 workspace 下的会话，避免跨项目噪音
- 已移除 `/new`、`/web-next` 等过渡期入口，`/` 为唯一主入口

## CLI 命令总览

以下命令来自当前 `rocode --help`：

- `tui`：启动交互式终端界面
- `attach`：附加到已运行的服务
- `run`：单次消息运行
- `serve`：启动 HTTP 服务
- `web`：启动 headless 服务并打开 Web 界面
- `acp`：启动 ACP 服务
- `models`：查看可用模型
- `session`：会话管理
- `stats`：token / cost 统计
- `db`：数据库工具
- `config`：查看配置
- `auth`：凭据管理
- `agent`：Agent 管理
- `debug`：调试与排障
- `mcp`：MCP 管理
- `export` / `import`：会话导出导入
- `github` / `pr`：GitHub 相关能力
- `upgrade` / `uninstall`：升级与卸载
- `generate`：生成 OpenAPI 规范
- `version`：查看版本

常用帮助：

```bash
rocode tui --help
rocode run --help
rocode serve --help
rocode session --help
```

## 配置

项目配置会在以下路径中按优先级合并（向上查找）：

- `rocode.jsonc`
- `rocode.json`
- `.rocode/rocode.jsonc`
- `.rocode/rocode.json`

全局配置默认路径：

- Linux / macOS：`~/.config/rocode/rocode.jsonc`（或 `.json`）

### `context_docs` 外部注册表

`context_docs` 的正式配置只在主配置中保留一个路径引用：

```jsonc
{
  "docs": {
    "contextDocsRegistryPath": "./docs/examples/context_docs/context-docs-registry.example.json"
  }
}
```

正式 schema 与示例入口：

- Registry schema：`docs/examples/context_docs/context-docs-registry.schema.json`
- Docs index schema：`docs/examples/context_docs/context-docs-index.schema.json`
- Registry example：`docs/examples/context_docs/context-docs-registry.example.json`
- Docs index example：`docs/examples/context_docs/react-router.docs-index.example.json`
- Secondary docs index example：`docs/examples/context_docs/tokio.docs-index.example.json`
- 说明文档：`docs/examples/context_docs/README.md`

只读校验命令：

```bash
rocode debug docs validate
rocode debug docs validate --registry ./docs/examples/context_docs/context-docs-registry.example.json
rocode debug docs validate --index ./docs/examples/context_docs/react-router.docs-index.example.json
```

## 仓库结构（模块与功能）

- `crates/rocode-cli`：CLI 入口与子命令编排（binary: `rocode`）
- `crates/rocode-tui`：终端交互界面与渲染状态机
- `crates/rocode-server`：HTTP / SSE / WebSocket API 与路由聚合
- `crates/rocode-session`：会话引擎、提示词主循环、工具回合控制
- `crates/rocode-agent`：Agent 注册、执行与消息封装
- `crates/rocode-orchestrator`：对话编排层（多步执行与 tool 调度）
- `crates/rocode-tool`：工具注册中心与内置工具实现
- `crates/rocode-permission`：权限规则与 `allow/deny/ask` 决策
- `crates/rocode-provider`：多 Provider 协议适配与流式解析
- `crates/rocode-storage`：SQLite 持久化与仓储层
- `crates/rocode-config`：配置发现、解析与合并
- `crates/rocode-plugin`：Hook 系统与 TS 子进程桥接
- `crates/rocode-command`：Slash Command 系统
- `crates/rocode-mcp`：MCP 客户端、OAuth 与传输抽象
- `crates/rocode-lsp`：LSP 客户端与注册表
- `crates/rocode-watcher`：文件监听与变更事件广播
- `crates/rocode-grep`：代码 / 文本搜索封装
- `crates/rocode-core`：基础能力（总线、ID、进程注册）
- `crates/rocode-types`：跨 crate 共享类型
- `crates/rocode-util`：文件、日志、JSON-ish 等通用能力

## 开发与验证

```bash
cargo fmt
cargo check
cargo clippy --workspace --all-targets
bash scripts/check_runtime_governance.sh
```

最小验证（常用）：

```bash
cargo check -p rocode-cli
cargo check -p rocode-tui
cargo check -p rocode-orchestrator
```

## 文档导航

当前仓库中实际维护中的入口文档：

- 用户指南：`USER_GUIDE.md`
- 文档索引：`docs/README.md`
- Scheduler 示例与说明：`docs/examples/scheduler/README.md`
- `/autoresearch` 统一实现设计：`docs/autoresearch-command-unified-design.md`
- Context Docs 示例与 schema：`docs/examples/context_docs/README.md`
- 插件 / skill / Rust 扩展示例：`docs/plugins_example/README.md`

## 说明

- 当前默认命令名为 `rocode`
- 公开 scheduler 只包含 4 个 presets：`sisyphus` / `prometheus` / `atlas` / `hephaestus`
