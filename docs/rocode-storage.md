# ROCode 存储总览（SeaORM 版）

> 生成时间：`2026-03-18`  
> 分支基线：`rewrite/sea-orm-storage`  
>
> 本文是一个“入口文档”，把你最关心的 **目录 / 会话 / 消息 / parts** 的存储形态与分页能力先讲清楚，并给出源码落点。更细节的字段表格与配置/缓存路径请看下方链接。

## 1. 你要找的两份详细文档

- `docs/storage-cache-config.md`：配置（JSONC）/ 缓存 / 各类落盘路径 / SQLite 表结构逐字段说明
- `docs/session-message-storage.md`：目录/会话/消息/parts（含“活动 part”）的存储形态 + 分页/offset/total 能力评估

## 2. 会话是如何存储的？（一句话 + 分层）

一句话：**运行时以 `SessionManager`（内存 HashMap）为主；持久化以 SQLite `rocode.db` 为主；两者通过 flush/upsert 同步。**

分层看：

1. **内存层（server 主工作集）**
   - 结构：`crates/rocode-session/src/session.rs` → `SessionManager { sessions: HashMap<String, Session> }`
   - 写路径：各路由/runner 在内存里增删改 `Session`/`SessionMessage`

2. **持久化层（SQLite）**
   - DB 文件：`dirs::data_local_dir()/rocode/rocode.db`
   - Schema（Source of truth）：`crates/rocode-storage-migration/src/*.rs`（SeaORM migrations）
   - 访问：`crates/rocode-storage/src/repository.rs`（SeaORM 查询）

3. **同步层（把内存写回 DB / 把 DB 灌入内存）**
   - 启动灌入：`crates/rocode-server/src/server.rs` → `load_sessions_from_storage()`
   - prompt 结束 flush：`crates/rocode-server/src/server.rs` → `flush_session_to_storage(session_id)`

## 3. 如何知道一个目录下有哪些会话？

关键点：**directory 是 canonical absolute path 的字符串，全等匹配。**

### 3.1 Server API（内存过滤）

- 路由：`crates/rocode-server/src/routes/session/session_crud.rs` → `list_sessions`
- 过滤逻辑：
  - 路由层先把 `?directory=` 通过 `resolved_session_directory()` 规范化（支持传 `.` / 相对路径）
  - 然后交给 `crates/rocode-session/src/session.rs` → `SessionManager::list_filtered` 做 `s.directory == filter.directory` 的全等匹配

因此客户端使用方式：

- 传 `directory=.`：取 server 当前工作目录的会话
- 传相对路径：相对 server 进程的 `cwd` 解析并 canonicalize 后过滤

### 3.2 Storage 层（DB 过滤）

如果你后续要把“列表查询”下沉到 DB（避免全量加载到内存），可直接用：

- `SessionRepository::count_for_directory(directory)`
- `SessionRepository::list_for_directory_page(directory, limit, offset)`

实现：`crates/rocode-storage/src/repository.rs`

配套索引迁移：`crates/rocode-storage-migration/src/m20260317_000010_add_pagination_indexes.rs`（`(directory, updated_at)`）

## 4. 分页 / offset / total 目前支持到什么程度？

### 4.1 Sessions（会话列表）

- Server API：`GET /sessions?limit=&offset=` ✅（内存过滤+排序+切片；total 通过 `X-Total-Count` header 返回）
- Storage：`SessionRepository::{count, list_page}` ✅（offset-based；可拿 total）

### 4.2 Messages（某会话消息列表）

- Server API：`GET /session/{id}/messages?after=&limit=` 或 `?offset=&limit=` 🟡（内存扫描/切片；total 通过 `X-Total-Count` header 返回）
- Storage：`MessageRepository::{count_for_session, list_for_session_page}` ✅（offset-based；可拿 total）

### 4.3 Parts（message parts）

- Schema 有 `parts` 表，并且本分支已形成 **写入闭环 + 懒加载读 API**（详见 `docs/session-message-storage.md` 的第 5 节）：
  - 写入：`SessionRepository::flush_with_messages(...)` 会 upsert messages + parts，并删除 stale parts
  - 历史回填：迁移 `m20260318_000012_backfill_parts_from_messages_data`
  - 读 API：
    - `GET /session/{id}/message/summary`：仅 message headers（避免拉取 `messages.data`）
    - `GET /session/{id}/message/{msgID}/part`：仅 parts summaries（轻量）
    - `GET /session/{id}/message/{msgID}/part/{partID}`：单个 part 详情（按需返回 `data`）

## 5. 后续 server 开发的建议（最实用的两条）

1. **如果你要做标准 REST 列表（total + limit/offset）**：优先走 storage 层的 `count_* + list_*_page`，并利用新增索引（目录/会话维度）。
2. **如果你要做大规模分页**：尽量用 keyset/cursor（`(updated_at, id)` / `(created_at, id)`），避免大 offset 的线性跳过成本。
