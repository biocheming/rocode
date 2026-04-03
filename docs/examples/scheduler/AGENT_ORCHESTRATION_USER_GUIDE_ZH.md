# ROCode 智能体编排讲解手册

这份讲义默认面向药学院天然产物、微生物、生物化学药学等方向的教师。文中的“应用场景”优先使用两类例子：

- 教学：课程讲义、题库、知识图谱、案例库、实验课材料、教学网站
- 科研：文献调研、数据清洗、谱图/组学分析脚本、数据库整理、研究流程自动化、结果验证

这份文档面向准备给同事做讲解的人，目标不是“把配置字段背下来”，而是把下面几件事讲清楚：

1. 什么是 agent，它在 ROCode 里到底承担什么角色。
2. 最简单的 agent 和最简单的 scheduler 配置是什么样子。
3. 如何把自定义 agent 接到 scheduler 里。
4. ROCode 现有内置 agent 各自负责什么、典型流程是什么、适合放在哪一层。
5. 四个 scheduler preset 的流程、适用场景和边界。
6. `autoresearch` 现在在 ROCode 里的定位、运行逻辑和应用方式。

说明：

- 前半部分有些 JSONC 代码块是“结构解释片段”，用于讲概念，不一定是完整顶层配置。
- 如果要现场真正运行，请优先使用第 26 节列出的“对应文件”；这些文件已经在仓库中落地，并做过实际加载验证。

---

## 1. 先用一句话讲清楚 ROCode 的“智能体编排”

在 ROCode 里，真正执行任务的不是一个抽象的“大模型人格”，而是：

- 一个或多个 **agent**
- 按 **scheduler stage** 串起来
- 再由 **preset / profile** 决定整体工作流

所以可以把它理解成三层：

```text
用户任务
  -> Scheduler Profile
      -> Stages
          -> Agent / Agent Tree / Skill Graph
              -> 工具调用、代码修改、验证、交付
```

最重要的区分是：

- **agent** 决定“谁来干”
- **stage** 决定“这个阶段要干什么”
- **preset** 决定“整个流程怎么走”
- **profile** 决定“本次项目里具体怎么配置”

---

## 2. 什么是 agent

在 ROCode 中，一个 agent 本质上就是一份“执行角色定义”。它通常包含这些要素：

- `name`：名字
- `system prompt`：角色指令
- `model`：是否覆盖模型
- `maxSteps`：最多走多少步
- `temperature`：输出发散度
- `allowedTools` 或工具权限：能用哪些工具

也就是说，agent 不是“有自主意识的实体”，而是：

> 一个被 scheduler 选中后，在给定任务、给定工具边界、给定提示词下运行的执行角色。

如果你要给同事讲，可以直接说：

> agent = 提示词 + 工具边界 + 模型参数 + 任务角色

---

## 3. 最简单的 agent 是什么样

最简单的 agent，不需要 children，不需要 graph，不需要 preset 魔法。

它就是一个单节点：

```jsonc
{
  "agentTree": {
    "agent": {
      "name": "general"
    }
  }
}
```

这表示：

- 只有一个 root agent
- 没有子 agent
- root 直接拿到任务并执行
- 没有额外聚合步骤

对应执行形态可以画成：

```text
任务 -> root agent -> 结果
```

如果这个 root agent 连内置名字都不想依赖，也可以直接内联写成一个“临时 agent”：

```jsonc
{
  "agentTree": {
    "agent": {
      "name": "pharm-teaching-assistant",
      "systemPrompt": "你是一个面向药学院教师的智能助教。请用准确、简洁、可教学的语言解释技术机制，并把输出组织成适合课堂讲授的形式。",
      "maxSteps": 6,
      "allowedTools": ["read", "glob", "grep"]
    }
  }
}
```

这个例子非常适合讲“agent 的最小闭环”：

- 有名字
- 有角色
- 有工具边界
- 没有 children
- 没有额外编排

---

## 4. 最简单的 scheduler 配置是什么样

最小可用 profile 其实只要一个 preset：

```jsonc
{
  "defaults": {
    "profile": "demo"
  },
  "profiles": {
    "demo": {
      "orchestrator": "sisyphus"
    }
  }
}
```

这表示：

- 默认 profile 叫 `demo`
- 它基于 `sisyphus`
- stage 使用 `sisyphus` 默认值
- 不额外定义 agent tree、skill graph、skill tree

如果再加上一个最简单的单 agent tree，就已经足够讲清“自定义执行者是谁”：

```jsonc
{
  "defaults": {
    "profile": "demo"
  },
  "profiles": {
    "demo": {
      "orchestrator": "sisyphus",
      "agentTree": {
        "agent": {
          "name": "general"
        }
      }
    }
  }
}
```

---

## 5. scheduler 中 agent 是怎么被执行的

`AgentTree` 是 scheduler 内最重要的“多 agent 组织方式”。

它的基本结构是：

```jsonc
{
  "agent": {
    "name": "deep-worker"
  },
  "prompt": "可选，给 root 的附加角色说明",
  "children": [
    {
      "agent": { "name": "code-explorer" },
      "prompt": "负责代码探索"
    },
    {
      "agent": { "name": "docs-researcher" },
      "prompt": "负责外部文档证据"
    }
  ]
}
```

### 5.1 Agent Tree 的真实执行流程

ROCode 当前 `agent_tree.rs` 的执行流程是固定三步：

```text
1. Root agent 先执行，生成初稿
2. Children 执行，默认并行
3. Root agent 再执行一次，聚合 child 输出，形成最终答案
```

如果没有 children，就退化为单 agent 执行。

所以可以把 Agent Tree 理解成：

> Root 不是普通父节点，而是“先起草，再汇总”的 coordinator。

### 5.2 Children 默认是并行

当前 `ChildExecutionMode` 默认是 `Parallel`，这意味着多个 child 一般同时展开。

Atlas 这类协调型 preset 会更适合这种结构。

### 5.3 stage 内到底优先用谁

当某个 stage 需要执行任务时，scheduler 当前优先级是：

```text
1. per-stage agentTree
2. profile-level agentTree
3. skillGraph
4. fallback 直接执行
```

这句话非常重要，讲解时建议专门强调：

> scheduler 不是“看到 preset 就直接跑”，而是先看这个 stage 有没有明确的执行组织结构。

---

## 6. 如何在 scheduler 中使用自定义 agent

ROCode 里有两种常见做法。

## 6.1 做法 A：直接在 scheduler 的 `agentTree` 里内联

这是最快、最适合演示的方式。

```jsonc
{
  "profiles": {
    "teaching-demo": {
      "orchestrator": "sisyphus",
      "agentTree": {
        "agent": {
          "name": "pharm-lab-helper",
          "systemPrompt": "你是药学院内部教学科研助手。你的输出要兼顾代码事实、科研可复现性与课堂表达。",
          "maxSteps": 8,
          "allowedTools": ["read", "glob", "grep", "bash"]
        }
      }
    }
  }
}
```

优点：

- 不依赖全局 agent 注册
- 配置集中，讲解直观
- 适合一次性实验或 demo

缺点：

- 不容易复用
- 多个 profile 里重复写会冗长

## 6.2 做法 B：在 `rocode.jsonc` 里注册可复用 agent，再在 scheduler 里按名字引用

顶层配置字段是 `agent`，不是 `agents`。

例如：

```jsonc
{
  "agent": {
    "natural-product-reviewer": {
      "description": "面向药学院教师的只读讲解与证据整理 agent",
      "mode": "subagent",
      "prompt": "你负责把技术机制、研究流程和证据边界讲得准确、保守、可讲授，不做代码修改。",
      "model": "openai:gpt-5.2",
      "temperature": 0.1,
      "maxSteps": 12,
      "tools": {
        "read": true,
        "glob": true,
        "grep": true,
        "bash": true,
        "write": false,
        "edit": false,
        "apply_patch": false
      }
    }
  }
}
```

然后在 scheduler profile 里这样用：

```jsonc
{
  "profiles": {
    "teaching-demo": {
      "orchestrator": "atlas",
      "agentTree": {
        "agent": { "name": "deep-worker" },
        "children": [
          {
            "agent": { "name": "natural-product-reviewer" },
            "prompt": "把执行过程解释成适合天然产物与生化药学教师理解的语言。"
          }
        ]
      }
    }
  }
}
```

优点：

- agent 可复用
- 多个 profile 可以共用
- 角色定义与 scheduler 编排分离

缺点：

- 对第一次接触的人来说，多了一层“先注册再引用”

## 6.3 什么时候用哪种方式

- 讲课 demo：优先用内联 `agentTree`
- 项目长期使用：优先注册到顶层 `agent`
- 想复用一整个团队结构：把 `agentTree` 单独放到外部 JSON/JSONC 文件

例如：

```jsonc
"agentTree": "./trees/deep-worker-tree.jsonc"
```

---

## 7. 自定义 agent 时最该讲清楚的 6 个字段

### 7.1 `name`

这是 agent 标识名。

- 如果引用内置 agent，就写内置名字
- 如果引用你自己注册的 agent，就写注册键名
- 如果完全内联自定义，也可以是一个临时名字

### 7.2 `systemPrompt`

这是角色定义的核心。建议一句话给职责，一句话给边界，一句话给输出要求。

一个好 prompt 的结构通常是：

```text
你是谁
你负责什么
你不能做什么
你输出应该是什么样
```

### 7.3 `allowedTools`

这是最容易讲出“智能体编排价值”的字段。

它决定 agent 只是“看”，还是能“动手”。

常见约束方式：

- 只读 agent：`read/glob/grep/ast_grep_search/bash`
- 可执行 agent：加上 `write/edit/apply_patch/shell_session`
- 文档证据 agent：加 `websearch/webfetch/context_docs/github_research`

### 7.4 `maxSteps`

防止 agent 过度发散。

- 讲解型 agent：通常 4 到 8 步就够
- 代码探索 agent：10 到 20 步
- 深度执行 agent：可以更高

### 7.5 `model`

如果某个 agent 特别重要，可以单独覆盖模型：

```jsonc
"model": {
  "providerId": "openai",
  "modelId": "gpt-5.2"
}
```

模型优先级是：

```text
per-agent model
  -> profile model
    -> session 当前模型
```

### 7.6 `mode`

如果是顶层注册 agent，通常建议：

- 可直接作为主执行者：`primary`
- 主要给 root/children 调用：`subagent`

---

## 8. 内置 agent 全景图

ROCode 当前内置 agent 可以分成三组：主执行者、辅助子 agent、内部系统 agent。

## 8.1 主执行者

| agent | 类型 | 典型流程 | 适用场景 |
|------|------|---------|---------|
| `build` | primary | 直接执行工具，根据权限做修改和验证 | 通用默认执行 |
| `plan` | primary | 分析任务 -> 产出计划，不做编辑 | 规划模式、方案输出 |
| `general` | primary | 通用问答/执行 | 默认通用角色 |
| `deep-worker` | primary | 先读后改 -> 跟踪任务 -> 修改 -> 验证 -> 交付 | 复杂代码实现、修复、重构 |

### `deep-worker` 最值得讲

`deep-worker` 是最接近“高自治执行代理”的内置 agent。

它的系统提示词强调：

- 先检查代码库再行动
- 长任务要显式跟踪任务状态
- 修改后必须做最小充分验证
- 不要假装拥有工具之外的能力

所以它的自然流程是：

```text
读代码 -> 拆步骤 -> 修改 -> 运行验证 -> 汇报变更与风险
```

这也是为什么 `hephaestus` 默认很适合挂 `deep-worker`。

## 8.2 常用子 agent

| agent | 类型 | 典型流程 | 核心边界 |
|------|------|---------|---------|
| `explore` | subagent | 搜索 -> 阅读 -> 汇总 | 只读探索 |
| `code-explorer` | subagent | 广搜 -> 缩小 -> 定位符号/关系 -> 证据化总结 | 只读代码发现 |
| `architecture-advisor` | subagent | 读代码 -> 找边界与风险 -> 给结构建议 | 不修改代码 |
| `docs-researcher` | subagent | 官方文档/GitHub/网页证据收集 -> 保守结论 | 偏外部研究，不做本地写入 |
| `media-reader` | subagent | 读取附件文本或媒体上下文 -> 提取事实 | 不做浏览、搜索、修改 |
| `oracle` | subagent | 高强度分析 -> 给单一路径建议 | 咨询型，只读 |
| `metis` | subagent | 先做意图分类 -> 找歧义/隐性风险 -> 给 planner 指令 | 规划前顾问 |
| `momus` | subagent | 核查引用、可执行性、阻塞项 -> `OKAY/REJECT` | 计划审校，不求完美 |
| `sisyphus-junior` | subagent | 作为分类派发后的执行者完成局部任务 | 无 `task/task_flow` |

### `code-explorer`

它适合回答：

- 相关实现在哪些文件
- 调用链怎么串起来
- 哪几个模块行为不同

典型流程：

```text
glob/grep 广搜 -> ast_grep_search 定位结构 -> read 确认 -> 输出文件/符号/关系
```

### `docs-researcher`

它适合回答：

- 某个库官方文档怎么规定
- 某个 API 是否变更
- GitHub issue / release note 里有什么依据

典型流程：

```text
context_docs / github_research / websearch / webfetch -> 收集证据 -> 区分事实与推断
```

### `architecture-advisor`

它适合做：

- 架构评审
- 边界泄漏分析
- 重构风险审查

典型流程：

```text
读代码 -> 找边界/重复/回归风险 -> 输出建议与证据
```

### `oracle`

它不是“执行者”，而是“高质量顾问”。

适合：

- 架构选型
- 困难 bug 的策略讨论
- 复杂 trade-off 判断

典型流程：

```text
读上下文 -> 做高强度推理 -> 给一条主建议 + 实施步骤 + 风险提醒
```

### `metis`

`metis` 是 Prometheus 很关键的前置顾问。

它先做 **意图分类**，然后针对不同任务类型给出不同的问题和 planner 指令：

- refactor
- build from scratch
- mid-sized task
- collaborative
- architecture
- research

所以 `metis` 的定位不是“直接产出计划”，而是：

> 在 planner 动手之前，先防止 planner 问错问题、漏掉风险、把范围做大。

### `momus`

`momus` 是“阻塞项审校 agent”，不是“吹毛求疵 agent”。

它只关心三件事：

- 引用文件是否存在
- 计划是否能启动
- 是否有真正会卡死执行的矛盾

它的输出目标非常简单：

- `[OKAY]`
- `[REJECT]`

### `sisyphus-junior`

这个 agent 的定位是：

> 被 task category 或派发策略选中后，做局部执行，但不自己做任务编排。

它和 `deep-worker` 的差别在于：

- `deep-worker` 更像“能自己推进全局任务的主力执行者”
- `sisyphus-junior` 更像“被调度后完成一段局部工作的执行子 agent”

## 8.3 内部系统 agent

| agent | 用途 | 是否适合拿来做业务讲解 |
|------|------|-----------------------|
| `summary` | 生成摘要 | 一般不作为教学重点 |
| `compaction` | 压缩上下文 | 一般不作为教学重点 |
| `title` | 生成会话标题 | 一般不作为教学重点 |

---

## 9. 四个 preset 的核心差异

讲 scheduler 时，最容易让听众混乱的是：他们以为四个 preset 只是 stage 数量不同。

其实不是。

四个 preset 的区别至少包括：

- stage 默认序列
- 是否允许改 stage
- 执行工作流类型
- 是否需要验证 gate
- child agent 是并行还是串行
- 最终交付模式

先记一张总表：

| preset | 默认 stages | 工作流 | 默认最大轮次 | 核心特点 |
|------|-------------|-------|-------------|---------|
| `sisyphus` | `request-analysis -> route -> execution-orchestration` | `SinglePass` | 1 | 单次执行到底 |
| `prometheus` | `request-analysis -> route -> interview -> plan -> review -> handoff` | `Direct` | 1 | 只规划，不执行 |
| `atlas` | `request-analysis -> execution-orchestration -> synthesis` | `CoordinationLoop` | 3 | 并行协调、验证、汇总 |
| `hephaestus` | `request-analysis -> execution-orchestration` | `AutonomousLoop` | 3 | 深度自治执行、自我验证 |

---

## 10. Sisyphus：执行优先

默认 stage：

```text
request-analysis -> route -> execution-orchestration
```

### 10.1 它在干什么

Sisyphus 的哲学是：

> 分类一次，然后执行到底。

它不强调多轮计划，不强调反复 review，而是把重心放在一次完整执行。

### 10.2 实际流程

可以这样讲：

```text
1. request-analysis：先把用户请求整理成 request brief
2. route：判断任务类型与执行姿势
3. execution-orchestration：进入单次执行主循环
4. 输出结果并结束
```

它的 workflow todo 也反映出这种风格：

- 分类任务
- 看清代码库形态
- 并行做探索和研究
- 执行或委派
- 给出验证证据

### 10.3 什么时候适合

- 需求明确的教学脚本或课程网站功能补充
- 已知方向的实验数据处理脚本修复
- 讲义、实验指导书、题库生成脚本改写
- 小中型科研代码仓任务

### 10.4 什么时候不适合

- 需要大量需求澄清的任务
- 需要多团队并行协调的大任务
- 需要显式 objective-driven 迭代优化的任务

### 10.5 典型讲法

如果你想给同事一句最短定义：

> Sisyphus 是“执行型默认 preset”，适合把明确任务一次性做完。

---

## 11. Prometheus：规划优先

默认 stage：

```text
request-analysis -> route -> interview -> plan -> review -> handoff
```

### 11.1 它最重要的边界

Prometheus 是 **planner-only**。

也就是说：

- 它负责澄清需求
- 负责形成计划
- 负责 review 和 handoff
- **不负责真正执行代码实现**

### 11.2 它和其他 preset 最大不同点

它是唯一一个 **stage 拓扑锁死** 的 preset。

你可以：

- 覆盖每个 stage 的 `toolPolicy`
- 覆盖 `loopBudget`
- 覆盖某个 stage 的 `agentTree`

但你不能：

- 增删默认 stage
- 改变它们的顺序

### 11.3 实际流程

Prometheus 的讲解最好分成显式 stage 和隐式 agent 机制两层。

#### 第一层：显式 stage

```text
1. request-analysis：整理需求
2. route：强制维持在 Prometheus 规划路径
3. interview：向用户问阻塞性澄清问题
4. plan：产出计划
5. review：把计划收紧并校验交付形状
6. handoff：把可执行计划交给下一阶段
```

#### 第二层：隐式关键 agent

Prometheus 的 workflow todo 显示它还会显式使用：

- `metis`：先做 gap analysis
- `momus`：如果走高精度 review，循环直到 `OKAY`

所以更真实的流程是：

```text
interview
  -> consult metis
  -> draft plan
  -> self review
  -> ask user whether high accuracy review is needed
  -> if yes: momus review loop until OKAY
  -> handoff
  -> /start-work {name} 转 Atlas 执行
```

### 11.4 为什么它很适合“明天讲解”

因为它特别适合讲清楚：

- scheduler 不只是“跑代码”
- agent 可以参与 planning quality control
- 多 agent 可以分工，不一定都做代码实现

### 11.5 什么时候适合

- 想先把课程建设方案想清楚再落代码
- 想先设计科研数据流程，而不是直接改脚本
- 需要形成正式 handoff 文档给学生或课题组成员
- 想讲“AI 先规划，再执行”的流程治理

### 11.6 什么时候不适合

- 用户就是要你马上修
- 任务已经足够明确
- 项目需要直接进执行闭环

### 11.7 一句总结

> Prometheus 不是 coder，它是 planner；它把计划做扎实，再把执行权交给 Atlas。

---

## 12. Atlas：协调优先

默认 stage：

```text
request-analysis -> execution-orchestration -> synthesis
```

### 12.1 它在干什么

Atlas 的哲学是：

> 把任务拆成多个工作流，并行推进，经过验证后再收敛成一个结果。

### 12.2 它的工作流类型

Atlas 使用的是 `CoordinationLoop`。

关键特征：

- child mode 默认是并行
- 最多 3 轮
- verification 是 required
- 有内部 coordination verification 和 gate

也就是说，虽然外部只看到 3 个 stage，但运行时内部还有验证和 gate 逻辑。

### 12.3 更真实的 Atlas 流程

```text
1. request-analysis
2. execution-orchestration
   - coordinator 派发并行工作流
   - worker 提交各自结果
3. coordination-verification（内部）
   - 不盲信 worker 自报完成
   - 用证据审计每个 task item
4. coordination-gate（内部）
   - done / continue / blocked
5. synthesis
   - 汇总已验证的结果
```

### 12.4 为什么 Atlas 适合多 agent 编排讲解

因为它最能体现“编排”的含义：

- root 负责协调
- children 负责不同 workstream
- verification 负责约束幻觉
- synthesis 负责最后收敛

### 12.5 什么时候适合

- 大型科研平台或教学平台任务拆分
- 需要并行推进数据清洗、算法实现、文档整理、结果验证
- 需要显式 task ownership
- 需要“先分工，后汇总”的组织方式

### 12.6 什么时候不适合

- 非常简单的一步到位任务
- 只要单 agent 深挖即可完成

### 12.7 一句总结

> Atlas 不是让一个 agent 更聪明，而是让多个 agent 更有组织。

---

## 13. Hephaestus：自治优先

默认 stage：

```text
request-analysis -> execution-orchestration
```

### 13.1 它在干什么

Hephaestus 的哲学是：

> 让一个强执行 agent 深入做完整任务，但必须经得起验证。

### 13.2 它的工作流类型

Hephaestus 使用 `AutonomousLoop`。

关键特征：

- child mode 默认是串行
- verification required
- 最大 3 轮
- 有 autonomous verification 和 finish gate

### 13.3 更真实的 Hephaestus 流程

官方 prompt 明确要求它走：

```text
EXPLORE -> PLAN -> DECIDE -> EXECUTE -> VERIFY
```

所以完整理解应该是：

```text
1. request-analysis
2. execution-orchestration
   - 深度执行
   - 自己探索、决策、修改、验证
3. autonomous-verification（内部）
   - 审计有没有真的完成 explore/plan/decide/execute/verify 闭环
4. autonomous-gate（内部）
   - done / continue / blocked
5. 必要时进入 bounded retry
```

### 13.4 什么时候适合

- 复杂科研流程优化或分析管线重构
- 非常具体的教学技术任务
- 已知目标但实现链路较长
- 需要强执行而不是强协调

### 13.5 什么时候不适合

- 需要多工作流并行
- 需要正式 planning handoff
- 需要广泛外部研究和多视角对抗

### 13.6 一句总结

> Hephaestus 是“强执行单兵”，不是“多兵种协同”。

---

## 14. 四个 preset 怎么选

如果要给同事一个最简单的决策表，可以直接用下面这版：

| 场景 | 推荐 |
|------|------|
| 明确的小中型教学/科研执行任务 | `sisyphus` |
| 先澄清课程建设或科研流程方案 | `prometheus` |
| 大任务拆分、并行协作 | `atlas` |
| 复杂但明确、想深度自治执行 | `hephaestus` |

如果再说得更实战一点：

- “先想清楚再干”用 `prometheus`
- “分几路同时干”用 `atlas`
- “一个强 agent 自己狠狠干完”用 `hephaestus`
- “普通执行默认档”用 `sisyphus`

---

## 15. `autoresearch` 在 ROCode 里的定位

这里建议用一句既积极又严谨的话来讲：

> `autoresearch` 在 ROCode 中已经不是“概念 demo”，而是已经接入 scheduler、可以真实触发的迭代工作流；其中命令入口与补参链路已经可用，但个别 full parity 细节仍在继续补齐。

明天讲的时候，最稳妥的表述可以拆成四句：

- **现在已经有真实 runtime，不只是配置草图**
- **`/autoresearch` 命令入口已经贯通 CLI、TUI、Web**
- **已经支持 objective-driven iteration、baseline、verify/guard、keep/discard、snapshot 这一条主干闭环**
- **但仍然有少数扩展项没有做到设计文档里的完全对齐**

如果听众问“现在能不能用”，更准确的回答是：

- **能用**
- **尤其是命令触发、缺参提问、用户补参、自动重提交，这条交互链路现在已经打通**
- **要想跑完整研究任务，还需要有可用模型 provider，以及真实可执行的 verify/guard 命令**

---

## 16. `autoresearch` 到底是什么

可以把 `autoresearch` 理解成：

> 一个以“研究目标、验证命令、保留/丢弃策略”为中心的迭代式研究与优化工作流。

它和普通 scheduler 的最大区别不是“多轮”，而是：

- 有 **objective**
- 有 **metric**
- 有 **verify**
- 有 **guard**
- 有 **keep / discard / rework / retry** 决策
- 有 **checkpoint / revert / ledger / artifact**

所以它不是普通的“多轮 agent 对话”，而是：

> 带实验设计和状态治理的自动迭代执行框架。

如果面向药学院教师讲，可以直接换成更熟悉的话：

> 普通 scheduler 更像“安排一个人把任务做完”；`autoresearch` 更像“按预先定义的科研评价标准，一轮轮试验、验证、保留更优结果”。

---

## 17. `autoresearch` 当前已识别的 mode

当前 `iterative_workflow.rs` 识别这些 mode：

- `run`
- `plan`
- `security`
- `debug`
- `fix`
- `ship`

并且会给出一个 base preset hint：

| mode | base preset hint |
|------|------------------|
| `plan` | `prometheus` |
| `security` | `atlas` |
| `ship` | `atlas` |
| `run` | `hephaestus` |
| `debug` | `hephaestus` |
| `fix` | `hephaestus` |

这很值得讲，因为它说明：

> `autoresearch` 不是替代 scheduler，而是把自己的领域语义叠加到 scheduler 之上。

但这里要强调一个边界：

> 当前 runtime 已经支持通用 autoresearch loop，并且 mode 已经能表达不同工作语义；不过它们还不是“彼此完全独立的专用执行内核”。

---

## 18. `autoresearch` 的核心运行流程

官方运行时文档给出的规范流程可以概括成：

```text
Initializing
  -> CapturingBaseline
  -> Iterating
     -> Snapshot
     -> Execute
     -> Verify
     -> Guard?
     -> Decide
        -> Keep
        -> Discard
        -> Rework
        -> RetryCrash
        -> StopSatisfied
        -> StopStalled
        -> StopBlocked
  -> Completing
  -> Completed
```

如果要给听众讲得更易懂，可以改写成下面这个口语版：

```text
1. 先定义目标和评价指标
2. 先测一遍基线
3. 每轮迭代前做快照
4. 用 scheduler 执行一轮改动
5. 跑 verify，必要时再跑 guard
6. 判断这轮是保留、丢弃、返工、崩溃重试，还是停止
7. 把结果写进 ledger 和 artifact
8. 继续下一轮，直到满足目标或停机条件
```

给教师讲时，可以把它翻译成一个更学术的闭环：

```text
提出目标
  -> 建立 baseline
  -> 进行一轮干预
  -> 重新测量
  -> 判断这轮结果是否值得保留
  -> 写入研究记录
  -> 决定下一轮是否继续
```

---

## 19. `autoresearch` 的关键运行角色

从运行时契约看，`autoresearch` 不是一个 agent，而是一组 runtime service 的协作。

最重要的几个组件是：

| 组件 | 作用 |
|------|------|
| `WorkflowController` | 统筹整个 run，拥有生命周期控制权 |
| `ObjectiveEvaluator` | 记录 metric history，比较 baseline 与当前结果 |
| `VerificationRunner` | 执行 verify / guard 命令 |
| `DecisionPolicy` | 决定 Keep / Discard / Rework / RetryCrash |
| `SnapshotEngine` | 管理 checkpoint、restore、release |
| `IterationLedger` | 记录每轮结果 |
| `ArtifactWriter` | 输出 summary、ledger、报告 |

所以讲解时可以强调：

> 普通 scheduler 关心“任务怎么执行”，autoresearch 还关心“这一轮结果是否值得保留”。

---

## 20. `autoresearch` 的决策语义

这是讲解里的关键亮点。

每轮迭代之后，系统不是只会说“成功/失败”，而是会做更细的判断：

- `Keep`
- `Discard`
- `Rework`
- `RetryCrash`
- `StopSatisfied`
- `StopStalled`
- `StopBlocked`

再由 `WorkflowController` 把它映射为 scheduler 看的 gate decision：

- `Done`
- `Continue`
- `Blocked`

这说明：

> scheduler 只负责通用执行循环，autoresearch 才拥有“实验决策语义”。

这也是为什么文档明确写了：

- scheduler 拥有 generic execution orchestration
- autoresearch 拥有 objective-driven iteration semantics

如果要给药学院教师一个最直观的类比，可以说：

> 这有点像把“课题设计 + 实验记录本 + 每轮结果判读 + 可回滚工作区”一起包进了调度系统。

---

## 21. 一个可运行的 `autoresearch` 示例应该怎么看

讲课时，不建议直接把例子讲成“优化某个软件回归分数”，因为对药学院教师来说不够贴近。

更适合的讲法是：

- **教学例子**：提高“天然产物课程问答助手”的评测分数
- **科研例子**：提高“天然产物结构分类/谱图注释脚本”的验证分数

两者背后的调度结构是一样的，只是 objective 和 verify 命令不同。

它做的事情大致是：

- 用 `hephaestus` 作为底层执行风格
- 目标是提高一个可量化的验证分数
- verify 命令是一次真实评测脚本
- guard 命令是一次“不能把基本能力弄坏”的安全检查
- 有 bounded iteration
- 有 baseline / keep / discard / crash retry / workspace snapshot

它的教学价值在于：

- 让同事看到 objective 不是抽象概念，而是配置字段
- 让同事看到 verify/guard 是可执行命令
- 让同事看到“保留还是回滚”是显式决策策略

一个提炼后的最小结构如下：

```jsonc
{
  "profiles": {
    "teaching-autoresearch-run": {
      "orchestrator": "hephaestus",
      "workflow": {
        "workflow": {
          "kind": "autoresearch",
          "mode": "run"
        },
        "objective": {
          "goal": "提高《天然产物化学》课程问答助手的评测分数",
          "direction": "higher-is-better",
          "metric": {
            "kind": "numeric-extract",
            "pattern": "score=([0-9.]+)"
          },
          "verify": {
            "command": "./scripts/evaluate-teaching-assistant.sh"
          },
          "guard": {
            "command": "./scripts/check-teaching-assistant-basics.sh"
          }
        },
        "iterationPolicy": {
          "mode": "bounded",
          "maxIterations": 6,
          "stuckThreshold": 2
        },
        "decisionPolicy": {
          "baselineStrategy": "capture-before-first-iteration",
          "keepConditions": ["metric-improved", "verify-passed"],
          "discardConditions": ["metric-regressed", "metric-unchanged", "verify-failed"]
        },
        "workspacePolicy": {
          "snapshotStrategy": "worktree-fork"
        }
      }
    }
  }
}
```

## 21.1 一个更适合现场展示的“最小实现案例”

如果你明天想给同事一个最容易理解、最不容易被细节淹没的例子，建议直接讲这个版本。

它只保留 5 个最核心的块：

- `workflow.kind/mode`
- `objective`
- `verify`
- `decisionPolicy`
- `workspacePolicy`

示例：

```jsonc
{
  "defaults": {
    "profile": "teaching-autoresearch-minimal"
  },
  "profiles": {
    "teaching-autoresearch-minimal": {
      "orchestrator": "hephaestus",
      "description": "最小可运行 autoresearch 示例：逐轮优化药学课程问答助手，只保留评测更优的版本。",
      "workflow": {
        "workflow": {
          "kind": "autoresearch",
          "mode": "run"
        },
        "objective": {
          "goal": "让《天然产物化学》课程问答评测分数持续提高",
          "scope": {
            "include": ["knowledge_base/**", "prompts/**", "scripts/**"],
            "exclude": ["tmp/**", "target/**"]
          },
          "direction": "higher-is-better",
          "metric": {
            "kind": "numeric-extract",
            "pattern": "score=([0-9.]+)"
          },
          "verify": {
            "command": "./scripts/evaluate-course-qa.sh",
            "timeoutMs": 600000
          }
        },
        "iterationPolicy": {
          "mode": "bounded",
          "maxIterations": 3
        },
        "decisionPolicy": {
          "baselineStrategy": "capture-before-first-iteration",
          "keepConditions": ["metric-improved", "verify-passed"],
          "discardConditions": ["metric-regressed", "metric-unchanged", "verify-failed"]
        },
        "workspacePolicy": {
          "snapshotStrategy": "patch-file"
        }
      }
    }
  }
}
```

这个例子为什么适合讲“最小实现”：

- `orchestrator` 用 `hephaestus`，因为它天然适合单执行者自治循环
- `mode` 用 `run`，避免一下子把 `security/debug/ship` 的额外语义都带进来
- `metric` 用正则抽取一个分数，最直观
- `verify` 只需要输出类似 `score=83.5`
- `decisionPolicy` 只保留 keep/discard 的核心条件
- `snapshotStrategy` 用 `patch-file`，概念上最容易理解

如果现场要口头解释，可以直接说：

```text
1. 先跑一次课程问答评测，得到 baseline
2. 执行一轮修改
3. 再跑评测脚本，抽取 score
4. 如果 score 变高且验证通过，就 keep
5. 否则 discard，并回到上一个快照
6. 最多做 3 轮
```

这就是一个完整的最小 autoresearch 闭环。

---

## 22. `autoresearch` 适合举哪些例子

### 22.1 `run`

场景：

- 想提高课程问答助手的题目覆盖与回答准确率
- 想提高天然产物结构分类脚本的验证得分
- 想提高实验课自动评分脚本的一致性指标

讲法：

> 每轮改一点，跑验证，只保留客观上更好的版本。

### 22.2 `debug`

场景：

- 一个 LC-MS 数据预处理流程偶发报错
- 一个教学知识库构建脚本时好时坏
- 症状明确，但根因不清楚

讲法：

> 它不是盲修，而是维护 symptom、hypothesis、experiment、finding 的循环。

### 22.3 `security`

场景：

- 需要审查教学平台或组内数据服务的暴露面
- 需要对研究数据接口做权限与泄露风险检查
- 需要多 workstream 并行审计

讲法：

> 这类更像 Atlas 风格的协调型 autoresearch，而不是单兵深挖。

### 22.4 `fix`

场景：

- 修文献整理流水线的 broken state
- 修图表生成脚本、Notebook 批处理、数据库同步任务的 broken state
- 修 lint/type/test/build broken state

讲法：

> 目标不是“写了多少代码”，而是把 broken-state count 逐轮降到 0。

### 22.5 `ship`

场景：

- 发布课程资源站点前 checklist
- 发布组内分析工具前 dry-run
- approval
- rollback

讲法：

> 它不是开发态 agent，而是带审批和副作用治理的发布工作流。

---

## 23. 讲 `autoresearch` 时必须提醒的现实边界

这一段建议你在分享里单独加一页“当前限制”。

### 23.1 它不是完全独立于 scheduler 的系统

当前实现和文档都明确表明：

- scheduler 仍然是唯一的通用执行内核
- autoresearch 是叠加在它之上的领域层

### 23.2 现有设计文档描述的 full parity 目标，不等于今天运行时的全部能力边界

这里最容易被误解，所以一定要说准确：

- 不是“还没实现”
- 而是“已经实现主干能力，但还不是文档里的完全最终形态”

你可以直接这样讲：

> 今天已经能跑 `autoresearch`，尤其是命令进入、补参、自动重提交流程已经打通；但 full parity 里仍有少数项还在继续补齐。

### 23.3 `childSession` 当前有文档/Schema 与实现不一致问题

这点很值得在内部讲解时提醒。

当前情况是：

- public schema 和示例配置暴露了 `childSession`
- 但 `SchedulerStageOverride` 当前并没有反序列化这个字段
- per-stage override merge 当前只实际应用了：
  - `toolPolicy`
  - `loopBudget`
  - `sessionProjection`

这意味着：

> 你今天可以在示例里写 `childSession`，但 per-stage 覆盖并不会像文档表面看起来那样完整生效。

不过 stage 默认值仍然存在：

- `ExecutionOrchestration` 默认 `child_session = true`
- `Delegation` 默认 `child_session = true`

所以最稳妥的讲法是：

> `childSession` 这个字段已经出现在公开文档和 schema 中，但当前 per-stage 覆盖路径还存在实现缺口，讲解时要把它当成“已设计、待进一步对齐”的能力，而不是百分之百可依赖的细粒度开关。

### 23.4 当前我确认到的一个真实未完成点

`baselineStrategy: from-last-run` 代码里明确写了 `not implemented yet`。

所以如果你要给同事一个“已经实现”和“还没实现”的边界例子，最稳妥的说法是：

- `capture-before-first-iteration`：已实现
- `from-config`：已实现，但要提供 `baselineValue`
- `from-last-run`：当前还没实现

### 23.5 示例中的 agent 名字不一定都是内置 agent

例如：

- `coordinator`
- `literature-curator`
- `spectra-analyst`
- `parallel-coordinator`

这些名字常常是“示例角色名”，不一定是 ROCode 内置 agent。

因此如果你要真正运行这些例子，需要：

- 先在顶层 `agent` 中注册这些名字
- 或直接在 `agentTree` 内联写完整 `systemPrompt`

---

## 24. 一个适合明天讲解的推荐顺序

如果你要做 30 到 45 分钟分享，推荐按这个顺序讲。

### 第一部分：先立框架，5 分钟

- 什么是 agent
- 什么是 stage
- 什么是 preset
- 什么是 profile

一句话压轴：

> agent 决定执行角色，scheduler 决定流程编排。

### 第二部分：从最小例子讲起，5 到 8 分钟

- 单 agent tree
- 最小 `sisyphus` profile
- 为什么这已经算“智能体编排”的最小单位

### 第三部分：讲 Agent Tree，8 到 10 分钟

- root / children
- root 先执行、children 再执行、root 再聚合
- children 默认并行
- per-stage tree 覆盖 profile-level tree

### 第四部分：讲内置 agent 分工，8 到 10 分钟

重点讲：

- `deep-worker`
- `code-explorer`
- `docs-researcher`
- `architecture-advisor`
- `metis`
- `momus`
- `oracle`

### 第五部分：讲四个 preset，10 分钟

- `sisyphus`
- `prometheus`
- `atlas`
- `hephaestus`

### 第六部分：讲 `autoresearch`，8 到 12 分钟

- objective / verify / guard / decision
- baseline / snapshot / keep / discard
- current status 和未来空间
- 最好配一个“课程问答优化”或“科研脚本优化”的最小案例

---

## 25. 一页版总结

如果最后只留一页给听众，可以用下面这版：

### ROCode 智能体编排的核心

- agent 是执行角色，不是抽象人格
- scheduler 负责编排 stages 与执行策略
- Agent Tree 是当前最直观、最重要的多 agent 组织方式
- `sisyphus` 偏执行
- `prometheus` 偏规划
- `atlas` 偏协调
- `hephaestus` 偏自治执行
- `autoresearch` 是目标驱动、验证驱动、可回滚的迭代工作流

### 最重要的一句

> ROCode 的价值，不只是“能调用模型”，而是“能把不同角色、不同阶段、不同验证责任组织成一个可治理的教学科研执行系统”。

---

## 26. 附：最推荐拿来现场展示的配置片段

下面 6 个片段都已经在仓库里有对应的可运行文件。现场演示时，建议优先打开“对应文件”而不是手敲。

### 26.1 最小单 agent

对应文件：
[pharmacy-course-demo.example.jsonc](/home/biocheming/tests/rust/rocode/docs/examples/scheduler/pharmacy-course-demo.example.jsonc)

```jsonc
{
  "$schema": "https://rocode.dev/schemas/scheduler-profile.schema.json",
  "version": "2026-04-01",
  "defaults": { "profile": "pharmacy-course-demo" },
  "profiles": {
    "pharmacy-course-demo": {
      "orchestrator": "sisyphus",
      "description": "Runnable teaching demo profile for pharmacy educators.",
      "skillList": ["request-analysis", "route", "execution-orchestration"],
      "stages": ["request-analysis", "route", "execution-orchestration"],
      "agentTree": {
        "agent": {
          "name": "course-assistant",
          "systemPrompt": "You are a pharmacy course assistant. Organize outputs into lecture notes, case discussions, quiz prompts, and lab teaching points."
        }
      }
    }
  }
}
```

### 26.2 `deep-worker + code-explorer + docs-researcher`

对应文件：
[pharmacy-evidence-team.example.jsonc](/home/biocheming/tests/rust/rocode/docs/examples/scheduler/pharmacy-evidence-team.example.jsonc)

```jsonc
{
  "$schema": "https://rocode.dev/schemas/scheduler-profile.schema.json",
  "version": "2026-04-01",
  "defaults": { "profile": "pharmacy-evidence-team" },
  "profiles": {
    "pharmacy-evidence-team": {
      "orchestrator": "sisyphus",
      "skillList": ["request-analysis", "route", "execution-orchestration"],
      "stages": ["request-analysis", "route", "execution-orchestration"],
      "agentTree": {
        "agent": { "name": "deep-worker" },
        "children": [
          {
            "agent": { "name": "code-explorer" },
            "prompt": "Map the course-knowledge or research-script implementation details before execution commits to a change."
          },
          {
            "agent": { "name": "docs-researcher" },
            "prompt": "Bring in literature, documentation, or database evidence only when it changes the task boundary or scientific interpretation."
          }
        ]
      }
    }
  }
}
```

### 26.3 `prometheus` 讲 planning

对应文件：
[pharmacy-prometheus.example.jsonc](/home/biocheming/tests/rust/rocode/docs/examples/scheduler/pharmacy-prometheus.example.jsonc)

```jsonc
{
  "$schema": "https://rocode.dev/schemas/scheduler-profile.schema.json",
  "version": "2026-04-01",
  "defaults": { "profile": "pharmacy-prometheus" },
  "profiles": {
    "pharmacy-prometheus": {
      "orchestrator": "prometheus",
      "skillList": ["request-analysis", "route", "interview", "plan", "review", "handoff"],
      "stages": [
        "request-analysis",
        "route",
        "interview",
        "plan",
        "review",
        "handoff"
      ]
    }
  }
}
```

适合拿来讲的口头任务：

> “先规划一个《天然产物化学》课程知识图谱构建方案，再决定要不要进入执行。”

### 26.4 `atlas` 讲协调

对应文件：
[pharmacy-atlas.example.jsonc](/home/biocheming/tests/rust/rocode/docs/examples/scheduler/pharmacy-atlas.example.jsonc)

```jsonc
{
  "$schema": "https://rocode.dev/schemas/scheduler-profile.schema.json",
  "version": "2026-04-01",
  "defaults": { "profile": "pharmacy-atlas" },
  "profiles": {
    "pharmacy-atlas": {
      "orchestrator": "atlas",
      "skillList": ["request-analysis", "execution-orchestration", "synthesis"],
      "stages": ["request-analysis", "execution-orchestration", "synthesis"],
      "agentTree": {
        "agent": { "name": "deep-worker" },
        "children": [
          { "agent": { "name": "code-explorer" }, "prompt": "Own repository evidence for data processing, evaluation scripts, and delivery artifacts." },
          { "agent": { "name": "docs-researcher" }, "prompt": "Own literature, database, and method-reference evidence for one workstream at a time." },
          { "agent": { "name": "architecture-advisor" }, "prompt": "Ensure the parallel workstreams converge on one coherent teaching or research deliverable." }
        ]
      }
    }
  }
}
```

适合拿来讲的口头任务：

> “并行推进天然产物数据库整理、谱图注释脚本验证、课程演示材料生成，最后统一收敛成一个交付包。”

### 26.5 `autoresearch` 讲 objective-driven loop

对应文件：
[pharmacy-autoresearch-course.runnable.example.jsonc](/home/biocheming/tests/rust/rocode/docs/examples/scheduler/pharmacy-autoresearch-course.runnable.example.jsonc)

配套脚本：
[evaluate-course-qa.sh](/home/biocheming/tests/rust/rocode/scripts/evaluate-course-qa.sh)
[check-pharmacy-demo-basics.sh](/home/biocheming/tests/rust/rocode/scripts/check-pharmacy-demo-basics.sh)

```jsonc
{
  "$schema": "https://rocode.dev/schemas/scheduler-profile.schema.json",
  "version": "2026-04-01",
  "defaults": { "profile": "pharmacy-course-autoresearch" },
  "profiles": {
    "pharmacy-course-autoresearch": {
      "orchestrator": "hephaestus",
      "workflow": {
        "workflow": { "kind": "autoresearch", "mode": "run" },
        "objective": {
          "goal": "Increase the local course-QA evaluation score for the natural-products teaching demo without breaking the packaged demo assets.",
          "scope": {
            "include": [
              "docs/examples/scheduler/demo_assets/course_qa/**",
              "scripts/evaluate-course-qa.sh",
              "scripts/check-pharmacy-demo-basics.sh"
            ],
            "exclude": ["target/**"]
          },
          "direction": "higher-is-better",
          "metric": { "kind": "numeric-extract", "pattern": "score=([0-9]+)" },
          "verify": { "command": "./scripts/evaluate-course-qa.sh" },
          "guard": { "command": "./scripts/check-pharmacy-demo-basics.sh" }
        },
        "iterationPolicy": { "mode": "bounded", "maxIterations": 3, "stuckThreshold": 2 },
        "decisionPolicy": {
          "baselineStrategy": "capture-before-first-iteration",
          "keepConditions": ["metric-improved", "verify-passed"],
          "discardConditions": ["metric-regressed", "metric-unchanged", "verify-failed"]
        },
        "workspacePolicy": { "snapshotStrategy": "patch-file" }
      },
      "skillTree": {
        "contextMarkdown": "Operate as an evidence-backed teaching-demo autoresearch loop. Improve the packaged course-QA assets one coherent change-set at a time and only keep changes when the measured score improves."
      }
    }
  }
}
```

### 26.6 `autoresearch` 最小实现版本

对应文件：
[pharmacy-autoresearch-research.runnable.example.jsonc](/home/biocheming/tests/rust/rocode/docs/examples/scheduler/pharmacy-autoresearch-research.runnable.example.jsonc)

配套脚本：
[evaluate-natural-product-model.sh](/home/biocheming/tests/rust/rocode/scripts/evaluate-natural-product-model.sh)
[check-pharmacy-demo-basics.sh](/home/biocheming/tests/rust/rocode/scripts/check-pharmacy-demo-basics.sh)

```jsonc
{
  "$schema": "https://rocode.dev/schemas/scheduler-profile.schema.json",
  "version": "2026-04-01",
  "defaults": { "profile": "pharmacy-research-autoresearch" },
  "profiles": {
    "pharmacy-research-autoresearch": {
      "orchestrator": "hephaestus",
      "workflow": {
        "workflow": { "kind": "autoresearch", "mode": "run" },
        "objective": {
          "goal": "Increase the local natural-product classification score for the packaged research demo without breaking the demo scripts.",
          "scope": {
            "include": [
              "docs/examples/scheduler/demo_assets/research_analysis/**",
              "scripts/evaluate-natural-product-model.sh",
              "scripts/check-pharmacy-demo-basics.sh"
            ],
            "exclude": ["target/**"]
          },
          "direction": "higher-is-better",
          "metric": { "kind": "numeric-extract", "pattern": "score=([0-9]+)" },
          "verify": { "command": "./scripts/evaluate-natural-product-model.sh" },
          "guard": { "command": "./scripts/check-pharmacy-demo-basics.sh" }
        },
        "iterationPolicy": { "mode": "bounded", "maxIterations": 3 },
        "decisionPolicy": {
          "baselineStrategy": "capture-before-first-iteration",
          "keepConditions": ["metric-improved", "verify-passed"],
          "discardConditions": ["metric-regressed", "metric-unchanged", "verify-failed"]
        },
        "workspacePolicy": { "snapshotStrategy": "patch-file" }
      },
      "skillTree": {
        "contextMarkdown": "Operate as an evidence-backed research-demo autoresearch loop. Improve the packaged natural-product classification assets one coherent change-set at a time and only keep changes when the measured score improves."
      }
    }
  }
}
```

---

## 27. 附：现场可用命令

```bash
rocode agent list
rocode debug agent deep-worker
rocode debug agent code-explorer
rocode debug agent docs-researcher
```

如果现场想演示 `autoresearch` 命令入口，建议口头补一句：

```bash
/autoresearch
```

它会先做命令预检；如果缺少目标、范围、评价标准、验证方式，就会先提问，再在补参后自动重提交。

如果要让听众看到当前项目到底用了哪个 scheduler 配置：

```bash
rocode debug config
```

如果要让听众回到概念图：

- `docs/examples/scheduler/SCHEDULER_GUIDE.md`
- `docs/examples/scheduler/README.md`
- `docs/examples/scheduler/AUTORESEARCH_RUNTIME.md`
- `docs/examples/scheduler/AUTORESEARCH_STATE_MODEL.md`
