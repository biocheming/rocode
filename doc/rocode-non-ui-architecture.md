# ROCode 非 UI 架构与完整工作流详解

> 结论先行：**ROCode 的核心不是某个前端界面，而是一套“以 server 为统一入口、以 session/orchestrator 为执行内核、以 tool/provider/plugin/MCP 为扩展底座”的运行时系统。**
>
> 本文只分析 **非 UI 架构**：CLI 入口、Server、Session 引擎、Scheduler、Agent、Tool、Provider、Config、Plugin、MCP、Storage、Runtime 状态与端到端执行流程。
>
> **明确排除**：TUI 渲染、Web 页面、样式、前端组件、页面路由、交互控件本身的实现细节。本文只在必要时把它们当成“同一后端的适配器/消费者”一笔带过。

---

## 0. 调查范围与依据

本分析基于仓库当前源码，重点阅读了以下文件与文档：

- 入口与 CLI
  - `crates/rocode-cli/src/main.rs`
  - `crates/rocode-cli/src/run.rs`
  - `crates/rocode-cli/src/server.rs`
  - `crates/rocode-cli/src/server_lifecycle.rs`
  - `crates/rocode-cli/src/api_client.rs`
- Server 与路由
  - `crates/rocode-server/src/server.rs`
  - `crates/rocode-server/src/routes/mod.rs`
  - `crates/rocode-server/src/routes/session.rs`
  - `crates/rocode-server/src/routes/session/prompt.rs`
  - `crates/rocode-server/src/routes/session/scheduler.rs`
  - `crates/rocode-server/src/runtime_control.rs`
  - `crates/rocode-server/src/session_runtime/mod.rs`
  - `crates/rocode-server/src/session_runtime/events.rs`
  - `crates/rocode-server/src/session_runtime/state.rs`
- Session / Orchestrator / Agent / Tool
  - `crates/rocode-session/src/session.rs`
  - `crates/rocode-session/src/prompt/mod.rs`
  - `crates/rocode-session/src/prompt/tools_and_output.rs`
  - `crates/rocode-orchestrator/src/request_execution.rs`
  - `crates/rocode-orchestrator/src/execution_resolver.rs`
  - `crates/rocode-orchestrator/src/runtime/loop_impl.rs`
  - `crates/rocode-orchestrator/src/runtime/traits.rs`
  - `crates/rocode-orchestrator/src/tool_runner.rs`
  - `crates/rocode-agent/src/agent/registry.rs`
  - `crates/rocode-agent/src/executor/mod.rs`
  - `crates/rocode-tool/src/lib.rs`
  - `crates/rocode-tool/src/registry.rs`
  - `crates/rocode-tool/src/task.rs`
  - `crates/rocode-tool/src/task_flow.rs`
  - `crates/rocode-tool/src/plan.rs`
- Config / Provider / Plugin / MCP / Storage
  - `crates/rocode-config/src/loader/mod.rs`
  - `crates/rocode-provider/src/provider.rs`
  - `crates/rocode-provider/src/bootstrap.rs`
  - `crates/rocode-plugin/src/lib.rs`
  - `crates/rocode-plugin/src/subprocess/loader.rs`
  - `crates/rocode-mcp/src/lib.rs`
  - `crates/rocode-mcp/src/transport.rs`
  - `crates/rocode-lsp/src/lib.rs`
  - `crates/rocode-storage/src/repository.rs`
- 辅助文档
  - `README.md`
  - `docs/README.md`
  - `docs/examples/scheduler/README.md`
  - `docs/session-message-storage.md`

---

## 1. 一句话架构图

```text
用户请求
  ↓
rocode CLI / 其他客户端
  ↓  (HTTP + SSE)
rocode-server
  ├─ ConfigStore / CategoryRegistry
  ├─ ProviderRegistry
  ├─ ToolRegistry
  ├─ SessionManager
  ├─ RuntimeStateStore
  ├─ RuntimeControlRegistry
  └─ SessionPrompt / Scheduler Orchestrator
         ↓
   统一执行内核
   ├─ 普通 prompt 路径：rocode-session::prompt::loop_inner
   └─ scheduler 路径：rocode-orchestrator scheduler kernel
         ↓
   rocode-orchestrator::runtime::run_loop
         ↓
   Provider.chat_stream  ↔  ToolDispatcher.execute
         ↓
   内置工具 / task 子代理 / plugin tool / MCP tool / LSP / shell 等
         ↓
   Session 更新 + Runtime 状态更新 + 执行拓扑更新 + SSE 广播 + SQLite 持久化
```

核心含义：

1. **UI 不是系统核心**，只是同一后端的不同入口。
2. **真正的执行权威**在 `rocode-server + rocode-session + rocode-orchestrator`。
3. **工具调用和子代理委派**不是附属功能，而是主循环中的一级公民。
4. **运行时状态、执行拓扑、持久化快照**是三套并行但互补的权威视图。

---

## 2. 模块分层：从 workspace 看 ROCode 的非 UI 核心

`Cargo.toml` 显示这是一个多 crate workspace。若只看非 UI 架构，最重要的层次如下。

### 2.1 入口与适配层

- `crates/rocode-cli`
  - 二进制入口 `rocode`
  - 负责参数解析、启动/发现 server、发 HTTP 请求、消费 SSE 事件
  - 非 UI 视角下，它是 **后端能力的命令行适配器**

### 2.2 服务编排层

- `crates/rocode-server`
  - 整个系统的运行时装配中心
  - 持有会话、provider、tool registry、runtime 状态、执行拓扑、storage repo、event bus
  - 对外暴露 `/session`、`/provider`、`/config`、`/mcp`、`/permission`、`/task` 等 HTTP API

### 2.3 会话执行层

- `crates/rocode-session`
  - 负责“普通 prompt 模式”的真实主循环
  - 管理 `Session`、消息、parts、增量更新、工具回合、上下文压缩、子任务处理

### 2.4 通用运行时内核层

- `crates/rocode-orchestrator`
  - 提供统一执行请求编译能力：
    `ExecutionResolutionContext -> ResolvedExecutionSpec -> CompiledExecutionRequest -> ChatRequest`
  - 提供统一 agentic run loop：模型流 → 工具调用归一化 → 工具执行 → 继续下一轮
  - 提供 scheduler 共享骨架与 preset/profile 机制

### 2.5 Agent / 委派层

- `crates/rocode-agent`
  - 管理 builtin agent + config agent
  - AgentExecutor 并不是另起一套执行器，而是复用 orchestrator 的同一个 `run_loop`
  - 说明 ROCode 的“代理”与“普通对话”底层是统一运行时，只是配置、权限和执行边界不同

### 2.6 Tool 层

- `crates/rocode-tool`
  - 工具注册中心与内置工具实现
  - 负责 schema 暴露、参数归一化、执行前后 plugin hook、错误改写
  - 包括 `read/write/edit/bash/glob/grep/task/task_flow/question/webfetch/todo/...`

### 2.7 配置与模型层

- `crates/rocode-config`
  - 配置发现、加载、合并、`.rocode` 目录扫描
- `crates/rocode-provider`
  - provider bootstrap、provider registry、模型解析、capability 合并

### 2.8 扩展与外部协议层

- `crates/rocode-plugin`
  - Hook 系统 + JS/TS 子进程插件加载器 + auth bridge
- `crates/rocode-mcp`
  - MCP client / OAuth / stdio-http-sse transport
- `crates/rocode-lsp`
  - LSP client 进程桥接

### 2.9 基础设施层

- `crates/rocode-storage`
  - SQLite/SeaORM repository
- `crates/rocode-core`
  - bus、ID、process registry、codec、contracts
- `crates/rocode-types`
  - 跨 crate 共享的稳定持久化/传输类型
- `crates/rocode-command`
  - slash command 与 stage protocol 等共享协议/命令能力

---

## 3. 先抓总纲：ROCode 的真正“核心对象”有哪些？

如果只看非 UI 架构，必须区分以下 6 个核心对象：

### 3.1 `SessionManager`：会话事实源

定义位置：`crates/rocode-server/src/server.rs`、`crates/rocode-session/src/session.rs`

- Server 中的 `ServerState.sessions: Mutex<SessionManager>` 是会话主工作集。
- `Session` 包含：
  - `id / directory / parent_id / title / status / metadata`
  - `messages: Vec<SessionMessage>`
- `SessionMessage` 不是大字符串，而是 **parts 列表**。

这意味着：

- ROCode 的会话模型天生适合流式输出、tool call、reasoning、file/patch/snapshot 等结构化内容。
- “一条 assistant 回答”本质上是很多 part 的累积，而不是一次性整块文本。

### 3.2 `RuntimeStateStore`：当前正在做什么

定义位置：`crates/rocode-server/src/session_runtime/state.rs`

这是 **当前运行态的权威投影**，而不是历史会话。

它维护：

- `run_status`: `idle/running/waiting_on_tool/waiting_on_user/cancelling`
- `current_message_id`
- `active_tools[]`
- `pending_question`
- `pending_permission`
- `child_sessions[]`

也就是说：

- 要知道“现在卡在哪个工具、是否在等用户点 permission/question”，应该看 `/runtime`，不是去猜历史消息。

### 3.3 `RuntimeControlRegistry`：执行拓扑权威

定义位置：`crates/rocode-server/src/runtime_control.rs`

它维护的是 **执行树/执行图**，而不是会话文本本身。

记录的节点类型包括：

- `PromptRun`
- `SchedulerRun`
- `SchedulerStage`
- `ToolCall`
- `AgentTask`
- `Question`

这个结构的作用是：

- 把“本次执行里有哪些 stage、哪些 tool call、哪些 agent task、父子关系是什么”表达成结构化拓扑。
- 供 CLI/TUI/Web 这类前端去渲染，但其语义权威在 server，不在 UI。

### 3.4 `ToolRegistry`：能力表 + 执行入口

定义位置：`crates/rocode-tool/src/registry.rs`

职责有两类：

1. **把工具暴露给模型**：输出 schema/description/parameters
2. **执行工具**：参数归一化、验证、plugin before/after hook、结果回写

### 3.5 `ProviderRegistry`：模型提供者事实源

定义位置：`crates/rocode-provider/src/provider.rs`

负责：

- provider 注册
- model → provider 归属解析
- `provider/model` 字符串解析
- provider 实例获取

### 3.6 `CompiledExecutionRequest`：请求级运行时契约

定义位置：

- `crates/rocode-orchestrator/src/request_execution.rs`
- `crates/rocode-orchestrator/src/execution_resolver.rs`

它把本次请求真正送给模型时所需的信息统一起来：

- `model_id`
- `max_tokens`
- `temperature`
- `top_p`
- `variant`
- `provider_options`

这是 ROCode 非常关键的设计点：

> **不让 CLI、Session、Scheduler、Subagent、Title 生成、Compaction 等路径各自拼请求。**
> 一旦进入 orchestrator 层，请求编译语义就统一了。

---

## 4. 启动流程：系统如何从 `rocode` 命令跑成一个可执行后端

### 4.1 二进制入口：`crates/rocode-cli/src/main.rs`

`main()` 做的事情很直接：

1. 初始化 tracing 日志
2. 启动后台 process reaper（`rocode_core::process_registry::global_registry().spawn_reaper(...)`）
3. 解析 CLI 参数 `Cli::parse()`
4. 按子命令分发到 `tui / attach / run / serve / web / acp / session / mcp / ...`

对非 UI 架构来说，重点是三条：

- `run`：单次执行入口
- `serve`：后端 HTTP 服务入口
- `web` / `tui`：虽然是 UI 入口，但底层依旧复用同一个 server

### 4.2 `run` 命令的本质：不是本地直接跑模型，而是优先走统一 server

关键文件：

- `crates/rocode-cli/src/run.rs`
- `crates/rocode-cli/src/server_lifecycle.rs`

`run_non_interactive(...)` 的关键逻辑：

1. 收集输入和附件
2. 若没有输入文本，则进入交互式 chat session（不展开 UI 细节）
3. 否则：
   - 若用户显式 `--attach`，直接连现有 server
   - 否则调用 `discover_or_start_server(None)`

`discover_or_start_server(...)` 的逻辑：

1. 先根据环境变量/默认端口解析 server URL
2. 先做 `/health` 探测
3. 若已有 server，则复用
4. 若没有，则 `tokio::spawn(async move { rocode_server::run_server(addr).await })`
5. 等待健康检查通过

这意味着：

> **CLI 在架构上不是独立执行器，而是同一后端 runtime 的轻量客户端。**

### 4.3 Server 启动：`rocode_server::run_server`

关键文件：`crates/rocode-server/src/server.rs`

`run_server(addr)` 做三件事：

1. 计算 server URL
2. 调用 `ServerState::new_with_storage_for_url(server_url).await?`
3. 用 `routes::router()` + CORS + TraceLayer 启动 axum

也就是说，**`ServerState` 才是 ROCode 核心后端的装配根对象**。

---

## 5. ServerState：ROCode 核心运行时装配中心

定义：`crates/rocode-server/src/server.rs`

```text
ServerState
├─ sessions: SessionManager
├─ providers: ProviderRegistry
├─ config_store: ConfigStore
├─ tool_registry: ToolRegistry
├─ prompt_runner: SessionPrompt
├─ runtime_control: RuntimeControlRegistry
├─ stage_event_log: StageEventLog
├─ auth_manager: AuthManager
├─ event_bus: broadcast::Sender<String>
├─ session_repo / message_repo / part_repo
├─ category_registry
├─ todo_manager
└─ runtime_state: RuntimeStateStore
```

### 5.1 `new_with_storage_for_url()` 的初始化顺序

这是理解系统启动的最关键函数之一。

其顺序基本是：

1. **加载认证信息**
   - `AuthManager::load_from_file(...)`

2. **加载配置**
   - `ConfigStore::from_project_dir(&cwd)`
   - 失败则回退到默认配置

3. **加载插件认证与插件运行时**
   - `load_plugin_auth_store(&server_url, auth_manager.clone(), &config_store).await`
   - 这里会初始化 `PluginLoader`、builtin auth plugin、配置插件、plugin hook 系统

4. **把 config provider 配置转换为 provider bootstrap 配置**
   - `convert_config_providers_for_bootstrap(...)`
   - `bootstrap_config_from_raw(...)`

5. **预热 models.dev 缓存**
   - `ModelsRegistry::default().get()`

6. **构造 ProviderRegistry**
   - `create_registry_from_bootstrap_config(&bootstrap_config, &auth_store)`

7. **加载任务 category registry**
   - 如果配置里有 `task_category_path` 就按文件加载，否则用 builtins

8. **构造 ToolRegistry**
   - `create_default_registry_with_config(Some(&config_store.config())).await`

9. **构造 SessionPrompt**
   - 挂上 `ToolRuntimeConfig`

10. **初始化数据库与仓储层**
    - `Database::new()`
    - `SessionRepository / MessageRepository / PartRepository`

11. **从 storage 把 session/message 灌回内存**
    - `load_sessions_from_storage().await?`

### 5.2 初始化顺序透露出的架构观念

这个顺序非常说明问题：

- **先有配置/认证/插件/provider/tool，再有 prompt 执行**
- **先构造运行时注册表，再恢复会话历史**
- **Server 把 DB 当持久化源，但把内存 SessionManager 当主工作集**

换句话说：

> ROCode 不是“数据库驱动型聊天服务”，而是“内存运行时 + 持久化快照”的本地代理系统。

---

## 6. 路由层：外部请求如何进入核心执行器

关键文件：

- `crates/rocode-server/src/routes/mod.rs`
- `crates/rocode-server/src/routes/session.rs`

### 6.1 顶层 router

`routes::router()` 注册了：

- `/event`：SSE 事件流
- `/command`：命令列表与 UI command 解析
- `/agent`：agent 列表
- `/mode`：执行模式
- `/skill`
- `/provider`
- `/config`
- `/mcp`
- `/permission`
- `/task`
- `/session`
- 以及若干文件、项目、进程、全局接口

从非 UI 角度看，最重要的是：

1. `/session/{id}/prompt`
2. `/event`
3. `/session/{id}/runtime`
4. `/session/{id}/executions`

### 6.2 `/session` 子路由

`session_routes()` 暴露的关键能力包括：

- session CRUD
- `/runtime`
- `/executions`
- `/message`
- `/prompt`
- `/prompt/abort`
- `/scheduler/stage/abort`
- `/events`
- `/recovery`

这说明 ROCode 把“会话、消息、运行时、执行树、恢复、调度阶段”都当作后端一等资源。

---

## 7. 统一配置解析：一次请求是如何决定 agent / scheduler / model / provider 的

这是整个系统最重要的“路由决策层”。

关键文件：

- `crates/rocode-server/src/routes/session/prompt.rs`
- `crates/rocode-server/src/routes/session/scheduler.rs`
- `crates/rocode-orchestrator/src/execution_resolver.rs`
- `crates/rocode-orchestrator/src/request_execution.rs`

### 7.1 请求入口：`POST /session/{id}/prompt`

`session_prompt(...)` 的处理顺序大致如下：

1. 校验 `agent` 与 `scheduler_profile` 互斥
2. 把输入统一成显示文本 `display_prompt_text`
3. 读取 session，拿到其 `directory`
4. `ensure_plugin_loader_active(&state)`
5. 调用 `resolve_prompt_payload(...)`
   - 如果是 `/command` 形式，则由 `CommandRegistry` 展开 slash command
6. 构造 **有效 agent / scheduler_profile**
7. 取得 **plugin-applied config snapshot**
8. 调用 `resolve_prompt_request_config(...)`
9. 拿到：
   - `scheduler_applied`
   - `scheduler_profile_name`
   - `resolved_agent`
   - `provider`
   - `provider_id`
   - `model_id`
   - `agent_system_prompt`
   - `compiled_request`
10. 开新异步任务真正执行 prompt

### 7.2 Slash command 不是 UI 特性，而是服务端语义扩展

`resolve_prompt_payload(...)` 会：

1. `CommandRegistry::new()` 注册 builtin commands
2. `load_from_directory(<session_directory>/.rocode/commands)` 加载项目命令
3. `parse(display_text)` 识别 `/xxx args`
4. `execute_with_hooks(...)` 展开为真正 prompt 文本

因此 `/commit`、`/review` 这类能力并不是前端快捷键，而是 **服务端 prompt 模板展开系统**。

### 7.3 `resolve_prompt_request_config(...)` 是本次请求的真正“决策中枢”

它会同时决策下面几件事：

### (1) 是否启用 scheduler

- `resolve_scheduler_request_defaults(config, requested_scheduler_profile)`
- 若命中 builtin preset 或外部 `schedulerPath` profile，则 `scheduler_applied = true`

### (2) 根 agent 如何选

优先级大致是：

1. 显式请求的 `agent`
2. scheduler 默认 root agent
3. config 默认 agent（默认 `build`）

### (3) 模型如何选

`resolve_request_model_inputs(...)` 会根据是否启用 scheduler 走不同优先级逻辑：

- scheduler 模式下，优先 profile/agent 对模型的控制
- 非 scheduler 模式下，优先 request model，再回退 agent/config model

### (4) provider / model 实例如何解出

通过 `resolve_provider_and_model(...)` 从 `ProviderRegistry` 找到：

- 真实 provider 实例
- `provider_id`
- `model_id`

### (5) system prompt 如何构成

- 先看 agent 的 system prompt
- 再看 request skill tree plan 是否要把 skill markdown 拼入 system prompt

### (6) 编译为统一请求契约

最后调用：

- `resolve_compiled_execution_request(config, &ExecutionResolutionContext { ... }).await`

而这个函数内部走的是：

```text
ExecutionResolutionContext
  -> resolve_request_execution_spec(...)
  -> ResolvedExecutionSpec
  -> compile()
  -> CompiledExecutionRequest
```

这里会统一处理：

- model/provider config override
- provider/model options merge
- reasoning/thinking 默认行为
- tuning 参数（max_tokens / temperature / top_p / variant）

这就是 ROCode 的 **请求编译权威层**。

---

## 8. 普通 prompt 路径：`SessionPrompt` 如何驱动一轮多步 agentic 对话

关键文件：

- `crates/rocode-server/src/routes/session/prompt.rs`
- `crates/rocode-session/src/prompt/mod.rs`
- `crates/rocode-orchestrator/src/runtime/loop_impl.rs`

这一条路径对应：**不启用 scheduler，直接由 session 引擎驱动多轮模型-工具循环。**

### 8.1 Server 侧先搭好“钩子环境”

在真正调用 `prompt_runner.prompt_with_update_hook(...)` 之前，server 会先准备：

- `update_hook`
  - 会话快照更新时推给后台 persistence worker
- `agent_lookup`
  - 让 prompt 内部工具/子任务可以查询 agent 信息
- `ask_question_hook`
  - 把问题请求桥接到 server question 路由
- `ask_permission_hook`
  - 把权限请求桥接到 permission 路由
- `event_broadcast`
  - 把运行中的事件转为 `ServerEvent`
- `output_block_hook`
  - 把结构化输出块广播到 SSE
- `publish_bus_hook`
  - 把 `agent_task.registered/completed` 事件送入 `RuntimeControlRegistry`

这一步说明：

> `SessionPrompt` 本身只关心执行；而与“交互问题、权限、拓扑、SSE、持久化”的集成，是 server 在外层注入的。

### 8.2 `prompt_with_update_hook(...)`：普通 prompt 的总入口

函数位置：`crates/rocode-session/src/prompt/mod.rs`

它做的事：

1. 校验 session 不 busy
2. `start(session_id)` 获取取消 token
3. 根据输入推导 `model_id/provider_id`
4. 创建 user message
5. 给最新 user message 打注释（system prompt、model 等）
6. 如果 session 还是默认标题，立即根据第一条 user message 生成临时标题
7. 标记 session 为 busy
8. 调用 `loop_inner(...)`
9. 循环结束后 `finish_run(session_id)`

### 8.3 `loop_inner(...)`：真正的 prompt 执行循环

`loop_inner()` 是普通 prompt 模式的核心。

每次循环大致做这些事：

1. 检查取消状态
2. 过滤 compacted messages
3. 定位最后一个 user message / assistant message
4. `process_pending_subtasks(...)`
   - 若上一轮产生了 pending subtask，则先跑子任务再继续
5. 检查上一条 assistant 是否已经 terminal finish
6. 步数加一，超出 `MAX_STEPS` 则退出
7. `maybe_compact_context(...)`
   - 需要时触发 LLM compaction 或 fallback 文本压缩
8. `prepare_chat_messages(...)`
   - plugin `chat.messages.transform`
   - 注入 plan/build reminder
   - 构造 provider 格式消息
   - 应用 cache 策略
9. 合并工具定义
   - 本地工具 + MCP 工具
10. 创建 assistant 占位消息
11. 调用 `run_runtime_step(...)`
12. 根据 step 输出：
   - 完成 assistant message usage/finalization
   - 追加 tool result message
   - 触发 `chat.message` plugin hook
   - 必要时生成标题/总结
   - 若 terminal finish，则退出循环
13. 循环结束后，若已取消则把未完成工具标为 aborted/error
14. `prune_after_loop(session)`

这个函数体现出 ROCode 的普通 prompt 模式本质上也是 **多步代理循环**，不是“一次发问一次回答”的简单 chat。

### 8.4 `prepare_chat_messages(...)`：发送给模型前的最后整形

这个函数很关键，因为它说明模型看到的上下文并不等于原始 session message：

- 先允许 plugin 在消息级做 transform
- 再根据 agent 模式注入提醒（比如 plan 模式）
- 再调用 `build_chat_messages(...)` 生成 provider message
- 再应用缓存策略

因此：

> **Session 是内部事实；Provider ChatRequest 是最终投影。**

两者之间有一个明确的“上下文编译/投影层”。

### 8.5 `run_runtime_step(...)`：把 session 世界桥接到通用 run loop

这个函数内部做了三件事：

### (1) 构造 `SimpleModelCaller`

- 持有 provider
- 持有本次 `CompiledExecutionRequest`

### (2) 构造 `SessionStepToolDispatcher`

它掌握：

- session_id / directory
- 当前 agent 名
- tool_registry
- provider/model 信息
- 已解析的工具定义
- question / permission / publish bus hook
- subsession/persisted subtask 状态

### (3) 构造 `SessionStepSink`

Sink 负责把 runtime loop 的标准化事件投影回 session：

- assistant text chunk
- tool call
- tool result
- reasoning
- step 边界
- output block
- 增量 update

然后调用：

```rust
run_loop(&model, &tools, &mut sink, &policy, &cancel, input.chat_messages).await
```

这一步就是把“会话引擎”接到“统一运行时内核”上。

---

## 9. `run_loop`：ROCode 的统一 agentic 执行内核

关键文件：

- `crates/rocode-orchestrator/src/runtime/loop_impl.rs`
- `crates/rocode-orchestrator/src/runtime/traits.rs`

源码直接把 `run_loop` 描述为：

> `the single source of truth for the agentic execution cycle`

这是全仓库最关键的架构事实之一。

### 9.1 它抽象了 3 个接口

### `ModelCaller`

职责：

- 接收 `LoopRequest { messages, tools }`
- 调用 provider 流式接口

### `ToolDispatcher`

职责：

- 列出可用 tool definition
- 执行一个 fully-assembled tool call

### `LoopSink`

职责：

- 接收标准化流事件
- 接收 tool result
- 接收 step boundary

这三个接口的意义非常大：

> 不论是 session prompt、scheduler、subagent，底层都可以复用同一个 run loop，只要各自实现/桥接这 3 个角色。

### 9.2 `run_loop` 的实际循环语义

每一轮 step：

1. **取消检查 #1**：模型调用前
2. `sink.on_step_boundary(Start)`
3. `tools.list_definitions()`
4. `model.call_stream(req)`
5. 用 `rocode_provider::assemble_tool_calls(...)` 把 provider 流规范化
6. 消费 stream：
   - 文本 chunk → `LoopEvent::TextChunk`
   - tool call ready → `LoopEvent::ToolCallReady`
   - usage / error / step done 等
7. **取消检查 #2**：每个流事件后
8. 若本轮没有 tool call：
   - assistant 文本写入 conversation
   - `FinishReason::EndTurn`
   - 返回
9. 若有 tool call：
   - 把 assistant + tool_use 写入 conversation
   - 遍历 tool calls
10. **取消检查 #3**：每个 tool dispatch 前
11. `tools.execute(call).await`
12. 根据 tool error policy 决定：失败中断 / skip / report-and-continue
13. tool result 写回 conversation
14. `sink.on_step_boundary(End { finish_reason: ToolUse, ... })`
15. 进入下一轮 step

直到：

- `EndTurn`
- `Cancelled`
- `MaxSteps`
- `ModelError`
- `ToolDispatchError`

### 9.3 为什么这个内核很关键

因为它把以下逻辑统一了：

- provider streaming 归一化
- tool call 组装
- tool result 回填 conversation
- step 边界
- finish reason
- cancellation checkpoints

也就是说，ROCode 的“代理性”并不散落在 UI 或某个脚本里，而是**固化在一个统一 runtime 内核中**。

---

## 10. Tool 系统：ROCode 的能力不是散函数，而是注册表驱动的运行时

关键文件：

- `crates/rocode-tool/src/lib.rs`
- `crates/rocode-tool/src/registry.rs`
- `crates/rocode-tool/src/task.rs`
- `crates/rocode-tool/src/task_flow.rs`
- `crates/rocode-tool/src/plan.rs`

### 10.1 默认工具注册表

`create_default_registry_with_config(...)` 注册的内置工具非常多，核心包括：

- 文件类：`read / write / edit / apply_patch / multiedit`
- Shell 类：`bash / shell_session`
- 搜索类：`glob / grep / ls / codesearch / ast_grep_*`
- 调度类：`task / task_flow / question / todo* / skill / plan_enter / plan_exit`
- 外部信息类：`webfetch / websearch / github_research / repo_history / media_inspect`
- 协议类：`lsp_tool / context_docs / browser_session`
- 保底类：`invalid`

另外还会：

- 若存在 plugin loader，则自动注册 plugin custom tools

### 10.2 工具暴露给模型的方式

`ToolRegistry::list_schemas()` 会输出：

- `name`
- `description`
- `parameters`

期间还允许 plugin `tool.definition` hook 修改 schema。

这意味着：

> 模型看到的工具定义，本身也可以被扩展层重写。

### 10.3 工具执行生命周期

`ToolRegistry::execute(tool_id, args, ctx)` 的流程：

1. 找到工具；找不到则给建议列表
2. `normalize_tool_arguments(...)`
3. 触发 plugin `tool.execute.before`
4. 参数校验
5. 真正 `tool.execute(...)`
6. 错误改写与参数错误诊断
7. 触发 plugin `tool.execute.after`
8. 返回 `ToolResult`

这说明工具不是简单函数调用，而是一个**可观测、可拦截、可扩展的执行管道**。

### 10.4 `task`：子代理/子会话委派工具

`crates/rocode-tool/src/task.rs` 显示，`task` 工具的关键动作是：

1. 根据 agent/category 决定委派目标
2. 计算 disabled tools
3. 若没有现成 `task_id`，则 `ctx.do_create_subsession(...)`
4. 根据 skill 载入情况构造子任务 prompt
5. 在 `global_task_registry()` 注册 agent task
6. 通过 `ctx.do_publish_bus("agent_task.registered", ...)` 告知 runtime topology
7. `ctx.do_prompt_subsession(session_id, subtask_prompt).await`
8. 完成后发布 `agent_task.completed`
9. 返回包含子 session / model / skill 信息的 metadata

这说明 `task` 的本质不是后台线程，而是：

> **在当前会话树下创建或复用一个子会话，并让它跑一遍同样的 prompt/runtime 流程。**

### 10.5 `task_flow`：任务生命周期语义门面

`task_flow` 并不是替代 `task`，而是一个 facade：

- `create / resume / get / list / cancel`
- 当前阶段：
  - `get/list` 读 registry
  - `cancel` 走 orchestration lifecycle mediation
  - `create/resume` 薄适配到现有 `task` 工具

这说明 ROCode 正在把“子任务”从单一工具提升为更稳定的生命周期接口。

### 10.6 `plan_enter / plan_exit`

`plan.rs` 显示这两个工具的意义不是写 UI，而是**切换执行模式/agent 模式**：

- `plan_enter`
  - 询问用户是否切入 plan 模式
  - 创建 synthetic user message
  - `ctx.do_switch_agent("plan", ...)`
- `plan_exit`
  - 从 plan 模式回 build 模式

这属于 workflow 控制工具，而不是前端功能。

---

## 11. Scheduler 路径：ROCode 的“调度器”并不是另一套引擎，而是共享骨架上的 profile/preset

关键文件：

- `crates/rocode-server/src/routes/session/prompt.rs`
- `crates/rocode-server/src/routes/session/scheduler.rs`
- `crates/rocode-orchestrator/src/lib.rs`
- `docs/examples/scheduler/README.md`

文档明确写道：

> `named orchestrators are presets over the shared scheduler profile kernel, not separate execution engines`

这是理解 ROCode scheduler 的关键句。

### 11.1 四个 public preset

根据 `docs/examples/scheduler/README.md`：

- `sisyphus`
  - execution-oriented single-loop
- `prometheus`
  - planning-first / interview → plan → review → handoff
- `atlas`
  - coordination / delegation / verification
- `hephaestus`
  - autonomous deep-worker

但它们共用同一套 scheduler kernel，而不是四套不同执行器。

### 11.2 scheduler 分支在 `/session/{id}/prompt` 中的执行过程

当 `task_scheduler_profile_name` 与对应 profile config 存在时，会进入 scheduler 分支：

1. 给 user message 写入 scheduler/profile 相关 metadata
2. 创建 assistant 占位消息
3. 若 session 还是默认标题，立刻用首条 user 文本生成临时标题
4. 更新 session 并广播 `prompt.scheduler.pending`
5. 构造 `AgentRegistry::from_config(&task_config)`
6. 将 `available_agents / available_categories / skill_list` 注入 profile config
7. 创建 scheduler cancel token，并注册到 `runtime_control`
8. 创建 `SessionSchedulerToolExecutor`
9. 创建 `SessionSchedulerModelResolver`
10. 创建 `SessionSchedulerLifecycleHook`
11. 组装 `OrchestratorContext`
12. `scheduler_orchestrator_from_profile(...).execute(&prompt_text, &ctx).await`
13. 执行结束后把 steps、tool_calls、usage、handoff metadata 写回 assistant/session
14. 广播 `prompt.scheduler.completed`
15. 持久化

### 11.3 `SessionSchedulerModelResolver`

它的责任是：

- 若 stage/agent 显式指定 model，则用该 model
- 否则回退到本次请求的 `fallback_provider_id + fallback_model_id + fallback_request`
- 最终调用 `provider.chat_stream(request)`

这意味着 scheduler 并不自己管理 provider 实现，只负责“按 stage/agent 语义解析该用哪个模型”。

### 11.4 `SessionSchedulerToolExecutor`

它负责把 orchestrator scheduler 世界桥回普通 ToolRegistry：

- 构造 `ToolContext`
- 注入：
  - 当前 session / message / directory
  - agent 名
  - abort token
  - 当前 model
  - agent info lookup
  - question hook
  - permission hook
  - category resolve
  - publish bus
- 然后调用：
  - `state.tool_registry.execute(tool_name, arguments, ctx).await`

所以 scheduler 本身不是工具系统；它只是 **调度语义层**，真正工具执行仍然回到统一 ToolRegistry。

### 11.5 `SessionSchedulerLifecycleHook`

这是 scheduler 路径中很关键的一层，它做的不是 UI 渲染，而是**运行时投影与观测**：

- 维护 active stage message
- 写 stage metadata
- 生成 output block
- 更新 runtime topology
- 记录 usage/cost
- 追踪 child session attachment
- 广播 server event

因此 scheduler 的 stage 可视化虽然最终会被前端消费，但**语义权威在生命周期 hook**。

---

## 12. Agent 系统：为什么说 ROCode 的 agent 与普通 prompt 底层是同一执行核

关键文件：

- `crates/rocode-agent/src/agent/registry.rs`
- `crates/rocode-agent/src/executor/mod.rs`

### 12.1 AgentRegistry

`AgentRegistry` 会：

- 内建 builtin agents
- 支持从 config 合并 agent 定义
- 提供 `default_agent / list_primary / list_subagents`

说明 agent 本质是**可配置执行人格/策略单元**，不是另一个独立 runtime。

### 12.2 AgentExecutor 如何执行

`AgentExecutor` 的核心步骤：

1. 维护自身 `Conversation`
2. 根据 agent 配置构造执行上下文
3. `build_tooling()`：创建 `ToolRegistryAdapter` + `ToolRunner`
4. `ModelCallerBridge::new(...)`
5. `ToolDispatcherBridge::new(...)`
6. 调用同一个 `run_loop(...)`

也就是说：

> **AgentExecutor 不是自己实现一套模型-工具循环，而是把 agent 配置桥接到 orchestrator runtime。**

这就解释了为什么 ROCode 可以把：

- 主会话执行
- scheduler stage 执行
- 子代理执行

统一到同一个 runtime 语义上。

---

## 13. Config 系统：ROCode 如何发现并合并配置

关键文件：`crates/rocode-config/src/loader/mod.rs`

`load_all(project_dir)` 的合并顺序非常明确：

1. Global config
2. `ROCODE_CONFIG`
3. Project config
4. `.rocode` 目录
5. `ROCODE_CONFIG_CONTENT`
6. Managed config

其中 `.rocode` 目录不仅加载 config 文件，还会加载：

- `commands`
- `agents`
- `modes`
- `plugins`

这意味着 `.rocode/` 不是单一配置文件夹，而是**项目级扩展点根目录**。

### 13.1 这套配置机制的架构价值

它让 ROCode 的运行时可在项目级扩展：

- 自定义 slash command
- 自定义 agent
- 自定义 execution mode
- 自定义 plugin
- 自定义 schedulerPath / categoryPath / skillPath

因此 ROCode 不是“固定 agent 产品”，而是一个**本地可编排代理 runtime**。

---

## 14. Provider 系统：模型调用不是简单写死 API，而是 bootstrap 后的 registry

关键文件：

- `crates/rocode-provider/src/bootstrap.rs`
- `crates/rocode-provider/src/provider.rs`
- `crates/rocode-orchestrator/src/execution_resolver.rs`

### 14.1 `ProviderRegistry`

它维护：

- `providers: HashMap<String, Arc<dyn Provider>>`
- `provider_info: HashMap<String, ProviderInfo>`

支持：

- `get()`
- `get_provider()`
- `list()`
- `find_model()`
- `parse_model_string()`

### 14.2 bootstrap 流程

server 初始化时会走：

1. `bootstrap_config_from_raw(...)`
2. `create_registry_from_bootstrap_config(config, auth_store)`
3. 内部 `bootstrap_registry(...)`
4. 加载 models.dev cache
5. `ProviderBootstrapState::init(...)`
6. 为每个 provider 创建 concrete provider 并注册进 registry
7. 若完全没有 provider，再回退 env provider 注册

这意味着 provider 的来源可以叠加：

- config
- auth store
- models.dev catalog
- plugin custom fetch / auth bridge
- env fallback

### 14.3 为什么 `CompiledExecutionRequest` 很重要

`execution_resolver.rs` 清晰表明：

- provider config
- model config
- catalog capabilities
- request tuning
- variant
- thinking 默认开关

都会在这里合并并编译。

所以对外看上去是“请求某个 model”，但内部其实会先经过一次**配置感知的请求解析与编译**。

---

## 15. Plugin 系统：ROCode 的扩展点不是外围脚本，而是深度嵌入执行管线

关键文件：

- `crates/rocode-plugin/src/lib.rs`
- `crates/rocode-plugin/src/subprocess/loader.rs`
- `crates/rocode-server/src/server.rs`

### 15.1 HookSystem 的作用

`HookEvent` 包括：

- `tool.execute.before/after`
- `tool.definition`
- `experimental.chat.system.transform`
- `experimental.chat.messages.transform`
- `chat.params`
- `chat.headers`
- `chat.message`
- `shell.env`
- `command.execute.before`
- `permission.ask`
- 以及 session/error/file/provider 相关事件

`PluginSystem::trigger(...)` 的特点：

- 支持顺序执行（TS parity）
- 某些 deterministic event 可缓存
- 某些事件 fire-and-forget

这意味着 plugin 可以插入的层次非常深：

- 配置加载后
- 命令执行前
- 模型请求前
- 消息变换时
- 工具定义暴露前
- 工具执行前后
- permission 询问时

### 15.2 PluginLoader：JS/TS 插件运行时管理器

`PluginLoader` 会管理：

- `clients`
- `auth_bridges`
- `tool_catalog`
- `hook_system`
- bootstrap context/specs

其启动逻辑：

1. 探测 JS runtime（bun/deno/node）
2. 写出 host script 到缓存目录
3. 清理 IPC 临时文件
4. `configure_bootstrap(...)`
5. `ensure_started()` 时按 context/specs 载入 builtin 与配置插件

### 15.3 server 如何接入 plugin

`load_plugin_auth_store(...)` 中会：

1. 初始化 `PluginLoader`
2. `init_global(loader.hook_system())`
3. 设置 global loader
4. 加载 native plugin 与 TS plugin
5. 刷新 plugin auth state
6. 对支持 custom fetch 的 provider 注册 `CustomFetchProxy`
7. `routes::refresh_agent_cache(config_store).await`
8. 启动 idle monitor，空闲时回收插件子进程

因此 plugin 并不是“旁路能力”，它直接参与：

- auth
- provider custom fetch
- tool catalog
- hook system
- config 注入
- agent cache 刷新

---

## 16. MCP / LSP：ROCode 如何接外部能力而不把它们写死进主循环

### 16.1 MCP

关键文件：

- `crates/rocode-mcp/src/lib.rs`
- `crates/rocode-mcp/src/transport.rs`
- `crates/rocode-session/src/prompt/tools_and_output.rs`

`rocode-mcp` 提供：

- `McpClient`
- `McpClientRegistry`
- `McpToolRegistry`
- `StdioTransport`
- `HttpTransport`
- `SseTransport`
- OAuth 相关能力

MCP transport 抽象为 `McpTransport` trait：

- `send`
- `receive`
- `close`

这样可以支持：

- stdio 子进程服务器
- HTTP streamable server
- SSE server

### MCP 工具如何进入主执行流

`resolve_tools_with_mcp_registry(...)` 会把：

- 本地 `ToolRegistry.list_schemas()`
- 动态 `mcp_registry.list()`

合并成模型可见的最终 tool definitions。

因此 MCP 在 ROCode 中的地位是：

> **被统一纳入工具系统，而不是绕开工具系统单独调用。**

### 16.2 LSP

关键文件：`crates/rocode-lsp/src/lib.rs`

LSP client 的实现也遵循类似模式：

- 启动子进程
- stdio 传 JSON-RPC
- 注册到 process registry
- 维护 diagnostics / pending response / file version

这说明 ROCode 对外部语义服务（MCP/LSP）的集成方法是一致的：

- **通过协议客户端 + 进程生命周期管理 + 工具层/调用层桥接**

而不是把这些能力硬编码到 session loop 内部。

---

## 17. Storage：持久化不是主循环，但对系统形态很重要

关键文件：

- `crates/rocode-server/src/server.rs`
- `crates/rocode-storage/src/repository.rs`
- `docs/session-message-storage.md`

### 17.1 启动时的读路径：全量灌入内存

`load_sessions_from_storage()` 会：

1. `session_repo.list(None, 100_000)`
2. 对每个 session `message_repo.list_for_session(&stored.id)`
3. hydrate 成 `rocode_session::Session`
4. 放入 `SessionManager`

这说明：

- Server 启动后，很多读请求其实直接读内存，而不是实时扫 DB。

### 17.2 运行中的写路径：增量 upsert + 结束 flush

普通 prompt 路径里：

- `update_hook` 会把最新 session snapshot 推给 coalescing persistence worker
- worker 做：
  - `SessionRepository::upsert`
  - `MessageRepository::upsert`
- prompt 结束时再 `flush_session_to_storage(session_id)`

`flush_with_messages(...)` 还会：

- upsert session
- upsert messages
- upsert parts
- 删除 DB 中已经过期的 stale message / stale part

### 17.3 为什么消息与 parts 很重要

`repository.rs` 表明：

- message 的 `data` 字段是 parts JSON blob
- parts 表会把 part 单独正规化存储，含：
  - `part_type`
  - `text`
  - `tool_name`
  - `tool_call_id`
  - `tool_status`
  - `tool_result`
  - `file_*`
  - `reasoning`
  - `sort_order`

因此 ROCode 的持久化不是“整段 transcript”，而是**结构化消息分片持久化**。

### 17.4 一个重要架构判断

结合 `docs/session-message-storage.md`：

- **历史事实**：messages / parts / session metadata 在 DB 中
- **当前运行态**：`RuntimeStateStore` 更权威

所以：

> DB 更像“可恢复、可查询、可回放的历史快照层”，不是当前执行态的唯一真相源。

---

## 18. 端到端完整工作流（普通 prompt）

下面用一条最常见路径说明：

> 用户执行：`rocode run "请调查这个仓库的风险点"`

### 阶段 A：CLI 入口

1. `rocode-cli/src/main.rs` 解析到 `Commands::Run`
2. 进入 `run_non_interactive(...)`
3. 收集文本和附件
4. `discover_or_start_server(None)`
5. 连接或启动本地 server
6. `CliApiClient::send_prompt(...)` 调用 `/session/{id}/prompt`
7. CLI 同时订阅 `/event` SSE，等待增量事件和 output block

### 阶段 B：Server 请求解析

8. `session_prompt(...)` 校验参数并读取 session
9. `resolve_prompt_payload(...)` 处理 slash command（如果有）
10. 根据 plugin-applied config 生成本次有效配置快照
11. `resolve_prompt_request_config(...)` 解析：
    - scheduler 是否启用
    - agent
    - provider/model
    - system prompt
    - compiled request
12. 在 session metadata 中记录 model/agent/variant 等元信息
13. 标记 session `Busy`

### 阶段 C：进入普通 prompt 引擎

14. server 构造 update/question/permission/output/bus hooks
15. `prompt_runner.prompt_with_update_hook(...)`
16. 创建 user message
17. 设置 session busy 状态
18. 进入 `loop_inner(...)`

### 阶段 D：一次 step 的执行

19. 检查是否需要处理 pending subtasks
20. 检查是否需要 context compaction
21. `prepare_chat_messages(...)` 生成发给模型的消息数组
22. 获取本轮工具定义（本地 + MCP）
23. 创建 assistant 占位消息
24. `run_runtime_step(...)`
25. 构造 `SimpleModelCaller + SessionStepToolDispatcher + SessionStepSink`
26. 进入统一 `run_loop(...)`

### 阶段 E：模型流与工具调用

27. `provider.chat_stream(...)`
28. 流事件标准化为 `LoopEvent`
29. 文本 chunk 持续写入 assistant part
30. 若模型产生 tool call，则 `ToolDispatcher.execute(...)`
31. `ToolRegistry.execute(...)` 真正执行工具
32. tool result 被写回 conversation 与 session
33. 若还有未完成任务，则进入下一轮 step

### 阶段 F：收尾

34. 本轮不再有 tool call 时，`FinishReason::EndTurn`
35. `finalize_assistant_message(...)` 写 usage/finish metadata
36. 触发 `chat.message` plugin hook
37. 第一次 assistant 完成后可自动生成标题和 summary
38. update worker 增量持久化 snapshot
39. prompt 结束后 `flush_session_to_storage(session_id)`
40. 广播 `prompt.final`
41. session 状态回到 `Idle`
42. CLI 从 SSE 和查询接口中拿到最终结果

这就是 ROCode 非 UI 核心下的一次完整执行闭环。

---

## 19. 端到端完整工作流（scheduler prompt）

> 用户请求某个 scheduler profile，例如 `prometheus` 或 `atlas`

### 阶段 A：请求解析

1. 进入同一个 `/session/{id}/prompt`
2. `resolve_prompt_request_config(...)` 判定 `scheduler_applied = true`
3. 解析 scheduler profile / root agent / model

### 阶段 B：构造 scheduler 运行环境

4. 给 user/assistant message 写 scheduler metadata
5. 构造 `AgentRegistry`
6. 把 `available_agents / available_categories / skill_list` 注入 profile config
7. 注册 scheduler cancel token 到 `runtime_control`
8. 创建：
   - `SessionSchedulerToolExecutor`
   - `SessionSchedulerModelResolver`
   - `SessionSchedulerLifecycleHook`
   - `OrchestratorContext`

### 阶段 C：执行 scheduler kernel

9. `scheduler_orchestrator_from_profile(...)`
10. `orchestrator.execute(&prompt_text, &ctx).await`
11. 每个 stage 内部仍会走模型调用与工具调用
12. 生命周期 hook 负责：
    - stage message
    - output block
    - usage/cost
    - child session attach/detach
    - topology update
    - session runtime update

### 阶段 D：收尾

13. 结果写入 assistant
14. handoff metadata 写回 session
15. 广播 `prompt.scheduler.completed`
16. 持久化

### 关键理解

scheduler 路径与普通 prompt 路径的最大差异，不是“是否能用工具”，而是：

- 普通 prompt：由 `SessionPrompt::loop_inner` 直接驱动会话循环
- scheduler：由 orchestrator profile/stage 先编排，再通过 resolver/executor/hook 回到同一套 provider/tool/runtime 体系

---

## 20. 子代理 / 子任务 / 子会话工作流

这是 ROCode 区别于普通 chat 应用的重要点。

### 20.1 `task` 工具触发委派

当模型调用 `task`/`task_flow`：

1. ToolRegistry 执行 `task`
2. `task.rs` 决定目标 agent/category
3. 创建或复用 subsession
4. 在 `global_task_registry()` 注册任务
5. 通过 `publish_bus` 通知 server runtime_control
6. `ctx.do_prompt_subsession(...)`
7. 子会话执行自己的 prompt/runtime loop
8. 完成后结果回填父执行流

### 20.2 为什么它是“子会话”，不是“函数调用”

因为它具备：

- 自己的 session_id
- 自己的 conversation/history
- 可独立使用 agent/model/tool 限制
- 可被执行拓扑追踪

所以 ROCode 的子代理不是轻量回调，而是**嵌套执行上下文**。

### 20.3 普通 prompt 路径里的 pending subtasks

`process_pending_subtasks(...)` 会扫描 user message 中 `PartType::Subtask { status == "pending" }`，然后：

- 创建/复用 persisted subsession
- 组合 prompt
- 调用 `execute_persisted_subsession_prompt(...)`
- 把结果写回 assistant message

说明即使不显式使用 scheduler，ROCode 的普通 prompt 也能在内部形成子任务分支。

---

## 21. 非 UI 视角下，ROCode 最重要的 8 个架构特征

### 特征 1：Server 是统一能力底座

CLI/TUI/Web 最终都指向同一个 server runtime。即使是 CLI `run`，默认也会先发现或启动 server。

### 特征 2：普通 prompt、scheduler、subagent 共用同一执行核

统一内核是 `rocode-orchestrator::runtime::run_loop`。

### 特征 3：执行请求有统一编译权威层

`ExecutionResolutionContext -> ResolvedExecutionSpec -> CompiledExecutionRequest` 让所有路径的模型请求语义统一。

### 特征 4：工具系统是第一公民

工具不是补充接口，而是 runtime loop 中的核心分支。

### 特征 5：插件是深度嵌入式扩展，不是外围脚本

插件能改工具定义、改工具执行、改消息、改模型参数、接 auth/custom fetch。

### 特征 6：MCP/LSP 走协议桥接，而不是主循环硬编码

这使系统扩展更统一。

### 特征 7：运行时状态、执行拓扑、持久化历史三者分离

- `SessionManager`：会话事实
- `RuntimeStateStore`：当前运行态
- `RuntimeControlRegistry`：执行拓扑
- `Storage`：持久化快照

### 特征 8：UI 只是消费者，不是语义权威

stage、tool lifecycle、question、permission、runtime、topology 的真实含义都在 server 与 runtime hook 中定义。

---

## 22. 我对 ROCode 非 UI 架构的总体判断

如果把所有 UI 都去掉，ROCode 仍然是一个完整的本地代理执行平台，其本质可以概括为：

### 22.1 它是“server-first”的本地代理 runtime

- CLI 默认复用 server
- 会话、执行拓扑、SSE、存储、权限、问题流、scheduler 都围绕 server 组织

### 22.2 它是“session-centered”的状态系统

- 所有执行最终都落回 session/message/parts
- 子任务本质上也是子 session

### 22.3 它是“tool-centric”的 agent 框架

- 模型流和工具调度组成统一的闭环
- scheduler/agent 只是决定“如何组织这套闭环”

### 22.4 它是“可扩展 runtime”，而不是固定产品逻辑

- agent 可配置
- command 可配置
- plugin 可插入
- MCP/LSP 可桥接
- scheduler 可 profile 化

### 22.5 它已经形成比较清晰的权威边界

- 请求编译权威：orchestrator request resolver
- 运行循环权威：orchestrator run loop
- 会话事实权威：SessionManager / Session
- 当前运行态权威：RuntimeStateStore
- 执行拓扑权威：RuntimeControlRegistry
- 工具执行权威：ToolRegistry

这套边界设计，是整个项目最值得肯定的地方。

---

## 23. 关键阅读顺序建议（适合继续深挖源码）

如果后续要继续深入，我建议按下面顺序读：

1. `crates/rocode-cli/src/main.rs`
2. `crates/rocode-cli/src/run.rs`
3. `crates/rocode-cli/src/server_lifecycle.rs`
4. `crates/rocode-server/src/server.rs`
5. `crates/rocode-server/src/routes/mod.rs`
6. `crates/rocode-server/src/routes/session/prompt.rs`
7. `crates/rocode-server/src/routes/session/scheduler.rs`
8. `crates/rocode-session/src/prompt/mod.rs`
9. `crates/rocode-orchestrator/src/runtime/loop_impl.rs`
10. `crates/rocode-orchestrator/src/execution_resolver.rs`
11. `crates/rocode-tool/src/registry.rs`
12. `crates/rocode-tool/src/task.rs`
13. `crates/rocode-agent/src/executor/mod.rs`
14. `crates/rocode-plugin/src/lib.rs`
15. `crates/rocode-plugin/src/subprocess/loader.rs`
16. `crates/rocode-provider/src/bootstrap.rs`
17. `crates/rocode-storage/src/repository.rs`
18. `docs/examples/scheduler/README.md`
19. `docs/session-message-storage.md`

---

## 24. 最后用一句话总结

**ROCode 的非 UI 架构，本质上是一套围绕 `ServerState -> SessionPrompt / Scheduler -> run_loop -> ToolRegistry / ProviderRegistry -> RuntimeState / Topology / Storage` 展开的本地代理执行平台；UI 只是这套后端事实与事件流的消费者，不是架构中心。**
