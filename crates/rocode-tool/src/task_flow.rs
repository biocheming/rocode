use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumString, IntoStaticStr};

use rocode_core::agent_task_registry::{global_task_registry, AgentTask, AgentTaskStatus};
use rocode_core::contracts::agent_tasks::bus_keys as agent_task_bus_keys;
use rocode_core::contracts::agent_tasks::AgentTaskStatusKind;
use rocode_core::contracts::output_blocks::keys as output_keys;
use rocode_core::contracts::task::metadata_keys as task_metadata_keys;
use rocode_core::contracts::todo::{keys as todo_keys, TodoPriority, TodoStatus};
use rocode_core::contracts::tools::{arg_keys as tool_arg_keys, BuiltinToolName};
use rocode_core::contracts::wire::aliases as wire_aliases;

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

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString, IntoStaticStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
enum TaskFlowOperation {
    Create,
    Resume,
    Get,
    List,
    Cancel,
}

impl TaskFlowOperation {
    fn as_str(self) -> &'static str {
        self.into()
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct TaskFlowModelView {
    provider_id: String,
    model_id: String,
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
        BuiltinToolName::TaskFlow.as_str()
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                (tool_arg_keys::OPERATION): {
                    "type": "string",
                    "enum": ["create", "resume", "get", "list", "cancel"],
                    "description": "Task lifecycle operation"
                },
                "task_id": {
                    "type": "string",
                    "description": "Registry task id or delegated session id depending on operation"
                },
                (tool_arg_keys::AGENT): {
                    "type": "string",
                    "description": "Agent name to delegate to for create"
                },
                (tool_arg_keys::DESCRIPTION): {
                    "type": "string",
                    "description": "Short task label for UI and summaries"
                },
                (tool_arg_keys::PROMPT): {
                    "type": "string",
                    "description": "Delegated prompt body for create or resume"
                },
                (tool_arg_keys::COMMAND): {
                    "type": "string",
                    "description": "Optional source command or trigger name"
                },
                (tool_arg_keys::LOAD_SKILLS): {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional explicit skills to inject into delegated prompt"
                },
                (tool_arg_keys::RUN_IN_BACKGROUND): {
                    "type": "boolean",
                    "default": false,
                    "description": "Whether create should return immediately after dispatch"
                },
                (tool_arg_keys::SYNC_TODO): {
                    "type": "boolean",
                    "default": false,
                    "description": "Whether to project task lifecycle into the session todo board"
                },
                (tool_arg_keys::TODO_ITEM): {
                    "type": "object",
                    "properties": {
                        (todo_keys::CONTENT): { "type": "string" },
                        (todo_keys::STATUS): {
                            "type": "string",
                            "enum": [
                                TodoStatus::Pending.as_str(),
                                TodoStatus::InProgress.as_str(),
                                TodoStatus::Completed.as_str(),
                                TodoStatus::Cancelled.as_str(),
                            ]
                        },
                        (todo_keys::PRIORITY): { "type": "string" }
                    },
                    "required": [todo_keys::CONTENT]
                },
                (tool_arg_keys::STATUS_FILTER): {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": [
                            AgentTaskStatusKind::Pending.as_str(),
                            AgentTaskStatusKind::Running.as_str(),
                            AgentTaskStatusKind::Completed.as_str(),
                            AgentTaskStatusKind::Cancelled.as_str(),
                            AgentTaskStatusKind::Failed.as_str(),
                        ]
                    },
                    "description": "Optional status filter for list"
                },
                (tool_arg_keys::LIMIT): {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 100,
                    "default": 20,
                    "description": "Maximum number of tasks to return for list"
                }
            },
            "required": [tool_arg_keys::OPERATION]
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

        let mut permission = PermissionRequest::new(BuiltinToolName::TaskFlow.as_str())
            .with_metadata(
                tool_arg_keys::OPERATION,
                serde_json::json!(input.operation.as_str()),
            )
            .always_allow();
        if let Some(task_id) = input.task_id.as_ref() {
            permission =
                permission.with_metadata(agent_task_bus_keys::TASK_ID, serde_json::json!(task_id));
        }
        if let Some(agent) = input.agent.as_ref() {
            permission = permission.with_metadata(tool_arg_keys::AGENT, serde_json::json!(agent));
        }
        if let Some(status_filter) = input.status_filter.as_ref() {
            permission = permission.with_metadata(
                tool_arg_keys::STATUS_FILTER,
                serde_json::json!(status_filter),
            );
        }
        permission = permission
            .with_metadata(tool_arg_keys::LIMIT, serde_json::json!(input.limit))
            .with_metadata(tool_arg_keys::SYNC_TODO, serde_json::json!(input.sync_todo));
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

    let mut task_args = serde_json::Map::new();
    task_args.insert(
        tool_arg_keys::SUBAGENT_TYPE.to_string(),
        serde_json::json!(agent),
    );
    task_args.insert(tool_arg_keys::PROMPT.to_string(), serde_json::json!(prompt));
    if let Some(description) = input
        .description
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        task_args.insert(
            tool_arg_keys::DESCRIPTION.to_string(),
            serde_json::json!(description),
        );
    }
    if let Some(task_id) = input
        .task_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        task_args.insert(
            agent_task_bus_keys::TASK_ID.to_string(),
            serde_json::json!(task_id),
        );
    }
    if let Some(command) = input
        .command
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        task_args.insert(
            tool_arg_keys::COMMAND.to_string(),
            serde_json::json!(command),
        );
    }
    if let Some(load_skills) = input.load_skills.as_ref() {
        task_args.insert(
            tool_arg_keys::LOAD_SKILLS.to_string(),
            serde_json::json!(load_skills),
        );
    }
    if input.run_in_background {
        task_args.insert(
            tool_arg_keys::RUN_IN_BACKGROUND.to_string(),
            serde_json::json!(true),
        );
    }

    let delegated = TaskTool::new()
        .execute(serde_json::Value::Object(task_args), ctx)
        .await?;

    let agent_task_id =
        delegated_metadata_string(&delegated.metadata, tool_arg_keys::AGENT_TASK_ID)?;
    let session_id =
        delegated_metadata_string(&delegated.metadata, wire_aliases::SESSION_ID_CAMEL)?;
    let has_text_output = delegated
        .metadata
        .get(task_metadata_keys::HAS_TEXT_OUTPUT)
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
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

    let mut metadata = Metadata::new();
    metadata.insert(
        tool_arg_keys::OPERATION.to_string(),
        serde_json::json!(operation.as_str()),
    );
    metadata.insert(
        tool_arg_keys::DELEGATED.to_string(),
        serde_json::json!(true),
    );
    metadata.insert(
        tool_arg_keys::AGENT_TASK_ID.to_string(),
        serde_json::json!(agent_task_id),
    );
    metadata.insert(
        wire_aliases::SESSION_ID_CAMEL.to_string(),
        serde_json::json!(session_id),
    );
    metadata.insert(
        task_metadata_keys::HAS_TEXT_OUTPUT.to_string(),
        serde_json::json!(has_text_output),
    );
    metadata.insert("todoSynced".to_string(), serde_json::json!(todo_synced));
    metadata.insert(
        tool_arg_keys::TASK.to_string(),
        serde_json::to_value(&view).unwrap(),
    );
    if let Some(model) = delegated
        .metadata
        .get(task_metadata_keys::MODEL)
        .and_then(parse_model_view)
    {
        metadata.insert(
            task_metadata_keys::MODEL.to_string(),
            serde_json::to_value(model).unwrap(),
        );
    }
    if let Some(loaded_skills) = delegated.metadata.get(tool_arg_keys::LOADED_SKILLS) {
        metadata.insert(
            tool_arg_keys::LOADED_SKILLS.to_string(),
            loaded_skills.clone(),
        );
    }
    if let Some(loaded_skill_count) = delegated
        .metadata
        .get(task_metadata_keys::LOADED_SKILL_COUNT)
    {
        metadata.insert(
            task_metadata_keys::LOADED_SKILL_COUNT.to_string(),
            loaded_skill_count.clone(),
        );
    }
    metadata.insert(
        output_keys::DISPLAY_SUMMARY.to_string(),
        serde_json::json!(format!(
            "Delegated {} task {} via session {}",
            operation.as_str(),
            metadata[tool_arg_keys::AGENT_TASK_ID]
                .as_str()
                .unwrap_or_default(),
            metadata[wire_aliases::SESSION_ID_CAMEL]
                .as_str()
                .unwrap_or_default()
        )),
    );

    Ok(ToolResult {
        title: format!(
            "{} Task {}",
            title_case(operation.as_str()),
            metadata[tool_arg_keys::AGENT_TASK_ID]
                .as_str()
                .unwrap_or_default()
        ),
        output: render_delegated_task_output(
            operation.as_str(),
            metadata[tool_arg_keys::AGENT_TASK_ID]
                .as_str()
                .unwrap_or_default(),
            metadata[wire_aliases::SESSION_ID_CAMEL]
                .as_str()
                .unwrap_or_default(),
            &view.status,
            &delegated.output,
        ),
        metadata,
        truncated: delegated.truncated,
    })
}

fn delegated_metadata_string(metadata: &Metadata, key: &str) -> Result<String, ToolError> {
    metadata
        .get(key)
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| {
            ToolError::ExecutionError(format!("delegated task metadata missing `{}`", key))
        })
}

fn parse_model_view(value: &serde_json::Value) -> Option<TaskFlowModelView> {
    Some(TaskFlowModelView {
        provider_id: value
            .get(task_metadata_keys::MODEL_PROVIDER_ID_CAMEL)?
            .as_str()?
            .to_string(),
        model_id: value
            .get(task_metadata_keys::MODEL_ID_CAMEL)?
            .as_str()?
            .to_string(),
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
            serde_json::json!({
                todo_keys::TODOS: todos.iter().map(todo_item_to_json).collect::<Vec<_>>()
            }),
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
        .as_str()
        .to_string();
    let priority = input
        .todo_item
        .as_ref()
        .and_then(|item| item.priority.as_deref())
        .map(normalize_projection_todo_priority)
        .unwrap_or(TodoPriority::Medium)
        .as_str()
        .to_string();

    TodoItemData {
        content,
        status,
        priority,
    }
}

fn todo_item_to_json(item: &TodoItemData) -> serde_json::Value {
    serde_json::json!({
        (todo_keys::CONTENT): item.content,
        (todo_keys::STATUS): item.status,
        (todo_keys::PRIORITY): item.priority,
    })
}

fn task_status_to_todo_status(status: &str) -> TodoStatus {
    match AgentTaskStatusKind::parse(status).unwrap_or(AgentTaskStatusKind::Pending) {
        AgentTaskStatusKind::Running => TodoStatus::InProgress,
        AgentTaskStatusKind::Completed => TodoStatus::Completed,
        AgentTaskStatusKind::Pending
        | AgentTaskStatusKind::Cancelled
        | AgentTaskStatusKind::Failed => TodoStatus::Pending,
    }
}

fn normalize_projection_todo_status(status: &str) -> TodoStatus {
    TodoStatus::parse(status).unwrap_or(TodoStatus::Pending)
}

fn normalize_projection_todo_priority(priority: &str) -> TodoPriority {
    TodoPriority::parse(priority).unwrap_or(TodoPriority::Medium)
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
    let mut metadata = Metadata::new();
    metadata.insert(
        tool_arg_keys::OPERATION.to_string(),
        serde_json::json!("get"),
    );
    metadata.insert("task".to_string(), serde_json::to_value(&view).unwrap());
    metadata.insert(
        output_keys::DISPLAY_SUMMARY.to_string(),
        serde_json::json!(format!("Loaded task {} ({})", view.task_id, view.status)),
    );

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

    let mut metadata = Metadata::new();
    metadata.insert(
        tool_arg_keys::OPERATION.to_string(),
        serde_json::json!("list"),
    );
    metadata.insert("count".to_string(), serde_json::json!(views.len()));
    metadata.insert("truncated".to_string(), serde_json::json!(truncated));
    metadata.insert("tasks".to_string(), serde_json::to_value(&views).unwrap());
    metadata.insert(
        output_keys::DISPLAY_SUMMARY.to_string(),
        serde_json::json!(format!(
            "Listed {} task(s){}",
            views.len(),
            if truncated { " (truncated)" } else { "" }
        )),
    );

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

    let mut metadata = Metadata::new();
    metadata.insert(
        tool_arg_keys::OPERATION.to_string(),
        serde_json::json!("cancel"),
    );
    metadata.insert("task".to_string(), serde_json::to_value(&view).unwrap());
    metadata.insert(
        output_keys::DISPLAY_SUMMARY.to_string(),
        serde_json::json!(format!("Cancelled task {}", view.task_id)),
    );

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
    let task_status = task.status.kind();
    status_filter.iter().any(|value| {
        AgentTaskStatusKind::parse(value).is_some_and(|candidate| candidate == task_status)
    })
}

fn task_to_view(task: &AgentTask, include_detail: bool) -> TaskFlowTaskView {
    let (status, step, steps, error) = status_fields(&task.status);
    TaskFlowTaskView {
        task_id: task.id.clone(),
        agent: task.agent_name.clone(),
        status: status.as_str().to_string(),
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
) -> (
    AgentTaskStatusKind,
    Option<u32>,
    Option<u32>,
    Option<String>,
) {
    match status {
        AgentTaskStatus::Pending => (status.kind(), None, None, None),
        AgentTaskStatus::Running { step } => (status.kind(), Some(*step), None, None),
        AgentTaskStatus::Completed { steps } => (status.kind(), None, Some(*steps), None),
        AgentTaskStatus::Cancelled => (status.kind(), None, None, None),
        AgentTaskStatus::Failed { error } => (status.kind(), None, None, Some(error.clone())),
    }
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
    if TodoStatus::parse(status).is_some() {
        return Ok(());
    }
    Err(ToolError::InvalidArguments(format!(
        "todo_item.status must be one of: {}, {}, {}, {}",
        TodoStatus::Pending.as_str(),
        TodoStatus::InProgress.as_str(),
        TodoStatus::Completed.as_str(),
        TodoStatus::Cancelled.as_str(),
    )))
}

fn validate_task_status_filter(status: &str) -> Result<(), ToolError> {
    if AgentTaskStatusKind::parse(status).is_some() {
        Ok(())
    } else {
        Err(ToolError::InvalidArguments(format!(
            "status_filter values must be one of: {}",
            AgentTaskStatusKind::allowed_values().join(", ")
        )))
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
            status_filter: Some(vec![
                AgentTaskStatusKind::Running.as_str().to_string(),
                AgentTaskStatusKind::Completed.as_str().to_string(),
            ]),
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
                status: Some(TodoStatus::Pending.as_str().to_string()),
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
        assert_eq!(view.status, AgentTaskStatusKind::Running.as_str());
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

        assert_eq!(view.status, AgentTaskStatusKind::Failed.as_str());
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
            Some(&[
                AgentTaskStatusKind::Running.as_str().to_string(),
                AgentTaskStatusKind::Completed.as_str().to_string(),
            ]),
            1,
        );

        assert_eq!(views.len(), 1);
        assert!(truncated);
        let kind = AgentTaskStatusKind::parse(&views[0].status)
            .expect("filtered view status should parse");
        assert!(matches!(
            kind,
            AgentTaskStatusKind::Running | AgentTaskStatusKind::Completed
        ));
    }

    #[test]
    fn render_task_detail_output_includes_recent_output() {
        let view = task_to_view(&sample_task(AgentTaskStatus::Completed { steps: 2 }), true);
        let output = render_task_detail_output(&view);

        assert!(output.contains("Task a42"));
        assert!(output.contains(&format!(
            "Status: {} (2 steps)",
            AgentTaskStatusKind::Completed.as_str()
        )));
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
            serde_json::json!(AgentTaskStatusKind::Cancelled.as_str())
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
            .find(|req| req.permission == BuiltinToolName::TaskFlow.as_str())
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
            serde_json::json!(AgentTaskStatusKind::Completed.as_str())
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
                    status: TodoStatus::Pending.as_str().to_string(),
                    priority: TodoPriority::High.as_str().to_string(),
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
        assert_eq!(todos[1].status, TodoStatus::Completed.as_str());
        assert_eq!(todos[1].priority, TodoPriority::Medium.as_str());

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
            serde_json::json!(AgentTaskStatusKind::Completed.as_str())
        );

        let prompted = prompted.lock().await.clone();
        assert_eq!(prompted.len(), 1);
        assert_eq!(prompted[0].0, "task_existing_42");
        assert_eq!(prompted[0].1, "Continue where you left off");
    }
}
