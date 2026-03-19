# ROCode Server 启动后（首个会话/首条消息前）到底发生了什么

> 适用范围：解释 **`rocode serve`（或 CLI 自动拉起 server）后，到你还没新建会话、没发任何消息之前** 的系统行为。  
> 只讲非 UI 后端运行时（不展开 TUI/Web 渲染）。

---

## 1) 先说结论（TL;DR）

当 server 启动完成但你还没进行任何新会话操作时，ROCode 已经做完：

1. **构建并持有一个完整 `ServerState` 运行时容器**（会话管理、provider、工具、配置、事件总线、运行态存储等）。
2. **加载配置、认证、插件、provider registry、工具注册表**。
3. **连接并迁移 SQLite，按需把历史 session/message 从 DB 灌入内存**。
4. **HTTP 路由和 SSE 通道已就绪，等待请求。**

但这时仍然是“待机态”：

- 没有新的 prompt 执行；
- 没有 active runtime execution 拓扑节点；
- `RuntimeStateStore` 里通常还是空（直到会话执行发生）。

---

## 2) 启动路径（从命令到 server ready）

### 2.1 CLI 入口分发

- 入口：`crates/rocode-cli/src/main.rs`
- `rocode serve` 最终调用：`run_server_command(...)`（`crates/rocode-cli/src/server.rs`）
- `run_server_command` 最终调用：`rocode_server::run_server(addr)`

### 2.2 server 主启动函数

`crates/rocode-server/src/server.rs::run_server`：

1. 生成 `server_url`
2. `ServerState::new_with_storage_for_url(server_url).await?`
3. `routes::router()` 组装路由 + CORS + trace layer
4. 绑定 TCP listener
5. `axum::serve(...)` 开始对外提供 HTTP/SSE

---

## 3) `ServerState` 初始化详细时间线（最核心）

对应 `crates/rocode-server/src/server.rs::new_with_storage_for_url`。

### T0. 先建一个默认骨架 `ServerState::new()`

此时先得到默认空壳（之后会被覆盖/填充）：

- `sessions = SessionManager::new()`（空）
- `providers = ProviderRegistry::new()`（空）
- `tool_registry = ToolRegistry::new()`（空）
- `prompt_runner = SessionPrompt::new(...)`
- `runtime_control = RuntimeControlRegistry::with_topology_callback(...)`
- `stage_event_log = StageEventLog::new()`
- `runtime_state = RuntimeStateStore::new()`
- `event_bus = broadcast::channel(1024)`
- `api_perf` 计数器归零

### T1. 加载认证存储

- `AuthManager::load_from_file(auth_data_dir)`
- 默认读取：`<data_local>/rocode/data/auth.json`（若存在）

结果：内存里会有 provider → AuthInfo 的 map（可能为空）。

### T2. 加载配置

- `ConfigStore::from_project_dir(&cwd)`
- 失败则回退 `Config::default()`

结果：`ConfigStore`（ArcSwap）成为配置真源（base config）。

### T3. 插件体系 bootstrap（很关键）

调用 `load_plugin_auth_store(...)`，做了这些事：

1. 创建 `PluginLoader`
2. `init_global(loader.hook_system())`
3. `rocode_plugin::set_global_loader(loader.clone())`
4. 配置插件 bootstrap 上下文（worktree、server_url、internal token）
5. 加载 builtin auth 插件（codex/copilot）
6. 加载配置中的 TS/native 插件（若有）
7. `refresh_plugin_auth_state(...)`：
   - 从插件 auth bridge 拉 token
   - 回灌到 `AuthManager`
   - 同步 custom fetch proxy（provider 侧）
8. `routes::set_plugin_loader(loader.clone())`
9. `routes::refresh_agent_cache(config_store).await`（预热 agent/mode 缓存）
10. 启动空闲监控，超时后可回收插件子进程

> 注意：如果插件加载失败，部分步骤会降级或提前返回；server 不一定整体失败。

### T4. 生成 provider bootstrap 配置

- 从 config 里的 provider/model 字段转换为 `BootstrapConfig`
- 同时结合 `AuthManager` 当前内存 auth store

### T5. 预热 models.dev 缓存（10 秒超时）

- `ModelsRegistry::default().get()` 尝试读取本地缓存或拉 `https://models.dev/api.json`
- 结果会写到缓存文件（如成功）

### T6. 构造 `ProviderRegistry`

- `create_registry_from_bootstrap_config(&bootstrap_config, &auth_store)`
- 内部会读取 models 缓存（`load_models_dev_cache()`）并合成 provider/runtime provider/model 列表
- 若完全无 provider，可回退 env provider 注册策略

结果：`state.providers` 持有可调用 provider 实例和 provider_info/model 列表。

### T7. 加载 category + tool registry + prompt runner

1. category：
   - 优先 `task_category_path`
   - 失败回退 builtins
2. tool：
   - `create_default_registry_with_config(...)`
   - 会注册大量内置工具
   - 若 plugin loader 可用，也会自动注册 plugin custom tools
3. prompt_runner：
   - `SessionPrompt::new(...).with_tool_runtime_config(...)`

### T8. 初始化数据库和仓储

1. `Database::new()`：连接 SQLite + 执行 migration + WAL/NORMAL pragma
2. 挂上：`SessionRepository / MessageRepository / PartRepository`

### T9. 从存储回灌会话到内存

`load_sessions_from_storage()`：

1. `session_repo.list(None, 100_000)`
2. 对每个 session 拉 `message_repo.list_for_session(session_id)`
3. 组装成 `rocode_session::Session`
4. 放入 `SessionManager`

> 所以“你没新开会话”并不等于内存一定没有 session：
> 如果数据库里有历史 session，会在启动时被加载进内存。

到这里，server 才算初始化完成并进入监听状态。

---

## 4) 此时内存里到底有什么？（按对象清单）

### 4.1 一定会有（启动完成即存在）

### A. `ServerState` 主对象

- `sessions`（可能空，也可能已有历史）
- `providers`（已构建）
- `bootstrap_config`（已保存）
- `config_store`（base config 已加载）
- `tool_registry`（已注册）
- `prompt_runner`
- `runtime_control`
- `stage_event_log`
- `auth_manager`
- `event_bus`
- `api_perf`
- `session_repo/message_repo/part_repo`
- `category_registry`
- `todo_manager`
- `runtime_state`

### B. 全局/静态缓存与句柄（进程级）

- `routes::INTERNAL_TOKEN`（首次访问时生成，启动阶段会用到）
- `routes` 里的 `AGENT_LIST_CACHE` / `MODE_LIST_CACHE`（`refresh_agent_cache` 后通常已填充）
- `rocode_plugin` 全局 loader / hook system（如果插件初始化成功）

### 4.2 “通常为空/默认值”的结构（尚未有新会话执行）

- `runtime_control.executions`：空（还没 prompt run）
- `runtime_control.scheduler_tokens`：空
- `runtime_control.execution_tokens`：空
- `runtime_control.question_waiters`：空
- `runtime_state.states`：空（无 session runtime snapshot）
- `stage_event_log.sessions`：空（无 stage event）
- `api_perf` 三个计数器：0
- `prompt_runner.state`（内部 prompt 状态表）：空

---

## 5) 你关心的重点：**模型相关数据**到底驻留在哪？

你问的“内存中会存哪些模型啥的”，可以拆成 4 层：

### 5.1 配置层模型（Config 语义）

在 `config_store` / `bootstrap_config` 里会保留：

- 默认 `model`
- `small_model`
- provider/model override
- provider options

这是“配置意图”。

### 5.2 运行层模型（ProviderRegistry 语义）

`state.providers` 里有两部分：

- `providers: HashMap<String, Arc<dyn Provider>>`
- `provider_info: HashMap<String, ProviderInfo>`（含 model 列表）

这是 server 真正可用于调用的 provider/model 视图。

### 5.3 models.dev 原始缓存（目录缓存 + 临时内存）

启动时会预热 `ModelsRegistry::get()`，该对象内部有 `RwLock<Option<ModelsData>>`。

但注意：

- 启动代码里这个 `ModelsRegistry` 是局部变量；
- 其内存缓存不直接挂在 `ServerState`；
- 主要效果是**确保本地 `models.json` 缓存文件可用**，后续 bootstrap 会读这个文件并构建 provider registry。

所以你可以理解为：

- **Server 常驻的是 ProviderRegistry（可调用视图）**，
- 而不是完整 models.dev 原始数据对象本身。

### 5.4 会话历史中的模型痕迹（若有历史会话）

如果 DB 里已有历史会话，加载后每条消息 metadata 里可能带有：

- `model_provider`
- `model_id`
- `variant`

这些是“历史执行记录”，不是当前激活模型。

---

## 6) 在“未新建会话/未发消息”时，不会发生什么？

以下通常还没发生：

1. 不会有新的 `PromptRun` / `SchedulerRun` 执行节点。
2. 不会有 tool call lifecycle。
3. 不会有 question/permission 挂起。
4. 不会有 session runtime 的 `waiting_on_tool` / `waiting_on_user`。
5. 不会有新 output block SSE 流（除非你主动调用其他接口产生事件）。

---

## 7) 一个“待机态”内存快照心智模型

你可以把启动后的系统理解成：

```text
已装弹（loaded）
  - 配置：已就绪
  - 认证：已就绪
  - provider+model：可查询/可调用
  - 工具：已注册
  - 插件：已加载或可被ensure_started恢复
  - 存储：已连接，历史会话已回灌（若存在）

未开火（idle）
  - 没有新prompt执行
  - 没有运行中拓扑节点
  - 没有实时runtime等待态
```

---

## 8) 你可以如何验证这些结论（本地最小验证）

假设服务在 `127.0.0.1:3000`：

```bash
# 1) 启动 server
rocode serve --port 3000 --hostname 127.0.0.1

# 2) 健康检查（应为ok）
curl -s http://127.0.0.1:3000/health

# 3) 看 session 列表（可能为空，也可能有历史）
curl -s http://127.0.0.1:3000/session

# 4) 看 session status（无运行中的通常都是idle/active）
curl -s http://127.0.0.1:3000/session/status

# 5) 看 provider/model 视图
curl -s http://127.0.0.1:3000/provider

# 6) 看 agent/mode（来自缓存+配置）
curl -s http://127.0.0.1:3000/agent
curl -s http://127.0.0.1:3000/mode
```

如果你希望更细验证“内存里当下有哪些对象”，下一步建议临时加 debug route（只在本地开发分支）导出：

- `sessions.len()`
- `runtime_control.list_all_executions().len()`
- `runtime_state` map size
- `providers.list_providers()`
- `tool_registry.list_ids()`

---

## 9) 关键源码索引（便于你继续深挖）

- 启动入口
  - `crates/rocode-cli/src/main.rs`
  - `crates/rocode-cli/src/server.rs`
  - `crates/rocode-server/src/server.rs` (`run_server`, `new_with_storage_for_url`)
- 内存主结构
  - `crates/rocode-server/src/server.rs` (`ServerState`)
  - `crates/rocode-server/src/runtime_control.rs`
  - `crates/rocode-server/src/session_runtime/state.rs`
  - `crates/rocode-server/src/stage_event_log.rs`
- 模型与 provider
  - `crates/rocode-provider/src/provider.rs` (`ProviderRegistry`)
  - `crates/rocode-provider/src/bootstrap.rs` (`BootstrapConfig`, `create_registry_from_bootstrap_config`)
  - `crates/rocode-provider/src/models.rs` (`ModelsRegistry`)
- 配置/插件/工具
  - `crates/rocode-config/src/store.rs`
  - `crates/rocode-server/src/routes/mod.rs`（agent/mode cache, plugin loader 入口）
  - `crates/rocode-plugin/src/subprocess/loader.rs`
  - `crates/rocode-tool/src/registry.rs`
- 存储
  - `crates/rocode-storage/src/database.rs`
  - `crates/rocode-storage/src/repository.rs`
