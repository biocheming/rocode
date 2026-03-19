use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use rocode_core::agent_task_registry::{global_task_registry, AgentTask, AgentTaskStatus};
use rocode_core::contracts::tools::BuiltinToolName;

use crate::task::TaskTool;
use crate::todo::TodoWriteTool;
use crate::{Metadata, PermissionRequest, TodoItemData, Tool, ToolContext, ToolError, ToolResult};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;
const DESCRIPTION: &str = r#"Task lifecycle orchestration facade.

This tool is the ROCode semantic entry point for task lifecycle operations.
It does not replace the `task` tool's delegated execution path. Instead, it
provides a stable request-level interface for task create/resume/get/list/cancel
operations that will be backed by existing authorities.

Phase 1 status:
- get/list are implemented as read-only registry-backed operations
- cancel is implemented via orchestration lifecycle mediation
- create/resume are implemented as thin adapters over the existing `task` tool
"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TaskFlowOperation {
    Create,
    Resume,
    Get,
    List,
    Cancel,
}

impl TaskFlowOperation {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Resume => "resume",
            Self::Get => "get",
            Self::List => "list",
            Self::Cancel => "cancel",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskFlowTodoItemInput {
    content: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    priority: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskFlowInput {
    operation: TaskFlowOperation,
    #[serde(default, alias = "task_id")]
    task_id: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default, alias = "load_skills")]
    load_skills: Option<Vec<String>>,
    #[serde(default, alias = "run_in_background")]
    run_in_background: bool,
    #[serde(default, alias = "sync_todo")]
    sync_todo: bool,
    #[serde(default, alias = "todo_item")]
    todo_item: Option<TaskFlowTodoItemInput>,
    #[serde(default, alias = "status_filter")]
    status_filter: Option<Vec<String>>,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct TaskFlowTaskView {
    task_id: String,
    agent: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    step: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    steps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_steps: Option<u32>,
    started_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    finished_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_tail: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct TaskFlowModelView {
    #[serde(rename = "providerID", alias = "providerId", alias = "provider_id")]
    provider_id: String,
    #[serde(rename = "modelID", alias = "modelId", alias = "model_id")]
    model_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DelegatedTaskMetadata<'a> {
    operation: &'a str,
    delegated: bool,
    agent_task_id: &'a str,
    session_id: &'a str,
    has_text_output: bool,
    todo_synced: bool,
    task: &'a TaskFlowTaskView,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<TaskFlowModelView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    loaded_skills: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    loaded_skill_count: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct TaskFlowOperationMetadata<'a> {
    operation: &'a str,
    task: &'a TaskFlowTaskView,
    #[serde(rename = "display.summary")]
    display_summary: String,
}

#[derive(Debug, Serialize)]
struct TaskFlowListMetadata<'a> {
    operation: &'a str,
    count: usize,
    truncated: bool,
    tasks: &'a [TaskFlowTaskView],
    #[serde(rename = "display.summary")]
    display_summary: String,
}

#[derive(Debug, Serialize)]
struct TaskInvokeArgs<'a> {
    subagent_type: &'a str,
    prompt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    load_skills: Option<&'a [String]>,
    run_in_background: bool,
}

#[derive(Debug, Serialize)]
struct TodoWriteArgs {
    todos: Vec<TodoItemData>,
}

fn deserialize_opt_string_lossy<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(value)) => Some(value),
        Some(serde_json::Value::Number(value)) => Some(value.to_string()),
        Some(serde_json::Value::Bool(value)) => Some(value.to_string()),
        _ => None,
    })
}

fn deserialize_bool_lossy<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Bool(value)) => value,
        Some(serde_json::Value::Number(value)) => value.as_i64().is_some_and(|value| value != 0),
        Some(serde_json::Value::String(value)) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        _ => false,
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DelegatedTaskMetadataWire {
    #[serde(
        default,
        alias = "agent_task_id",
        deserialize_with = "deserialize_opt_string_lossy"
    )]
    agent_task_id: Option<String>,
    #[serde(
        default,
        alias = "session_id",
        deserialize_with = "deserialize_opt_string_lossy"
    )]
    session_id: Option<String>,
    #[serde(
        default,
        alias = "has_text_output",
        deserialize_with = "deserialize_bool_lossy"
    )]
    has_text_output: bool,
    #[serde(default)]
    model: Option<TaskFlowModelView>,
    #[serde(default, alias = "loaded_skills")]
    loaded_skills: Option<serde_json::Value>,
    #[serde(default, alias = "loaded_skill_count")]
    loaded_skill_count: Option<serde_json::Value>,
}

fn delegated_task_metadata_wire(metadata: &Metadata) -> DelegatedTaskMetadataWire {
    serde_json::from_value::<DelegatedTaskMetadataWire>(serde_json::Value::Object(
        metadata.clone().into_iter().collect(),
    ))
    .unwrap_or_default()
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

pub struct TaskFlowTool;

impl TaskFlowTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TaskFlowTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for TaskFlowTool {
    fn id(&self) -> &str {
        "task_flow"
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["create", "resume", "get", "list", "cancel"],
                    "description": "Task lifecycle operation"
                },
                "task_id": {
                    "type": "string",
                    "description": "Registry task id or delegated session id depending on operation"
                },
                "agent": {
                    "type": "string",
                    "description": "Agent name to delegate to for create"
                },
                "description": {
                    "type": "string",
                    "description": "Short task label for UI and summaries"
                },
                "prompt": {
                    "type": "string",
                    "description": "Delegated prompt body for create or resume"
                },
                "command": {
                    "type": "string",
                    "description": "Optional source command or trigger name"
                },
                "load_skills": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional explicit skills to inject into delegated prompt"
                },
                "run_in_background": {
                    "type": "boolean",
                    "default": false,
                    "description": "Whether create should return immediately after dispatch"
                },
                "sync_todo": {
                    "type": "boolean",
                    "default": false,
                    "description": "Whether to project task lifecycle into the session todo board"
                },
                "todo_item": {
                    "type": "object",
                    "properties": {
                        "content": { "type": "string" },
                        "status": {
                            "type": "string",
                            "enum": ["pending", "in_progress", "completed"]
                        },
                        "priority": { "type": "string" }
                    },
                    "required": ["content"]
                },
                "status_filter": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["pending", "running", "completed", "cancelled", "failed"]
                    },
                    "description": "Optional status filter for list"
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100,
                    "default": 20,
                    "description": "Maximum number of tasks to return for list"
                }
            },
            "required": ["operation"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let input: TaskFlowInput =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
        validate_input(&input)?;

        let mut permission = PermissionRequest::for_tool(BuiltinToolName::TaskFlow)
            .with_metadata("operation", serde_json::json!(input.operation.as_str()))
            .always_allow();
        if let Some(task_id) = input.task_id.as_ref() {
            permission = permission.with_metadata("task_id", serde_json::json!(task_id));
        }
        if let Some(agent) = input.agent.as_ref() {
            permission = permission.with_metadata("agent", serde_json::json!(agent));
        }
        if let Some(status_filter) = input.status_filter.as_ref() {
            permission =
                permission.with_metadata("status_filter", serde_json::json!(status_filter));
        }
        permission = permission
            .with_metadata("limit", serde_json::json!(input.limit))
            .with_metadata("sync_todo", serde_json::json!(input.sync_todo));
        ctx.ask_permission(permission).await?;

        match input.operation {
            TaskFlowOperation::Get => execute_get(&input),
            TaskFlowOperation::List => execute_list(&input),
            TaskFlowOperation::Cancel => execute_cancel(&input),
            TaskFlowOperation::Create => {
                execute_delegate(&input, ctx, TaskFlowOperation::Create).await
            }
            TaskFlowOperation::Resume => {
                execute_delegate(&input, ctx, TaskFlowOperation::Resume).await
            }
        }
    }
}

async fn execute_delegate(
    input: &TaskFlowInput,
    ctx: ToolContext,
    operation: TaskFlowOperation,
) -> Result<ToolResult, ToolError> {
    let agent = input.agent.as_deref().unwrap_or_default().trim();
    let prompt = input.prompt.as_deref().unwrap_or_default().trim();
    let todo_ctx = build_todo_projection_context(&ctx);

    let task_args = TaskInvokeArgs {
        subagent_type: agent,
        prompt,
        description: input
            .description
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty()),
        task_id: input
            .task_id
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty()),
        command: input
            .command
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty()),
        load_skills: input.load_skills.as_deref(),
        run_in_background: input.run_in_background,
    };

    let delegated = TaskTool::new()
        .execute(
            serde_json::to_value(task_args).unwrap_or(serde_json::Value::Null),
            ctx,
        )
        .await?;

    let delegated_metadata = delegated_task_metadata_wire(&delegated.metadata);
    let agent_task_id = delegated_metadata.agent_task_id.clone().ok_or_else(|| {
        ToolError::ExecutionError("delegated task metadata missing `agentTaskId`".to_string())
    })?;
    let session_id = delegated_metadata.session_id.clone().ok_or_else(|| {
        ToolError::ExecutionError("delegated task metadata missing `sessionId`".to_string())
    })?;
    let has_text_output = delegated_metadata.has_text_output;
    let task = global_task_registry().get(&agent_task_id).ok_or_else(|| {
        ToolError::ExecutionError(format!(
            "delegated task `{}` completed but could not be reloaded from agent task registry",
            agent_task_id
        ))
    })?;
    let view = task_to_view(&task, true);
    let todo_synced = if matches!(operation, TaskFlowOperation::Create) && input.sync_todo {
        project_task_to_todo(input, &view, todo_ctx).await?
    } else {
        false
    };

    let metadata_wire = DelegatedTaskMetadata {
        operation: operation.as_str(),
        delegated: true,
        agent_task_id: &agent_task_id,
        session_id: &session_id,
        has_text_output,
        todo_synced,
        task: &view,
        model: delegated_metadata.model,
        loaded_skills: delegated_metadata.loaded_skills,
        loaded_skill_count: delegated_metadata.loaded_skill_count,
    };
    let mut metadata = serde_json::to_value(metadata_wire)
        .ok()
        .and_then(|value| serde_json::from_value::<Metadata>(value).ok())
        .unwrap_or_default();
    metadata.insert(
        "display.summary".to_string(),
        serde_json::json!(format!(
            "Delegated {} task {} via session {}",
            operation.as_str(),
            metadata["agentTaskId"].as_str().unwrap_or_default(),
            metadata["sessionId"].as_str().unwrap_or_default()
        )),
    );

    Ok(ToolResult {
        title: format!(
            "{} Task {}",
            title_case(operation.as_str()),
            metadata["agentTaskId"].as_str().unwrap_or_default()
        ),
        output: render_delegated_task_output(
            operation.as_str(),
            metadata["agentTaskId"].as_str().unwrap_or_default(),
            metadata["sessionId"].as_str().unwrap_or_default(),
            &view.status,
            &delegated.output,
        ),
        metadata,
        truncated: delegated.truncated,
    })
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn build_todo_projection_context(ctx: &ToolContext) -> ToolContext {
    ctx.clone()
}

async fn project_task_to_todo(
    input: &TaskFlowInput,
    view: &TaskFlowTaskView,
    todo_ctx: ToolContext,
) -> Result<bool, ToolError> {
    if todo_ctx.todo_get.is_none() || todo_ctx.todo_update.is_none() {
        return Ok(false);
    }

    let mut todos = todo_ctx.do_todo_get().await?;
    todos.push(build_projection_todo_item(input, view));

    TodoWriteTool
        .execute(
            serde_json::to_value(TodoWriteArgs { todos }).unwrap_or(serde_json::Value::Null),
            todo_ctx,
        )
        .await?;

    Ok(true)
}

fn build_projection_todo_item(input: &TaskFlowInput, view: &TaskFlowTaskView) -> TodoItemData {
    let content = input
        .todo_item
        .as_ref()
        .map(|item| item.content.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            input
                .description
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .or_else(|| {
            input
                .prompt
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| {
                    value
                        .lines()
                        .next()
                        .unwrap_or(value)
                        .trim()
                        .chars()
                        .take(120)
                        .collect()
                })
        })
        .unwrap_or_else(|| format!("Delegated {} task", view.agent));

    let status = input
        .todo_item
        .as_ref()
        .and_then(|item| item.status.as_deref())
        .map(normalize_projection_todo_status)
        .unwrap_or_else(|| task_status_to_todo_status(&view.status))
        .to_string();
    let priority = input
        .todo_item
        .as_ref()
        .and_then(|item| item.priority.as_deref())
        .map(normalize_projection_todo_priority)
        .unwrap_or("medium")
        .to_string();

    TodoItemData {
        content,
        status,
        priority,
    }
}

fn task_status_to_todo_status(status: &str) -> &'static str {
    match status.trim() {
        "running" => "in_progress",
        "completed" => "completed",
        "cancelled" | "failed" => "pending",
        _ => "pending",
    }
}

fn normalize_projection_todo_status(status: &str) -> &'static str {
    match status.trim().to_ascii_lowercase().as_str() {
        "in_progress" | "in-progress" | "in progress" | "doing" => "in_progress",
        "completed" | "done" => "completed",
        _ => "pending",
    }
}

fn normalize_projection_todo_priority(priority: &str) -> &'static str {
    match priority.trim().to_ascii_lowercase().as_str() {
        "high" => "high",
        "low" => "low",
        _ => "medium",
    }
}

fn execute_get(input: &TaskFlowInput) -> Result<ToolResult, ToolError> {
    let task_id = input.task_id.as_deref().unwrap_or_default().trim();
    let task = global_task_registry().get(task_id).ok_or_else(|| {
        ToolError::ExecutionError(format!(
            "task `{}` was not found in agent task registry",
            task_id
        ))
    })?;

    let view = task_to_view(&task, true);
    let metadata = serde_json::to_value(TaskFlowOperationMetadata {
        operation: "get",
        task: &view,
        display_summary: format!("Loaded task {} ({})", view.task_id, view.status),
    })
    .ok()
    .and_then(|value| serde_json::from_value::<Metadata>(value).ok())
    .unwrap_or_default();

    Ok(ToolResult {
        title: format!("Task {}", view.task_id),
        output: render_task_detail_output(&view),
        metadata,
        truncated: false,
    })
}

fn execute_list(input: &TaskFlowInput) -> Result<ToolResult, ToolError> {
    let tasks = global_task_registry().list();
    let (views, truncated) = list_task_views(&tasks, input.status_filter.as_deref(), input.limit);

    let metadata = serde_json::to_value(TaskFlowListMetadata {
        operation: "list",
        count: views.len(),
        truncated,
        tasks: &views,
        display_summary: format!(
            "Listed {} task(s){}",
            views.len(),
            if truncated { " (truncated)" } else { "" }
        ),
    })
    .ok()
    .and_then(|value| serde_json::from_value::<Metadata>(value).ok())
    .unwrap_or_default();

    Ok(ToolResult {
        title: format!("Task List ({})", views.len()),
        output: render_task_list_output(&views, truncated),
        metadata,
        truncated,
    })
}

fn execute_cancel(input: &TaskFlowInput) -> Result<ToolResult, ToolError> {
    let task_id = input.task_id.as_deref().unwrap_or_default().trim();
    rocode_orchestrator::global_lifecycle()
        .cancel_task(task_id)
        .map_err(ToolError::ExecutionError)?;

    let task = global_task_registry().get(task_id).ok_or_else(|| {
        ToolError::ExecutionError(format!(
            "task `{}` was cancelled but could not be reloaded from agent task registry",
            task_id
        ))
    })?;
    let view = task_to_view(&task, true);

    let metadata = serde_json::to_value(TaskFlowOperationMetadata {
        operation: "cancel",
        task: &view,
        display_summary: format!("Cancelled task {}", view.task_id),
    })
    .ok()
    .and_then(|value| serde_json::from_value::<Metadata>(value).ok())
    .unwrap_or_default();

    Ok(ToolResult {
        title: format!("Cancelled Task {}", view.task_id),
        output: render_task_detail_output(&view),
        metadata,
        truncated: false,
    })
}

fn list_task_views(
    tasks: &[AgentTask],
    status_filter: Option<&[String]>,
    limit: usize,
) -> (Vec<TaskFlowTaskView>, bool) {
    let mut selected = tasks
        .iter()
        .filter(|task| task_matches_status_filter(task, status_filter))
        .map(|task| task_to_view(task, false))
        .collect::<Vec<_>>();
    let truncated = selected.len() > limit;
    selected.truncate(limit);
    (selected, truncated)
}

fn task_matches_status_filter(task: &AgentTask, status_filter: Option<&[String]>) -> bool {
    let Some(status_filter) = status_filter else {
        return true;
    };
    let status = task_status_label(&task.status);
    status_filter.iter().any(|value| value == status)
}

fn task_to_view(task: &AgentTask, include_detail: bool) -> TaskFlowTaskView {
    let (status, step, steps, error) = status_fields(&task.status);
    TaskFlowTaskView {
        task_id: task.id.clone(),
        agent: task.agent_name.clone(),
        status: status.to_string(),
        step,
        steps,
        max_steps: task.max_steps,
        started_at: task.started_at,
        finished_at: task.finished_at,
        prompt: include_detail.then(|| task.prompt.clone()),
        output_tail: include_detail.then(|| task.output_tail.iter().cloned().collect()),
        error,
    }
}

fn status_fields(
    status: &AgentTaskStatus,
) -> (&'static str, Option<u32>, Option<u32>, Option<String>) {
    match status {
        AgentTaskStatus::Pending => ("pending", None, None, None),
        AgentTaskStatus::Running { step } => ("running", Some(*step), None, None),
        AgentTaskStatus::Completed { steps } => ("completed", None, Some(*steps), None),
        AgentTaskStatus::Cancelled => ("cancelled", None, None, None),
        AgentTaskStatus::Failed { error } => ("failed", None, None, Some(error.clone())),
    }
}

fn task_status_label(status: &AgentTaskStatus) -> &'static str {
    let (label, _, _, _) = status_fields(status);
    label
}

fn render_task_detail_output(view: &TaskFlowTaskView) -> String {
    let mut lines = vec![format!("Task {} — {}", view.task_id, view.agent)];
    let mut status_line = format!("Status: {}", view.status);
    if let Some(step) = view.step {
        let suffix = view
            .max_steps
            .map(|max_steps| format!(" (step {}/{})", step, max_steps))
            .unwrap_or_else(|| format!(" (step {}/?)", step));
        status_line.push_str(&suffix);
    }
    if let Some(steps) = view.steps {
        status_line.push_str(&format!(" ({} steps)", steps));
    }
    lines.push(status_line);
    lines.push(format!("StartedAt: {}", view.started_at));
    if let Some(finished_at) = view.finished_at {
        lines.push(format!("FinishedAt: {}", finished_at));
    }
    if let Some(prompt) = view.prompt.as_deref() {
        lines.push(format!("Prompt: {}", prompt));
    }
    if let Some(error) = view.error.as_deref() {
        lines.push(format!("Error: {}", error));
    }
    if let Some(output_tail) = view.output_tail.as_ref() {
        if !output_tail.is_empty() {
            lines.push("Recent output:".to_string());
            for line in output_tail {
                lines.push(format!("  {}", line));
            }
        }
    }
    lines.join("\n")
}

fn render_task_list_output(views: &[TaskFlowTaskView], truncated: bool) -> String {
    if views.is_empty() {
        return "No agent tasks matched the requested filters.".to_string();
    }

    let mut lines = Vec::with_capacity(views.len() + 2);
    for view in views {
        let mut extra = String::new();
        if let Some(step) = view.step {
            extra = view
                .max_steps
                .map(|max_steps| format!(" step={}/{}", step, max_steps))
                .unwrap_or_else(|| format!(" step={}/?", step));
        } else if let Some(steps) = view.steps {
            extra = format!(" steps={}", steps);
        }
        lines.push(format!(
            "- {} agent={} status={}{}",
            view.task_id, view.agent, view.status, extra
        ));
    }
    if truncated {
        lines.push(format!("… truncated to {} task(s)", views.len()));
    }
    lines.join("\n")
}

fn render_delegated_task_output(
    operation: &str,
    agent_task_id: &str,
    session_id: &str,
    status: &str,
    delegated_output: &str,
) -> String {
    format!(
        "task_flow_operation: {}\nagent_task_id: {}\nsession_id: {}\nstatus: {}\n\n{}",
        operation, agent_task_id, session_id, status, delegated_output
    )
}

fn validate_input(input: &TaskFlowInput) -> Result<(), ToolError> {
    if input.limit == 0 || input.limit > MAX_LIMIT {
        return Err(ToolError::InvalidArguments(format!(
            "limit must be between 1 and {}",
            MAX_LIMIT
        )));
    }

    if let Some(todo_item) = input.todo_item.as_ref() {
        if todo_item.content.trim().is_empty() {
            return Err(ToolError::InvalidArguments(
                "todo_item.content must be non-empty".to_string(),
            ));
        }
        if let Some(status) = todo_item.status.as_deref() {
            validate_todo_status(status)?;
        }
    }

    if let Some(statuses) = input.status_filter.as_ref() {
        for status in statuses {
            validate_task_status_filter(status)?;
        }
    }

    match input.operation {
        TaskFlowOperation::Create => {
            require_non_empty(input.agent.as_deref(), "agent is required for create")?;
            require_non_empty(input.prompt.as_deref(), "prompt is required for create")?;
        }
        TaskFlowOperation::Resume => {
            require_non_empty(input.task_id.as_deref(), "task_id is required for resume")?;
            require_non_empty(input.agent.as_deref(), "agent is required for resume")?;
            require_non_empty(input.prompt.as_deref(), "prompt is required for resume")?;
        }
        TaskFlowOperation::Get => {
            require_non_empty(input.task_id.as_deref(), "task_id is required for get")?;
        }
        TaskFlowOperation::List => {}
        TaskFlowOperation::Cancel => {
            require_non_empty(input.task_id.as_deref(), "task_id is required for cancel")?;
        }
    }

    Ok(())
}

fn require_non_empty(value: Option<&str>, message: &str) -> Result<(), ToolError> {
    if value.map(str::trim).filter(|v| !v.is_empty()).is_none() {
        return Err(ToolError::InvalidArguments(message.to_string()));
    }
    Ok(())
}

fn validate_todo_status(status: &str) -> Result<(), ToolError> {
    match status.trim() {
        "pending" | "in_progress" | "completed" => Ok(()),
        _ => Err(ToolError::InvalidArguments(
            "todo_item.status must be one of: pending, in_progress, completed".to_string(),
        )),
    }
}

fn validate_task_status_filter(status: &str) -> Result<(), ToolError> {
    match status.trim() {
        "pending" | "running" | "completed" | "cancelled" | "failed" => Ok(()),
        _ => Err(ToolError::InvalidArguments(
            "status_filter values must be one of: pending, running, completed, cancelled, failed"
                .to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn sample_task(status: AgentTaskStatus) -> AgentTask {
        let mut output_tail = VecDeque::new();
        output_tail.push_back("first line".to_string());
        output_tail.push_back("second line".to_string());
        AgentTask {
            id: "a42".to_string(),
            session_id: Some("ses_test".to_string()),
            agent_name: "build".to_string(),
            prompt: "Investigate runtime behavior".to_string(),
            status,
            started_at: 100,
            finished_at: Some(130),
            max_steps: Some(8),
            output_tail,
        }
    }

    #[test]
    fn schema_exposes_phase_one_operations() {
        let tool = TaskFlowTool::new();
        let schema = tool.parameters();
        let ops = schema["properties"]["operation"]["enum"]
            .as_array()
            .expect("operation enum should exist");

        assert!(ops.iter().any(|v| v == "create"));
        assert!(ops.iter().any(|v| v == "resume"));
        assert!(ops.iter().any(|v| v == "get"));
        assert!(ops.iter().any(|v| v == "list"));
        assert!(ops.iter().any(|v| v == "cancel"));
    }

    #[test]
    fn create_requires_agent_and_prompt() {
        let input = TaskFlowInput {
            operation: TaskFlowOperation::Create,
            task_id: None,
            agent: None,
            description: None,
            prompt: None,
            command: None,
            load_skills: None,
            run_in_background: false,
            sync_todo: false,
            todo_item: None,
            status_filter: None,
            limit: DEFAULT_LIMIT,
        };
        match validate_input(&input) {
            Err(ToolError::InvalidArguments(msg)) => assert!(msg.contains("agent is required")),
            other => panic!("unexpected result: {:?}", other),
        }

        let input = TaskFlowInput {
            agent: Some("build".to_string()),
            ..input
        };
        match validate_input(&input) {
            Err(ToolError::InvalidArguments(msg)) => assert!(msg.contains("prompt is required")),
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn resume_requires_task_id_agent_and_prompt() {
        let input = TaskFlowInput {
            operation: TaskFlowOperation::Resume,
            task_id: None,
            agent: Some("build".to_string()),
            description: None,
            prompt: Some("Continue".to_string()),
            command: None,
            load_skills: None,
            run_in_background: false,
            sync_todo: false,
            todo_item: None,
            status_filter: None,
            limit: DEFAULT_LIMIT,
        };
        match validate_input(&input) {
            Err(ToolError::InvalidArguments(msg)) => assert!(msg.contains("task_id is required")),
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn get_and_cancel_require_task_id() {
        let get_input = TaskFlowInput {
            operation: TaskFlowOperation::Get,
            task_id: None,
            agent: None,
            description: None,
            prompt: None,
            command: None,
            load_skills: None,
            run_in_background: false,
            sync_todo: false,
            todo_item: None,
            status_filter: None,
            limit: DEFAULT_LIMIT,
        };
        match validate_input(&get_input) {
            Err(ToolError::InvalidArguments(msg)) => assert!(msg.contains("required for get")),
            other => panic!("unexpected result: {:?}", other),
        }

        let cancel_input = TaskFlowInput {
            operation: TaskFlowOperation::Cancel,
            ..get_input
        };
        match validate_input(&cancel_input) {
            Err(ToolError::InvalidArguments(msg)) => assert!(msg.contains("required for cancel")),
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn limit_and_filters_are_validated() {
        let input = TaskFlowInput {
            operation: TaskFlowOperation::List,
            task_id: None,
            agent: None,
            description: None,
            prompt: None,
            command: None,
            load_skills: None,
            run_in_background: false,
            sync_todo: false,
            todo_item: None,
            status_filter: Some(vec!["running".to_string(), "completed".to_string()]),
            limit: 0,
        };
        assert!(matches!(
            validate_input(&input),
            Err(ToolError::InvalidArguments(_))
        ));

        let input = TaskFlowInput {
            limit: 999,
            ..input
        };
        assert!(matches!(
            validate_input(&input),
            Err(ToolError::InvalidArguments(_))
        ));

        let input = TaskFlowInput {
            limit: DEFAULT_LIMIT,
            status_filter: Some(vec!["unknown".to_string()]),
            ..input
        };
        match validate_input(&input) {
            Err(ToolError::InvalidArguments(msg)) => assert!(msg.contains("status_filter values")),
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn todo_item_content_and_status_are_validated() {
        let input = TaskFlowInput {
            operation: TaskFlowOperation::Create,
            task_id: None,
            agent: Some("build".to_string()),
            description: None,
            prompt: Some("Investigate".to_string()),
            command: None,
            load_skills: None,
            run_in_background: false,
            sync_todo: true,
            todo_item: Some(TaskFlowTodoItemInput {
                content: "   ".to_string(),
                status: Some("pending".to_string()),
                priority: None,
            }),
            status_filter: None,
            limit: DEFAULT_LIMIT,
        };
        match validate_input(&input) {
            Err(ToolError::InvalidArguments(msg)) => assert!(msg.contains("todo_item.content")),
            other => panic!("unexpected result: {:?}", other),
        }

        let input = TaskFlowInput {
            todo_item: Some(TaskFlowTodoItemInput {
                content: "Track task".to_string(),
                status: Some("bogus".to_string()),
                priority: None,
            }),
            ..input
        };
        match validate_input(&input) {
            Err(ToolError::InvalidArguments(msg)) => assert!(msg.contains("todo_item.status")),
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[test]
    fn task_to_view_maps_running_task_for_list() {
        let task = AgentTask {
            finished_at: None,
            status: AgentTaskStatus::Running { step: 3 },
            ..sample_task(AgentTaskStatus::Running { step: 3 })
        };
        let view = task_to_view(&task, false);

        assert_eq!(view.task_id, "a42");
        assert_eq!(view.agent, "build");
        assert_eq!(view.status, "running");
        assert_eq!(view.step, Some(3));
        assert_eq!(view.steps, None);
        assert_eq!(view.max_steps, Some(8));
        assert_eq!(view.prompt, None);
        assert_eq!(view.output_tail, None);
    }

    #[test]
    fn task_to_view_maps_failed_task_for_detail() {
        let task = sample_task(AgentTaskStatus::Failed {
            error: "boom".to_string(),
        });
        let view = task_to_view(&task, true);

        assert_eq!(view.status, "failed");
        assert_eq!(view.error.as_deref(), Some("boom"));
        assert_eq!(view.prompt.as_deref(), Some("Investigate runtime behavior"));
        assert_eq!(view.output_tail.as_ref().map(|v| v.len()), Some(2));
    }

    #[test]
    fn list_task_views_filters_and_truncates() {
        let running = AgentTask {
            id: "a1".to_string(),
            finished_at: None,
            status: AgentTaskStatus::Running { step: 1 },
            ..sample_task(AgentTaskStatus::Running { step: 1 })
        };
        let completed = AgentTask {
            id: "a2".to_string(),
            status: AgentTaskStatus::Completed { steps: 5 },
            ..sample_task(AgentTaskStatus::Completed { steps: 5 })
        };
        let cancelled = AgentTask {
            id: "a3".to_string(),
            status: AgentTaskStatus::Cancelled,
            ..sample_task(AgentTaskStatus::Cancelled)
        };

        let (views, truncated) = list_task_views(
            &[running, completed, cancelled],
            Some(&["running".to_string(), "completed".to_string()]),
            1,
        );

        assert_eq!(views.len(), 1);
        assert!(truncated);
        assert!(matches!(views[0].status.as_str(), "running" | "completed"));
    }

    #[test]
    fn render_task_detail_output_includes_recent_output() {
        let view = task_to_view(&sample_task(AgentTaskStatus::Completed { steps: 2 }), true);
        let output = render_task_detail_output(&view);

        assert!(output.contains("Task a42"));
        assert!(output.contains("Status: completed (2 steps)"));
        assert!(output.contains("Prompt: Investigate runtime behavior"));
        assert!(output.contains("Recent output:"));
        assert!(output.contains("first line"));
    }

    #[test]
    fn execute_cancel_returns_error_for_missing_task() {
        let input = TaskFlowInput {
            operation: TaskFlowOperation::Cancel,
            task_id: Some("nonexistent-task-flow-cancel".to_string()),
            agent: None,
            description: None,
            prompt: None,
            command: None,
            load_skills: None,
            run_in_background: false,
            sync_todo: false,
            todo_item: None,
            status_filter: None,
            limit: DEFAULT_LIMIT,
        };

        let error = execute_cancel(&input).expect_err("missing task should fail");
        match error {
            ToolError::ExecutionError(message) => {
                assert!(message.contains("not found") || message.contains("finished"));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn execute_cancel_marks_registered_task_cancelled() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();
        let task_id = global_task_registry().register(
            None,
            "build".to_string(),
            "cancel me".to_string(),
            Some(5),
            Arc::new(move || {
                called_clone.store(true, Ordering::SeqCst);
            }),
        );

        let input = TaskFlowInput {
            operation: TaskFlowOperation::Cancel,
            task_id: Some(task_id.clone()),
            agent: None,
            description: None,
            prompt: None,
            command: None,
            load_skills: None,
            run_in_background: false,
            sync_todo: false,
            todo_item: None,
            status_filter: None,
            limit: DEFAULT_LIMIT,
        };

        let result = execute_cancel(&input).expect("cancel should succeed");
        assert!(called.load(Ordering::SeqCst));
        assert_eq!(result.metadata["operation"], serde_json::json!("cancel"));
        assert_eq!(
            result.metadata["task"]["taskId"],
            serde_json::json!(task_id)
        );
        assert_eq!(
            result.metadata["task"]["status"],
            serde_json::json!("cancelled")
        );
    }

    #[tokio::test]
    async fn create_permission_metadata_includes_sync_todo_flag() {
        let requests = Arc::new(Mutex::new(Vec::<crate::PermissionRequest>::new()));

        let ctx = ToolContext::new("session-1".into(), "message-1".into(), ".".into())
            .with_ask({
                let requests = requests.clone();
                move |req| {
                    let requests = requests.clone();
                    async move {
                        requests.lock().await.push(req);
                        Ok(())
                    }
                }
            })
            .with_get_agent_info(|name| async move {
                if name == "build" {
                    Ok(Some(crate::TaskAgentInfo {
                        name: "build".to_string(),
                        model: None,
                        can_use_task: true,
                        steps: Some(4),
                        execution: None,
                        max_tokens: None,
                        temperature: None,
                        top_p: None,
                        variant: None,
                    }))
                } else {
                    Ok(None)
                }
            })
            .with_get_last_model(|_session_id| async move { Ok(Some("provider-x:model-y".into())) })
            .with_create_subsession(|_agent, _title, _model, _disabled_tools| async move {
                Ok("task_build_122".to_string())
            })
            .with_prompt_subsession(|_session_id, _prompt| async move {
                Ok("subagent output".to_string())
            });

        TaskFlowTool::new()
            .execute(
                serde_json::json!({
                    "operation": "create",
                    "agent": "build",
                    "prompt": "Please inspect runtime behavior",
                    "sync_todo": true
                }),
                ctx,
            )
            .await
            .expect("create should succeed");

        let requests = requests.lock().await.clone();
        let task_flow_request = requests
            .iter()
            .find(|req| req.permission == "task_flow")
            .expect("task_flow permission request should exist");
        assert_eq!(
            task_flow_request.metadata.get("operation"),
            Some(&serde_json::json!("create"))
        );
        assert_eq!(
            task_flow_request.metadata.get("agent"),
            Some(&serde_json::json!("build"))
        );
        assert_eq!(
            task_flow_request.metadata.get("sync_todo"),
            Some(&serde_json::json!(true))
        );
    }

    #[tokio::test]
    async fn execute_create_delegates_to_task_tool_and_returns_dual_ids() {
        let create_calls = Arc::new(Mutex::new(Vec::<(
            String,
            Option<String>,
            Option<String>,
            Vec<String>,
        )>::new()));
        let prompt_calls = Arc::new(Mutex::new(Vec::<(String, String)>::new()));

        let ctx = ToolContext::new("session-1".into(), "message-1".into(), ".".into())
            .with_get_agent_info(|name| async move {
                if name == "build" {
                    Ok(Some(crate::TaskAgentInfo {
                        name: "build".to_string(),
                        model: None,
                        can_use_task: true,
                        steps: Some(4),
                        execution: None,
                        max_tokens: None,
                        temperature: None,
                        top_p: None,
                        variant: None,
                    }))
                } else {
                    Ok(None)
                }
            })
            .with_get_last_model(|_session_id| async move { Ok(Some("provider-x:model-y".into())) })
            .with_create_subsession({
                let create_calls = create_calls.clone();
                move |agent, title, model, disabled_tools| {
                    let create_calls = create_calls.clone();
                    async move {
                        create_calls
                            .lock()
                            .await
                            .push((agent, title, model, disabled_tools));
                        Ok("task_build_123".to_string())
                    }
                }
            })
            .with_prompt_subsession({
                let prompt_calls = prompt_calls.clone();
                move |session_id, prompt| {
                    let prompt_calls = prompt_calls.clone();
                    async move {
                        prompt_calls.lock().await.push((session_id, prompt));
                        Ok("subagent output".to_string())
                    }
                }
            });

        let result = TaskFlowTool::new()
            .execute(
                serde_json::json!({
                    "operation": "create",
                    "agent": "build",
                    "description": "Investigate issue",
                    "prompt": "Please inspect runtime behavior"
                }),
                ctx,
            )
            .await
            .expect("create should succeed");

        assert_eq!(result.metadata["operation"], serde_json::json!("create"));
        assert_eq!(result.metadata["delegated"], serde_json::json!(true));
        assert_eq!(
            result.metadata["sessionId"],
            serde_json::json!("task_build_123")
        );
        assert!(result.metadata["agentTaskId"]
            .as_str()
            .is_some_and(|value| value.starts_with('a')));
        assert_eq!(result.metadata["task"]["agent"], serde_json::json!("build"));
        assert_eq!(
            result.metadata["task"]["status"],
            serde_json::json!("completed")
        );
        assert!(result.output.contains("agent_task_id:"));
        assert!(result.output.contains("session_id: task_build_123"));

        let create_calls = create_calls.lock().await.clone();
        assert_eq!(create_calls.len(), 1);
        assert_eq!(create_calls[0].0, "build");

        let prompt_calls = prompt_calls.lock().await.clone();
        assert_eq!(prompt_calls.len(), 1);
        assert_eq!(prompt_calls[0].0, "task_build_123");
    }

    #[tokio::test]
    async fn execute_create_syncs_projected_todo_when_requested() {
        let create_calls = Arc::new(Mutex::new(Vec::<(
            String,
            Option<String>,
            Option<String>,
            Vec<String>,
        )>::new()));
        let prompt_calls = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
        let updated_todos = Arc::new(Mutex::new(Vec::<crate::TodoItemData>::new()));

        let ctx = ToolContext::new("session-1".into(), "message-1".into(), ".".into())
            .with_get_agent_info(|name| async move {
                if name == "build" {
                    Ok(Some(crate::TaskAgentInfo {
                        name: "build".to_string(),
                        model: None,
                        can_use_task: true,
                        steps: Some(4),
                        execution: None,
                        max_tokens: None,
                        temperature: None,
                        top_p: None,
                        variant: None,
                    }))
                } else {
                    Ok(None)
                }
            })
            .with_get_last_model(|_session_id| async move { Ok(Some("provider-x:model-y".into())) })
            .with_create_subsession({
                let create_calls = create_calls.clone();
                move |agent, title, model, disabled_tools| {
                    let create_calls = create_calls.clone();
                    async move {
                        create_calls
                            .lock()
                            .await
                            .push((agent, title, model, disabled_tools));
                        Ok("task_build_234".to_string())
                    }
                }
            })
            .with_prompt_subsession({
                let prompt_calls = prompt_calls.clone();
                move |session_id, prompt| {
                    let prompt_calls = prompt_calls.clone();
                    async move {
                        prompt_calls.lock().await.push((session_id, prompt));
                        Ok("subagent output".to_string())
                    }
                }
            })
            .with_todo_get(|_session_id| async move {
                Ok(vec![crate::TodoItemData {
                    content: "existing item".to_string(),
                    status: "pending".to_string(),
                    priority: "high".to_string(),
                }])
            })
            .with_todo_update({
                let updated_todos = updated_todos.clone();
                move |_session_id, todos| {
                    let updated_todos = updated_todos.clone();
                    async move {
                        *updated_todos.lock().await = todos;
                        Ok(())
                    }
                }
            });

        let result = TaskFlowTool::new()
            .execute(
                serde_json::json!({
                    "operation": "create",
                    "agent": "build",
                    "description": "Investigate issue",
                    "prompt": "Please inspect runtime behavior",
                    "sync_todo": true,
                    "todo_item": {
                        "content": "Track delegated investigation"
                    }
                }),
                ctx,
            )
            .await
            .expect("create should succeed");

        assert_eq!(result.metadata["todoSynced"], serde_json::json!(true));

        let todos = updated_todos.lock().await.clone();
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].content, "existing item");
        assert_eq!(todos[1].content, "Track delegated investigation");
        assert_eq!(todos[1].status, "completed");
        assert_eq!(todos[1].priority, "medium");

        let create_calls = create_calls.lock().await.clone();
        assert_eq!(create_calls.len(), 1);

        let prompt_calls = prompt_calls.lock().await.clone();
        assert_eq!(prompt_calls.len(), 1);
    }

    #[tokio::test]
    async fn execute_create_without_todo_callbacks_reports_unsynced() {
        let ctx = ToolContext::new("session-1".into(), "message-1".into(), ".".into())
            .with_get_agent_info(|name| async move {
                if name == "build" {
                    Ok(Some(crate::TaskAgentInfo {
                        name: "build".to_string(),
                        model: None,
                        can_use_task: true,
                        steps: Some(4),
                        execution: None,
                        max_tokens: None,
                        temperature: None,
                        top_p: None,
                        variant: None,
                    }))
                } else {
                    Ok(None)
                }
            })
            .with_get_last_model(|_session_id| async move { Ok(Some("provider-x:model-y".into())) })
            .with_create_subsession(|_agent, _title, _model, _disabled_tools| async move {
                Ok("task_build_345".to_string())
            })
            .with_prompt_subsession(|_session_id, _prompt| async move {
                Ok("subagent output".to_string())
            });

        let result = TaskFlowTool::new()
            .execute(
                serde_json::json!({
                    "operation": "create",
                    "agent": "build",
                    "prompt": "Please inspect runtime behavior",
                    "sync_todo": true
                }),
                ctx,
            )
            .await
            .expect("create should succeed without todo callbacks");

        assert_eq!(result.metadata["todoSynced"], serde_json::json!(false));
    }

    #[tokio::test]
    async fn execute_resume_does_not_sync_todo_projection() {
        let todo_updates = Arc::new(AtomicBool::new(false));
        let prompted = Arc::new(Mutex::new(Vec::<(String, String)>::new()));

        let ctx = ToolContext::new("session-1".into(), "message-1".into(), ".".into())
            .with_get_agent_info(|name| async move {
                if name == "build" {
                    Ok(Some(crate::TaskAgentInfo {
                        name: "build".to_string(),
                        model: None,
                        can_use_task: true,
                        steps: Some(4),
                        execution: None,
                        max_tokens: None,
                        temperature: None,
                        top_p: None,
                        variant: None,
                    }))
                } else {
                    Ok(None)
                }
            })
            .with_get_last_model(|_session_id| async move { Ok(Some("provider-x:model-y".into())) })
            .with_create_subsession(|_agent, _title, _model, _disabled_tools| async move {
                Ok("should_not_be_used".to_string())
            })
            .with_prompt_subsession({
                let prompted = prompted.clone();
                move |session_id, prompt| {
                    let prompted = prompted.clone();
                    async move {
                        prompted.lock().await.push((session_id, prompt));
                        Ok("continued output".to_string())
                    }
                }
            })
            .with_todo_get(|_session_id| async move { Ok(Vec::new()) })
            .with_todo_update({
                let todo_updates = todo_updates.clone();
                move |_session_id, _todos| {
                    let todo_updates = todo_updates.clone();
                    async move {
                        todo_updates.store(true, Ordering::SeqCst);
                        Ok(())
                    }
                }
            });

        let result = TaskFlowTool::new()
            .execute(
                serde_json::json!({
                    "operation": "resume",
                    "task_id": "task_existing_99",
                    "agent": "build",
                    "prompt": "Continue where you left off",
                    "sync_todo": true
                }),
                ctx,
            )
            .await
            .expect("resume should succeed");

        assert_eq!(result.metadata["todoSynced"], serde_json::json!(false));
        assert!(!todo_updates.load(Ordering::SeqCst));

        let prompted = prompted.lock().await.clone();
        assert_eq!(prompted.len(), 1);
        assert_eq!(prompted[0].0, "task_existing_99");
    }

    #[tokio::test]
    async fn execute_resume_reuses_session_id_and_returns_new_agent_task_id() {
        let created = Arc::new(Mutex::new(false));
        let prompted = Arc::new(Mutex::new(Vec::<(String, String)>::new()));

        let ctx = ToolContext::new("session-1".into(), "message-1".into(), ".".into())
            .with_get_agent_info(|name| async move {
                if name == "build" {
                    Ok(Some(crate::TaskAgentInfo {
                        name: "build".to_string(),
                        model: None,
                        can_use_task: true,
                        steps: Some(4),
                        execution: None,
                        max_tokens: None,
                        temperature: None,
                        top_p: None,
                        variant: None,
                    }))
                } else {
                    Ok(None)
                }
            })
            .with_get_last_model(|_session_id| async move { Ok(Some("provider-x:model-y".into())) })
            .with_create_subsession({
                let created = created.clone();
                move |_agent, _title, _model, _disabled_tools| {
                    let created = created.clone();
                    async move {
                        *created.lock().await = true;
                        Ok("should_not_be_used".to_string())
                    }
                }
            })
            .with_prompt_subsession({
                let prompted = prompted.clone();
                move |session_id, prompt| {
                    let prompted = prompted.clone();
                    async move {
                        prompted.lock().await.push((session_id, prompt));
                        Ok("continued output".to_string())
                    }
                }
            });

        let result = TaskFlowTool::new()
            .execute(
                serde_json::json!({
                    "operation": "resume",
                    "task_id": "task_existing_42",
                    "agent": "build",
                    "prompt": "Continue where you left off"
                }),
                ctx,
            )
            .await
            .expect("resume should succeed");

        assert!(!(*created.lock().await));
        assert_eq!(result.metadata["operation"], serde_json::json!("resume"));
        assert_eq!(
            result.metadata["sessionId"],
            serde_json::json!("task_existing_42")
        );
        assert!(result.metadata["agentTaskId"]
            .as_str()
            .is_some_and(|value| value.starts_with('a')));
        assert_eq!(
            result.metadata["task"]["status"],
            serde_json::json!("completed")
        );

        let prompted = prompted.lock().await.clone();
        assert_eq!(prompted.len(), 1);
        assert_eq!(prompted[0].0, "task_existing_42");
        assert_eq!(prompted[0].1, "Continue where you left off");
    }
}
