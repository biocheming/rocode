# ROCode 认证指南

ROCode 需要凭证来调用 LLM Provider API。本文档覆盖所有支持的认证方式、Provider 注册表以及模型目录系统。

---

## 认证方式

ROCode 通过以下优先级顺序查找凭证：

1. `rocode.jsonc` 中 Provider 配置的 `apiKey` 字段（最高优先级）
2. Provider 对应的环境变量
3. 认证插件（如 GitHub Copilot）提供的凭证
4. `~/.local/share/rocode/auth.json` 中持久化的凭证

第一个非空凭证将被使用。

---

## 方式一：环境变量（推荐）

最简单且最安全的方式。将密钥写入 shell profile，避免将凭证提交到版本控制。

```bash
# 添加到 ~/.bashrc 或 ~/.zshrc
export ZHIPUAI_API_KEY="zhipu-..."
```

Windows (Command Prompt)：

```cmd
setx ZHIPUAI_API_KEY "zhipu-..."
```

Windows (PowerShell)：

```powershell
$env:ZHIPUAI_API_KEY = "zhipu-..."
# 持久化：
[System.Environment]::SetEnvironmentVariable("ZHIPUAI_API_KEY","zhipu-...","User")
```

### Provider 环境变量

每个 Provider 通过 `{PROVIDER_ID}_API_KEY` 环境变量读取密钥，其中 Provider ID 中的连字符转为下划线并大写。

| Provider | Provider ID | 环境变量 |
|----------|------------|---------|
| Zhipu BigModel | `zhipuai` | `ZHIPUAI_API_KEY` |
| Alibaba Cloud Bailian | `alibaba-cn` | `ALIBABA_CN_API_KEY` |
| Moonshot Kimi API | `kimi-for-coding` | `KIMI_FOR_CODING_API_KEY` |
| Google | `google` | `GOOGLE_API_KEY` |
| Azure AI | `azure` | `AZURE_API_KEY` |
| AWS Bedrock | `amazon-bedrock` | `AMAZON_BEDROCK_API_KEY` |
| OpenRouter | `openrouter` | `OPENROUTER_API_KEY` |
| Groq | `groq` | `GROQ_API_KEY` |
| Mistral | `mistral` | `MISTRAL_API_KEY` |
| DeepSeek | `deepseek` | `DEEPSEEK_API_KEY` |
| xAI | `xai` | `XAI_API_KEY` |
| Cohere | `cohere` | `COHERE_API_KEY` |
| DeepInfra | `deepinfra` | `DEEPINFRA_API_KEY` |
| Cerebras | `cerebras` | `CEREBRAS_API_KEY` |
| Together AI | `togetherai` | `TOGETHER_API_KEY` |
| Perplexity | `perplexity` | `PERPLEXITY_API_KEY` |
| Venice | `venice` | `VENICE_API_KEY` |
| GitHub Copilot | `github-copilot` | `GITHUB_TOKEN` |
| GitLab | `gitlab` | `GITLAB_API_KEY` |
| Google Vertex | `google-vertex` | `GOOGLE_VERTEX_API_KEY` |

ROCode 通过 `models.dev` 获取完整的 Provider 目录，因此支持的 Provider 远不止上表所列。任何在 `models.dev` 中注册且具有兼容 chat/completions 接口的 Provider 均可使用。

按 2026-04-12 查到的官方模型归属，`qwen3.6-plus` 对应阿里云百炼 / Model Studio，`glm-5.1` 对应智谱 BigModel 开放平台，`kimi-k2.5` 对应 Moonshot AI / Kimi 开放平台。文档中的大陆示例统一按这三类来源展开。

---

## 方式二：配置文件

在 `rocode.jsonc` 中为 Provider 配置 API 密钥：

```jsonc
{
  "provider": {
    "zhipuai": {
      "name": "Zhipu AI",
      "apiKey": "zhipu-..."
    },
    "alibaba-cn": {
      "name": "Alibaba Cloud Bailian",
      "apiKey": "dashscope-..."
    },
    "kimi-for-coding": {
      "name": "Moonshot Kimi",
      "apiKey": "kimi-..."
    }
  }
}
```

也可以在单个模型级别设置不同的 API 密钥：

```jsonc
{
  "provider": {
    "custom-provider": {
      "name": "My Provider",
      "baseURL": "https://api.example.com/v1",
      "models": {
        "my-model": {
          "apiKey": "custom-key-for-this-model",
          "toolCall": true
        }
      }
    }
  }
}
```

> **注意：** 在共享或 CI 系统上，建议使用环境变量而非配置文件存储密钥。

---

## 方式三：CLI 登录

ROCode 提供内置的认证管理命令：

```bash
# 列出所有支持的 Provider 及当前认证状态
rocode auth list

# 为 Provider 设置凭证（进程内，不持久化）
rocode auth login zhipuai --token zhipu-...

# 清除 Provider 凭证
rocode auth logout zhipuai
```

---

## 方式四：认证插件

ROCode 的插件系统可以扩展认证方式。例如 GitHub Copilot 插件通过设备码流程自动获取 OAuth 令牌。

认证插件通过 `AuthBridge` 协议与宿主通信，自动注入到 Provider 注册表中。

```bash
# 检查 GitHub 状态
rocode github status

# 安装 GitHub Agent
rocode github install
```

---

## 持久化凭证

凭证可持久化到 `~/.local/share/rocode/auth.json`。文件格式为 Provider ID 到凭证信息的映射：

```json
{
  "zhipuai": {
    "type": "api",
    "key": "zhipu-..."
  },
  "github-copilot": {
    "type": "oauth",
    "access": "...",
    "refresh": "...",
    "expires": 1700000000,
    "accountId": "acct_1",
    "enterpriseUrl": "https://enterprise.example.com"
  },
  "https://corp.example.com": {
    "type": "wellknown",
    "key": "CORP_TOKEN",
    "token": "secret-123"
  }
}
```

凭证类型：

| 类型 | 字段 | 说明 |
|------|------|------|
| `api` | `key` | 直接 API 密钥 |
| `oauth` | `access`, `refresh`, `expires`, `accountId`, `enterpriseUrl` | OAuth 令牌对 |
| `wellknown` | `key`, `token` | 通过 `.well-known/opencode` 获取的远程凭证 |

该文件在 Unix 系统上自动设置 `600` 权限（仅用户可读写）。不要将其提交到版本控制。

---

## Well-Known 远程配置

对于企业部署，ROCode 支持通过 `.well-known/opencode` 端点获取远程配置。在 `auth.json` 中添加 `wellknown` 类型的条目：

```json
{
  "https://corp.example.com": {
    "type": "wellknown",
    "key": "CORP_TOKEN",
    "token": "secret-123"
  }
}
```

ROCode 启动时会自动：

1. 从 `auth.json` 读取 `wellknown` 条目
2. 设置对应的环境变量（如 `CORP_TOKEN`）
3. 请求 `{url}/.well-known/opencode` 获取远程配置
4. 将远程配置作为最低优先级合并到本地配置

远程配置在内存中缓存 5 分钟（TTL）。网络失败不会阻止启动。

---

## Provider 注册表

ROCode 使用三层 Provider 发现机制：

### 第一层：models.dev 目录

ROCode 在首次运行时从 `https://models.dev` 获取完整的 Provider 和模型目录，并缓存到本地。这提供了对所有主流 Provider 的内置支持。

```bash
# 刷新模型目录
rocode models --refresh

# 列出所有可用模型
rocode models

# 列出特定 Provider 的模型
rocode models zhipuai

# 详细输出（包含能力信息）
rocode models --verbose
```

### 第二层：配置文件覆盖

`rocode.jsonc` 中的 `provider` 字段可以覆盖或扩展 models.dev 中的 Provider 定义：

```jsonc
{
  "provider": {
    "zhipuai": {
      "name": "Zhipu AI",
      "whitelist": ["glm-5.1"]
    },
    "my-custom": {
      "name": "My Custom Provider",
      "baseURL": "https://api.custom.com/v1",
      "models": {
        "my-model": {
          "name": "My Model",
          "toolCall": true,
          "limit": { "context": 128000, "output": 4096 }
        }
      }
    }
  }
}
```

### 第三层：Provider 启用/禁用

```jsonc
{
  "disabledProviders": ["groq", "cerebras"],
  "enabledProviders": ["zhipuai", "alibaba-cn", "kimi-for-coding"]
}
```

`enabledProviders` 如果非空，则只有列出的 Provider 会被激活。`disabledProviders` 总是排除指定的 Provider。

---

## 模型目录

### 查看可用模型

```bash
# 列出所有模型
rocode models

# 列出特定 Provider 的模型
rocode models alibaba-cn

# 刷新缓存
rocode models --refresh

# 包含详细能力信息
rocode models --verbose
```

### 切换模型

运行时通过 CLI 参数指定模型：

```bash
rocode run -m glm-5.1 "task"
rocode tui -m qwen3.6-plus
```

在 TUI 中使用斜杠命令切换：

```
/models              # 列出可用模型
/model glm-5.1       # 切换到指定模型
```

### 默认模型

在 `rocode.jsonc` 中设置默认模型：

```jsonc
{
  "model": "glm-5.1",
  "smallModel": "qwen3.6-plus"
}
```

- `model` -- 主模型，用于常规任务
- `smallModel` -- 小型模型，用于轻量任务（如摘要、路由）

---

## 多 Provider 配置示例

```jsonc
{
  "model": "glm-5.1",
  "provider": {
    "zhipuai": {
      "name": "Zhipu AI",
      // 密钥通过环境变量 ZHIPUAI_API_KEY 提供
    },
    "alibaba-cn": {
      "name": "Alibaba Cloud Bailian",
      // 密钥通过环境变量 ALIBABA_CN_API_KEY 提供
    },
    "kimi-for-coding": {
      "name": "Moonshot Kimi",
      "apiKey": "kimi-..."
    },
    "ollama": {
      "name": "Ollama",
      "baseURL": "http://localhost:11434"
    },
    "openrouter": {
      "name": "OpenRouter",
      "apiKey": "sk-or-...",
      "whitelist": ["zhipuai/glm-5.1", "alibaba-cn/qwen3.6-plus", "kimi-for-coding/kimi-k2.5"]
    }
  }
}
```

### 本地模型（无需 API 密钥）

**Ollama：**

```bash
# 安装 Ollama: https://ollama.ai
ollama pull llama3.2

# 在配置中设置 Ollama 为默认 Provider
rocode tui -m llama3.2
```

对应 `rocode.jsonc`：

```jsonc
{
  "model": "llama3.2",
  "provider": {
    "ollama": {
      "baseURL": "http://localhost:11434"
    }
  }
}
```

---

## 检查认证状态

```bash
rocode auth list
```

输出所有支持的 Provider 及当前环境中的密钥状态。

查看 Token 使用统计：

```bash
rocode stats
rocode stats --days 7 --tools 5 --models 10
```

---

## 安全建议

- 优先使用环境变量存储 API 密钥，而非配置文件
- 限制 `~/.local/share/rocode/` 目录权限：

```bash
chmod 700 ~/.local/share/rocode
chmod 600 ~/.local/share/rocode/auth.json
```

- 不要将 `auth.json` 提交到版本控制
- 将 `.rocode/` 添加到 `.gitignore`
- 定期在 Provider 控制台轮换 API 密钥
- 在共享机器上使用 `rocode auth logout` 清除凭证

---

## 参见

- [配置参考](configuration) -- `rocode.jsonc` 完整配置
- [安装指南](installation) -- 构建和环境配置
