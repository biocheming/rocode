# USER GUIDE - RustingOpenCode (ROCode)

本手册面向日常使用者，覆盖启动、常用命令、scheduler 预设、TUI 交互与故障排查。  
品牌名为 `RustingOpenCode`（简称 `ROCode`），当前 CLI 命令为 `rocode`。

## 0. 版本

- 当前版本：`v2026.4.4`

## 1. 快速启动

如果你从源码运行：

```bash
cd RustingOpenCode
cargo run -p rocode-cli -- --help
```

默认进入 TUI：

```bash
cargo run -p rocode-cli --
```

等价于：

```bash
cargo run -p rocode-cli -- tui
```

单次非交互运行：

```bash
cargo run -p rocode-cli -- run "请总结这个仓库当前风险"
```

启动 HTTP 服务：

```bash
cargo run -p rocode-cli -- serve --port 3000 --hostname 127.0.0.1
```

## 2. 常用命令

### 2.1 会话管理

```bash
rocode session list
rocode session list --format json
rocode session show <SESSION_ID>
rocode session delete <SESSION_ID>
```

### 2.2 模型与配置

```bash
rocode models
rocode models --refresh --verbose
rocode config
```

### 2.3 认证管理

```bash
rocode auth list
rocode auth login --help
rocode auth logout --help
```

说明：`auth login/logout` 的具体参数请以 `--help` 输出为准。

### 2.4 MCP 管理

```bash
rocode mcp list
rocode mcp connect <NAME>
rocode mcp disconnect <NAME>
rocode mcp add --help
rocode mcp auth --help
```

如果本地服务不在默认地址，可加：

```bash
rocode mcp --server http://127.0.0.1:3000 list
```

### 2.5 调试命令

```bash
rocode debug paths
rocode debug config
rocode debug skill
rocode debug agent
```

## 3. Scheduler 预设速览

当前公开 scheduler 预设共有 4 个：

- `sisyphus`：偏执行，delegation-first，适合具体 coding / bugfix / repo tasks
- `prometheus`：偏规划，interview → plan → review → handoff，不直接执行代码实现
- `atlas`：偏协调，围绕 task ledger、delegation waves、verification 与 synthesis
- `hephaestus`：偏自主执行，强调深度探索、方案决策、落地与验证

交互上需要注意：

- `Prometheus` 的阻塞性 interview 问题应通过正式 `question` 卡片回答
- `Atlas` 的 gate Yes / No 是内部 QA rubric，不是用户问卷
- `Hephaestus` 使用更明确的失败恢复升级协议，而不是泛化重试

补充说明：

- `/autoresearch` 不是第五个公开 preset，也不是一个独立 mode；它是一个命令入口，底层仍然调用 scheduler 配置文件
- 如果项目里定义了自定义 scheduler profile，命令会解析并把它作为实际执行配置；mode 列表与 `/autoresearch` 的职责不同，不要求一一对应

## 4. TUI 与 Run 常用参数

查看完整参数：

```bash
rocode tui --help
rocode run --help
```

高频参数（两者都常用）：

- `-m, --model <MODEL>`：指定模型
- `-c, --continue`：继续最近会话
- `-s, --session <SESSION>`：继续指定会话
- `--fork`：分叉会话
- `--agent <AGENT>`：指定 agent（默认 `build`）
- `--port <PORT>` / `--hostname <HOSTNAME>`：服务地址参数

`run` 额外常用：

- `--format default|json`
- `-f, --file <FILE>`
- `--thinking`

## 5. TUI 交互说明

### 5.1 首轮 system prompt

- 首轮 user turn 会显示精简版 system prompt preview
- 后续 turn 不重复完整展示，以减少视觉噪音

### 5.2 Scheduler stage 投影

- scheduler 的每个 stage transcript 会投影到主 session
- `route` 阶段会以可读摘要形式展示，而不是原始 JSON
- `Prometheus` / `Sisyphus` / `Atlas` / `Hephaestus` 的行为仍以各自 preset authority 为准

### 5.3 Busy 与 question 的关系

- 当当前 session `busy` 时，普通输入不会插入到正在运行的 workflow 中，而是进入排队
- 如果当前 workflow 需要你回答，它应该通过 `question` 卡片发起，而不是要求你在普通输入框里插话
- question 卡片有选项时会自动追加 `Other (type your own)`，用于自定义答案

### 5.4 Slash Commands 与 `/autoresearch`

- TUI、CLI、Web 三端现在使用统一的 slash command 语义
- 输入 `/autoresearch` 后，如果参数不完整，会进入标准 question 补参流程
- 已注册 slash 命令仍然提供提示与建议；未知命令不会强制阻塞普通输入流
- `/autoresearch` 运行期间，scheduler stage、verification 和 reasoning 会持续投影到当前 session

最小示例：

```text
/autoresearch --profile autoresearch-run --goal "继续扩写 book/ml_drug_discovery_book.md"
```

常见前提：

- 项目目录内存在 `.rocode/scheduler/autoresearch.jsonc` 或等价 profile
- profile 中配置了 `verify.command`
- 如果需要只保留改进结果，应同时配置 metric 提取与 discard/keep 条件

### 5.5 Web 会话与侧栏

- Web 首页以当前 workspace 为主视角
- 左侧侧栏展示当前 workspace 的 session tree
- 右侧侧栏展示 workspace 文件树
- 历史 `/new`、`/web-next` 路由已经收口，默认访问 `/`

## 6. 配置文件位置

程序会按优先级合并多份配置（向上查找）：

- `rocode.jsonc`
- `rocode.json`
- `.rocode/rocode.jsonc`
- `.rocode/rocode.json`

全局配置默认位置：

- `~/.config/rocode/rocode.jsonc`（或 `.json`）

建议：先使用项目级最小配置，再逐步增加 provider / mcp / agent / lsp。

项目纯净性说明：

- 项目级配置入口以 `rocode.jsonc` / `rocode.json` 为准
- 历史 `opencode` 配置排查建议使用仓库脚本 `scripts/scan_legacy_config.py`

## 7. 推荐工作流

### 7.1 本地交互开发

1. `cargo run -p rocode-cli --`
2. 在 TUI 中执行任务
3. 用 `rocode session list/show` 回看历史

### 7.2 脚本或集成场景

1. `rocode serve --port 3000`
2. 用 `rocode run ... --format json` 或服务 API 集成
3. 用 `rocode stats` 追踪 token / cost

### 7.3 文档/书稿的 `autoresearch` 工作流

1. 在项目内定义 `.rocode/scheduler/autoresearch.jsonc`
2. 配置 `verify.command`、metric 提取规则、iterationPolicy
3. 用 `/autoresearch` 或 `rocode run "/autoresearch ..."` 启动
4. 根据 `stats=`、`gaps=`、章节长度等验证输出持续收敛目标稿件

如果目标是教材或书稿，建议验证器至少约束：

- 全书总字数
- 每章最小字数
- 表格与参考文献数量
- 近三年论文覆盖
- 章节固定栏目完整度

## 8. 故障排查

### 8.1 端口冲突

- 换端口：`rocode serve --port 3001`

### 8.2 模型不可用

1. `rocode auth list`
2. `rocode models --refresh`
3. `rocode config` 检查 provider 配置是否生效

### 8.3 配置疑难

1. `rocode debug paths` 查看配置搜索路径
2. `rocode debug config` 查看最终合并结果

### 8.4 MCP 连接异常

1. `rocode mcp list`
2. `rocode mcp debug <NAME>`
3. `rocode mcp connect <NAME>`

## 9. 文档索引

- 项目总览：`README.md`
- 文档总索引：`docs/README.md`
- Scheduler 文档：`docs/examples/scheduler/README.md`
- `/autoresearch` 统一设计：`docs/autoresearch-command-unified-design.md`
- Context Docs：`docs/examples/context_docs/README.md`
- 插件 / skill / Rust 示例：`docs/plugins_example/README.md`
