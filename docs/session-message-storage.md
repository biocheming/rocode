# ROCode 会话 / 消息 / Message Parts（含“活动 part”）存储形式与分页能力评估（源码导读）

> 生成时间：`2026-03-17`  
> 适用代码基线：workspace `version = 2026.3.16`（见 `Cargo.toml`）  
> 分支基线：`rewrite/sea-orm-storage`（持久化层已迁移到 SeaORM，并补齐 sessions/messages 的 count + limit/offset 查询能力）  
>
> 你关心的问题可以拆成两部分：
>
> 1. **数据长什么样、放在哪里**：目录（directory）/ 会话（session）/ 消息（message）/ parts（消息分片）各自的存储形态（内存 & SQLite & 文件）。
> 2. **能不能“很好地”支持 server 侧的总数 / 分页 / offset**：从“Schema 能不能做”到“当前仓库实现有没有做”分别评估，并给出面向后续 server 开发的改造建议。
>
> 本文只基于当前仓库源码，不假设外部服务或历史版本。

---

## 1. 术语对齐：目录 / 会话 / 消息 / parts / “活动 part”

为了避免“同一个词在不同层含义不同”，先给出 ROCode 当前实现里比较准确的对应关系：

- **目录（directory）**：会话的“工作目录”字符串（通常是项目 worktree 的绝对路径）。  
  - 存在于 `Session.directory`（内存）与 `sessions.directory`（SQLite）中。

- **会话（session）**：一次对话/任务的顶层容器（包含元数据 + `messages: Vec<SessionMessage>`）。  
  - 内存模型：`crates/rocode-session/src/session.rs` 的 `Session` / `SessionManager`  
  - 持久化模型：`crates/rocode-types/src/session.rs` 的 `Session`（用于 DB 序列化/反序列化）

- **消息（message）**：会话里的单条消息（user/assistant/system/tool），其核心内容不是“一个大字符串”，而是 **parts 列表**。  
  - 内存消息模型：`crates/rocode-session/src/message.rs` 的 `SessionMessage`
  - 持久化消息模型：`crates/rocode-types/src/message.rs` 的 `SessionMessage`

- **消息分片（parts / MessagePart）**：一条消息的内容由多个 part 组成，例如 `text`、`toolCall`、`toolResult`、`reasoning`、`file`、`patch` 等。  
  - 内存 part 类型更“富”：`rocode-session::PartType::ToolCall` 带 `status/raw/state`  
  - DB 里主要存 `rocode-types::MessagePart`（字段更少），序列化进 `messages.data`（JSON 字符串）

- **“活动 part”（in-flight / running part）**：严格来说不是一个单独的持久化概念，而是一个**运行时投影**，常见于：
  1) 工具调用 part：`ToolCall.status = Pending/Running/Completed/Error`（内存里有）  
  2) Server 侧聚合的运行时状态：`RuntimeStateStore.active_tools/current_message_id`（只在内存里）  
  3) SSE/stream 过程中 `message.part.delta` 这类增量事件（只在内存/事件流里）

结论先说在前面：**当前“活动 part”主要是运行时状态，不是持久化的一等公民**；数据库里能还原“发生过什么 tool call / tool result”，但对“当时是否 running”支持不完整（详见第 5 节）。

---

## 2. “放在哪里”：内存态 vs SQLite 持久化 vs 文件（快照）

### 2.1 内存态：`SessionManager` 是 server 的主工作集

Server 进程里会维护一个 `SessionManager`（`HashMap<id, Session>`），作为绝大多数路由的读写对象：

- `crates/rocode-session/src/session.rs`：`SessionManager`  
- `crates/rocode-server/src/server.rs`：`ServerState.sessions: Mutex<SessionManager>`

重要实现事实（影响分页/总数能力）：

- server 启动时会把 **SQLite 中的 sessions 全部读出来**，并把 **每个 session 的 messages 全部读出来**，再 hydrate 成内存 session：  
  - `crates/rocode-server/src/server.rs`：`load_sessions_from_storage()`
  - 其内部：`session_repo.list(None, 100_000)` + `message_repo.list_for_session(session_id)`
- 这意味着：  
  - “读路径”很多时候并没有直接查询 DB，而是扫内存集合  
  - “分页/offset”如果只做在 API 层，很可能只是对内存列表做切片（而不是数据库分页）

### 2.2 SQLite：真正的持久化源（`rocode.db`）

主要表（详见 `docs/storage-cache-config.md` 的 DB 章节）：

- `sessions`：会话元数据（含 `directory` / `time.updated` / `status` / `metadata` 等）
- `messages`：消息元数据 + `data`（JSON，存 `Vec<MessagePart>`）
- `parts`：可选的“可索引拆分表”（当前仓库**存在 schema + repository**，但在 server 的 flush 链路里**未形成完整写入闭环**）

实现入口：

- Schema（迁移定义）：`crates/rocode-storage-migration/src/*.rs`
- Repo：`crates/rocode-storage/src/repository.rs`（SeaORM）

### 2.3 文件：`.opencode/snapshot`（回滚/对比用）

除了 SQLite，ROCode 还有一个“项目目录内的快照仓库”，用于 diff / revert 相关能力（这类信息不会进 DB）：

- 默认路径（历史兼容命名）：`<worktree>/.opencode/snapshot/`
- 相关实现：`crates/rocode-session/src/snapshot.rs`
- 关联字段：
  - DB `sessions.revert`（JSON）里会记录 “回滚定位所需的 message_id/part_id/snapshot/diff”
  - 消息里也可能出现 `PartType::Snapshot` / `PartType::Patch`

对“分页/offset”来说，它不是核心问题；但如果你后续 server 要做“只查 DB 也能定位回滚”，就必须理解：**回滚的真正内容往往在项目目录的 snapshot 文件里**，DB 里更多是索引/指针。

---

## 3. 目录（directory）在 ROCode 里到底怎么存？

### 3.1 目录字符串的规范化：canonical absolute path

ROCode 在创建/更新会话时，会把传入的 directory 做一次“尽量规范化”：

- 入口函数：`crates/rocode-server/src/routes/session/session_crud.rs` 的 `resolved_session_directory(raw: &str) -> String`
- 规则（按代码逻辑）：
  1. `raw.trim()` 为空或 `"."`：取 `std::env::current_dir()`
  2. `raw` 是相对路径：拼到 `cwd.join(raw)`
  3. `raw` 是绝对路径：直接用
  4. 对 candidate 做 `canonicalize()`；失败则退回 candidate
  5. 最终存为 `to_string_lossy().to_string()`（平台相关路径分隔符将被保留）

会话创建点：

- `crates/rocode-server/src/routes/session/session_crud.rs` 的 `create_session`：
  - 新会话：`sessions.create("default", resolved_session_directory("."))`
  - 子会话：`sessions.create_child(parent_id)`（继承父会话的 `directory`，随后也会再做一次 resolved 归一化）

### 3.2 directory 在内存与 DB 的映射

- 内存：`rocode_session::Session.directory: String`（`crates/rocode-session/src/session.rs`）
- DB：`sessions.directory TEXT`（`crates/rocode-storage-migration/src/m20260317_000001_create_sessions.rs`）

server 启动加载 DB → 内存的路径：

- `crates/rocode-server/src/server.rs`：`load_sessions_from_storage()`
  - 先从 `sessions` 表拿到 `rocode_types::Session`（目录字段已是字符串）
  - 再 `serde_json` 转成 `rocode_session::Session`（directory 直接落进内存）

### 3.3 目录维度查询的一个“坑”：过滤是字符串全等

会话列表过滤（server 路由）：

- `crates/rocode-server/src/routes/session/session_crud.rs`：`list_sessions`
  - 会先对 `query.directory` 调用 `resolved_session_directory()` 做 canonicalize，再构造 `rocode_session::SessionFilter { directory, ... }`
- `crates/rocode-session/src/session.rs`：`SessionManager::list_filtered`
  - `if let Some(ref dir) = filter.directory { if s.directory != *dir { return false } }`

也就是说：directory 过滤最终仍是**“完全字符串匹配”**；但路由层已经做了 canonicalize，保证 `.`/相对路径/未规范化路径也能稳定匹配到会话目录。仍然不支持前缀匹配（目录树查询）与大小写归一化。

对后续 server 开发的建议（强烈）：

- 如果你希望客户端传“相对路径/未规范化路径”也能过滤：  
  ✅ 当前已在 `list_sessions` 路由层对 `query.directory` 调用 `resolved_session_directory()` 再过滤。
- 如果你希望支持“列出某目录树下所有会话”：  
  需要定义新的查询语义（例如 `directory_prefix`）并确保路径分隔符/大小写一致；否则跨平台很容易踩坑。

---

## 4. 会话（session）的存储形式：字段、落盘、加载、更新

### 4.1 Session 核心字段（你后续做 API 很可能要用到）

会话结构（内存态）：

- `crates/rocode-session/src/session.rs`：`pub struct Session`

其中与 server API/分页最相关的字段是：

- `id`：`ses_...`（字符串主键）
- `project_id`：当前代码里常见 `"default"`（但 schema 支持按项目聚合）
- `directory`：会话工作目录（见第 3 节）
- `parent_id`：存在则为 child session；`roots=true` 过滤会要求 `parent_id.is_none()`
- `title`：支持 search（子串匹配）
- `time.created/time.updated`：epoch millis（**updated 是列表排序/分页的天然排序键**）
- `status`：生命周期状态（active/completed/archived/compacting）
- `metadata`：扩展字段（JSON map）
- `messages: Vec<SessionMessage>`：消息数组（注意：这是“全量消息”）

持久化结构（用于 DB 序列化/反序列化）：

- `crates/rocode-types/src/session.rs`：`pub struct Session`

它与 rocode-session 的 Session 字段基本一致，方便 `serde_json` 互转。

### 4.2 SQLite 的写入方式：upsert +（可选）flush 全量消息

两条主要路径：

1) **增量 upsert（prompt 运行中 coalesce 持久化）**  
   `crates/rocode-server/src/routes/session/prompt.rs`：
   - background worker 会对最新 snapshot 做：
     - `SessionRepository::upsert(&stored_session)`
     - `MessageRepository::upsert(&msg)`（对该 snapshot 里的 messages 逐条 upsert）

2) **flush（prompt 结束时把该 session “强一致”落盘）**  
   `crates/rocode-server/src/server.rs`：`flush_session_to_storage(session_id)`
   - 内部调用 `SessionRepository::flush_with_messages(&session, &messages)`  
   - 该 flush 逻辑会：
     - upsert session
     - upsert messages
     - 删除 DB 中“该 session 下已不存在的消息”（防止内存删除后 DB 仍残留）

因此对 server 语义的影响是：

- DB 里 session/messages 视图通常是“最终一致”；prompt 进行中可能已经写入部分增量。
- 但“活动状态”并不是 DB 里的强一致概念（见第 5.4、5.5）。

### 4.3 SQLite 的读取方式：启动时全量灌入内存

`crates/rocode-server/src/server.rs`：`load_sessions_from_storage()`

- `SessionRepository::list(None, 100_000)`：一次拿很多 sessions（按 `updated_at DESC`）
- 对每个 session：`MessageRepository::list_for_session(session_id)`：**读全量消息**
- 然后把 `rocode_types::Session` + messages 转成 `rocode_session::Session` 并放进 `SessionManager`

对后续 server 的“分页/offset/总数”影响非常关键：

- 只要你走 `SessionManager`，你做分页基本就是对内存集合做“过滤 + 排序 + 切片”
- 对大量数据：
  - 启动成本会变高（尤其是 messages 多的 session）
  - API 的分页即使做了，也可能是 O(n) 扫描（不是真正的 DB 分页）

---

## 5. 消息（message）与 parts 的存储形式：JSON parts、parts 表、以及“活动 part”

### 5.1 一条消息不是字符串，而是 `Vec<MessagePart>`

内存模型：

- `crates/rocode-session/src/message.rs`：`SessionMessage { parts: Vec<MessagePart> }`
- `MessagePart { id, part_type, created_at, message_id }`
- `PartType` 常见变体：
  - `Text { text, synthetic?, ignored? }`
  - `ToolCall { id, name, input, status, raw?, state? }`
  - `ToolResult { tool_call_id, content, is_error, ... }`
  - 以及 `Reasoning/File/Patch/Snapshot/...`

API 输出（面向前端/TUI）：

- `crates/rocode-server/src/routes/session/messages.rs`：`part_to_info(...)`
  - 会把 `ToolCall.status` 映射成 `"pending"|"running"|"completed"|"error"`

### 5.2 SQLite：messages 表的核心字段是 `data`（parts JSON）

持久化写入：

- `crates/rocode-storage/src/repository.rs`：`MessageRepository::{create, upsert}`
  - `data_json = serde_json::to_string(&message.parts)`  
  - 写入列：`finish/metadata/data`

读取：

- `crates/rocode-storage/src/repository.rs`：`MessageRepository::list_for_session(session_id)`
  - SQL：`ORDER BY created_at ASC`
  - `parts = serde_json::from_str(row.data).unwrap_or_default()`

也就是说：**消息内容是“整条消息一个 JSON blob”，里边是 parts 数组**。

### 5.3 一个容易忽略的细节：DB 用的是 `rocode-types` 的 parts（字段更少）

server flush 时会做：

- `rocode_session::Session` → `serde_json::Value` → `rocode_types::Session`

而 `rocode_types::PartType::ToolCall` 的字段只有：

- `id/name/input`

这会带来一个很现实的差异：

- 内存态的 `ToolCall` 有 `status`（Pending/Running/Completed/Error），但 **落到 DB 的 `messages.data` 里会丢失 status**  
  - 原因：DB 的 `MessageRepository` 使用 `rocode_types::SessionMessage`（见 `crates/rocode-storage/src/repository.rs` 的 imports）
  - 序列化时多出来的字段会在反序列化到 `rocode_types` 时被忽略
- server 重启从 DB 还原到内存时，`rocode_session::PartType::ToolCall.status` 有 `#[serde(default)]`，因此会默认变回 `Pending`（这会让“历史 tool call 的运行态”不可追溯）

同样的，**消息级 usage 也不会持久化**：

- 内存：`rocode_session::SessionMessage.usage: Option<MessageUsage>`（`crates/rocode-session/src/message.rs`）
- DB：`rocode_types::SessionMessage` 不包含 `usage` 字段（`crates/rocode-types/src/message.rs`）
- SQLite schema 虽然有 `messages.tokens_* / cost / provider_id / model_id` 等列（见迁移 `crates/rocode-storage-migration/src/m20260317_000002_create_messages.rs`），但当前 `MessageRepository::{create, upsert}` 并未写入这些列（只写 `finish/metadata/data`）

> 对后续 server 设计来说：如果你想做“分页 + 列表里带 token/cost/模型信息”，最好别只依赖 `messages.data` 这个 JSON blob；应该把可排序/可聚合字段正规化进列，并写入。

### 5.4 `parts` 表：存在“可索引拆分表”，但目前未接入 server flush 链路

SQLite schema 里有 `parts` 表（见迁移 `crates/rocode-storage-migration/src/m20260317_000003_create_parts.rs`），设计目标是把 parts 拆出来，以便：

- 做全文/条件查询（例如只查 `toolCall`，或只查某个 `tool_name`）
- 做更细粒度分页（message 内按 `sort_order` 分页）
- 避免每次都解析 `messages.data` 的 JSON blob

同时仓库也实现了 `PartRepository`：

- `crates/rocode-storage/src/repository.rs`：`PartRepository::{upsert, list_for_message, list_for_session, ...}`

但目前全仓库搜索不到 `PartRepository` 的实际调用点（除了定义本身），意味着：

- **在现有 server 写入路径里，parts 表大概率不会被填充**
- 你如果要在 server 上做“按 tool/status 搜索、按 part 分页”，目前只能：
  - 解析 `messages.data`（应用层拆 JSON）  
  - 或者先补齐 parts 表的写入闭环

### 5.5 “活动 part / 正在执行的工具调用”当前靠什么暴露？

当前仓库里，“活动 part”更像是两条并行的运行时视图：

1) **消息 parts 内的 ToolCall.status（内存态）**  
   - 类型：`rocode_session::ToolCallStatus`（`crates/rocode-session/src/message.rs`）  
   - API 输出：`crates/rocode-server/src/routes/session/messages.rs` → `part_to_info`
   - 注意：如 5.3 所述，这个 status 不会可靠持久化（重启后会丢）

2) **Server 聚合运行时状态：`RuntimeStateStore`（纯内存）**  
   - 类型：`crates/rocode-server/src/session_runtime/state.rs`
   - 提供：`GET /session/{id}/runtime`（路由：`crates/rocode-server/src/routes/session/session_crud.rs` 的 `get_session_runtime`）
   - 关键字段：
     - `run_status`：Idle/Running/WaitingOnTool/WaitingOnUser/...
     - `current_message_id`：当前正在生成/处理的消息
     - `active_tools[]`：当前活跃的 tool call（`tool_call_id/tool_name/started_at`）

> 如果你要做 server：“页面上展示当前 session 正在跑哪个工具、跑了多久、是否卡在 permission/question”，建议把 `/runtime` 作为主信息源，而不是试图从历史 messages 反推。

### 5.6 现有 API 对消息分页的“真实能力”：支持 `after + limit`，但不是 offset

消息列表路由：

- `crates/rocode-server/src/routes/session/messages.rs`：`list_messages`

它支持：

- `limit: Option<usize>`：限制返回条数
- `after: Option<String>`：以 message id 为锚点，从该 message **之后**开始返回

实现方式是：

- 从内存 `session.messages: Vec<SessionMessage>` 顺序扫描
- 找到 `after` 的 message id 后开始 push，直到达到 limit
- 如果 `after` 找不到，会 fallback 返回从头开始的前 `limit` 条（让客户端能自愈）

这类分页的特点：

- 优点：适合“增量拉取”（类似 cursor pagination）
- 缺点：
  - 锚点是 message id，不是排序键（created_at）——虽然目前消息 vector 是按时间 append 的
  - 没有总数（total）
  - 不是 offset-based
  - 扫描复杂度 O(n)（对超长 session 会变慢）

---

## 6. 能不能“很好支持”总数 / 分页 / offset：分层评估（Schema vs Repo vs Server API）

下面我按 **sessions / messages / parts** 三个对象，分别从三层能力评估：

- **Schema 层**：SQLite 表和索引是否允许高效实现
- **Repository 层**：`rocode-storage` 是否提供现成方法
- **Server API/内存层**：当前路由实现是否真的做到（以及性能形态）

### 6.1 Sessions（会话列表）

**总数（COUNT）**

- Schema：✅ 当然可以 `SELECT COUNT(*) FROM sessions WHERE ...`
- Repository：✅ 已提供
  - `SessionRepository::count(project_id: Option<&str>)`
  - `SessionRepository::count_for_directory(directory: &str)`
- Server API：❌ 当前没有返回 total 的接口（`list_sessions` 只返回列表）

**分页（LIMIT / cursor）**

- Schema：✅ `ORDER BY updated_at DESC LIMIT ? OFFSET ?` 或 keyset 都可
- Repository：✅ 已提供 offset-based 分页方法
  - `SessionRepository::list_page(project_id, limit, offset)`
  - `SessionRepository::list_for_directory_page(directory, limit, offset)`
- Server API（内存层）：✅ `GET /sessions` 现在支持
  - `limit`（生效）
  - `offset`（新增参数）
  - 稳定排序：按 `time.updated DESC`（并用 `time.created` / `id` 做 tie-break）
  - `directory` 过滤会先 `resolved_session_directory()` 规范化（减少路径不一致导致的“查不到”）

**offset 获取**

- Schema：✅ 可实现
- Repository：✅ 已实现（见上）
- Server API：✅ 已实现（但目前是对内存列表做切片；数据规模极大时仍会 O(n) 过滤+排序）

> 结论：会话列表现在已具备“可用的 offset + limit + count”能力。  
> 如果未来要做真正的 server 化（避免启动时全量加载 sessions/messages），建议把 `GET /sessions` 的读路径下沉到 storage 层（或改为 cursor/keyset 分页）。

### 6.2 Messages（某会话的消息列表）

**总数（COUNT）**

- Schema：✅ `SELECT COUNT(*) FROM messages WHERE session_id = ?`
- Repository：✅ `MessageRepository::count_for_session(session_id)`
- Server API：❌ `GET /session/{id}/messages` 不返回 total

**分页**

- Schema：✅（建议使用 `(created_at, id)` 做 keyset；也可以 OFFSET）
- Repository：✅ 新增 offset-based 分页
  - `MessageRepository::list_for_session_page(session_id, limit, offset)`
- Server API：🟡 仍主要是 `after + limit`（见 5.6），但它是：
  - 基于内存 vector 的扫描分页
  - 锚点是 message id（而不是时间戳/复合键）
  - 不返回 total

**offset 获取**

- Schema：✅ 可实现
- Repository：✅ 已实现（见上）
- Server API：❌ 未实现（只有 after）

> 结论：storage 层现在已经能支持“total + limit/offset”风格的标准分页；  
> API 层如果需要“标准分页 + total”，可以直接复用 repository，并基于新增索引做查询。

### 6.3 Parts（message parts / tool calls 等）

**总数（COUNT）**

- Schema：✅ 可对 `parts` 表 count，也可对 `messages.data` 做应用层统计
- Repository：❌ `PartRepository` 没有 count 方法
- Server API：❌ 没有 parts 分页/统计 API

**分页 / offset**

- Schema：✅ `ORDER BY sort_order LIMIT/OFFSET` 当然可实现
- Repository：❌ `PartRepository::list_for_message/list_for_session` 无 limit/offset
- Server：🟡 内存里 parts 是 message JSON 的一部分，理论上可在 API 层 slice，但目前并没有对应接口

更关键的是：如 5.4 所述，**parts 表目前没有写入链路**，所以即使 schema 能做，也拿不到数据。

---

## 7. 面向后续 server 开发的建议：怎么让“总数 + 分页 + offset/cursor”变得靠谱

这里给一套“尽量不破坏现有模型，但能让 server 做标准列表接口”的建议路径。你可以按优先级分阶段落地。

### 7.1 会话列表的“最小可用分页能力”（已落地）

✅ 当前分支已经完成以下最小闭环（不改 DB，先让 API 行为正确且可分页）：

- `crates/rocode-session/src/session.rs`：`SessionManager::list_filtered`
  - 过滤后按 `time.updated DESC` 排序（`time.created` / `id` tie-break）
  - 支持 `limit` + `offset`
- `crates/rocode-server/src/routes/session/session_crud.rs`：`list_sessions`
  - 支持 `offset` query 参数（与 `limit` 组合做 offset-based pagination）
  - `directory` query 会先 `resolved_session_directory()` 规范化（减少路径不一致导致的“查不到”）

这能让前端稳定拿到“最近会话”列表，并且分页语义明确。

### 7.2 把分页下沉到 storage 层（推荐：keyset/cursor）

对于真正的 server（尤其是未来需要横向扩展/多进程/不希望全量载入内存的情况），推荐把列表读写走 DB。

✅ 当前分支已先补齐 offset-based 的 DB 查询能力（便于快速落地标准 REST 分页）：

- `crates/rocode-storage/src/repository.rs`：
  - `SessionRepository::{count, list_page, count_for_directory, list_for_directory_page}`
  - `MessageRepository::{count_for_session, list_for_session_page}`
- 索引迁移：`crates/rocode-storage-migration/src/m20260317_000010_add_pagination_indexes.rs`

下一步更推荐 keyset/cursor（避免大 offset 线性跳过成本）：

**Sessions：建议的 storage API 形态**

- `count_sessions(filter...) -> i64`
- `list_sessions(filter..., limit, cursor|offset) -> Vec<SessionHeader>`

并使用 keyset pagination（避免 OFFSET 在大数据下的线性跳过成本）：

- 排序：`ORDER BY updated_at DESC, id DESC`
- cursor：`(updated_at, id)`

SQL 示意（descending keyset）：

```sql
SELECT id, project_id, parent_id, slug, directory, title, version, status, created_at, updated_at
FROM sessions
WHERE 1=1
  AND (?1 IS NULL OR project_id = ?1)
  AND (?2 IS NULL OR directory = ?2)
  AND (?3 IS NULL OR updated_at < ?3 OR (updated_at = ?3 AND id < ?4))
ORDER BY updated_at DESC, id DESC
LIMIT ?5;
```

**Messages：建议的 storage API 形态**

- `count_messages(session_id) -> i64`
- `list_messages(session_id, limit, cursor|offset) -> Vec<MessageHeader>`
- `get_message(id) -> MessageFull`（需要内容时再取 `data`）

排序键建议用 `(created_at, id)`，避免同毫秒写入导致的不稳定分页。

### 7.3 重新审视 “messages.data 是 JSON blob” 对分页/搜索的影响

当前设计的优点：

- 写入简单：一个 message 一行，`data` 存完整 parts
- 还原简单：读出 message 后就能完整恢复 UI 展示所需结构

但对 server 化/分页/统计有明显代价：

- 想要“只列出消息摘要/头信息”也会把 `data` 拉回来（除非专门写 projection SQL）
- 想做“按 tool_name / part_type 过滤/搜索”要么：
  - 全表扫 JSON（性能差）
  - 要么维护冗余索引表（就是当前 `parts` 表的初衷）

因此建议二选一（或两者结合）：

1) **补齐 parts 表写入闭环**（推荐，改动较小，能显著提升可查询性）  
   - 在 message upsert/flush 时，把 `Vec<MessagePart>` 拆成 `parts` 表多行（填 `sort_order/tool_*` 等列）  
   - 并在 `parts.data` 存一份完整 part JSON，便于还原  
   - 之后：
     - 列表/搜索用 parts 表
     - 完整渲染仍可用 messages.data

2) **增强 messages 表的正规化列**（把常用聚合字段写入列）  
   - 例如把 `provider_id/model_id/tokens_*/cost` 在写入时就填上  
   - 这样“消息列表页”无需解析 `metadata/data` 就能显示模型与成本

### 7.4 “活动 part”如果要可恢复（重启后仍能看到 running）：需要新增持久化

当前活动状态基本都在内存：

- `ToolCall.status`（内存）
- `RuntimeStateStore`（内存）

如果你未来要做“server 重启后仍能恢复 in-flight 状态/展示 running tools”，需要额外持久化设计，例如：

- 新表 `session_runtime`（session_id, run_status, current_message_id, active_tools_json, updated_at）
- 或把 tool call status 写入 `parts.tool_status` 并确保 parts 表写入闭环存在
- 或直接让 `rocode-types::PartType::ToolCall` 增加 status 字段（会涉及格式兼容，需要迁移策略）

---

## 8. 建议你后续 server 开发时的接口形态（便于前端与审计）

这里给一个“既能拿 total，又能分页”的比较通用的 REST 形态（示例）：

- `GET /sessions?directory=...&limit=50&cursor=...`
  - 返回：`{ items: [...], next_cursor: "...", total: 123 }`
  - total 可以按需：提供 `include_total=true` 才计算（避免每次都 COUNT）

- `GET /sessions/{id}/messages?limit=50&cursor=...`
  - 默认返回 message header（不带 `data`）  
  - 需要完整内容再：`GET /messages/{id}`

- `GET /sessions/{id}/runtime`
  - 当前仓库已经有，适合做“活动 part”展示

如果你短期内只做单机 server、并且接受“所有 sessions/messages 都在内存”，也可以先把现有 after+limit 用起来；但一旦数据规模上来，还是建议把分页落到 DB。
