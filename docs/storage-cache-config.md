# ROCode 存储 / 缓存 / 配置路径与数据库结构（源码导读）

> 生成时间：`2026-03-18`  
> 代码基线：workspace `version = 2026.3.16`（见 `Cargo.toml`）  
> 分支基线：`rewrite/sea-orm-storage`（SeaORM + parts 正规化写入闭环）  
> 说明：DB schema 以 `crates/rocode-storage-migration`（SeaORM migrations）为准；`crates/rocode-storage/src/schema.rs` 仅作为 legacy SQL 参考。
>
> 本文从**当前仓库源码**出发，系统梳理 ROCode 的：
>
> - **配置文件**：默认路径、优先级（merge order）、格式（JSONC）、字段含义（schema）
> - **持久化存储**：SQLite 数据库 `rocode.db` 的表结构、字段语义、数据格式（JSON 字段）
> - **缓存与临时文件**：模型目录缓存、插件运行时缓存、TUI 状态文件、IPC 临时文件等
>
> 说明：
>
> - 目录基准来自 `dirs` crate：`config_dir / data_dir / data_local_dir / cache_dir / state_dir`。
> - 不同 OS 的基准路径不同；文中以“**函数语义 + 常见 Linux/XDG 示例**”的方式描述。
> - 代码中仍存在 `opencode` 命名的历史兼容路径（例如插件缓存、`.opencode/` 项目目录）；文中会显式标注。

---

## 0. 一句话总览：ROCode 会把东西放到哪？

按“最常被问到”的顺序：

1. **全局配置文件**（JSON/JSONC）  
   `dirs::config_dir()/rocode/rocode.jsonc` 或 `.../rocode/rocode.json`  
   代码：`crates/rocode-config/src/loader/file_ops.rs`

2. **项目配置文件**（JSON/JSONC，可放在项目根或 `.rocode/`）  
   `<worktree>/rocode.json(c)`、`<worktree>/.rocode/rocode.json(c)`（以及向上祖先目录）  
   代码：`crates/rocode-config/src/loader/mod.rs`

3. **会话数据库**（SQLite）  
   `dirs::data_local_dir()/rocode/rocode.db`  
   代码：`crates/rocode-storage/src/database.rs`

4. **日志**（文件日志，便于 TUI 抓 stderr 时仍可查）  
   `dirs::data_local_dir()/rocode/log/rocode.log`  
   代码：`crates/rocode-cli/src/main.rs`

5. **模型目录缓存**（来自 models.dev）  
   `dirs::cache_dir()/rocode/models.json`  
   代码：`crates/rocode-provider/src/models.rs`

6. **TUI 输入状态**（历史、frecency、stash）  
   `dirs::state_dir()/rocode/tui/prompt-*.json`  
   代码：`crates/rocode-tui/src/components/prompt.rs`

7. **项目级“快照仓库”**（用于 revert / diff 等能力）  
   `<worktree>/.opencode/snapshot/`（注意是 `.opencode`，历史兼容）  
   代码：`crates/rocode-session/src/snapshot.rs`

8. **插件运行时缓存**（宿主脚本、npm 安装目录等）  
   `dirs::cache_dir()/opencode/...`（历史兼容命名）  
   代码：`crates/rocode-plugin/src/subprocess/loader.rs`

---

## 1. 目录基准（`dirs` crate）与常见默认值

ROCode 在源码中大量使用以下基准目录函数（并在其下再 `join("rocode")` 或 `join("opencode")`）：

- `dirs::config_dir()`：配置目录基准（Linux 常见 `~/.config`）
- `dirs::data_dir()`：数据目录基准（Linux 常见 `~/.local/share`）
- `dirs::data_local_dir()`：本地数据目录基准（Linux 通常同 `~/.local/share`）
- `dirs::cache_dir()`：缓存目录基准（Linux 常见 `~/.cache`）
- `dirs::state_dir()`：状态目录基准（Linux 常见 `~/.local/state`）

> Linux/XDG 常见展开（仅示例，实际以 `dirs` 返回为准）：
>
> - config：`~/.config`
> - data / data_local：`~/.local/share`
> - cache：`~/.cache`
> - state：`~/.local/state`

快速查看当前机器上 `dirs` 的实际值：  
`rocode debug paths`（见 `crates/rocode-cli/src/debug.rs`）

---

## 2. 配置系统（`rocode-config`）：路径、优先级、格式与字段含义

### 2.1 配置文件格式：JSONC + 两种预处理

ROCode 的主配置采用 **JSONC**（允许注释）并允许 **尾随逗号**（trailing commas）。  
解析入口：`crates/rocode-config/src/loader/file_ops.rs` → `parse_jsonc(...)`

加载单个配置文件时，会先做两类文本级预处理（然后再 JSONC parse）：

#### 2.1.1 `{env:VAR}` 环境变量替换

- 规则：把配置文本中的 `{env:VAR_NAME}` 替换为环境变量值；若环境变量不存在则替换为空字符串。
- 代码：`crates/rocode-config/src/loader/file_ops.rs` → `substitute_env_vars(...)`

示例：

```jsonc
{
  "provider": {
    "openai": { "api_key": "{env:OPENAI_API_KEY}" }
  }
}
```

#### 2.1.2 `{file:path}` 文件内容内联

- 规则：把 `{file:...}` 替换为目标文件内容（`trim()` 后），并做 JSON 字符串转义（`\n`, `\"` 等）。
- 路径解析：
  - `~/` 开头：用 home 展开
  - 绝对路径：直接读
  - 相对路径：相对 **当前配置文件所在目录**（`base_dir`）
- **注释行跳过**：如果 `{file:...}` 出现在 `// ...` 注释行中，会跳过。
- 代码：`crates/rocode-config/src/loader/file_ops.rs` → `resolve_file_references(...)`

适用场景：把长 prompt / 证书 / JSON 片段放到单独文件中管理，再注入配置。

---

### 2.2 配置来源与优先级（Merge Order）

核心入口：`crates/rocode-config/src/loader/mod.rs` → `ConfigLoader::load_all(...)`

#### 2.2.1 不带远端 `.well-known` 的加载顺序（`load_all`）

从低优先级到高优先级（后加载的覆盖先加载的）：

1. **全局配置**：`~/.config/rocode/rocode.jsonc` 或 `rocode.json`  
   代码：`crates/rocode-config/src/loader/file_ops.rs` → `get_global_config_paths()`

2. **自定义配置文件路径**：环境变量 `ROCODE_CONFIG` 指向的文件（如果存在）

3. **项目配置**：从 `project_dir` 向上查找并加载（直到 git worktree root）：
   - `rocode.jsonc`
   - `rocode.json`
   - `.rocode/rocode.jsonc`
   - `.rocode/rocode.json`
   代码：`crates/rocode-config/src/loader/mod.rs`（`PROJECT_CONFIG_TARGETS` + `find_up`）

   关键细节：对每个 target，会把“祖先目录中的配置”先加载，再加载“更靠近当前目录的配置”，以保证**子目录覆盖父目录**。

4. **`.rocode` 目录扫描**：收集若干“配置目录”，并在每个目录里：
   - 读 `rocode.jsonc` / `rocode.json`
   - 读 markdown 形式的 commands / agents / modes
   代码：`crates/rocode-config/src/loader/discovery.rs` → `collect_rocode_directories(...)`

5. **插件发现（path-driven）**：
   - 默认插件目录（config dir / home / project）
   - 配置里 `plugin_paths` 指定的额外目录
   - 自动发现 `.ts` / `.js` 文件并转成 `file://...` spec  
   代码：`crates/rocode-config/src/loader/discovery.rs` → `collect_plugin_roots(...)`、`load_plugins_from_path(...)`

6. **内联配置**：环境变量 `ROCODE_CONFIG_CONTENT`（字符串形式 JSON/JSONC）

7. **托管配置目录（企业）**：最高优先级（见 2.4）  
   代码：`crates/rocode-config/src/loader/discovery.rs` → `get_managed_config_dir()`

8. **后处理（兼容迁移 + flag 覆盖）**  
   代码：`crates/rocode-config/src/loader/transforms.rs` → `apply_post_load_transforms(...)`

#### 2.2.2 带远端 `.well-known/opencode` 的加载（`load_all_with_remote`）

入口：`crates/rocode-config/src/loader/mod.rs` → `load_all_with_remote(...)`

在 2.2.1 的顺序之前，会先加载一个“最低优先级”的远端配置：

- 从本地 `auth.json` 中读取若干 `type: "wellknown"` 条目（URL → key/token）。
- 对每个 URL 请求 `{url}/.well-known/opencode`，期望返回形如 `{ "config": { ... } }` 的 JSON。
- 拉到的 `config` 先 merge 进 Config，再按 2.2.1 的顺序继续覆盖。

代码：`crates/rocode-config/src/wellknown.rs`

重要兼容点（容易踩坑）：

- `wellknown` 读取的 `auth.json` 默认路径是：`dirs::data_dir()/opencode/auth.json`  
  （注意目录名是 `opencode`，这是历史兼容命名）

---

### 2.3 全局配置文件路径（Global）

全局配置基准路径由 `get_global_config_paths()` 决定：  
`dirs::config_dir()/rocode/rocode.{jsonc|json}`

代码：`crates/rocode-config/src/loader/file_ops.rs`

并且包含一个 **legacy TOML 全局配置迁移**：

- 旧路径：`~/.config/rocode/config`（TOML，无扩展名）
- 新路径：`~/.config/rocode/rocode.json`
- 行为：读取 legacy TOML → 转为 JSON → merge 到当前 config → 写出 JSON → 删除旧 TOML

代码：`crates/rocode-config/src/loader/file_ops.rs` → `migrate_legacy_toml_config(...)`

---

### 2.4 托管配置目录（Managed / Enterprise）

托管目录用于企业部署场景，优先级最高。

默认路径（由 OS 决定）：

- macOS：`/Library/Application Support/rocode`
- Windows：`%ProgramData%/rocode`
- Linux/其他：`/etc/rocode`

支持测试环境覆盖：

- `ROCODE_TEST_MANAGED_CONFIG_DIR=<path>`

代码：`crates/rocode-config/src/loader/discovery.rs` → `get_managed_config_dir()`

加载文件名与 `.rocode` 目录一致：`rocode.jsonc` / `rocode.json`

---

### 2.5 `.rocode` 目录扫描：目录来源与内容结构

`.rocode` 目录（注意：这里指“一个配置目录”，不一定是项目根下的那个）来自：

- 全局 config 文件所在目录：`dirs::config_dir()/rocode/`（如果存在）
- 项目路径向上查找的 `.rocode` 目录（直到 git root）
- Home 下的 `~/.rocode`（如果存在）
- `ROCODE_CONFIG_DIR` 指定的目录

代码：`crates/rocode-config/src/loader/discovery.rs` → `collect_rocode_directories(...)`

对每个 `.rocode` 目录，加载以下内容：

1. **配置文件**：`rocode.jsonc` / `rocode.json`
2. **commands**：`command/` 或 `commands/` 下的 `**/*.md`
3. **agents**：`agent/` 或 `agents/` 下的 `**/*.md`
4. **modes**：`mode/` 或 `modes/` 下的 `**/*.md`（加载后强制标记为 Primary agent）

Markdown 解析规则（frontmatter + body）：

- command：YAML frontmatter → `CommandConfig`，正文 body → `template`
- agent/mode：YAML frontmatter → `AgentConfig`，正文 body → `prompt`

代码：`crates/rocode-config/src/loader/markdown_parser.rs`

命名规则：name 由相对路径推导（保留子目录层级），去掉 `.md`。  
示例：`.rocode/commands/git/review.md` → 命令名 `git/review`  
代码：`derive_name_from_path(...)`

---

### 2.6 插件发现（File Plugins）路径与 spec 格式

插件“扫描目录”来源（从低到高优先级）：

- `dirs::config_dir()/rocode/plugins`、`.../rocode/plugin`
- `~/.rocode/plugins`、`~/.rocode/plugin`
- 项目向上每个 `.rocode`：`.rocode/plugins`、`.rocode/plugin`
- 配置项 `plugin_paths` 指定的目录（相对路径相对 `project_dir` 解析）

代码：`crates/rocode-config/src/loader/discovery.rs` → `collect_plugin_roots(...)`

扫描规则：

- 只识别 **直接文件**，扩展名 `.ts` 或 `.js`
- 同时兼容三个位置：
  - `<root>/*.ts|js`
  - `<root>/plugin/*.ts|js`
  - `<root>/plugins/*.ts|js`

扫描结果会转成 `file://<absolute-path>` 的 spec。  
代码：`load_plugins_from_path(...)`

插件配置结构：`Config.plugin` 支持两种写法：

- 新格式：`{ "pluginName": { "type": "...", ... } }`（map）
- 旧格式：`["pkg@version", "file:///path/to/plugin.ts"]`（list，加载时转 map）

代码：`crates/rocode-config/src/schema/plugin.rs` → `deserialize_plugin_map(...)`

---

### 2.7 Config schema（字段含义速查）

Rust schema 定义位置：

- `crates/rocode-config/src/schema/mod.rs`（主配置）
- `crates/rocode-config/src/schema/plugin.rs`（MCP/LSP/formatter/permission/enterprise 等）

下面按“你能在 `rocode.json(c)` 里写什么”整理（非穷尽枚举字段内的超长 keybind 列表）。

#### 2.7.1 顶层字段（`Config`）

- `$schema`：JSON schema 地址（可选）
- `theme`：主题名
- `keybinds`：键位配置（大量可选字段，均为字符串）
- `logLevel`：日志等级（字符串）
- `tui`：TUI 行为配置（侧边栏、滚动速度等）
- `server`：`rocode serve/web` 服务器配置（端口、host、mDNS、CORS）
- `command`：命令配置 map（`CommandConfig`）；可来自 config 或 `.rocode/commands/**/*.md` frontmatter
- `skills`：额外 skills 根目录（paths）以及远程 skill URL 列表（urls）
- `docs.contextDocsRegistryPath`：context_docs registry 文件路径
- `schedulerPath`：scheduler profile 路径（加载时会相对 config 文件目录归一化为绝对路径字符串）
- `taskCategoryPath`：任务分类 registry 路径（同上）
- `skill_paths` / `skillPaths`：技能根目录映射（name → path）
- `watcher.ignore`：watcher 忽略 glob 列表
- `plugin`：插件配置 map（支持 legacy list）
- `plugin_paths` / `pluginPaths`：插件扫描根目录映射（name → path）
- `snapshot`：是否启用 snapshot 行为（布尔）
- `share`：分享模式（`manual|auto|disabled`）
- `autoshare`：deprecated（会迁移到 `share`）
- `autoupdate`：自动更新（`bool` 或 `"notify"`）
- `disabled_providers` / `enabled_providers`：provider 启用/禁用列表
- `model` / `small_model`：默认模型（通常 `"provider/model"`）
- `default_agent`：默认 agent 名称
- `username`：显示用户名覆盖
- `mode`：deprecated（会迁移到 `agent`）
- `agent`：agent map（name → `AgentConfig`）
- `composition.skillTree`：skill tree（组合/编排）配置
- `provider`：provider map（自定义 provider、models、options 等）
- `mcp`：MCP server map
- `formatter`：formatter 配置（bool 或 map）
- `lsp`：LSP 配置（bool 或 map）
- `instructions`：额外 instruction 源列表（文件/glob/URL）
- `layout`：布局模式（`auto|stretch`）
- `uiPreferences`：UI 偏好（主题、recent models 等）
- `permission`：权限规则（tool → ask/allow/deny 或子 key 规则）
- `tools`：legacy（会迁移到 permission）
- `webSearch`：web search MCP 端点配置
- `enterprise`：企业配置（当前 loader 未使用 `managed_config_dir` 字段）
- `compaction`：压缩/剪枝配置
- `experimental`：实验开关
- `env`：额外 env map（键值对）

#### 2.7.2 后处理与环境变量覆盖（`apply_post_load_transforms`）

代码：`crates/rocode-config/src/loader/transforms.rs`

- `mode` → `agent` 迁移（并把 modes 标为 Primary）
- `ROCODE_PERMISSION`：若设置则解析 JSON 合并到 `config.permission`
- legacy `tools` → `permission` 迁移（并保证显式 permission 优先）
- 默认 `username`：从 `USER` 或 `USERNAME` 环境变量推断
- `autoshare: true` 且 `share` 未设 → `share = auto`
- `ROCODE_DISABLE_AUTOCOMPACT` → `compaction.auto=false`
- `ROCODE_DISABLE_PRUNE` → `compaction.prune=false`

---

### 2.8 Skills（`SKILL.md`）系统：存储路径、扫描顺序与文件格式

实现：`crates/rocode-tool/src/skill.rs`

#### 2.8.1 Skill Roots（会扫描哪些目录？）

ROCode 会从一组“技能根目录（skill roots）”里递归寻找 `SKILL.md` 文件。根目录来源按 **低优先级 → 高优先级** 追加，后加入的 root 在“同名 skill”冲突时会覆盖先前的定义。

根目录列表（源码顺序）：

1. **全局 config 目录下**（如果 `dirs::config_dir()` 可用）：
   - `<config_dir>/rocode/skill`
   - `<config_dir>/rocode/skills`
2. **Home 下**（如果 `dirs::home_dir()` 可用）：
   - `~/.rocode/skill`
   - `~/.rocode/skills`
   - `~/.agents/skills`
   - `~/.claude/skills`
3. **项目目录（base = cwd/worktree）下**：
   - `<worktree>/.rocode/skill`
   - `<worktree>/.rocode/skills`
   - `<worktree>/.agents/skills`
   - `<worktree>/.claude/skills`
4. **配置文件补充的额外路径**：
   - `config.skills.paths[]`（数组，每个元素是一个目录路径）
   - `config.skill_paths{ name: path }`（map，会按 key 排序后追加 path）

去重规则：路径去重但保持顺序（先出现者保留）。

> 兼容提示：
>
> - `skills.urls[]` 字段目前在 schema 中存在（`SkillsConfig`），但当前 Rust 的 skill 扫描逻辑 **不会** 自动从 URL 拉取 skills（仓库内未发现使用点）。如需支持远端 skill，需要额外实现/接入。
> - legacy `.opencode/skills` 不会被自动加入根目录；但你可以通过 `skill_paths` 或 `skills.paths` 显式加入（例如把 `"legacy-opencode": ".opencode/skills"` 写进 config）。

#### 2.8.2 `SKILL.md` 格式（frontmatter + body）

一个 skill 必须是一个 `SKILL.md` 文件，格式要求：

1. 文件开头第一行必须是 `---`
2. frontmatter 区域里必须包含：
   - `name: <skill-name>`
   - `description: <short description>`
3. frontmatter 以第二个 `---` 结束
4. 之后的正文是 skill 内容（会 `trim()`）

示例：

```md
---
name: frontend-ui-ux
description: Frontend / UI / UX practices
---
Use clear visual hierarchy.
Prefer small, reviewable diffs.
```

解析方式（关键点）：

- 只解析 `SKILL.md`，并且要求有上述 frontmatter 包裹（没有则忽略）
- 同名（`name` 相同）skill：后扫描到的会覆盖先扫描到的

---

## 3. Instruction 文件（AGENTS.md / CLAUDE.md / …）：路径与加载规则

> 这部分不是 `rocode.json`，但属于“提示词/规则配置”体系，实际对行为影响很大。

实现：`crates/rocode-session/src/instruction.rs`

### 3.1 项目内（Project）指令文件搜索

- 搜索目录：从 `project_dir` 向上走到 git worktree root（目录里存在 `.git`）
- “候选文件名”顺序：`AGENTS.md` → `CLAUDE.md` → `CONTEXT.md`
- 关键规则（TS parity）：**只要某一个文件名在向上路径里找到了至少一个匹配，就会加载所有匹配并停止，不再尝试下一个文件名**。
  - 举例：如果任何层级存在 `AGENTS.md`，则不会再加载 `CLAUDE.md`/`CONTEXT.md`。

### 3.2 全局（Global）指令文件搜索

全局搜索会按以下顺序尝试，并且 **“只取第一个存在的文件”**：

1. `ROCODE_CONFIG_DIR/AGENTS.md`（legacy fallback：`OPENCODE_CONFIG_DIR` 也会参与解析）
2. `dirs::config_dir()/rocode/AGENTS.md`
3. `dirs::config_dir()/opencode/AGENTS.md`（历史兼容）
4. `~/.claude/CLAUDE.md`（可通过 env 禁用）

可禁用 Claude prompt 的环境变量：

- `ROCODE_DISABLE_CLAUDE_CODE_PROMPT=1`
- `OPENCODE_DISABLE_CLAUDE_CODE_PROMPT=1`

### 3.3 `config.instructions`：额外指令源（文件/Glob/URL）

`rocode.json(c)` 的 `instructions: string[]` 可以指定：

- 绝对路径：`/abs/path/to/AGENTS.md`
- 相对路径：`rules/agent.md`（会向上 findUp）
- glob：`**/AGENTS.md`（会向上 globUp）
- URL：`https://example.com/prompt.md`（会发 HTTP GET）

相对路径/相对 glob 的行为受 `ROCODE_DISABLE_PROJECT_CONFIG` 影响：

- 若 **未禁用**：相对路径从 project_dir → worktree root 向上查找
- 若 **禁用**：相对路径需要 `ROCODE_CONFIG_DIR`（否则会跳过并 warn）

---

## 4. 持久化存储：SQLite 数据库 `rocode.db`（`rocode-storage`）

实现入口：

- Schema（迁移定义 / Source of truth）：`crates/rocode-storage-migration/src/*.rs`
- Entity Models：`crates/rocode-storage/src/entities/*.rs`
- 连接与迁移：`crates/rocode-storage/src/database.rs`（SeaORM connect + `Migrator::up`）
- Repository（读写逻辑）：`crates/rocode-storage/src/repository.rs`（SeaORM Query API）
- 数据结构（序列化格式）：`crates/rocode-types/src/*.rs`（DB 序列化/反序列化用的“持久化视图”）

### 4.1 数据库文件路径

默认路径（本地数据目录）：

- `dirs::data_local_dir()/rocode/rocode.db`

代码：`crates/rocode-storage/src/database.rs` → `get_database_path()`

CLI 可打印该路径：

- `rocode db path`（见 `crates/rocode-cli/src/db.rs`）

### 4.2 SQLite 运行参数（WAL / 同步策略）

连接后会尝试设置：

- `PRAGMA journal_mode=WAL`
- `PRAGMA synchronous=NORMAL`

代码：`crates/rocode-storage/src/database.rs` → `Database::new()`

### 4.3 表结构与字段含义（逐表）

> 时间戳字段说明：绝大多数 `*_at` / `time_*` 字段都是 **epoch millis**（整数毫秒）。
>
> 如果你关注的是“目录/会话/消息/活动 part 的存储形式，以及是否支持总数/分页/offset（面向后续 server 开发）”，建议同时阅读：`docs/session-message-storage.md`。

#### 4.3.1 `sessions`：会话元数据

迁移定义：`crates/rocode-storage-migration/src/m20260317_000001_create_sessions.rs`

|字段|类型|含义|
|---|---|---|
|`id`|TEXT PK|会话 ID（`ses_...`，见 `crates/rocode-session/src/session.rs`）|
|`project_id`|TEXT|项目标识（可用于聚合统计/筛选）|
|`parent_id`|TEXT nullable|父会话 ID（fork/child session）|
|`slug`|TEXT|slug（例如 `session-<uuid8>`）|
|`directory`|TEXT|会话工作目录（通常是 worktree 目录字符串）|
|`title`|TEXT|会话标题|
|`version`|TEXT|会话版本（默认 `'1.0.0'`）|
|`share_url`|TEXT nullable|对外 share URL（若启用分享）|
|`summary_additions`|INTEGER|summary：新增行数|
|`summary_deletions`|INTEGER|summary：删除行数|
|`summary_files`|INTEGER|summary：涉及文件数|
|`summary_diffs`|TEXT nullable|JSON 字符串：`Option<Vec<FileDiff>>`（见 `rocode-types::SessionSummary`）|
|`revert`|TEXT nullable|JSON 字符串：`SessionRevert`（用于回滚定位）|
|`permission`|TEXT nullable|JSON 字符串：`PermissionRuleset`（会话级权限规则）|
|`metadata`|TEXT nullable|JSON 字符串：`HashMap<String, Value>`（任意扩展元数据）|
|`usage_input_tokens`|INTEGER|累计输入 tokens|
|`usage_output_tokens`|INTEGER|累计输出 tokens|
|`usage_reasoning_tokens`|INTEGER|累计 reasoning tokens|
|`usage_cache_write_tokens`|INTEGER|累计 cache write tokens|
|`usage_cache_read_tokens`|INTEGER|累计 cache read tokens|
|`usage_total_cost`|REAL|累计成本（美元）|
|`status`|TEXT|`active/completed/archived/compacting`（见 `rocode-types::SessionStatus`）|
|`created_at`|INTEGER|创建时间（ms）|
|`updated_at`|INTEGER|更新时间（ms）|
|`time_compacting`|INTEGER nullable|进入 compaction 的时间（ms）|
|`time_archived`|INTEGER nullable|进入 archived 的时间（ms）|

Rust 写入/读取映射：`crates/rocode-storage/src/repository.rs` → `session_insert_model(...)` / `session_update_model(...)` / `session_from_model(...)`

#### 4.3.2 `messages`：消息元数据 + parts JSON

迁移定义：`crates/rocode-storage-migration/src/m20260317_000002_create_messages.rs`

|字段|类型|含义|
|---|---|---|
|`id`|TEXT PK|消息 ID（`msg_...`）|
|`session_id`|TEXT FK|所属会话|
|`role`|TEXT|`user/assistant/system/tool`（见 `rocode-types::MessageRole`）|
|`created_at`|INTEGER|创建时间（ms）|
|`provider_id`|TEXT nullable|provider id（预留/兼容字段；当前常见做法是写进 `metadata`）|
|`model_id`|TEXT nullable|model id（预留/兼容字段；当前常见做法是写进 `metadata`）|
|`tokens_input`|INTEGER|输入 tokens（来自 `SessionMessage.usage`；若无则为 0）|
|`tokens_output`|INTEGER|输出 tokens（来自 `SessionMessage.usage`；若无则为 0）|
|`tokens_reasoning`|INTEGER|reasoning tokens（来自 `SessionMessage.usage`；若无则为 0）|
|`tokens_cache_read`|INTEGER|cache read tokens（来自 `SessionMessage.usage`；若无则为 0）|
|`tokens_cache_write`|INTEGER|cache write tokens（来自 `SessionMessage.usage`；若无则为 0）|
|`cost`|REAL|成本（美元，来自 `SessionMessage.usage.total_cost`；若无则为 0）|
|`finish`|TEXT nullable|LLM finish reason（例如 `"stop"` / `"tool-calls"`）|
|`metadata`|TEXT nullable|JSON：`HashMap<String, Value>`（经常存 provider/model 等）|
|`data`|TEXT nullable|JSON：`Vec<MessagePart>`（核心内容，见下）|

> 实现细节：当前 Rust 的 `MessageRepository` 会写入 `finish/metadata/data`，并把 `SessionMessage.usage` 映射到 `tokens_* / cost`（见 `crates/rocode-storage/src/repository.rs` → `message_insert_model(...)`）。  
> `provider_id/model_id` 仍主要通过 `metadata` 携带（历史兼容；如需强约束可后续正规化写入列）。

`data` 的 JSON 结构来源：`rocode-types::MessagePart` / `PartType`（`crates/rocode-types/src/message.rs`）

典型片段（示意）：

```json
[
  {"id":"prt_x","type":"text","text":"hello"},
  {"id":"prt_y","type":"toolCall","id":"call_1","name":"read","input":{"file_path":"README.md"}}
]
```

#### 4.3.3 `parts`：message parts 的“可索引”拆分表（可选）

迁移定义：`crates/rocode-storage-migration/src/m20260317_000003_create_parts.rs`

该表用于把 `MessagePart` 的一些字段拆出来，方便查询/过滤（例如 tool calls）。

完整字段列表（来自 schema）：

|字段|类型|含义|
|---|---|---|
|`id`|TEXT PK|part ID（`prt_...`）|
|`message_id`|TEXT FK|所属消息|
|`session_id`|TEXT FK|所属会话|
|`created_at`|INTEGER|创建时间（ms）|
|`part_type`|TEXT|part 类型（例如 `text/toolCall/...`，见 `rocode-types::PartType`）|
|`text`|TEXT nullable|文本内容（当 part 是文本类时）|
|`tool_name`|TEXT nullable|工具名（当 part 是 tool call/result 时）|
|`tool_call_id`|TEXT nullable|工具调用 ID|
|`tool_arguments`|TEXT nullable|工具入参（JSON 字符串或文本）|
|`tool_result`|TEXT nullable|工具输出（文本）|
|`tool_error`|TEXT nullable|工具错误（文本）|
|`tool_status`|TEXT nullable|工具状态（文本）|
|`file_url`|TEXT nullable|文件 URL（当 part 是 file 时）|
|`file_filename`|TEXT nullable|文件名|
|`file_mime`|TEXT nullable|MIME|
|`reasoning`|TEXT nullable|推理文本（当 part 是 reasoning 时）|
|`sort_order`|INTEGER|同一 message 内排序（默认 0）|
|`data`|TEXT nullable|完整 part JSON（冗余，便于反序列化还原）|

当前仓库的 `PartRepository`（`crates/rocode-storage/src/repository.rs`）只暴露并写入其中一部分字段（主要是 text/tool 相关 + sort_order + created_at）。

> 实务建议：如果你需要“完整还原消息内容”，请优先使用 `messages.data`。  
> `parts` 更多是用于增量/索引/快速查询的冗余结构，具体写入路径取决于上层业务。

补充：在本分支中，server 的 `flush` 链路已把 `parts` 写入闭环（并提供 message/parts 的懒加载 API），可用于“只读 summary，按需拉详情”的大规模会话加载优化（详见 `docs/session-message-storage.md`）。

#### 4.3.4 `todos`：会话 todo 列表

迁移定义：`crates/rocode-storage-migration/src/m20260317_000004_create_todos.rs`

主键：`(session_id, todo_id)`

|字段|类型|含义|
|---|---|---|
|`session_id`|TEXT FK|所属会话|
|`todo_id`|TEXT|todo 条目 ID|
|`content`|TEXT|内容|
|`status`|TEXT|`pending/in_progress/completed/cancelled`（见 `crates/rocode-types/src/todo.rs`）|
|`priority`|TEXT|`high/medium/low`|
|`position`|INTEGER|排序位置|
|`created_at`|INTEGER|创建时间（ms）|
|`updated_at`|INTEGER|更新时间（ms）|

#### 4.3.5 `session_shares`：分享会话的 id/secret/url

迁移定义：`crates/rocode-storage-migration/src/m20260317_000006_create_session_shares.rs`

|字段|类型|含义|
|---|---|---|
|`session_id`|TEXT PK|会话 ID|
|`id`|TEXT|share id|
|`secret`|TEXT|share secret（敏感）|
|`url`|TEXT|share URL|
|`created_at`|INTEGER|创建时间（ms）|

Repo：`crates/rocode-storage/src/repository.rs` → `ShareRepository`

#### 4.3.6 `permissions`：项目级权限（当前仓库未见完整读写链路）

迁移定义：`crates/rocode-storage-migration/src/m20260317_000005_create_permissions.rs`

|字段|类型|含义|
|---|---|---|
|`project_id`|TEXT PK|项目 ID|
|`created_at`|INTEGER|创建时间（ms）|
|`updated_at`|INTEGER|更新时间（ms）|
|`data`|TEXT|JSON（项目级权限数据）|

> 注意：当前仓库 `repository.rs` 未提供 `permissions` 表的读写 Repository；该表更像预留/兼容结构。

---

### 4.4 索引（Indexes）

索引迁移：

- 基础索引：`crates/rocode-storage-migration/src/m20260317_000007_create_indexes.rs`
- 分页相关补充索引：`crates/rocode-storage-migration/src/m20260317_000010_add_pagination_indexes.rs`
- parts/todos 分页补充索引：`crates/rocode-storage-migration/src/m20260317_000011_add_part_todo_pagination_indexes.rs`

主要包含：

- sessions：按 `project_id`、`parent_id`、`updated_at DESC`、`status`
- messages：按 `session_id`、`created_at`
- messages（补充）：按 `(session_id, created_at)`（更适合 `WHERE session_id = ? ORDER BY created_at`）
- parts：按 `message_id`、`session_id`、`sort_order`
- parts（补充）：按 `(message_id, sort_order, created_at, id)` 与 `(session_id, sort_order, created_at, id)`（更适合稳定分页/避免同序号 tie）
- todos：按 `session_id`、`status`
- todos（补充）：按 `(session_id, position, todo_id)`（更适合 `WHERE session_id = ? ORDER BY position`）
- sessions（补充）：按 `(directory, updated_at)`（更适合 `WHERE directory = ? ORDER BY updated_at`）

---

### 4.5 迁移（Migrations）

迁移定义入口：

- `crates/rocode-storage-migration/src/lib.rs` → `rocode_storage_migration::Migrator`
- 每个 migration 一个文件：`crates/rocode-storage-migration/src/m20260317_*.rs`

执行逻辑：

- `crates/rocode-storage/src/database.rs` 在连接成功后调用 `rocode_storage_migration::Migrator::up(&conn, None)`。
- 迁移是增量执行的：已执行的 migration 会记录在 SeaORM 的 migration 表中（默认 `seaql_migrations`）。

兼容性迁移（用于历史 DB 升级）：

- `crates/rocode-storage-migration/src/m20260317_000008_legacy_alter_columns.rs`：为旧 DB 补列（`finish/metadata` 等），并忽略 “duplicate column / already exists”。
- `crates/rocode-storage-migration/src/m20260317_000009_migrate_tool_call_input_data.rs`：修复 `messages.data` 中历史 `toolCall.input` 的异常形态（截断字符串 / legacy sentinel 等），把它们规整成稳定 payload 以便安全回放。

分页/查询相关补充：

- `crates/rocode-storage-migration/src/m20260317_000010_add_pagination_indexes.rs`：新增 `(directory, updated_at)` 与 `(session_id, created_at)` 索引（更适合 server 侧分页查询）。
- `crates/rocode-storage-migration/src/m20260317_000011_add_part_todo_pagination_indexes.rs`：新增 parts/todos 的复合索引（更适合 message parts 与 todo 列表的稳定分页查询）。

---

## 5. 认证与凭据：哪些 JSON 文件会落盘？

### 5.1 Provider Auth Store：`auth.json`（服务器侧）

数据结构：`crates/rocode-provider/src/auth.rs` → `AuthManager` / `AuthInfo`

落盘位置由 server 决定（注意：不是 rocode-config 的 wellknown 那个）：

- `crates/rocode-server/src/server.rs` → `auth_data_dir()`

目录选择优先级：

1. 环境变量 `ROCODE_DATA_DIR`（fallback：`OPENCODE_DATA_DIR`）
2. `dirs::data_local_dir()`（fallback：`dirs::data_dir()`）
3. `std::env::temp_dir()`

然后再 `join("rocode").join("data")`，最终文件：

- `<auth_data_dir>/auth.json`

文件权限（Unix）：写入后会尝试设置 `0600`  
代码：`AuthManager::persist(...)`

内容格式（示意）：

```jsonc
{
  "openai": { "type": "api", "key": "sk-..." },
  "github-copilot": { "type": "oauth", "access": "...", "refresh": "...", "expires": 0 }
}
```

### 5.2 MCP OAuth Store：`mcp-auth.json`

实现：`crates/rocode-mcp/src/auth.rs`

默认路径：

- `dirs::data_dir()/rocode/mcp-auth.json`

内容是以“mcp server 名称”为 key 的 map，存 tokens / client_info / code_verifier / oauth_state 等。

### 5.3 wellknown 远端配置凭据：`opencode/auth.json`（历史兼容）

实现：`crates/rocode-config/src/wellknown.rs`

默认路径：

- `dirs::data_dir()/opencode/auth.json`

该文件用于远端 `.well-known/opencode` 配置拉取，与 5.1 的 provider auth store **不是同一个目录，也不保证同一种 JSON 结构**。

---

## 6. 缓存与临时文件：逐项列清楚

### 6.1 models.dev 缓存：`models.json`

用途：provider 列表、模型信息、变体推断等。

默认路径：

- `dirs::cache_dir()/rocode/models.json`

实现：`crates/rocode-provider/src/models.rs`、`crates/rocode-server/src/routes/provider.rs`

行为要点：

- 先读缓存文件；失败则请求 `https://models.dev/api.json` 并写回缓存
- 内存中也会 cache 一份（`ModelsRegistry.data: Arc<RwLock<Option<ModelsData>>>`）

### 6.2 GitHub Research 缓存：本地 git mirror

用途：`github_research` 工具会在本地缓存目标 repo（git clone + fetch），便于快速检索与复用。

默认根目录：

- `dirs::cache_dir()/rocode/github_research/`

实现：`crates/rocode-tool/src/github_research.rs` → `local_repo_root(...)`

可通过 ToolContext extra 覆盖：

- `ctx.extra["github_research_cache_root"] = "/custom/path"`

### 6.3 插件运行时缓存（JS runtime host / npm_dir）

实现：`crates/rocode-plugin/src/subprocess/loader.rs`

注意：这里大量使用 `opencode` 作为缓存目录名（历史兼容）。

主要路径：

- host script：`dirs::cache_dir()/opencode/plugin-host.ts`
- builtin auth 插件：`dirs::cache_dir()/opencode/plugins/builtin/builtin-*.ts`
- npm 安装目录：`dirs::cache_dir()/opencode/plugins/`（里面会生成 `package.json` 并执行 install）

JS runtime 选择：

- env 强制：`ROCODE_PLUGIN_RUNTIME`（fallback：`OPENCODE_PLUGIN_RUNTIME`）
- 默认偏好：`bun > deno > node`（node 需要 >=22.6 才能跑 TS）  
代码：`crates/rocode-plugin/src/subprocess/runtime.rs`

### 6.4 插件 IPC 临时目录（大 payload file IPC）

实现：`crates/rocode-plugin/src/subprocess/client.rs`

默认 IPC 目录（按 PID namespace，避免并发冲突）：

- `std::env::temp_dir()/rocode-plugin-ipc/<pid>/`

当 payload 超过阈值且 feature flag 开启时，会：

1. 写入 `<ipc_dir>/<token>.json`（Unix 下尝试 `0600`）
2. 通过 `hook.invoke.file` 让子进程从文件读取
3. 调用后删除临时文件

同时，`PluginLoader::new()` 会清理：

- 自己 PID namespace 下的残留文件
- 其他 PID 目录中“1 小时以上”的孤儿目录

### 6.5 插件工具输出落盘（大输出“降级为附件”）

实现：`crates/rocode-tool/src/plugin_tool.rs`

当插件工具输出过大时，会把完整输出保存到文件，并在 tool result 中返回“预览 + 保存路径”。

优先保存到项目目录：

- `<session_dir>/.rocode/plugin-tool-output/plugin_output_<ts>_<pid>_<seq>.txt`

无法写入时回退到：

- `std::env::temp_dir()/rocode/plugin-tool-output/...`

### 6.6 TUI prompt 状态文件（history / frecency / stash）

实现：`crates/rocode-tui/src/components/prompt.rs`

目录选择优先级：

1. `ROCODE_STATE_DIR`（fallback：`OPENCODE_STATE_DIR`）
2. `dirs::state_dir()/rocode`
3. `std::env::temp_dir()/rocode`

最终目录：

- `<state_base>/tui/`

文件名：

- `prompt-history.json`：输入历史（最多保留 `MAX_HISTORY_ENTRIES`）
- `prompt-frecency.json`：frecency（用于补全排序）
- `prompt-stash.json`：stash 列表（最多保留 `MAX_STASH_ENTRIES`）

### 6.7 CLI Uninstall 目标目录（历史 `opencode` 命名）

实现：`crates/rocode-cli/src/upgrade.rs` → `handle_uninstall_command(...)`

当前卸载逻辑会列出并（在 `--force` 时）删除以下目录（注意目录名是 `opencode`）：

- `dirs::data_local_dir()/opencode`
- `dirs::cache_dir()/opencode`
- `dirs::config_dir()/opencode`
- `dirs::state_dir()/opencode`

这通常用于清理插件运行时缓存等“沿用 opencode 命名”的历史目录（例如 6.3 的插件缓存）。  
如果你正在排查“为什么卸载后 rocode 还有残留 / 为什么删的是 opencode 目录”，这里就是原因。

---

## 7. 项目目录内的本地数据：`.rocode/`、`.opencode/`、`.sisyphus/`

### 7.1 `.rocode/`（当前主推荐命名）

常见用途（按实现来源分）：

- **项目配置文件**：`.rocode/rocode.json` / `.rocode/rocode.jsonc`（`rocode-config` 会扫描）
- **自定义 slash commands**：`.rocode/commands/*.md`（`rocode-command` 会加载并执行）
- **agent/mode 定义**：`.rocode/agents/**/*.md`、`.rocode/modes/**/*.md`（`rocode-config` 会解析 frontmatter）
- **file plugins**：`.rocode/plugins/*.ts`（`rocode-config` 的插件发现会转成 `file://...`）
- **技能目录（skills）**：`.rocode/skills/**/SKILL.md`（`rocode-tool` 会扫描）

### 7.2 `.opencode/`（历史兼容命名，仍在使用）

当前仓库仍显式使用的内容：

- `.opencode/snapshot/`：snapshot “git dir”（用于 track/restore/diff）  
  代码：`crates/rocode-session/src/snapshot.rs`
- `.opencode/PLAN.md`：plan 工具默认 plan 文件  
  代码：`crates/rocode-tool/src/plan.rs`
- `.opencode/skills/`：旧版 skills 根目录（可通过 config.skill_paths 显式加入）  
  代码参考：`crates/rocode-tool/src/task.rs`（测试用例）

### 7.3 `.sisyphus/`（工作流状态：boulder + plans）

这不是 ROCode 的“全局目录”，但属于某些命令/工作流的项目内状态文件。

实现：`crates/rocode-command/src/start_work.rs`

- `.sisyphus/boulder.json`：记录 active_plan、session_ids、worktree_path 等
- `.sisyphus/plans/*.md`：Prometheus 风格计划文件（用 `- [ ]` / `- [x]` 表示进度）

---

## 8. 你可以如何验证这些结论（推荐命令）

### 8.1 查看 ROCode 认为的路径

- `rocode info`：打印 Data/Config/Cache 基准路径（见 `crates/rocode-cli/src/main.rs`）
- `rocode debug paths`：打印 home/config/data/cache/cwd（见 `crates/rocode-cli/src/debug.rs`）

### 8.2 查看当前加载出来的配置（最终 merge 后）

- `rocode debug config`：打印 JSON（见 `crates/rocode-cli/src/debug.rs`）

### 8.3 查数据库

- 打印 db 路径：`rocode db path`
- 直接进 sqlite3：`sqlite3 "$(rocode db path)"`
- 常用查询：
  - 列表：`.tables`
  - schema：`.schema sessions`
  - 最近会话：`SELECT id, title, updated_at, status FROM sessions ORDER BY updated_at DESC LIMIT 20;`

---

## 9. 附录：与配置相关的环境变量清单（按模块）

### 9.1 `rocode-config`（配置加载/后处理）

- `ROCODE_CONFIG`：额外配置文件路径
- `ROCODE_CONFIG_CONTENT`：内联 JSON/JSONC 配置内容
- `ROCODE_CONFIG_DIR`：额外 `.rocode` 目录来源
- `ROCODE_TEST_MANAGED_CONFIG_DIR`：测试覆盖托管配置目录
- `ROCODE_PERMISSION`：JSON 覆盖/合并 permission 规则
- `ROCODE_DISABLE_AUTOCOMPACT`：关闭自动 compaction
- `ROCODE_DISABLE_PRUNE`：关闭 prune

### 9.2 `rocode-session`（instruction loader）

- `ROCODE_DISABLE_PROJECT_CONFIG`（fallback：`OPENCODE_DISABLE_PROJECT_CONFIG`）
- `ROCODE_CONFIG_DIR`（fallback：`OPENCODE_CONFIG_DIR`）
- `ROCODE_DISABLE_CLAUDE_CODE_PROMPT`（fallback：`OPENCODE_DISABLE_CLAUDE_CODE_PROMPT`）

### 9.3 `rocode-server`（auth data dir）

- `ROCODE_DATA_DIR`（fallback：`OPENCODE_DATA_DIR`）

### 9.4 `rocode-tui`（state dir）

- `ROCODE_STATE_DIR`（fallback：`OPENCODE_STATE_DIR`）

### 9.5 `rocode-plugin`（JS runtime）

- `ROCODE_PLUGIN_RUNTIME`（fallback：`OPENCODE_PLUGIN_RUNTIME`）
