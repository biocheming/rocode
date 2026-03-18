use async_trait::async_trait;
use rocode_core::contracts::output_blocks::keys as output_keys;
use rocode_core::contracts::todo::{keys as todo_keys, TodoPriority, TodoStatus};
use rocode_core::contracts::tools::BuiltinToolName;
use serde::{Deserialize, Serialize};

use crate::{TodoItemData, Tool, ToolContext, ToolError, ToolResult};

pub struct TodoReadTool;

pub struct TodoWriteTool;

#[derive(Debug, Serialize, Deserialize)]
struct TodoReadInput {
    #[serde(default, alias = "sessionId", alias = "sessionID")]
    session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TodoWriteInput {
    todos: Vec<TodoWriteItem>,
    #[serde(alias = "sessionId", alias = "sessionID")]
    session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TodoWriteItem {
    id: Option<String>,
    content: String,
    status: Option<String>,
    priority: Option<String>,
}

#[async_trait]
impl Tool for TodoReadTool {
    fn id(&self) -> &str {
        BuiltinToolName::TodoRead.as_str()
    }

    fn description(&self) -> &str {
        "Read the current todo list for the session. Returns all todo items with their status."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                (todo_keys::SESSION_ID): {
                    "type": "string",
                    "description": "Optional session ID. If not provided, uses current session."
                }
            }
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let input: TodoReadInput =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;

        let session_id = input
            .session_id
            .clone()
            .unwrap_or_else(|| ctx.session_id.clone());

        ctx.ask_permission(
            crate::PermissionRequest::new(BuiltinToolName::TodoRead.as_str())
                .with_metadata(todo_keys::SESSION_ID, serde_json::json!(&session_id))
                .always_allow(),
        )
        .await?;

        let todos = ctx.do_todo_get().await?;

        let output = format_todos_from_data(&todos);

        let todos_json: Vec<serde_json::Value> = todos
            .iter()
            .map(|t| {
                serde_json::json!({
                    todo_keys::CONTENT: t.content,
                    todo_keys::STATUS: t.status,
                    todo_keys::PRIORITY: t.priority
                })
            })
            .collect();

        let mut metadata = std::collections::HashMap::new();
        metadata.insert(todo_keys::TODOS.to_string(), serde_json::json!(todos_json));
        metadata.insert(todo_keys::COUNT.to_string(), serde_json::json!(todos.len()));

        Ok(ToolResult {
            title: format!("Todo List ({} items)", todos.len()),
            output,
            metadata,
            truncated: false,
        })
    }
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn id(&self) -> &str {
        BuiltinToolName::TodoWrite.as_str()
    }

    fn description(&self) -> &str {
        "Create or update the todo list for the session. Use this to track tasks and progress. Only call when the list actually changes; do not repeat identical updates."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                (todo_keys::TODOS): {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            (todo_keys::CONTENT): { "type": "string" },
                            (todo_keys::STATUS): { "type": "string", "enum": [
                                TodoStatus::Pending.as_str(),
                                TodoStatus::InProgress.as_str(),
                                TodoStatus::Completed.as_str(),
                                TodoStatus::Cancelled.as_str(),
                            ] },
                            (todo_keys::PRIORITY): { "type": "string" }
                        },
                        "required": [todo_keys::CONTENT]
                    },
                    "description": "List of todo items"
                },
                (todo_keys::SESSION_ID): {
                    "type": "string",
                    "description": "Optional session ID"
                }
            },
            "required": [todo_keys::TODOS]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let input: TodoWriteInput =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;

        let session_id = input
            .session_id
            .clone()
            .unwrap_or_else(|| ctx.session_id.clone());

        ctx.ask_permission(
            crate::PermissionRequest::new(BuiltinToolName::TodoWrite.as_str())
                .with_metadata(todo_keys::SESSION_ID, serde_json::json!(&session_id))
                .with_metadata(todo_keys::COUNT, serde_json::json!(input.todos.len()))
                .always_allow(),
        )
        .await?;

        let mut new_todos: Vec<TodoItemData> = Vec::new();

        for item in input.todos {
            let status = normalize_todo_status(item.status.as_deref())
                .as_str()
                .to_string();

            let _id = item
                .id
                .unwrap_or_else(|| format!("todo_{}", &uuid::Uuid::new_v4().to_string()[..8]));

            new_todos.push(TodoItemData {
                content: item.content,
                status,
                priority: normalize_todo_priority(item.priority.as_deref())
                    .as_str()
                    .to_string(),
            });
        }

        let existing_todos = ctx.do_todo_get().await?;
        if todos_equivalent(&existing_todos, &new_todos) {
            tracing::info!(
                session_id = %ctx.session_id,
                count = new_todos.len(),
                "todowrite deduplicated unchanged todo payload"
            );
            let mut metadata = std::collections::HashMap::new();
            metadata.insert(todo_keys::COUNT.to_string(), serde_json::json!(new_todos.len()));
            metadata.insert(todo_keys::NO_OP.to_string(), serde_json::json!(true));
            metadata.insert(
                output_keys::DISPLAY_SUMMARY.to_string(),
                serde_json::json!(format!(
                    "Todo list unchanged ({} items), skipped duplicate update",
                    new_todos.len()
                )),
            );

            return Ok(ToolResult {
                title: format!("Todo List unchanged ({} items)", new_todos.len()),
                output: "No todo changes detected. Skipped duplicate update.".to_string(),
                metadata,
                truncated: false,
            });
        }

        ctx.do_todo_update(new_todos.clone()).await?;

        let output = format_todos_from_data(&new_todos);

        let todos_json: Vec<serde_json::Value> = new_todos
            .iter()
            .map(|t| {
                serde_json::json!({
                    todo_keys::CONTENT: t.content,
                    todo_keys::STATUS: t.status,
                    todo_keys::PRIORITY: t.priority
                })
            })
            .collect();

        let mut metadata = std::collections::HashMap::new();
        metadata.insert(todo_keys::TODOS.to_string(), serde_json::json!(todos_json));
        metadata.insert(
            todo_keys::COUNT.to_string(),
            serde_json::json!(new_todos.len()),
        );

        Ok(ToolResult {
            title: format!("Updated Todo List ({} items)", new_todos.len()),
            output,
            metadata,
            truncated: false,
        })
    }
}

fn todos_equivalent(current: &[TodoItemData], next: &[TodoItemData]) -> bool {
    current.len() == next.len()
        && current.iter().zip(next.iter()).all(|(a, b)| {
            normalize_todo_content(&a.content) == normalize_todo_content(&b.content)
                && normalize_todo_status(Some(&a.status)) == normalize_todo_status(Some(&b.status))
                && normalize_todo_priority(Some(&a.priority))
                    == normalize_todo_priority(Some(&b.priority))
        })
}

fn normalize_todo_content(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_todo_status(value: Option<&str>) -> TodoStatus {
    value
        .and_then(|s| TodoStatus::parse(s))
        .unwrap_or(TodoStatus::Pending)
}

fn normalize_todo_priority(value: Option<&str>) -> TodoPriority {
    value
        .and_then(|s| TodoPriority::parse(s))
        .unwrap_or(TodoPriority::Medium)
}

fn format_todos_from_data(todos: &[TodoItemData]) -> String {
    if todos.is_empty() {
        return "No todos in the list.".to_string();
    }

    let mut output = String::new();
    output.push_str("# Todo List\n\n");

    for (i, todo) in todos.iter().enumerate() {
        let status = normalize_todo_status(Some(todo.status.as_str()));
        let status_icon = match status {
            TodoStatus::InProgress => "🔄",
            TodoStatus::Completed => "✅",
            _ => "⬜",
        };

        let priority_str = if !todo.priority.is_empty() {
            format!(" [{}]", todo.priority)
        } else {
            String::new()
        };

        output.push_str(&format!(
            "{} todo_{}{}\n   {}\n\n",
            status_icon, i, priority_str, todo.content
        ));
    }

    let pending = todos
        .iter()
        .filter(|t| normalize_todo_status(Some(t.status.as_str())) == TodoStatus::Pending)
        .count();
    let in_progress = todos
        .iter()
        .filter(|t| normalize_todo_status(Some(t.status.as_str())) == TodoStatus::InProgress)
        .count();
    let completed = todos
        .iter()
        .filter(|t| normalize_todo_status(Some(t.status.as_str())) == TodoStatus::Completed)
        .count();

    output.push_str(&format!(
        "Summary: {} pending, {} in progress, {} completed",
        pending, in_progress, completed
    ));

    output
}

impl Default for TodoReadTool {
    fn default() -> Self {
        Self
    }
}

impl Default for TodoWriteTool {
    fn default() -> Self {
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[tokio::test]
    async fn todowrite_skips_duplicate_updates() {
        let update_calls = Arc::new(AtomicUsize::new(0));
        let update_calls_clone = Arc::clone(&update_calls);

        let existing = vec![TodoItemData {
            content: "分析 t2.html 当前内容和结构".to_string(),
            status: TodoStatus::Completed.as_str().to_string(),
            priority: TodoPriority::High.as_str().to_string(),
        }];

        let ctx = ToolContext::new(
            "session-1".to_string(),
            "message-1".to_string(),
            ".".to_string(),
        )
        .with_todo_get({
            let existing = existing.clone();
            move |_| {
                let existing = existing.clone();
                async move { Ok(existing) }
            }
        })
        .with_todo_update(move |_, _| {
            let update_calls_clone = Arc::clone(&update_calls_clone);
            async move {
                update_calls_clone.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });

        let result = TodoWriteTool
            .execute(
                serde_json::json!({
                    (todo_keys::TODOS): [{
                        (todo_keys::CONTENT): "分析 t2.html 当前内容和结构",
                        (todo_keys::STATUS): TodoStatus::Completed.as_str(),
                        (todo_keys::PRIORITY): TodoPriority::High.as_str()
                    }]
                }),
                ctx,
            )
            .await
            .expect("todowrite should succeed");

        assert_eq!(update_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            result.metadata.get(todo_keys::NO_OP),
            Some(&serde_json::json!(true))
        );
        assert!(
            result.output.contains("No todo changes detected"),
            "expected no-op output"
        );
    }

    #[tokio::test]
    async fn todowrite_updates_when_content_changes() {
        let update_calls = Arc::new(AtomicUsize::new(0));
        let update_calls_clone = Arc::clone(&update_calls);

        let existing = vec![TodoItemData {
            content: "分析 t2.html 当前内容和结构".to_string(),
            status: TodoStatus::Pending.as_str().to_string(),
            priority: TodoPriority::High.as_str().to_string(),
        }];

        let ctx = ToolContext::new(
            "session-1".to_string(),
            "message-1".to_string(),
            ".".to_string(),
        )
        .with_todo_get({
            let existing = existing.clone();
            move |_| {
                let existing = existing.clone();
                async move { Ok(existing) }
            }
        })
        .with_todo_update(move |_, _| {
            let update_calls_clone = Arc::clone(&update_calls_clone);
            async move {
                update_calls_clone.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });

        let result = TodoWriteTool
            .execute(
                serde_json::json!({
                    (todo_keys::TODOS): [{
                        (todo_keys::CONTENT): "分析 t2.html 当前内容和结构",
                        (todo_keys::STATUS): TodoStatus::Completed.as_str(),
                        (todo_keys::PRIORITY): TodoPriority::High.as_str()
                    }]
                }),
                ctx,
            )
            .await
            .expect("todowrite should succeed");

        assert_eq!(update_calls.load(Ordering::SeqCst), 1);
        assert_ne!(
            result.metadata.get(todo_keys::NO_OP),
            Some(&serde_json::json!(true))
        );
    }

    #[tokio::test]
    async fn todowrite_skips_duplicates_with_whitespace_or_case_differences() {
        let update_calls = Arc::new(AtomicUsize::new(0));
        let update_calls_clone = Arc::clone(&update_calls);

        let existing = vec![TodoItemData {
            content: "Analyze   t2.html content".to_string(),
            status: "in_progress".to_string(),
            priority: "HIGH".to_string(),
        }];

        let ctx = ToolContext::new(
            "session-1".to_string(),
            "message-1".to_string(),
            ".".to_string(),
        )
        .with_todo_get({
            let existing = existing.clone();
            move |_| {
                let existing = existing.clone();
                async move { Ok(existing) }
            }
        })
        .with_todo_update(move |_, _| {
            let update_calls_clone = Arc::clone(&update_calls_clone);
            async move {
                update_calls_clone.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });

        let result = TodoWriteTool
            .execute(
                serde_json::json!({
                    (todo_keys::TODOS): [{
                        (todo_keys::CONTENT): "Analyze t2.html    content",
                        (todo_keys::STATUS): "in progress",
                        (todo_keys::PRIORITY): TodoPriority::High.as_str()
                    }]
                }),
                ctx,
            )
            .await
            .expect("todowrite should succeed");

        assert_eq!(update_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            result.metadata.get(todo_keys::NO_OP),
            Some(&serde_json::json!(true))
        );
    }
}
