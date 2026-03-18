# serde_json::json! C类裁决清单

更新时间：2026-03-18（第二轮清零）

本清单用于回填 Plan #2684 / Step 9321 的“C类逐项裁决与清尾”。

## 裁决标准

- **KEEP**：动态键/开放元数据协议/JSON Schema 声明体等，`json!` 保留更合理。
- **REFACTOR**：稳定、重复、可抽象为结构体（`Serialize`）的 JSON 构造。

## 模块汇总（生产代码）

- `crates/rocode-command/src`：KEEP 1，REFACTOR 1（已完成）
- `crates/rocode-tool/src`：KEEP 40，REFACTOR 12（已完成）

> 注：统计来自 `src/**`，排除了 tests/examples/docs 与 `#[cfg(test)]` 代码。

## REFACTOR 项（含本步处理结果）

### 已完成（本步）

1. `crates/rocode-command/src/lib.rs`
   - 场景：`execute_with_hooks` command hook payload。
   - 处理：改为 `CommandExecuteBeforeHookPayload` / `CommandHookPart` 结构体序列化。

2. `crates/rocode-tool/src/task_flow.rs`
   - 场景：`execute_delegate` 构造 `task_args`。
   - 处理：改为 `TaskInvokeArgs` 结构体序列化。

3. `crates/rocode-tool/src/task_flow.rs`
   - 场景：投影 todo 时包装 `{"todos": ...}`。
   - 处理：改为 `TodoWriteArgs` 结构体序列化。

4. `crates/rocode-tool/src/plan.rs`
   - 场景：`create_user_message_with_part` 中 message/part 稳定结构。
   - 处理：改为 `UserMessageWire` / `TextPartWire` 结构体序列化。

5. `crates/rocode-tool/src/apply_patch.rs`
   - 场景：`files_metadata` 单文件条目对象反复拼接。
   - 处理：改为 `ApplyPatchFileChangeMeta`（含 wire key rename）。

6. `crates/rocode-tool/src/read.rs`
   - 场景：二进制附件对象构造。
   - 处理：改为 `ReadAttachment` 结构体序列化。

7. `crates/rocode-tool/src/media_inspect.rs`
   - 场景：预读 `read` 调用参数对象。
   - 处理：改为 `ReadInvokeArgs` 结构体序列化。

8. `crates/rocode-tool/src/edit/tool.rs`
   - 场景：LSP diagnostics 单条对象。
   - 处理：改为 `LspDiagnosticEntry` 结构体序列化。

9. `crates/rocode-tool/src/websearch.rs`
   - 场景：MCP 参数对象构造。
   - 处理：改为 `WebSearchMcpArguments` 并合并进 arguments map。

10. `crates/rocode-tool/src/webfetch.rs`
    - 场景：图片 attachment 对象。
    - 处理：改为 `WebFetchImageAttachment` 结构体序列化。

11. `crates/rocode-tool/src/plugin_tool.rs`
    - 场景：plugin invoke context。
    - 处理：改为 `PluginInvokeContext` 结构体序列化。

12. `crates/rocode-tool/src/plugin_tool.rs`
    - 场景：大输出 attachment 元信息。
    - 处理：改为 `PluginOutputAttachment` 结构体序列化。

13. `crates/rocode-tool/src/registry.rs`
    - 场景：`recover_write_args_from_jsonish` 重建对象。
    - 处理：改为 `RecoveredWriteArgs` 结构体序列化。

### 剩余待改（下一批）

已清零（0 项）。

## KEEP 项（按类别回填）

1. **JSON Schema 声明体（保留）**
   - `task.rs`, `task_flow.rs`, `batch.rs`, `question.rs`, `todo.rs`, `read.rs`, `media_inspect.rs`,
     `ast_grep_*`, `bash.rs`, `write.rs`, `grep_tool.rs`, `glob_tool.rs`, `websearch.rs`,
     `webfetch.rs`, `skill.rs`, `repo_history.rs`, `plan.rs`, `multiedit.rs`, `lsp_tool.rs`,
     `invalid.rs`, `ls.rs`, `codesearch.rs`, `apply_patch.rs` 等的 `parameters()`。
   - 原因：Schema 本质是声明式 JSON 文档，`json!` 更直观。

2. **权限/插件 Hook/事件总线动态 metadata（保留）**
   - `task_flow.rs`（权限 metadata）、`registry.rs`（plugin hook payload）、
     `read.rs`/`write.rs`/`edit`/`shell_session` 等权限与运行态 metadata。
   - 原因：字段集合开放、演进快，动态 map 更稳定。

3. **结果统计/展示型动态 metadata（保留）**
   - `github_research.rs`, `repo_history.rs`, `context_docs.rs`, `ast_grep_*` 等。
   - 原因：展示层字段高度可选且跨工具不一致，结构体收益低。

4. **外部目录与跨工具桥接上下文（保留）**
   - `external_directory.rs`, `media_inspect.rs`（部分）、`browser_session.rs` 等。
   - 原因：与运行环境、扩展点耦合，动态 JSON 更合适。
