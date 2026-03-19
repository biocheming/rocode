pub mod compaction_helpers;
mod file_parts;
pub(crate) mod hooks;
mod message_building;
pub mod shell;
pub mod subtask;
mod tool_calls;
mod tool_execution;
pub mod tools_and_output;

pub use compaction_helpers::{should_compact, trigger_compaction};
pub(crate) use hooks::{
    apply_chat_message_hook_outputs, apply_chat_messages_hook_outputs, session_message_hook_payload,
};
#[cfg(test)]
pub(crate) use shell::resolve_shell_invocation;
pub use shell::{resolve_command_template, shell_exec, CommandInput, ShellInput};
pub use subtask::{tool_definitions_from_schemas, SubtaskExecutor, ToolSchema};
pub use tools_and_output::{
    compose_session_title_source, create_structured_output_tool, extract_structured_output,
    generate_session_title, generate_session_title_for_session, generate_session_title_llm,
    insert_reminders, max_steps_for_agent, merge_tool_definitions, prioritize_tool_definitions,
    resolve_tools, resolve_tools_with_mcp, resolve_tools_with_mcp_registry,
    structured_output_system_prompt, was_plan_agent, ResolvedTool, StructuredOutputConfig,
};

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use rocode_content::output_blocks::{
    MessageBlock, OutputBlock, ReasoningBlock, Role as OutputMessageRole, ToolBlock,
};
use rocode_orchestrator::runtime::events::{
    CancelToken as RuntimeCancelToken, FinishReason as RuntimeFinishReason,
    LoopError as RuntimeLoopError, LoopEvent, StepBoundary, ToolCallReady as RuntimeToolCallReady,
    ToolResult as RuntimeToolResult,
};
use rocode_orchestrator::runtime::policy::{LoopPolicy, ToolDedupScope};
use rocode_orchestrator::runtime::run_loop;
use rocode_orchestrator::runtime::traits::{LoopSink, ToolDispatcher};
use rocode_orchestrator::runtime::{SimpleModelCaller, SimpleModelCallerConfig};
use rocode_orchestrator::{session_runtime_request_defaults, CompiledExecutionRequest};
use rocode_plugin::{HookContext, HookEvent};
use rocode_provider::transform::{apply_caching, ProviderType};
use rocode_provider::{Provider, ToolDefinition};
use serde::{Deserialize, Serialize};

use crate::compaction::{run_compaction, CompactionResult};
use crate::message_v2::ModelRef as V2ModelRef;
use crate::{PartType, Role, Session, SessionMessage, SessionStateManager};

const MAX_STEPS: u32 = 100;
const STREAM_UPDATE_INTERVAL_MS: u64 = 120;

#[derive(Debug, Serialize)]
struct ToolLifecycleEvent<'a> {
    #[serde(rename = "type")]
    event_type: &'static str,
    #[serde(rename = "sessionID")]
    session_id: &'a str,
    #[serde(rename = "toolCallId")]
    tool_call_id: &'a str,
    phase: &'static str,
    #[serde(rename = "toolName")]
    tool_name: &'a str,
}

#[derive(Debug, Serialize)]
struct PendingSubtaskMetadata<'a> {
    id: &'a str,
    agent: &'a str,
    prompt: &'a str,
    description: &'a str,
}

#[derive(Debug, Serialize)]
struct HookModelPayload {
    id: String,
    name: String,
    provider: String,
}

/// Returns `true` when the finish reason indicates the conversation turn is
/// complete (i.e. not a tool-use continuation or unknown state).
fn is_terminal_finish(reason: Option<&str>) -> bool {
    !matches!(
        reason,
        None | Some("tool-calls") | Some("tool_calls") | Some("unknown")
    )
}

#[derive(Debug, Clone)]
pub struct PromptInput {
    pub session_id: String,
    pub message_id: Option<String>,
    pub model: Option<ModelRef>,
    pub agent: Option<String>,
    pub no_reply: bool,
    pub system: Option<String>,
    pub variant: Option<String>,
    pub parts: Vec<PartInput>,
    pub tools: Option<HashMap<String, bool>>,
}

#[derive(Debug, Clone)]
pub struct ModelRef {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PartInput {
    Text {
        text: String,
    },
    File {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime: Option<String>,
    },
    Agent {
        name: String,
    },
    Subtask {
        prompt: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        agent: String,
    },
}

impl TryFrom<serde_json::Value> for PartInput {
    type Error = String;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value).map_err(|e| format!("Invalid PartInput: {}", e))
    }
}

impl PartInput {
    /// Parse a JSON array of parts into a Vec<PartInput>, skipping invalid entries.
    pub fn parse_array(value: &serde_json::Value) -> Vec<PartInput> {
        match value.as_array() {
            Some(arr) => arr
                .iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect(),
            None => Vec::new(),
        }
    }
}

struct PromptState {
    cancel_token: CancellationToken,
}

#[derive(Debug, Clone)]
struct PendingSubtask {
    part_index: usize,
    subtask_id: String,
    agent: String,
    prompt: String,
    description: String,
}

#[derive(Debug, Clone)]
struct StreamToolState {
    name: String,
    raw_input: String,
    input: serde_json::Value,
    status: crate::ToolCallStatus,
    state: crate::ToolState,
    emitted_output_start: bool,
    emitted_output_detail: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub(super) struct PersistedSubsession {
    agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    directory: Option<String>,
    #[serde(default)]
    disabled_tools: Vec<String>,
    #[serde(default)]
    history: Vec<PersistedSubsessionTurn>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct PersistedSubsessionTurn {
    prompt: String,
    output: String,
}

/// LLM parameters derived from agent configuration.
#[derive(Debug, Clone, Default)]
pub struct AgentParams {
    pub max_tokens: Option<u64>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
}

pub type SessionUpdateHook = Arc<dyn Fn(&Session) + Send + Sync + 'static>;
pub type EventBroadcastHook = Arc<dyn Fn(serde_json::Value) + Send + Sync + 'static>;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputBlockEvent {
    pub session_id: String,
    pub block: OutputBlock,
    pub id: Option<String>,
}
pub type OutputBlockHook = Arc<
    dyn Fn(OutputBlockEvent) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static,
>;
pub type AgentLookup =
    Arc<dyn Fn(&str) -> Option<rocode_tool::TaskAgentInfo> + Send + Sync + 'static>;
pub type PublishBusHook = Arc<
    dyn Fn(String, serde_json::Value) -> Pin<Box<dyn Future<Output = ()> + Send>>
        + Send
        + Sync
        + 'static,
>;
pub type AskQuestionHook = Arc<
    dyn Fn(
            String,
            Vec<rocode_tool::QuestionDef>,
        )
            -> Pin<Box<dyn Future<Output = Result<Vec<Vec<String>>, rocode_tool::ToolError>> + Send>>
        + Send
        + Sync
        + 'static,
>;
pub type AskPermissionHook = Arc<
    dyn Fn(
            String,
            rocode_tool::PermissionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<(), rocode_tool::ToolError>> + Send>>
        + Send
        + Sync
        + 'static,
>;

#[derive(Clone, Default)]
pub struct PromptHooks {
    pub update_hook: Option<SessionUpdateHook>,
    pub event_broadcast: Option<EventBroadcastHook>,
    pub output_block_hook: Option<OutputBlockHook>,
    pub agent_lookup: Option<AgentLookup>,
    pub ask_question_hook: Option<AskQuestionHook>,
    pub ask_permission_hook: Option<AskPermissionHook>,
    pub publish_bus_hook: Option<PublishBusHook>,
}

#[derive(Clone)]
pub struct PromptRequestContext {
    pub provider: Arc<dyn Provider>,
    pub system_prompt: Option<String>,
    pub tools: Vec<ToolDefinition>,
    pub compiled_request: CompiledExecutionRequest,
    pub hooks: PromptHooks,
}

pub struct SessionPrompt {
    state: Arc<Mutex<HashMap<String, PromptState>>>,
    session_state: Arc<RwLock<SessionStateManager>>,
    mcp_clients: Option<Arc<rocode_mcp::McpClientRegistry>>,
    lsp_registry: Option<Arc<rocode_lsp::LspClientRegistry>>,
    tool_runtime_config: rocode_tool::ToolRuntimeConfig,
}

type StreamToolResultEntry = (
    String,
    String,
    bool,
    Option<String>,
    Option<HashMap<String, serde_json::Value>>,
    Option<Vec<serde_json::Value>>,
);

#[derive(Default)]
struct SessionStepShared {
    assistant_message_id: Option<String>,
}

#[derive(Clone)]
struct SessionStepCancelToken {
    user_cancel: CancellationToken,
    step_complete: Arc<AtomicBool>,
}

impl RuntimeCancelToken for SessionStepCancelToken {
    fn is_cancelled(&self) -> bool {
        self.user_cancel.is_cancelled() || self.step_complete.load(Ordering::Relaxed)
    }
}

#[derive(Clone)]
struct PromptLoopContext {
    provider: Arc<dyn Provider>,
    model_id: String,
    provider_id: String,
    agent_name: Option<String>,
    system_prompt: Option<String>,
    tools: Vec<ToolDefinition>,
    compiled_request: CompiledExecutionRequest,
    hooks: PromptHooks,
}

#[derive(Clone)]
struct RuntimeStepContext {
    provider: Arc<dyn Provider>,
    model_id: String,
    provider_id: String,
    agent_name: Option<String>,
    compiled_request: CompiledExecutionRequest,
    hooks: PromptHooks,
}

struct RuntimeStepInput {
    session_id: String,
    assistant_index: usize,
    chat_messages: Vec<rocode_provider::Message>,
    tool_registry: Arc<rocode_tool::ToolRegistry>,
    step_ctx: RuntimeStepContext,
}

struct SessionToolExecutor {
    tool_registry: Arc<rocode_tool::ToolRegistry>,
    tool_ctx_builder: Arc<dyn Fn() -> rocode_tool::ToolContext + Send + Sync>,
    allowed_tools: Option<Arc<HashSet<String>>>,
}

#[async_trait::async_trait]
impl rocode_orchestrator::ToolExecutor for SessionToolExecutor {
    async fn execute(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
        _exec_ctx: &rocode_orchestrator::ExecutionContext,
    ) -> Result<rocode_orchestrator::ToolOutput, rocode_orchestrator::ToolExecError> {
        if let Some(allowed_tools) = self.allowed_tools.as_ref() {
            if !allowed_tools.contains(tool_name) {
                return Err(rocode_orchestrator::ToolExecError::PermissionDenied(
                    format!("Tool `{}` is not allowed in this session", tool_name),
                ));
            }
        }
        let ctx = (self.tool_ctx_builder)();
        let result = self
            .tool_registry
            .execute(tool_name, arguments, ctx)
            .await
            .map_err(|e| match e {
                rocode_tool::ToolError::InvalidArguments(msg) => {
                    rocode_orchestrator::ToolExecError::InvalidArguments(msg)
                }
                rocode_tool::ToolError::PermissionDenied(msg) => {
                    rocode_orchestrator::ToolExecError::PermissionDenied(msg)
                }
                rocode_tool::ToolError::Cancelled => {
                    rocode_orchestrator::ToolExecError::ExecutionError("cancelled".to_string())
                }
                other => rocode_orchestrator::ToolExecError::ExecutionError(other.to_string()),
            })?;
        Ok(rocode_orchestrator::ToolOutput {
            output: result.output,
            is_error: false,
            title: if result.title.is_empty() {
                None
            } else {
                Some(result.title)
            },
            metadata: if result.metadata.is_empty() {
                None
            } else {
                Some(serde_json::to_value(result.metadata).unwrap_or(serde_json::Value::Null))
            },
        })
    }

    async fn list_ids(&self) -> Vec<String> {
        let mut ids = self.tool_registry.list_ids().await;
        if let Some(allowed_tools) = self.allowed_tools.as_ref() {
            ids.retain(|id| allowed_tools.contains(id));
        }
        ids
    }

    async fn list_definitions(
        &self,
        _exec_ctx: &rocode_orchestrator::ExecutionContext,
    ) -> Vec<ToolDefinition> {
        let mut tools: Vec<ToolDefinition> = self
            .tool_registry
            .list_schemas()
            .await
            .into_iter()
            .map(|s| ToolDefinition {
                name: s.name,
                description: Some(s.description),
                parameters: s.parameters,
            })
            .collect();
        if let Some(allowed_tools) = self.allowed_tools.as_ref() {
            tools.retain(|tool| allowed_tools.contains(&tool.name));
        }
        prioritize_tool_definitions(&mut tools);
        tools
    }
}

struct SessionStepToolDispatcher {
    session_id: String,
    directory: String,
    agent_name: String,
    abort_token: CancellationToken,
    tool_registry: Arc<rocode_tool::ToolRegistry>,
    provider: Arc<dyn Provider>,
    provider_id: String,
    model_id: String,
    resolved_tools: Vec<ToolDefinition>,
    allowed_tools: Option<Arc<HashSet<String>>>,
    shared: Arc<Mutex<SessionStepShared>>,
    subsessions: Arc<Mutex<HashMap<String, PersistedSubsession>>>,
    agent_lookup: Option<AgentLookup>,
    ask_question_hook: Option<AskQuestionHook>,
    ask_permission_hook: Option<AskPermissionHook>,
    publish_bus_hook: Option<PublishBusHook>,
    tool_runtime_config: rocode_tool::ToolRuntimeConfig,
}

#[async_trait::async_trait]
impl ToolDispatcher for SessionStepToolDispatcher {
    async fn execute(&self, call: &RuntimeToolCallReady) -> RuntimeToolResult {
        let message_id = {
            let shared = self.shared.lock().await;
            shared
                .assistant_message_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        };

        let default_model = format!("{}:{}", self.provider_id, self.model_id);
        let session_id = self.session_id.clone();
        let directory = self.directory.clone();
        let agent_name = self.agent_name.clone();
        let abort_token = self.abort_token.clone();
        let subsessions = self.subsessions.clone();
        let provider = self.provider.clone();
        let tool_registry = self.tool_registry.clone();
        let agent_lookup = self.agent_lookup.clone();
        let ask_question_hook = self.ask_question_hook.clone();
        let ask_permission_hook = self.ask_permission_hook.clone();
        let publish_bus_hook = self.publish_bus_hook.clone();
        let call_id = call.id.clone();
        let tool_runtime_config = self.tool_runtime_config.clone();

        let tool_ctx_builder = Arc::new(move || {
            let mut base_ctx = rocode_tool::ToolContext::new(
                session_id.clone(),
                message_id.clone(),
                directory.clone(),
            )
            .with_agent(agent_name.clone())
            .with_tool_runtime_config(tool_runtime_config.clone())
            .with_abort(abort_token.clone());
            base_ctx.call_id = Some(call_id.clone());
            let ctx = SessionPrompt::with_persistent_subsession_callbacks(
                base_ctx,
                subsessions.clone(),
                provider.clone(),
                tool_registry.clone(),
                default_model.clone(),
                agent_lookup.clone(),
                ask_question_hook.clone(),
                ask_permission_hook.clone(),
            )
            .with_registry(tool_registry.clone());
            // Wire publish_bus so TaskTool agent_task events reach the server.
            if let Some(ref hook) = publish_bus_hook {
                let hook = hook.clone();
                ctx.with_publish_bus(move |event_type, properties| {
                    let hook = hook.clone();
                    async move { hook(event_type, properties).await }
                })
            } else {
                ctx
            }
        });

        let executor = Arc::new(SessionToolExecutor {
            tool_registry: self.tool_registry.clone(),
            tool_ctx_builder,
            allowed_tools: self.allowed_tools.clone(),
        });
        let tool_runner = rocode_orchestrator::ToolRunner::new(executor);
        let exec_ctx = rocode_orchestrator::ExecutionContext {
            session_id: self.session_id.clone(),
            workdir: self.directory.clone(),
            agent_name: self.agent_name.clone(),
            metadata: std::collections::HashMap::new(),
        };

        let input = rocode_orchestrator::tool_runner::ToolCallInput {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        };
        let output = tool_runner.execute_tool_call(input, &exec_ctx).await;

        RuntimeToolResult {
            tool_call_id: output.tool_call_id,
            tool_name: output.tool_name,
            output: output.content,
            is_error: output.is_error,
            title: output.title,
            metadata: output.metadata,
        }
    }

    async fn list_definitions(&self) -> Vec<ToolDefinition> {
        self.resolved_tools.clone()
    }
}

struct SessionStepRuntimeOutput {
    stream_tool_results: Vec<StreamToolResultEntry>,
    finish_reason: Option<String>,
    prompt_tokens: u64,
    completion_tokens: u64,
    reasoning_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    executed_local_tools_this_step: bool,
}

struct SessionStepSink<'a> {
    session: &'a mut Session,
    assistant_index: usize,
    update_hook: Option<&'a SessionUpdateHook>,
    event_broadcast: Option<&'a EventBroadcastHook>,
    output_block_hook: Option<&'a OutputBlockHook>,
    last_emit: Instant,
    tool_calls: HashMap<String, StreamToolState>,
    stream_tool_results: Vec<StreamToolResultEntry>,
    finish_reason: Option<String>,
    prompt_tokens: u64,
    completion_tokens: u64,
    reasoning_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    executed_local_tools_this_step: bool,
    step_complete: Arc<AtomicBool>,
    assistant_output_started: bool,
    reasoning_output_started: bool,
}

impl<'a> SessionStepSink<'a> {
    fn new(
        session: &'a mut Session,
        assistant_index: usize,
        update_hook: Option<&'a SessionUpdateHook>,
        event_broadcast: Option<&'a EventBroadcastHook>,
        output_block_hook: Option<&'a OutputBlockHook>,
        step_complete: Arc<AtomicBool>,
    ) -> Self {
        Self {
            session,
            assistant_index,
            update_hook,
            event_broadcast,
            output_block_hook,
            last_emit: Instant::now() - Duration::from_millis(STREAM_UPDATE_INTERVAL_MS),
            tool_calls: HashMap::new(),
            stream_tool_results: Vec::new(),
            finish_reason: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            reasoning_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            executed_local_tools_this_step: false,
            step_complete,
            assistant_output_started: false,
            reasoning_output_started: false,
        }
    }

    fn into_output(self) -> SessionStepRuntimeOutput {
        SessionStepRuntimeOutput {
            stream_tool_results: self.stream_tool_results,
            finish_reason: self.finish_reason,
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            reasoning_tokens: self.reasoning_tokens,
            cache_read_tokens: self.cache_read_tokens,
            cache_write_tokens: self.cache_write_tokens,
            executed_local_tools_this_step: self.executed_local_tools_this_step,
        }
    }

    fn assistant_message_id(&self) -> Option<String> {
        self.session
            .messages
            .get(self.assistant_index)
            .map(|message| message.id.clone())
    }

    async fn emit_output_block(&self, block: OutputBlock, id: Option<String>) {
        if let Some(output_block_hook) = self.output_block_hook {
            output_block_hook(OutputBlockEvent {
                session_id: self.session.id.clone(),
                block,
                id,
            })
            .await;
        }
    }

    async fn ensure_assistant_output_started(&mut self) {
        if self.assistant_output_started {
            return;
        }
        self.emit_output_block(
            OutputBlock::Message(MessageBlock::start(OutputMessageRole::Assistant)),
            self.assistant_message_id(),
        )
        .await;
        self.assistant_output_started = true;
    }

    async fn ensure_reasoning_output_started(&mut self) {
        self.ensure_assistant_output_started().await;
        if self.reasoning_output_started {
            return;
        }
        self.emit_output_block(
            OutputBlock::Reasoning(ReasoningBlock::start()),
            self.assistant_message_id(),
        )
        .await;
        self.reasoning_output_started = true;
    }

    async fn finish_output_blocks(&mut self) {
        let assistant_message_id = self.assistant_message_id();
        if self.reasoning_output_started {
            self.emit_output_block(
                OutputBlock::Reasoning(ReasoningBlock::end()),
                assistant_message_id.clone(),
            )
            .await;
            self.reasoning_output_started = false;
        }
        if self.assistant_output_started {
            self.emit_output_block(
                OutputBlock::Message(MessageBlock::end(OutputMessageRole::Assistant)),
                assistant_message_id,
            )
            .await;
            self.assistant_output_started = false;
        }
    }
}

#[async_trait::async_trait]
impl<'a> LoopSink for SessionStepSink<'a> {
    async fn on_event(&mut self, event: &LoopEvent) -> std::result::Result<(), RuntimeLoopError> {
        match event {
            LoopEvent::TextChunk(text) => {
                self.ensure_assistant_output_started().await;
                if let Some(assistant) = self.session.messages.get_mut(self.assistant_index) {
                    SessionPrompt::append_delta_part(assistant, false, text);
                }
                self.emit_output_block(
                    OutputBlock::Message(MessageBlock::delta(
                        OutputMessageRole::Assistant,
                        text.clone(),
                    )),
                    self.assistant_message_id(),
                )
                .await;
                self.session.touch();
                SessionPrompt::maybe_emit_session_update(
                    self.update_hook,
                    self.session,
                    &mut self.last_emit,
                    false,
                );
            }
            LoopEvent::ReasoningChunk { text, .. } => {
                self.ensure_reasoning_output_started().await;
                if let Some(assistant) = self.session.messages.get_mut(self.assistant_index) {
                    SessionPrompt::append_delta_part(assistant, true, text);
                }
                self.emit_output_block(
                    OutputBlock::Reasoning(ReasoningBlock::delta(text.clone())),
                    self.assistant_message_id(),
                )
                .await;
                self.session.touch();
                SessionPrompt::maybe_emit_session_update(
                    self.update_hook,
                    self.session,
                    &mut self.last_emit,
                    false,
                );
            }
            LoopEvent::ToolCallProgress {
                id,
                name,
                partial_input,
            } => {
                if let Some(next_name) = name {
                    if next_name.trim().is_empty() {
                        return Ok(());
                    }

                    // Broadcast canonical tool lifecycle event.
                    if let Some(broadcast) = &self.event_broadcast {
                        let event = serde_json::to_value(ToolLifecycleEvent {
                            event_type: "tool_call.lifecycle",
                            session_id: &self.session.id,
                            tool_call_id: &id,
                            phase: "start",
                            tool_name: next_name,
                        })
                        .unwrap_or(serde_json::Value::Null);
                        broadcast(event);
                    }

                    let (tool_input, tool_raw, tool_state, should_emit_start) = {
                        let entry =
                            self.tool_calls
                                .entry(id.clone())
                                .or_insert_with(|| StreamToolState {
                                    name: String::new(),
                                    raw_input: String::new(),
                                    input: serde_json::json!({}),
                                    status: crate::ToolCallStatus::Pending,
                                    state: crate::ToolState::Pending {
                                        input: serde_json::json!({}),
                                        raw: String::new(),
                                    },
                                    emitted_output_start: false,
                                    emitted_output_detail: None,
                                });
                        if entry.name.is_empty() {
                            entry.name = next_name.clone();
                        }
                        let should_emit_start = !entry.emitted_output_start;
                        if should_emit_start {
                            entry.emitted_output_start = true;
                        }
                        entry.status = crate::ToolCallStatus::Pending;
                        entry.state = crate::ToolState::Pending {
                            input: entry.input.clone(),
                            raw: entry.raw_input.clone(),
                        };
                        (
                            entry.input.clone(),
                            entry.raw_input.clone(),
                            entry.state.clone(),
                            should_emit_start,
                        )
                    };
                    if should_emit_start {
                        self.ensure_assistant_output_started().await;
                        self.emit_output_block(
                            OutputBlock::Tool(ToolBlock::start(next_name.clone())),
                            Some(id.clone()),
                        )
                        .await;
                    }
                    if let Some(assistant) = self.session.messages.get_mut(self.assistant_index) {
                        SessionPrompt::upsert_tool_call_part(
                            assistant,
                            id,
                            Some(next_name),
                            Some(tool_input),
                            Some(tool_raw),
                            Some(crate::ToolCallStatus::Pending),
                            Some(tool_state),
                        );
                    }
                }
                if !partial_input.is_empty() {
                    let (tool_input, tool_raw, tool_state, tool_name, detail) = {
                        let entry =
                            self.tool_calls
                                .entry(id.clone())
                                .or_insert_with(|| StreamToolState {
                                    name: String::new(),
                                    raw_input: String::new(),
                                    input: serde_json::json!({}),
                                    status: crate::ToolCallStatus::Pending,
                                    state: crate::ToolState::Pending {
                                        input: serde_json::json!({}),
                                        raw: String::new(),
                                    },
                                    emitted_output_start: false,
                                    emitted_output_detail: None,
                                });
                        entry.raw_input.push_str(partial_input);
                        if rocode_provider::is_parsable_json(&entry.raw_input) {
                            if let Ok(parsed) = serde_json::from_str(&entry.raw_input) {
                                entry.input = parsed;
                            }
                        }
                        entry.state = crate::ToolState::Pending {
                            input: entry.input.clone(),
                            raw: entry.raw_input.clone(),
                        };
                        let detail = tool_progress_detail(
                            &entry.input,
                            Some(entry.raw_input.as_str()),
                            &crate::ToolCallStatus::Pending,
                        );
                        let tool_name = if entry.name.trim().is_empty() {
                            id.clone()
                        } else {
                            entry.name.clone()
                        };
                        let should_emit_detail = detail.as_ref().is_some_and(|detail| {
                            entry.emitted_output_detail.as_ref() != Some(detail)
                        });
                        if should_emit_detail {
                            entry.emitted_output_detail = detail.clone();
                        }
                        (
                            entry.input.clone(),
                            entry.raw_input.clone(),
                            entry.state.clone(),
                            tool_name,
                            if should_emit_detail { detail } else { None },
                        )
                    };
                    if let Some(detail) = detail {
                        self.ensure_assistant_output_started().await;
                        self.emit_output_block(
                            OutputBlock::Tool(ToolBlock::running(tool_name, detail)),
                            Some(id.clone()),
                        )
                        .await;
                    }
                    if let Some(assistant) = self.session.messages.get_mut(self.assistant_index) {
                        SessionPrompt::upsert_tool_call_part(
                            assistant,
                            id,
                            None,
                            Some(tool_input),
                            Some(tool_raw),
                            Some(crate::ToolCallStatus::Pending),
                            Some(tool_state),
                        );
                    }
                }
            }
            LoopEvent::ToolCallReady(call) => {
                if call.name.trim().is_empty() {
                    return Ok(());
                }

                // Broadcast canonical tool lifecycle event.
                if let Some(broadcast) = &self.event_broadcast {
                    let event = serde_json::to_value(ToolLifecycleEvent {
                        event_type: "tool_call.lifecycle",
                        session_id: &self.session.id,
                        tool_call_id: &call.id,
                        phase: "complete",
                        tool_name: &call.name,
                    })
                    .unwrap_or(serde_json::Value::Null);
                    broadcast(event);
                }

                let (tool_input, tool_raw, tool_state, should_emit_start, detail) = {
                    let entry =
                        self.tool_calls
                            .entry(call.id.clone())
                            .or_insert_with(|| StreamToolState {
                                name: String::new(),
                                raw_input: String::new(),
                                input: serde_json::json!({}),
                                status: crate::ToolCallStatus::Pending,
                                state: crate::ToolState::Pending {
                                    input: serde_json::json!({}),
                                    raw: String::new(),
                                },
                                emitted_output_start: false,
                                emitted_output_detail: None,
                            });
                    entry.name = call.name.clone();
                    entry.input = call.arguments.clone();
                    entry.raw_input = serde_json::to_string(&call.arguments).unwrap_or_default();
                    let should_emit_start = !entry.emitted_output_start;
                    if should_emit_start {
                        entry.emitted_output_start = true;
                    }
                    let detail = tool_progress_detail(
                        &entry.input,
                        Some(entry.raw_input.as_str()),
                        &crate::ToolCallStatus::Running,
                    );
                    let should_emit_detail = detail
                        .as_ref()
                        .is_some_and(|detail| entry.emitted_output_detail.as_ref() != Some(detail));
                    if should_emit_detail {
                        entry.emitted_output_detail = detail.clone();
                    }
                    entry.status = crate::ToolCallStatus::Running;
                    entry.state = crate::ToolState::Running {
                        input: entry.input.clone(),
                        title: None,
                        metadata: None,
                        time: crate::RunningTime {
                            start: chrono::Utc::now().timestamp_millis(),
                        },
                    };
                    (
                        entry.input.clone(),
                        entry.raw_input.clone(),
                        entry.state.clone(),
                        should_emit_start,
                        if should_emit_detail { detail } else { None },
                    )
                };
                self.ensure_assistant_output_started().await;
                if should_emit_start {
                    self.emit_output_block(
                        OutputBlock::Tool(ToolBlock::start(call.name.clone())),
                        Some(call.id.clone()),
                    )
                    .await;
                }
                if let Some(detail) = detail {
                    self.emit_output_block(
                        OutputBlock::Tool(ToolBlock::running(call.name.clone(), detail)),
                        Some(call.id.clone()),
                    )
                    .await;
                }
                if let Some(assistant) = self.session.messages.get_mut(self.assistant_index) {
                    SessionPrompt::upsert_tool_call_part(
                        assistant,
                        &call.id,
                        Some(&call.name),
                        Some(tool_input),
                        Some(tool_raw),
                        Some(crate::ToolCallStatus::Running),
                        Some(tool_state),
                    );
                }
                self.session.touch();
                SessionPrompt::maybe_emit_session_update(
                    self.update_hook,
                    self.session,
                    &mut self.last_emit,
                    true,
                );
            }
            LoopEvent::StepDone {
                finish_reason,
                usage,
            } => {
                self.finish_reason = Some(match finish_reason {
                    RuntimeFinishReason::ToolUse => "tool-calls".to_string(),
                    RuntimeFinishReason::EndTurn => "stop".to_string(),
                    RuntimeFinishReason::Provider(reason) => reason.clone(),
                    RuntimeFinishReason::MaxSteps => "max_steps".to_string(),
                    RuntimeFinishReason::Cancelled => "cancelled".to_string(),
                    RuntimeFinishReason::Error(message) => format!("error:{}", message),
                });
                if let Some(usage) = usage {
                    self.prompt_tokens = self.prompt_tokens.max(usage.prompt_tokens);
                    self.completion_tokens = self.completion_tokens.max(usage.completion_tokens);
                    self.reasoning_tokens = self.reasoning_tokens.max(usage.reasoning_tokens);
                    self.cache_read_tokens = self.cache_read_tokens.max(usage.cache_read_tokens);
                    self.cache_write_tokens = self.cache_write_tokens.max(usage.cache_write_tokens);
                }
                self.finish_output_blocks().await;
            }
            LoopEvent::Error(msg) => {
                self.finish_output_blocks().await;
                return Err(RuntimeLoopError::ModelError(msg.clone()));
            }
        }
        Ok(())
    }

    async fn on_tool_result(
        &mut self,
        call: &RuntimeToolCallReady,
        result: &RuntimeToolResult,
    ) -> std::result::Result<(), RuntimeLoopError> {
        self.executed_local_tools_this_step = true;

        if let Some(entry) = self.tool_calls.get_mut(&call.id) {
            entry.input = call.arguments.clone();
            entry.name = result.tool_name.clone();
            entry.status = if result.is_error {
                crate::ToolCallStatus::Error
            } else {
                crate::ToolCallStatus::Completed
            };
            let now = chrono::Utc::now().timestamp_millis();
            entry.state = if result.is_error {
                crate::ToolState::Error {
                    input: call.arguments.clone(),
                    error: result.output.clone(),
                    metadata: None,
                    time: crate::ErrorTime {
                        start: now,
                        end: now,
                    },
                }
            } else {
                let mut metadata = result
                    .metadata
                    .clone()
                    .and_then(|value| value.as_object().cloned())
                    .map(|obj| obj.into_iter().collect::<HashMap<_, _>>())
                    .unwrap_or_default();
                let (_, state_attachments) = SessionPrompt::extract_tool_attachments_from_metadata(
                    &mut metadata,
                    &self.session.id,
                    &self
                        .session
                        .messages
                        .get(self.assistant_index)
                        .map(|m| m.id.clone())
                        .unwrap_or_default(),
                );
                crate::ToolState::Completed {
                    input: call.arguments.clone(),
                    output: result.output.clone(),
                    title: result
                        .title
                        .clone()
                        .unwrap_or_else(|| "Tool Result".to_string()),
                    metadata,
                    time: crate::CompletedTime {
                        start: now,
                        end: now,
                        compacted: None,
                    },
                    attachments: state_attachments,
                }
            };
            if let Some(assistant) = self.session.messages.get_mut(self.assistant_index) {
                SessionPrompt::upsert_tool_call_part(
                    assistant,
                    &call.id,
                    Some(&result.tool_name),
                    Some(call.arguments.clone()),
                    Some(serde_json::to_string(&call.arguments).unwrap_or_default()),
                    Some(entry.status.clone()),
                    Some(entry.state.clone()),
                );
            }
        }

        let mut metadata_map = result
            .metadata
            .clone()
            .and_then(|value| value.as_object().cloned())
            .map(|obj| obj.into_iter().collect::<HashMap<_, _>>())
            .unwrap_or_default();
        let (attachments, _) = SessionPrompt::extract_tool_attachments_from_metadata(
            &mut metadata_map,
            &self.session.id,
            &self
                .session
                .messages
                .get(self.assistant_index)
                .map(|m| m.id.clone())
                .unwrap_or_default(),
        );
        self.stream_tool_results.push((
            call.id.clone(),
            result.output.clone(),
            result.is_error,
            result.title.clone(),
            if metadata_map.is_empty() {
                None
            } else {
                Some(metadata_map)
            },
            attachments,
        ));

        let detail = tool_result_detail(result.title.as_deref(), &result.output);
        let block = if result.is_error {
            OutputBlock::Tool(ToolBlock::error(
                result.tool_name.clone(),
                detail.unwrap_or_else(|| result.output.clone()),
            ))
        } else {
            OutputBlock::Tool(ToolBlock::done(result.tool_name.clone(), detail))
        };
        self.emit_output_block(block, Some(call.id.clone())).await;

        self.session.touch();
        SessionPrompt::maybe_emit_session_update(
            self.update_hook,
            self.session,
            &mut self.last_emit,
            true,
        );
        Ok(())
    }

    async fn on_step_boundary(
        &mut self,
        ctx: &StepBoundary,
    ) -> std::result::Result<(), RuntimeLoopError> {
        if let StepBoundary::End { .. } = ctx {
            self.step_complete.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}

fn tool_progress_detail(
    input: &serde_json::Value,
    raw: Option<&str>,
    status: &crate::ToolCallStatus,
) -> Option<String> {
    if let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) {
        return Some(raw.to_string());
    }

    match status {
        crate::ToolCallStatus::Pending | crate::ToolCallStatus::Running => {
            if input.is_null() {
                return None;
            }
            if let Some(obj) = input.as_object() {
                if obj.is_empty() {
                    return None;
                }
            }
            if let Some(arr) = input.as_array() {
                if arr.is_empty() {
                    return None;
                }
            }
            if let Some(text) = input.as_str() {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return None;
                }
                return Some(trimmed.to_string());
            }
            Some(input.to_string())
        }
        crate::ToolCallStatus::Completed | crate::ToolCallStatus::Error => None,
    }
}

fn tool_result_detail(title: Option<&str>, content: &str) -> Option<String> {
    match title.map(str::trim).filter(|value| !value.is_empty()) {
        Some(title) => Some(format!("{title}: {content}")),
        None if content.trim().is_empty() => None,
        None => Some(content.to_string()),
    }
}

impl SessionPrompt {
    fn text_from_prompt_parts(parts: &[PartInput]) -> String {
        parts
            .iter()
            .filter_map(|p| match p {
                PartInput::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn truncate_debug_text(value: &str, max_chars: usize) -> String {
        if value.chars().count() <= max_chars {
            return value.to_string();
        }
        let mut out = value.chars().take(max_chars).collect::<String>();
        out.push_str("...[truncated]");
        out
    }

    fn annotate_latest_user_message(
        session: &mut Session,
        input: &PromptInput,
        system_prompt: Option<&str>,
    ) {
        let Some(user_msg) = session
            .messages
            .iter_mut()
            .rfind(|m| matches!(m.role, Role::User))
        else {
            return;
        };

        if let Some(agent) = input.agent.as_deref() {
            user_msg
                .metadata
                .insert("resolved_agent".to_string(), serde_json::json!(agent));
        }

        if let Some(system) = system_prompt {
            user_msg.metadata.insert(
                "resolved_system_prompt".to_string(),
                serde_json::json!(Self::truncate_debug_text(system, 8000)),
            );
            user_msg.metadata.insert(
                "resolved_system_prompt_applied".to_string(),
                serde_json::json!(true),
            );
        } else if input.agent.is_some() {
            user_msg.metadata.insert(
                "resolved_system_prompt_applied".to_string(),
                serde_json::json!(false),
            );
        }

        let user_prompt = Self::text_from_prompt_parts(&input.parts);
        if !user_prompt.is_empty() {
            user_msg.metadata.insert(
                "resolved_user_prompt".to_string(),
                serde_json::json!(Self::truncate_debug_text(&user_prompt, 8000)),
            );
        }
    }

    pub fn new(session_state: Arc<RwLock<SessionStateManager>>) -> Self {
        Self {
            state: Arc::new(Mutex::new(HashMap::new())),
            session_state,
            mcp_clients: None,
            lsp_registry: None,
            tool_runtime_config: rocode_tool::ToolRuntimeConfig::default(),
        }
    }

    pub fn with_tool_runtime_config(
        mut self,
        tool_runtime_config: rocode_tool::ToolRuntimeConfig,
    ) -> Self {
        self.tool_runtime_config = tool_runtime_config;
        self
    }

    pub fn with_mcp_clients(mut self, clients: Arc<rocode_mcp::McpClientRegistry>) -> Self {
        self.mcp_clients = Some(clients);
        self
    }

    pub fn with_lsp_registry(mut self, registry: Arc<rocode_lsp::LspClientRegistry>) -> Self {
        self.lsp_registry = Some(registry);
        self
    }

    pub async fn assert_not_busy(&self, session_id: &str) -> anyhow::Result<()> {
        let state = self.state.lock().await;
        if state.contains_key(session_id) {
            return Err(anyhow::anyhow!("Session {} is busy", session_id));
        }
        Ok(())
    }

    pub async fn create_user_message(
        &self,
        input: &PromptInput,
        session: &mut Session,
    ) -> anyhow::Result<()> {
        // Collect text parts for the primary message
        let text = input
            .parts
            .iter()
            .filter_map(|p| match p {
                PartInput::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");

        let has_non_text = input
            .parts
            .iter()
            .any(|p| !matches!(p, PartInput::Text { .. }));

        if text.is_empty() && !has_non_text {
            return Err(anyhow::anyhow!("No content in prompt"));
        }

        let project_root = session.directory.clone();

        // Create the user message with text (or empty if only non-text parts)
        let msg = if text.is_empty() {
            session.add_user_message(" ")
        } else {
            session.add_user_message(&text)
        };

        // Add non-text parts to the message
        for part in &input.parts {
            match part {
                PartInput::Text { .. } => {} // already handled above
                PartInput::File {
                    url,
                    filename,
                    mime,
                } => {
                    self.add_file_part(
                        msg,
                        url,
                        filename.as_deref(),
                        mime.as_deref(),
                        &project_root,
                    )
                    .await;
                }
                PartInput::Agent { name } => {
                    msg.add_agent(name.clone());
                    // Add synthetic text instructing the LLM to invoke the agent
                    msg.add_text(format!(
                        "Use the above message and context to generate a prompt and prefer calling task_flow with operation=create and agent=\"{}\". Only fall back to the task tool if task_flow is unavailable in this session.",
                        name
                    ));
                }
                PartInput::Subtask {
                    prompt,
                    description,
                    agent,
                } => {
                    let subtask_id = format!("sub_{}", uuid::Uuid::new_v4());
                    let description = description.clone().unwrap_or_else(|| prompt.clone());
                    msg.add_subtask(subtask_id.clone(), description.clone());
                    let mut pending = Self::pending_subtasks_metadata_values(&msg.metadata);
                    pending.push(
                        serde_json::to_value(PendingSubtaskMetadata {
                            id: &subtask_id,
                            agent,
                            prompt,
                            description: &description,
                        })
                        .unwrap_or(serde_json::Value::Null),
                    );
                    msg.metadata.insert(
                        "pending_subtasks".to_string(),
                        serde_json::Value::Array(pending),
                    );
                }
            }
        }

        Ok(())
    }

    // --- file_parts methods moved to file_parts.rs ---

    async fn start(&self, session_id: &str) -> Option<CancellationToken> {
        let state = self.state.lock().await;
        if state.contains_key(session_id) {
            return None;
        }
        drop(state);

        let token = CancellationToken::new();
        let mut state = self.state.lock().await;
        state.insert(
            session_id.to_string(),
            PromptState {
                cancel_token: token.clone(),
            },
        );
        Some(token)
    }

    async fn resume(&self, session_id: &str) -> Option<CancellationToken> {
        let state = self.state.lock().await;
        state.get(session_id).map(|s| s.cancel_token.clone())
    }

    pub async fn is_running(&self, session_id: &str) -> bool {
        let state = self.state.lock().await;
        state.contains_key(session_id)
    }

    async fn finish_run(&self, session_id: &str) {
        let mut state = self.state.lock().await;
        state.remove(session_id);
        drop(state);

        let mut session_state = self.session_state.write().await;
        session_state.set_idle(session_id);
    }

    pub async fn cancel(&self, session_id: &str) {
        let mut state = self.state.lock().await;
        if let Some(prompt_state) = state.remove(session_id) {
            prompt_state.cancel_token.cancel();
        }

        let mut session_state = self.session_state.write().await;
        session_state.set_idle(session_id);
    }

    pub async fn prompt(
        &self,
        input: PromptInput,
        session: &mut Session,
        provider: Arc<dyn Provider>,
        system_prompt: Option<String>,
        tools: Vec<ToolDefinition>,
        compiled_request: CompiledExecutionRequest,
    ) -> anyhow::Result<()> {
        self.prompt_with_update_hook(
            input,
            session,
            PromptRequestContext {
                provider,
                system_prompt,
                tools,
                compiled_request,
                hooks: PromptHooks::default(),
            },
        )
        .await
    }

    pub async fn prompt_with_update_hook(
        &self,
        input: PromptInput,
        session: &mut Session,
        request: PromptRequestContext,
    ) -> anyhow::Result<()> {
        let PromptRequestContext {
            provider,
            system_prompt,
            tools,
            compiled_request,
            hooks,
        } = request;

        self.assert_not_busy(&input.session_id).await?;

        let cancel_token = self.start(&input.session_id).await;
        let token = match cancel_token {
            Some(t) => t,
            None => return Err(anyhow::anyhow!("Session already running")),
        };

        // Keep model/provider resolution aligned for both hook payloads and prompt loop.
        let model_id = input
            .model
            .as_ref()
            .map(|m| m.model_id.clone())
            .unwrap_or_else(|| "default".to_string());
        let provider_id = input
            .model
            .as_ref()
            .map(|m| m.provider_id.clone())
            .unwrap_or_else(|| "anthropic".to_string());

        self.create_user_message(&input, session).await?;
        Self::annotate_latest_user_message(session, &input, system_prompt.as_deref());

        // Set an immediate title from the first user message when the title is
        // still the auto-generated default.  This gives frontends a meaningful
        // label right away.  The LLM-generated title will silently replace it
        // after the first assistant step completes (see ensure_title below).
        if session.is_default_title() {
            if let Some(text) = session
                .messages
                .iter()
                .find(|m| matches!(m.role, Role::User))
                .map(|m| m.get_text())
            {
                let immediate = tools_and_output::generate_session_title(&text);
                if !immediate.is_empty() && immediate != "New Session" {
                    session.set_auto_title(immediate);
                }
            }
        }

        session.touch();
        Self::emit_session_update(hooks.update_hook.as_ref(), session);

        if input.no_reply {
            self.finish_run(&input.session_id).await;
            return Ok(());
        }

        {
            let mut session_state = self.session_state.write().await;
            session_state.set_busy(&input.session_id);
        }

        let session_id = input.session_id.clone();

        let result = self
            .loop_inner(
                session_id.clone(),
                token,
                session,
                PromptLoopContext {
                    provider,
                    model_id,
                    provider_id,
                    agent_name: input.agent.clone(),
                    system_prompt,
                    tools,
                    compiled_request,
                    hooks,
                },
            )
            .await;

        self.finish_run(&session_id).await;

        if let Err(e) = result {
            tracing::error!("Prompt loop error for session {}: {}", session_id, e);
            return Err(e);
        }

        Ok(())
    }

    pub async fn resume_session(
        &self,
        session_id: &str,
        session: &mut Session,
        provider: Arc<dyn Provider>,
        system_prompt: Option<String>,
        tools: Vec<ToolDefinition>,
        compiled_request: CompiledExecutionRequest,
    ) -> anyhow::Result<()> {
        let token = self.resume(session_id).await;

        let token = match token {
            Some(t) => t,
            None => {
                return Err(anyhow::anyhow!(
                    "Session {} is not running, cannot resume",
                    session_id
                ));
            }
        };

        fn deserialize_opt_string_lossy<'de, D>(
            deserializer: D,
        ) -> std::result::Result<Option<String>, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let value = Option::<serde_json::Value>::deserialize(deserializer)?;
            Ok(match value {
                Some(serde_json::Value::String(value)) => Some(value),
                _ => None,
            })
        }

        #[derive(Debug, Default, Deserialize)]
        struct ResumeSessionMetadataWire {
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
            model_provider: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
            model_id: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
            agent: Option<String>,
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
            model_variant: Option<String>,
        }

        let metadata = serde_json::to_value(&session.metadata)
            .ok()
            .and_then(|value| serde_json::from_value::<ResumeSessionMetadataWire>(value).ok())
            .unwrap_or_default();

        let model = session.messages.iter().rev().find_map(|m| match m.role {
            Role::User => metadata
                .model_provider
                .as_deref()
                .zip(metadata.model_id.as_deref())
                .map(|(provider_id, model_id)| ModelRef {
                    provider_id: provider_id.to_string(),
                    model_id: model_id.to_string(),
                }),
            _ => None,
        });

        let model_id = model
            .as_ref()
            .map(|m| m.model_id.clone())
            .unwrap_or_else(|| "default".to_string());
        let provider_id = model
            .as_ref()
            .map(|m| m.provider_id.clone())
            .unwrap_or_else(|| "anthropic".to_string());

        let session_id = session_id.to_string();
        let resume_agent = metadata.agent.clone();
        let compiled_request = compiled_request.inherit_missing(&session_runtime_request_defaults(
            metadata.model_variant.clone(),
        ));

        {
            let mut session_state = self.session_state.write().await;
            session_state.set_busy(&session_id);
        }

        let result = self
            .loop_inner(
                session_id.clone(),
                token,
                session,
                PromptLoopContext {
                    provider,
                    model_id,
                    provider_id,
                    agent_name: resume_agent,
                    system_prompt,
                    tools,
                    compiled_request: compiled_request.clone(),
                    hooks: PromptHooks::default(),
                },
            )
            .await;

        self.finish_run(&session_id).await;

        if let Err(e) = result {
            tracing::error!("Resume prompt loop error for session {}: {}", session_id, e);
            return Err(e);
        }

        Ok(())
    }

    async fn run_runtime_step(
        &self,
        token: CancellationToken,
        session: &mut Session,
        resolved_tools: Vec<ToolDefinition>,
        input: RuntimeStepInput,
    ) -> anyhow::Result<SessionStepRuntimeOutput> {
        let assistant_message_id = session
            .messages
            .get(input.assistant_index)
            .map(|m| m.id.clone())
            .unwrap_or_default();
        let shared = Arc::new(Mutex::new(SessionStepShared {
            assistant_message_id: Some(assistant_message_id),
        }));
        let step_complete = Arc::new(AtomicBool::new(false));
        let cancel = SessionStepCancelToken {
            user_cancel: token.clone(),
            step_complete: step_complete.clone(),
        };

        let subsessions = Arc::new(Mutex::new(Self::load_persisted_subsessions(session)));

        let model = SimpleModelCaller {
            provider: input.step_ctx.provider.clone(),
            config: SimpleModelCallerConfig {
                request: input
                    .step_ctx
                    .compiled_request
                    .with_model(input.step_ctx.model_id.clone())
                    .inherit_missing(&session_runtime_request_defaults(None)),
            },
        };
        let allowed_tools = Some(Arc::new(
            resolved_tools
                .iter()
                .map(|tool| tool.name.clone())
                .collect::<HashSet<_>>(),
        ));

        let tools = SessionStepToolDispatcher {
            session_id: input.session_id.clone(),
            directory: session.directory.clone(),
            agent_name: input.step_ctx.agent_name.clone().unwrap_or_default(),
            abort_token: token.clone(),
            tool_registry: input.tool_registry,
            provider: input.step_ctx.provider.clone(),
            provider_id: input.step_ctx.provider_id.clone(),
            model_id: input.step_ctx.model_id.clone(),
            resolved_tools,
            allowed_tools,
            shared,
            subsessions: subsessions.clone(),
            agent_lookup: input.step_ctx.hooks.agent_lookup.clone(),
            ask_question_hook: input.step_ctx.hooks.ask_question_hook.clone(),
            ask_permission_hook: input.step_ctx.hooks.ask_permission_hook.clone(),
            publish_bus_hook: input.step_ctx.hooks.publish_bus_hook.clone(),
            tool_runtime_config: self.tool_runtime_config.clone(),
        };

        let mut sink = SessionStepSink::new(
            session,
            input.assistant_index,
            input.step_ctx.hooks.update_hook.as_ref(),
            input.step_ctx.hooks.event_broadcast.as_ref(),
            input.step_ctx.hooks.output_block_hook.as_ref(),
            step_complete,
        );
        let policy = LoopPolicy {
            max_steps: Some(MAX_STEPS),
            tool_dedup: ToolDedupScope::None,
            ..Default::default()
        };
        let outcome = run_loop(
            &model,
            &tools,
            &mut sink,
            &policy,
            &cancel,
            input.chat_messages,
        )
        .await;
        let output = sink.into_output();

        let persisted = subsessions.lock().await.clone();
        Self::save_persisted_subsessions(session, &persisted);

        match outcome {
            Ok(_) => Ok(output),
            Err(RuntimeLoopError::ModelError(message)) => Err(anyhow::anyhow!("{}", message)),
            Err(RuntimeLoopError::ToolDispatchError { tool, error }) => {
                let lower = error.to_ascii_lowercase();
                if token.is_cancelled()
                    || lower.contains("cancelled")
                    || lower.contains("canceled")
                    || lower.contains("aborted")
                {
                    Ok(output)
                } else {
                    Err(anyhow::anyhow!(
                        "Tool dispatch failed ({}): {}",
                        tool,
                        error
                    ))
                }
            }
            Err(RuntimeLoopError::Cancelled) => Ok(output),
            Err(RuntimeLoopError::SinkError(message)) | Err(RuntimeLoopError::Other(message)) => {
                Err(anyhow::anyhow!("{}", message))
            }
        }
    }

    /// Check if context overflow requires compaction; if so, run LLM-driven
    /// compaction with fallback to simple text truncation.
    async fn maybe_compact_context(
        session_id: &str,
        provider_id: &str,
        model_id: &str,
        session: &mut Session,
        provider: &Arc<dyn Provider>,
        filtered_messages: &[SessionMessage],
        compiled_request: &CompiledExecutionRequest,
    ) {
        if !Self::should_compact(
            filtered_messages,
            provider.as_ref(),
            model_id,
            compiled_request.max_tokens,
        ) {
            return;
        }

        tracing::info!(
            "Context overflow detected, triggering compaction for session {}",
            session_id
        );

        let parent_id = filtered_messages
            .last()
            .map(|m| m.id.clone())
            .unwrap_or_default();
        let compaction_messages =
            Self::build_chat_messages(filtered_messages, None).unwrap_or_default();
        let model_ref = V2ModelRef {
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
        };

        match run_compaction::<crate::compaction::NoopSessionOps>(
            session_id,
            &parent_id,
            compaction_messages,
            model_ref,
            provider.clone(),
            crate::compaction::RunCompactionOptions {
                abort: CancellationToken::new(),
                auto: true,
                config: None,
                session_ops: None,
            },
        )
        .await
        {
            Ok(CompactionResult::Continue) => {
                tracing::info!(
                    "LLM compaction complete for session {}, continuing",
                    session_id
                );
            }
            Ok(CompactionResult::Stop) => {
                tracing::warn!("LLM compaction returned stop for session {}, falling back to simple compaction", session_id);
                if let Some(summary) = Self::trigger_compaction(session, filtered_messages) {
                    tracing::info!("Fallback compaction (from stop) complete: {}", summary);
                }
            }
            Err(e) => {
                tracing::warn!(
                    "LLM compaction failed for session {}: {}, falling back to simple compaction",
                    session_id,
                    e
                );
                if let Some(summary) = Self::trigger_compaction(session, filtered_messages) {
                    tracing::info!("Fallback compaction complete: {}", summary);
                }
            }
        }
    }

    /// Apply plugin message transforms, insert agent reminders, and build the
    /// final provider-format chat messages for the LLM call.
    async fn prepare_chat_messages(
        session_id: &str,
        agent_name: Option<&str>,
        system_prompt: Option<&str>,
        mut filtered_messages: Vec<SessionMessage>,
        provider_type: ProviderType,
    ) -> anyhow::Result<Vec<rocode_provider::Message>> {
        if rocode_plugin::should_trigger_script_hooks(HookEvent::ChatMessagesTransform, agent_name)
            .await
        {
            let hook_messages = serde_json::Value::Array(
                filtered_messages
                    .iter()
                    .map(session_message_hook_payload)
                    .collect(),
            );
            let message_hook_outputs = rocode_plugin::trigger_collect(
                HookContext::new(HookEvent::ChatMessagesTransform)
                    .with_session(session_id)
                    .with_data("message_count", serde_json::json!(filtered_messages.len()))
                    .with_data("messages", hook_messages),
            )
            .await;
            apply_chat_messages_hook_outputs(&mut filtered_messages, message_hook_outputs);
        }

        let mut prompt_messages = filtered_messages;
        if let Some(agent) = agent_name {
            let was_plan = was_plan_agent(&prompt_messages);
            prompt_messages = insert_reminders(&prompt_messages, agent, was_plan);
        }

        let mut chat_messages = Self::build_chat_messages(&prompt_messages, system_prompt)?;
        apply_caching(&mut chat_messages, provider_type);
        Ok(chat_messages)
    }

    /// Finalize the assistant placeholder message with usage metadata from the
    /// runtime step output.
    fn finalize_assistant_message(
        session: &mut Session,
        assistant_index: usize,
        step_output: &SessionStepRuntimeOutput,
    ) {
        if let Some(assistant_msg) = session.messages.get_mut(assistant_index) {
            if let Some(reason) = step_output.finish_reason.clone() {
                assistant_msg
                    .metadata
                    .insert("finish_reason".to_string(), serde_json::json!(reason));
            }
            assistant_msg.metadata.insert(
                "completed_at".to_string(),
                serde_json::json!(chrono::Utc::now().timestamp_millis()),
            );
            assistant_msg.metadata.insert(
                "usage".to_string(),
                serde_json::json!({
                    "prompt_tokens": step_output.prompt_tokens,
                    "completion_tokens": step_output.completion_tokens,
                    "reasoning_tokens": step_output.reasoning_tokens,
                    "cache_read_tokens": step_output.cache_read_tokens,
                    "cache_write_tokens": step_output.cache_write_tokens,
                }),
            );
            assistant_msg.metadata.insert(
                "tokens_input".to_string(),
                serde_json::json!(step_output.prompt_tokens),
            );
            assistant_msg.metadata.insert(
                "tokens_output".to_string(),
                serde_json::json!(step_output.completion_tokens),
            );
            assistant_msg.metadata.insert(
                "tokens_reasoning".to_string(),
                serde_json::json!(step_output.reasoning_tokens),
            );
            assistant_msg.metadata.insert(
                "tokens_cache_read".to_string(),
                serde_json::json!(step_output.cache_read_tokens),
            );
            assistant_msg.metadata.insert(
                "tokens_cache_write".to_string(),
                serde_json::json!(step_output.cache_write_tokens),
            );
            assistant_msg.usage = Some(crate::message::MessageUsage {
                input_tokens: step_output.prompt_tokens,
                output_tokens: step_output.completion_tokens,
                reasoning_tokens: step_output.reasoning_tokens,
                cache_read_tokens: step_output.cache_read_tokens,
                cache_write_tokens: step_output.cache_write_tokens,
                ..Default::default()
            });
        }
    }

    /// Fire the `chat.message` plugin hook after the assistant message is
    /// finalized. Mirrors TS parity.
    async fn run_chat_message_hook(
        session: &mut Session,
        session_id: &str,
        assistant_index: usize,
        agent_name: Option<&str>,
        provider: &Arc<dyn Provider>,
        model_id: &str,
        has_tool_calls: bool,
    ) {
        if !rocode_plugin::should_trigger_script_hooks(HookEvent::ChatMessage, agent_name).await {
            return;
        }
        let Some(assistant_msg) = session.messages.get(assistant_index).cloned() else {
            return;
        };

        let mut hook_ctx = HookContext::new(HookEvent::ChatMessage)
            .with_session(session_id)
            .with_data("message_id", serde_json::json!(&assistant_msg.id))
            .with_data("message", session_message_hook_payload(&assistant_msg))
            .with_data("parts", serde_json::json!(&assistant_msg.parts))
            .with_data("has_tool_calls", serde_json::json!(has_tool_calls));

        if let Some(model) = provider.get_model(model_id) {
            let model_payload = HookModelPayload {
                id: model.id.clone(),
                name: model.name.clone(),
                provider: model.provider.clone(),
            };
            hook_ctx = hook_ctx.with_data(
                "model",
                serde_json::to_value(model_payload).unwrap_or(serde_json::Value::Null),
            );
        } else {
            hook_ctx = hook_ctx.with_data("model_id", serde_json::json!(model_id));
        }
        hook_ctx = hook_ctx.with_data("sessionID", serde_json::json!(session_id));
        if let Some(agent) = agent_name {
            hook_ctx = hook_ctx.with_data("agent", serde_json::json!(agent));
        }

        let hook_outputs = rocode_plugin::trigger_collect(hook_ctx).await;
        if let Some(current_assistant) = session.messages.get_mut(assistant_index) {
            apply_chat_message_hook_outputs(current_assistant, hook_outputs);
        }
    }

    async fn loop_inner(
        &self,
        session_id: String,
        token: CancellationToken,
        session: &mut Session,
        prompt_ctx: PromptLoopContext,
    ) -> anyhow::Result<()> {
        let mut step = 0u32;
        let provider_type = ProviderType::from_provider_id(&prompt_ctx.provider_id);
        let mut post_first_step_ran = false;

        loop {
            if token.is_cancelled() {
                tracing::info!("Prompt loop cancelled for session {}", session_id);
                break;
            }

            let filtered_messages = Self::filter_compacted_messages(&session.messages);

            let last_user_idx = filtered_messages
                .iter()
                .rposition(|m| matches!(m.role, Role::User));

            let last_assistant_idx = filtered_messages
                .iter()
                .rposition(|m| matches!(m.role, Role::Assistant));

            let last_user_idx = match last_user_idx {
                Some(idx) => idx,
                None => return Err(anyhow::anyhow!("No user message found")),
            };

            if self
                .process_pending_subtasks(
                    session,
                    prompt_ctx.provider.clone(),
                    &prompt_ctx.provider_id,
                    &prompt_ctx.model_id,
                    &prompt_ctx.hooks,
                )
                .await?
            {
                tracing::info!("Processed pending subtask parts for session {}", session_id);
                continue;
            }

            // Early exit: if the last assistant message has a terminal finish
            // reason and it came after the last user message, the conversation
            // turn is complete. Mirrors TS prompt.ts:318-325.
            if let Some(assistant_idx) = last_assistant_idx {
                let assistant = &filtered_messages[assistant_idx];
                if is_terminal_finish(assistant.finish.as_deref()) && last_user_idx < assistant_idx
                {
                    tracing::info!(
                        finish = ?assistant.finish,
                        "Prompt loop complete for session {}", session_id
                    );
                    break;
                }
            }

            step += 1;
            if step > MAX_STEPS {
                tracing::warn!("Max steps reached for session {}", session_id);
                break;
            }

            // ── Context compaction (P3: uses collect_text_chunks via orchestrator) ──
            Self::maybe_compact_context(
                &session_id,
                &prompt_ctx.provider_id,
                &prompt_ctx.model_id,
                session,
                &prompt_ctx.provider,
                &filtered_messages,
                &prompt_ctx.compiled_request,
            )
            .await;

            tracing::info!(
                step = step,
                session_id = %session_id,
                message_count = filtered_messages.len(),
                "prompt loop step start"
            );

            // ── Prepare chat messages (plugin transforms, reminders, caching) ──
            let chat_messages = Self::prepare_chat_messages(
                &session_id,
                prompt_ctx.agent_name.as_deref(),
                prompt_ctx.system_prompt.as_deref(),
                filtered_messages,
                provider_type,
            )
            .await?;
            let resolved_tools = merge_tool_definitions(
                prompt_ctx.tools.clone(),
                Self::mcp_tools_from_session(session),
            );

            let tool_registry = Arc::new(rocode_tool::create_default_registry().await);

            // Create assistant message placeholder before consuming the stream so
            // callers can observe incremental output updates.
            let assistant_index = session.messages.len();
            let assistant_message_id =
                rocode_core::id::create(rocode_core::id::Prefix::Message, true, None);
            let mut assistant_metadata = HashMap::new();
            assistant_metadata.insert(
                "model_provider".to_string(),
                serde_json::json!(&prompt_ctx.provider_id),
            );
            assistant_metadata.insert(
                "model_id".to_string(),
                serde_json::json!(&prompt_ctx.model_id),
            );
            if let Some(agent) = prompt_ctx.agent_name.as_deref() {
                assistant_metadata.insert("agent".to_string(), serde_json::json!(agent));
                assistant_metadata.insert("mode".to_string(), serde_json::json!(agent));
            }
            session.messages.push(SessionMessage {
                id: assistant_message_id,
                session_id: session_id.clone(),
                role: Role::Assistant,
                parts: Vec::new(),
                created_at: chrono::Utc::now(),
                metadata: assistant_metadata,
                usage: None,
                finish: None,
            });
            session.touch();
            Self::emit_session_update(prompt_ctx.hooks.update_hook.as_ref(), session);

            let step_output = self
                .run_runtime_step(
                    token.clone(),
                    session,
                    resolved_tools,
                    RuntimeStepInput {
                        session_id: session_id.clone(),
                        assistant_index,
                        chat_messages,
                        tool_registry: tool_registry.clone(),
                        step_ctx: RuntimeStepContext {
                            provider: prompt_ctx.provider.clone(),
                            model_id: prompt_ctx.model_id.clone(),
                            provider_id: prompt_ctx.provider_id.clone(),
                            agent_name: prompt_ctx.agent_name.clone(),
                            compiled_request: prompt_ctx.compiled_request.clone(),
                            hooks: prompt_ctx.hooks.clone(),
                        },
                    },
                )
                .await?;

            let finish_reason = step_output.finish_reason.clone();
            let executed_local_tools_this_step = step_output.executed_local_tools_this_step;

            // ── Finalize assistant message with usage metadata ──
            Self::finalize_assistant_message(session, assistant_index, &step_output);

            // ── Append tool results to session ──
            if !step_output.stream_tool_results.is_empty() {
                let mut tool_msg = SessionMessage::tool(session_id.clone());
                for (tool_call_id, content, is_error, title, metadata, attachments) in
                    step_output.stream_tool_results
                {
                    Self::push_tool_result_part(
                        &mut tool_msg,
                        tool_call_id,
                        content,
                        is_error,
                        title,
                        metadata,
                        attachments,
                    );
                }
                session.messages.push(tool_msg);
            }

            let has_tool_calls = session
                .messages
                .get(assistant_index)
                .map(Self::has_unresolved_tool_calls)
                .unwrap_or(false);

            session.touch();
            Self::emit_session_update(prompt_ctx.hooks.update_hook.as_ref(), session);

            // ── Plugin hook: chat.message ──
            Self::run_chat_message_hook(
                session,
                &session_id,
                assistant_index,
                prompt_ctx.agent_name.as_deref(),
                &prompt_ctx.provider,
                &prompt_ctx.model_id,
                has_tool_calls,
            )
            .await;

            if executed_local_tools_this_step {
                continue;
            }

            if !post_first_step_ran {
                Self::ensure_title(session, prompt_ctx.provider.clone(), &prompt_ctx.model_id)
                    .await;
                let _ = Self::summarize_session(
                    session,
                    &session_id,
                    &prompt_ctx.provider_id,
                    &prompt_ctx.model_id,
                    prompt_ctx.provider.as_ref(),
                )
                .await;
                post_first_step_ran = true;
            }

            if is_terminal_finish(finish_reason.as_deref()) {
                tracing::info!(
                    "Prompt loop complete for session {} with finish: {:?}",
                    session_id,
                    finish_reason
                );
                break;
            }
        }

        // Abort handling: mark any pending tool calls as error when cancelled.
        // Mirrors TS processor.ts lines 393-409 where incomplete tool parts
        // are set to error status with "Tool execution aborted".
        if token.is_cancelled() {
            Self::abort_pending_tool_calls(session);
        }

        Self::prune_after_loop(session);
        session.touch();
        Self::emit_session_update(prompt_ctx.hooks.update_hook.as_ref(), session);

        Ok(())
    }

    fn emit_session_update(update_hook: Option<&SessionUpdateHook>, session: &Session) {
        if let Some(hook) = update_hook {
            hook(session);
        }
    }

    fn maybe_emit_session_update(
        update_hook: Option<&SessionUpdateHook>,
        session: &Session,
        last_emit: &mut Instant,
        force: bool,
    ) {
        let elapsed = last_emit.elapsed();
        if force || elapsed >= Duration::from_millis(STREAM_UPDATE_INTERVAL_MS) {
            Self::emit_session_update(update_hook, session);
            *last_emit = Instant::now();
        }
    }

    fn pending_subtasks_metadata_values(
        metadata: &HashMap<String, serde_json::Value>,
    ) -> Vec<serde_json::Value> {
        #[derive(Debug, Default, Deserialize)]
        struct PendingSubtasksWire {
            #[serde(default)]
            pending_subtasks: Vec<serde_json::Value>,
        }

        let Ok(value) = serde_json::to_value(metadata) else {
            return Vec::new();
        };
        let wire = serde_json::from_value::<PendingSubtasksWire>(value).unwrap_or_default();
        wire.pending_subtasks
    }

    fn collect_pending_subtasks(message: &SessionMessage) -> Vec<PendingSubtask> {
        #[derive(Debug, Deserialize, Default)]
        struct PendingSubtaskMetadataWire {
            #[serde(default)]
            id: String,
            #[serde(default)]
            agent: Option<String>,
            #[serde(default)]
            prompt: Option<String>,
            #[serde(default)]
            description: Option<String>,
        }

        let pending = Self::pending_subtasks_metadata_values(&message.metadata);

        let metadata_by_id: HashMap<String, (String, String, String)> =
            serde_json::from_value::<Vec<PendingSubtaskMetadataWire>>(serde_json::Value::Array(
                pending,
            ))
            .ok()
            .map(|items| {
                items
                    .into_iter()
                    .filter_map(|item| {
                        if item.id.trim().is_empty() {
                            return None;
                        }
                        let agent = item
                            .agent
                            .unwrap_or_else(|| "general".to_string())
                            .trim()
                            .to_string();
                        let prompt = item.prompt.unwrap_or_default();
                        let description = item.description.unwrap_or_default();
                        Some((item.id, (agent, prompt, description)))
                    })
                    .collect()
            })
            .unwrap_or_default();

        message
            .parts
            .iter()
            .enumerate()
            .filter_map(|(part_index, part)| match &part.part_type {
                PartType::Subtask {
                    id,
                    description,
                    status,
                } if status == "pending" => {
                    let (agent, prompt, meta_description) = metadata_by_id
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| (id.clone(), description.clone(), description.clone()));
                    let description = if meta_description.is_empty() {
                        description.clone()
                    } else {
                        meta_description
                    };
                    let prompt = if prompt.trim().is_empty() {
                        description.clone()
                    } else {
                        prompt
                    };
                    Some(PendingSubtask {
                        part_index,
                        subtask_id: id.clone(),
                        agent,
                        prompt,
                        description,
                    })
                }
                _ => None,
            })
            .collect()
    }

    async fn process_pending_subtasks(
        &self,
        session: &mut Session,
        provider: Arc<dyn Provider>,
        provider_id: &str,
        model_id: &str,
        hooks: &PromptHooks,
    ) -> anyhow::Result<bool> {
        let last_user_idx = session
            .messages
            .iter()
            .rposition(|m| matches!(m.role, Role::User));
        let Some(last_user_idx) = last_user_idx else {
            return Ok(false);
        };

        let pending = Self::collect_pending_subtasks(&session.messages[last_user_idx]);
        if pending.is_empty() {
            return Ok(false);
        }

        let mut results: Vec<(usize, String, bool, String, String)> = Vec::new();
        let tool_registry = Arc::new(rocode_tool::create_default_registry().await);
        let mut persisted = Self::load_persisted_subsessions(session);
        let default_model = format!("{}:{}", provider_id, model_id);
        let user_text = session.messages[last_user_idx].get_text();

        for subtask in &pending {
            let combined_prompt = if user_text.trim().is_empty() {
                subtask.prompt.clone()
            } else {
                format!("{}\n\nSubtask: {}", user_text, subtask.prompt)
            };
            let subsession_id = format!("task_subtask_{}", subtask.subtask_id);
            persisted
                .entry(subsession_id.clone())
                .or_insert_with(|| PersistedSubsession {
                    agent: subtask.agent.clone(),
                    model: Some(default_model.clone()),
                    directory: Some(session.directory.clone()),
                    disabled_tools: Vec::new(),
                    history: Vec::new(),
                });
            let state_snapshot =
                persisted
                    .get(&subsession_id)
                    .cloned()
                    .unwrap_or(PersistedSubsession {
                        agent: subtask.agent.clone(),
                        model: Some(default_model.clone()),
                        directory: Some(session.directory.clone()),
                        disabled_tools: Vec::new(),
                        history: Vec::new(),
                    });

            match Self::execute_persisted_subsession_prompt(
                &state_snapshot,
                &combined_prompt,
                provider.clone(),
                tool_registry.clone(),
                tool_execution::PersistedSubsessionPromptOptions {
                    default_model: default_model.clone(),
                    fallback_directory: Some(session.directory.clone()),
                    hooks: PromptHooks {
                        agent_lookup: hooks.agent_lookup.clone(),
                        ask_question_hook: hooks.ask_question_hook.clone(),
                        ask_permission_hook: hooks.ask_permission_hook.clone(),
                        ..Default::default()
                    },
                    question_session_id: Some(session.id.clone()),
                    abort: None,
                    tool_runtime_config: self.tool_runtime_config.clone(),
                },
            )
            .await
            {
                Ok(output) => {
                    if let Some(existing) = persisted.get_mut(&subsession_id) {
                        existing.history.push(PersistedSubsessionTurn {
                            prompt: combined_prompt,
                            output: output.clone(),
                        });
                    }
                    results.push((
                        subtask.part_index,
                        subtask.subtask_id.clone(),
                        false,
                        subtask.description.clone(),
                        output,
                    ));
                }
                Err(error) => {
                    results.push((
                        subtask.part_index,
                        subtask.subtask_id.clone(),
                        true,
                        subtask.description.clone(),
                        error.to_string(),
                    ));
                }
            }
        }

        for (part_index, subtask_id, is_error, description, output) in results {
            if let Some(part) = session.messages[last_user_idx].parts.get_mut(part_index) {
                if let PartType::Subtask { status, .. } = &mut part.part_type {
                    *status = if is_error {
                        "error".to_string()
                    } else {
                        "completed".to_string()
                    };
                }
            }

            let assistant = session.add_assistant_message();
            assistant
                .metadata
                .insert("subtask_id".to_string(), serde_json::json!(subtask_id));
            assistant.metadata.insert(
                "subtask_status".to_string(),
                serde_json::json!(if is_error { "error" } else { "completed" }),
            );
            assistant.add_text(format!(
                "Subtask `{}` {}:\n{}",
                description,
                if is_error { "failed" } else { "completed" },
                output
            ));
        }

        Self::save_persisted_subsessions(session, &persisted);

        Ok(true)
    }
}

impl Default for SessionPrompt {
    fn default() -> Self {
        Self::new(Arc::new(RwLock::new(SessionStateManager::new())))
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum PromptError {
    #[error("Session is busy: {0}")]
    Busy(String),
    #[error("No user message found")]
    NoUserMessage,
    #[error("Provider error: {0}")]
    Provider(String),
    #[error("Cancelled")]
    Cancelled,
}

/// Regex that matches `@reference` patterns. We use a capturing group for the
/// preceding character instead of a lookbehind (unsupported by the `regex` crate).
/// Group 1 = preceding char (or empty at start of string), Group 2 = the reference name.
const FILE_REFERENCE_REGEX: &str = r"(?:^|([^\w`]))@(\.?[^\s`,.]*(?:\.[^\s`,.]+)*)";

pub async fn resolve_prompt_parts(
    template: &str,
    worktree: &std::path::Path,
    known_agents: &[String],
) -> Vec<PartInput> {
    let mut parts = vec![PartInput::Text {
        text: template.to_string(),
    }];

    let re = regex::Regex::new(FILE_REFERENCE_REGEX).unwrap();
    let mut seen = std::collections::HashSet::new();

    for cap in re.captures_iter(template) {
        // Group 1 is the preceding char — if it matched a word char or backtick
        // the overall pattern wouldn't match (they're excluded by [^\w`]).
        // Group 2 is the actual reference name.
        if let Some(name) = cap.get(2) {
            let name = name.as_str();
            if name.is_empty() || seen.contains(name) {
                continue;
            }
            seen.insert(name.to_string());

            let filepath = if let Some(stripped) = name.strip_prefix("~/") {
                if let Some(home) = dirs::home_dir() {
                    home.join(stripped)
                } else {
                    continue;
                }
            } else {
                worktree.join(name)
            };

            if let Ok(metadata) = tokio::fs::metadata(&filepath).await {
                let url = format!("file://{}", filepath.display());

                if metadata.is_dir() {
                    parts.push(PartInput::File {
                        url,
                        filename: Some(name.to_string()),
                        mime: Some("application/x-directory".to_string()),
                    });
                } else {
                    parts.push(PartInput::File {
                        url,
                        filename: Some(name.to_string()),
                        mime: Some("text/plain".to_string()),
                    });
                }
            } else if known_agents.iter().any(|a| a == name) {
                // Not a file — check if it's a known agent name
                parts.push(PartInput::Agent {
                    name: name.to_string(),
                });
            }
        }
    }

    parts
}

pub fn extract_file_references(template: &str) -> Vec<String> {
    let re = regex::Regex::new(FILE_REFERENCE_REGEX).unwrap();
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();

    for cap in re.captures_iter(template) {
        if let Some(name) = cap.get(2) {
            let name = name.as_str().to_string();
            if !name.is_empty() && !seen.contains(&name) {
                seen.insert(name.clone());
                result.push(name);
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::MessagePart;
    use async_trait::async_trait;
    use futures::stream;
    use rocode_provider::{
        ChatRequest, ChatResponse, ModelInfo, ProviderError, StreamEvent, StreamResult, StreamUsage,
    };
    use rocode_tool::{Tool, ToolContext, ToolError, ToolResult};
    use std::sync::Mutex as StdMutex;

    struct StaticModelProvider {
        model: Option<ModelInfo>,
    }

    impl StaticModelProvider {
        fn with_model(model_id: &str, context_window: u64, max_output_tokens: u64) -> Self {
            Self {
                model: Some(ModelInfo {
                    id: model_id.to_string(),
                    name: "Static Model".to_string(),
                    provider: "mock".to_string(),
                    context_window,
                    max_input_tokens: None,
                    max_output_tokens,
                    supports_vision: false,
                    supports_tools: false,
                    cost_per_million_input: 0.0,
                    cost_per_million_output: 0.0,
                }),
            }
        }
    }

    #[async_trait]
    impl Provider for StaticModelProvider {
        fn id(&self) -> &str {
            "mock"
        }

        fn name(&self) -> &str {
            "Mock"
        }

        fn models(&self) -> Vec<ModelInfo> {
            self.model.clone().into_iter().collect()
        }

        fn get_model(&self, id: &str) -> Option<&ModelInfo> {
            self.model.as_ref().filter(|model| model.id == id)
        }

        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ProviderError> {
            Err(ProviderError::InvalidRequest(
                "chat() not used in this test".to_string(),
            ))
        }

        async fn chat_stream(&self, _request: ChatRequest) -> Result<StreamResult, ProviderError> {
            Ok(Box::pin(stream::empty()))
        }
    }

    struct ScriptedStreamProvider {
        model: ModelInfo,
        events: Vec<StreamEvent>,
    }

    #[async_trait]
    impl Provider for ScriptedStreamProvider {
        fn id(&self) -> &str {
            "mock"
        }

        fn name(&self) -> &str {
            "Mock"
        }

        fn models(&self) -> Vec<ModelInfo> {
            vec![self.model.clone()]
        }

        fn get_model(&self, id: &str) -> Option<&ModelInfo> {
            if self.model.id == id {
                Some(&self.model)
            } else {
                None
            }
        }

        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ProviderError> {
            Err(ProviderError::InvalidRequest(
                "chat() not used in this test".to_string(),
            ))
        }

        async fn chat_stream(&self, _request: ChatRequest) -> Result<StreamResult, ProviderError> {
            Ok(Box::pin(stream::iter(
                self.events
                    .clone()
                    .into_iter()
                    .map(Result::<StreamEvent, ProviderError>::Ok),
            )))
        }
    }

    struct MultiTurnScriptedProvider {
        model: ModelInfo,
        turns: Arc<StdMutex<std::collections::VecDeque<Vec<StreamEvent>>>>,
        request_count: Arc<StdMutex<usize>>,
    }

    impl MultiTurnScriptedProvider {
        fn new(model: ModelInfo, turns: Vec<Vec<StreamEvent>>) -> Self {
            Self {
                model,
                turns: Arc::new(StdMutex::new(turns.into())),
                request_count: Arc::new(StdMutex::new(0)),
            }
        }
    }

    #[async_trait]
    impl Provider for MultiTurnScriptedProvider {
        fn id(&self) -> &str {
            "mock"
        }

        fn name(&self) -> &str {
            "Mock"
        }

        fn models(&self) -> Vec<ModelInfo> {
            vec![self.model.clone()]
        }

        fn get_model(&self, id: &str) -> Option<&ModelInfo> {
            if self.model.id == id {
                Some(&self.model)
            } else {
                None
            }
        }

        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ProviderError> {
            Err(ProviderError::InvalidRequest(
                "chat() not used in this test".to_string(),
            ))
        }

        async fn chat_stream(&self, _request: ChatRequest) -> Result<StreamResult, ProviderError> {
            {
                let mut count = self
                    .request_count
                    .lock()
                    .expect("request_count lock should not poison");
                *count += 1;
            }

            let events = self
                .turns
                .lock()
                .expect("turns lock should not poison")
                .pop_front()
                .ok_or_else(|| {
                    ProviderError::InvalidRequest(
                        "no scripted response left for chat_stream".to_string(),
                    )
                })?;

            Ok(Box::pin(stream::iter(
                events
                    .into_iter()
                    .map(Result::<StreamEvent, ProviderError>::Ok),
            )))
        }
    }

    struct NoArgEchoTool;

    #[async_trait]
    impl Tool for NoArgEchoTool {
        fn id(&self) -> &str {
            "noarg_echo"
        }

        fn description(&self) -> &str {
            "Echoes input for tests"
        }

        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {}
            })
        }

        async fn execute(
            &self,
            args: serde_json::Value,
            _ctx: ToolContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::simple("NoArg Echo", args.to_string()))
        }
    }

    struct AlwaysInvalidArgsTool;

    #[async_trait]
    impl Tool for AlwaysInvalidArgsTool {
        fn id(&self) -> &str {
            "needs_path"
        }

        fn description(&self) -> &str {
            "Fails validation for tests"
        }

        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "filePath": { "type": "string" }
                },
                "required": ["filePath"]
            })
        }

        fn validate(&self, _args: &serde_json::Value) -> Result<(), ToolError> {
            Err(ToolError::InvalidArguments(
                "filePath is required".to_string(),
            ))
        }

        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: ToolContext,
        ) -> Result<ToolResult, ToolError> {
            Err(ToolError::ExecutionError(
                "validate should prevent execute".to_string(),
            ))
        }
    }
    #[test]
    fn insert_reminders_adds_plan_prompt_for_plan_agent() {
        let messages = vec![SessionMessage::user("ses_test", "plan this")];
        let output = insert_reminders(&messages, "plan", false);
        let last = output.last().unwrap();
        let injected = last
            .parts
            .iter()
            .filter_map(|p| match &p.part_type {
                PartType::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(injected.contains("You are in PLAN mode"));
    }

    #[test]
    fn insert_reminders_adds_build_switch_after_plan() {
        let mut user = SessionMessage::user("ses_test", "execute this");
        user.metadata
            .insert("agent".to_string(), serde_json::json!("plan"));
        let output = insert_reminders(&[user], "build", true);
        let last = output.last().unwrap();
        let injected = last
            .parts
            .iter()
            .filter_map(|p| match &p.part_type {
                PartType::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(injected.contains("The user has approved your plan"));
    }

    #[tokio::test]
    async fn prompt_with_update_hook_emits_incremental_snapshots() {
        let prompt = SessionPrompt::default();
        let mut session = Session::new(".");
        let provider = Arc::new(ScriptedStreamProvider {
            model: ModelInfo {
                id: "test-model".to_string(),
                name: "Test Model".to_string(),
                provider: "mock".to_string(),
                context_window: 8192,
                max_input_tokens: None,
                max_output_tokens: 1024,
                supports_vision: false,
                supports_tools: false,
                cost_per_million_input: 0.0,
                cost_per_million_output: 0.0,
            },
            events: vec![
                StreamEvent::Start,
                StreamEvent::TextDelta("Hel".to_string()),
                StreamEvent::TextDelta("lo".to_string()),
                StreamEvent::FinishStep {
                    finish_reason: Some("stop".to_string()),
                    usage: StreamUsage {
                        prompt_tokens: 3,
                        completion_tokens: 2,
                        ..Default::default()
                    },
                    provider_metadata: None,
                },
                StreamEvent::Done,
            ],
        });

        let snapshots = Arc::new(StdMutex::new(Vec::<Session>::new()));
        let snapshot_sink = snapshots.clone();
        let hook: SessionUpdateHook = Arc::new(move |snapshot| {
            snapshot_sink
                .lock()
                .expect("snapshot lock should not poison")
                .push(snapshot.clone());
        });

        let input = PromptInput {
            session_id: session.id.clone(),
            message_id: None,
            model: Some(ModelRef {
                provider_id: "mock".to_string(),
                model_id: "test-model".to_string(),
            }),
            agent: None,
            no_reply: false,
            system: None,
            variant: None,
            parts: vec![PartInput::Text {
                text: "Say hello".to_string(),
            }],
            tools: None,
        };

        prompt
            .prompt_with_update_hook(
                input,
                &mut session,
                PromptRequestContext {
                    provider,
                    system_prompt: None,
                    tools: Vec::new(),
                    compiled_request: CompiledExecutionRequest::default(),
                    hooks: PromptHooks {
                        update_hook: Some(hook),
                        ..Default::default()
                    },
                },
            )
            .await
            .expect("prompt_with_update_hook should succeed");

        let snapshots_guard = snapshots.lock().expect("snapshot lock should not poison");
        assert!(snapshots_guard.len() >= 3);
        let saw_partial = snapshots_guard.iter().any(|snap| {
            snap.messages
                .iter()
                .rev()
                .find(|m| matches!(m.role, Role::Assistant))
                .map(|m| m.get_text() == "Hel")
                .unwrap_or(false)
        });
        assert!(
            saw_partial,
            "expected at least one streamed partial assistant snapshot"
        );
        drop(snapshots_guard);

        let final_text = session
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, Role::Assistant))
            .map(SessionMessage::get_text)
            .unwrap_or_default();
        assert_eq!(final_text, "Hello");
    }

    #[tokio::test]
    async fn prompt_with_output_block_hook_emits_realtime_blocks() {
        let prompt = SessionPrompt::default();
        let mut session = Session::new(".");
        let provider = Arc::new(ScriptedStreamProvider {
            model: ModelInfo {
                id: "test-model".to_string(),
                name: "Test Model".to_string(),
                provider: "mock".to_string(),
                context_window: 8192,
                max_input_tokens: None,
                max_output_tokens: 1024,
                supports_vision: false,
                supports_tools: false,
                cost_per_million_input: 0.0,
                cost_per_million_output: 0.0,
            },
            events: vec![
                StreamEvent::Start,
                StreamEvent::ReasoningDelta {
                    id: "reasoning-1".to_string(),
                    text: "thinking".to_string(),
                },
                StreamEvent::TextDelta("Hello".to_string()),
                StreamEvent::FinishStep {
                    finish_reason: Some("stop".to_string()),
                    usage: StreamUsage::default(),
                    provider_metadata: None,
                },
                StreamEvent::Done,
            ],
        });

        let emitted = Arc::new(StdMutex::new(Vec::<OutputBlockEvent>::new()));
        let emitted_sink = emitted.clone();
        let hook: OutputBlockHook = Arc::new(move |event| {
            let emitted_sink = emitted_sink.clone();
            Box::pin(async move {
                emitted_sink
                    .lock()
                    .expect("output block lock should not poison")
                    .push(event);
            })
        });

        let input = PromptInput {
            session_id: session.id.clone(),
            message_id: None,
            model: Some(ModelRef {
                provider_id: "mock".to_string(),
                model_id: "test-model".to_string(),
            }),
            agent: None,
            no_reply: false,
            system: None,
            variant: None,
            parts: vec![PartInput::Text {
                text: "Say hello".to_string(),
            }],
            tools: None,
        };

        prompt
            .prompt_with_update_hook(
                input,
                &mut session,
                PromptRequestContext {
                    provider,
                    system_prompt: None,
                    tools: Vec::new(),
                    compiled_request: CompiledExecutionRequest::default(),
                    hooks: PromptHooks {
                        output_block_hook: Some(hook),
                        ..Default::default()
                    },
                },
            )
            .await
            .expect("prompt_with_update_hook should succeed");

        use rocode_content::output_blocks::{MessagePhase, OutputBlock};

        let emitted = emitted.lock().expect("output block lock should not poison");
        assert!(emitted.iter().all(|event| event.session_id == session.id));
        let blocks = emitted
            .iter()
            .map(|event| event.block.clone())
            .collect::<Vec<_>>();

        assert!(matches!(
            blocks.as_slice(),
            [
                OutputBlock::Message(message_start),
                OutputBlock::Reasoning(reasoning_start),
                OutputBlock::Reasoning(reasoning_delta),
                OutputBlock::Message(message_delta),
                OutputBlock::Reasoning(reasoning_end),
                OutputBlock::Message(message_end),
            ] if message_start.phase == MessagePhase::Start
                && reasoning_start.phase == MessagePhase::Start
                && reasoning_delta.phase == MessagePhase::Delta
                && reasoning_delta.text == "thinking"
                && message_delta.phase == MessagePhase::Delta
                && message_delta.text == "Hello"
                && reasoning_end.phase == MessagePhase::End
                && message_end.phase == MessagePhase::End
        ));
    }

    #[tokio::test]
    async fn prompt_merges_split_usage_snapshots_within_a_step() {
        let prompt = SessionPrompt::default();
        let mut session = Session::new(".");
        let provider = Arc::new(ScriptedStreamProvider {
            model: ModelInfo {
                id: "test-model".to_string(),
                name: "Test Model".to_string(),
                provider: "mock".to_string(),
                context_window: 8192,
                max_input_tokens: None,
                max_output_tokens: 1024,
                supports_vision: false,
                supports_tools: false,
                cost_per_million_input: 0.0,
                cost_per_million_output: 0.0,
            },
            events: vec![
                StreamEvent::Start,
                StreamEvent::Usage {
                    prompt_tokens: 3,
                    completion_tokens: 0,
                },
                StreamEvent::TextDelta("Hi".to_string()),
                StreamEvent::FinishStep {
                    finish_reason: Some("stop".to_string()),
                    usage: StreamUsage {
                        prompt_tokens: 0,
                        completion_tokens: 2,
                        ..Default::default()
                    },
                    provider_metadata: None,
                },
                StreamEvent::Done,
            ],
        });

        let input = PromptInput {
            session_id: session.id.clone(),
            message_id: None,
            model: Some(ModelRef {
                provider_id: "mock".to_string(),
                model_id: "test-model".to_string(),
            }),
            agent: None,
            no_reply: false,
            system: None,
            variant: None,
            parts: vec![PartInput::Text {
                text: "Say hi".to_string(),
            }],
            tools: None,
        };

        prompt
            .prompt_with_update_hook(
                input,
                &mut session,
                PromptRequestContext {
                    provider,
                    system_prompt: None,
                    tools: Vec::new(),
                    compiled_request: CompiledExecutionRequest::default(),
                    hooks: PromptHooks::default(),
                },
            )
            .await
            .expect("prompt_with_update_hook should succeed");

        let assistant = session
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, Role::Assistant))
            .expect("assistant should exist");
        let usage = assistant.usage.as_ref().expect("usage should exist");
        assert_eq!(usage.input_tokens, 3);
        assert_eq!(usage.output_tokens, 2);
    }

    #[tokio::test]
    async fn prompt_continues_after_tool_calls_without_finish_step_reason() {
        let prompt = SessionPrompt::default();
        let mut session = Session::new(".");
        let temp_dir = tempfile::tempdir().expect("tempdir should create");
        let file_path = temp_dir.path().join("sample.txt");
        tokio::fs::write(&file_path, "alpha\nbeta")
            .await
            .expect("file should write");
        let file_path = file_path.to_string_lossy().to_string();

        let scripted = MultiTurnScriptedProvider::new(
            ModelInfo {
                id: "test-model".to_string(),
                name: "Test Model".to_string(),
                provider: "mock".to_string(),
                context_window: 8192,
                max_input_tokens: None,
                max_output_tokens: 1024,
                supports_vision: false,
                supports_tools: true,
                cost_per_million_input: 0.0,
                cost_per_million_output: 0.0,
            },
            vec![
                vec![
                    StreamEvent::Start,
                    StreamEvent::ToolCallStart {
                        id: "tool-call-0".to_string(),
                        name: "read".to_string(),
                    },
                    StreamEvent::ToolCallEnd {
                        id: "tool-call-0".to_string(),
                        name: "read".to_string(),
                        input: serde_json::json!({ "file_path": file_path }),
                    },
                    StreamEvent::Done,
                ],
                vec![
                    StreamEvent::Start,
                    StreamEvent::TextDelta("Read complete".to_string()),
                    StreamEvent::FinishStep {
                        finish_reason: Some("stop".to_string()),
                        usage: StreamUsage::default(),
                        provider_metadata: None,
                    },
                    StreamEvent::Done,
                ],
            ],
        );
        let request_count = scripted.request_count.clone();
        let provider: Arc<dyn Provider> = Arc::new(scripted);

        let input = PromptInput {
            session_id: session.id.clone(),
            message_id: None,
            model: Some(ModelRef {
                provider_id: "mock".to_string(),
                model_id: "test-model".to_string(),
            }),
            agent: None,
            no_reply: false,
            system: None,
            variant: None,
            parts: vec![PartInput::Text {
                text: "Read the file and summarize".to_string(),
            }],
            tools: None,
        };

        prompt
            .prompt_with_update_hook(
                input,
                &mut session,
                PromptRequestContext {
                    provider,
                    system_prompt: None,
                    tools: Vec::new(),
                    compiled_request: CompiledExecutionRequest::default(),
                    hooks: PromptHooks::default(),
                },
            )
            .await
            .expect("prompt_with_update_hook should succeed");

        let request_count = *request_count
            .lock()
            .expect("request_count lock should not poison");
        assert_eq!(request_count, 2, "expected a second model round");

        let final_text = session
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, Role::Assistant))
            .map(SessionMessage::get_text)
            .unwrap_or_default();
        assert_eq!(final_text, "Read complete");
    }

    #[tokio::test]
    async fn create_user_message_for_agent_prefers_task_flow_instruction() {
        let prompt = SessionPrompt::default();
        let mut session = Session::new(".");
        let input = PromptInput {
            session_id: session.id.clone(),
            message_id: None,
            model: None,
            agent: None,
            no_reply: false,
            system: None,
            variant: None,
            tools: None,
            parts: vec![PartInput::Agent {
                name: "explore".to_string(),
            }],
        };

        prompt
            .create_user_message(&input, &mut session)
            .await
            .expect("create_user_message should succeed");

        let msg = session.messages.last().expect("user message should exist");
        let final_text = msg.get_text();
        assert!(final_text.contains("prefer calling task_flow"));
        assert!(final_text.contains("operation=create"));
        assert!(final_text.contains("agent=\"explore\""));
        assert!(final_text.contains("fall back to the task tool"));
        assert!(msg.parts.iter().any(|p| match &p.part_type {
            PartType::Agent { name, .. } => name == "explore",
            _ => false,
        }));
    }

    #[tokio::test]
    async fn create_user_message_persists_pending_subtask_payload() {
        let prompt = SessionPrompt::default();
        let mut session = Session::new(".");
        let input = PromptInput {
            session_id: session.id.clone(),
            message_id: None,
            model: None,
            agent: None,
            no_reply: false,
            system: None,
            variant: None,
            tools: None,
            parts: vec![PartInput::Subtask {
                prompt: "Inspect codegen path".to_string(),
                description: Some("Inspect codegen".to_string()),
                agent: "explore".to_string(),
            }],
        };

        prompt
            .create_user_message(&input, &mut session)
            .await
            .expect("create_user_message should succeed");

        let msg = session.messages.last().expect("user message should exist");
        let pending = msg
            .metadata
            .get("pending_subtasks")
            .and_then(|v| v.as_array())
            .expect("pending_subtasks metadata should exist");
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0].get("agent").and_then(|v| v.as_str()),
            Some("explore")
        );
        assert_eq!(
            pending[0].get("prompt").and_then(|v| v.as_str()),
            Some("Inspect codegen path")
        );
        assert!(msg.parts.iter().any(|p| match &p.part_type {
            PartType::Subtask { status, .. } => status == "pending",
            _ => false,
        }));
    }
    #[test]
    fn shell_exec_uses_zsh_login_invocation() {
        let invocation = resolve_shell_invocation(Some("/bin/zsh"), "echo hello");
        assert_eq!(invocation.program, "/bin/zsh");
        assert_eq!(invocation.args[0], "-c");
        assert_eq!(invocation.args[1], "-l");
        assert!(invocation.args[2].contains(".zshenv"));
        assert!(invocation.args[2].contains("eval"));
    }

    #[test]
    fn shell_exec_uses_bash_login_invocation() {
        let invocation = resolve_shell_invocation(Some("/bin/bash"), "echo hello");
        assert_eq!(invocation.program, "/bin/bash");
        assert_eq!(invocation.args[0], "-c");
        assert_eq!(invocation.args[1], "-l");
        assert!(invocation.args[2].contains("shopt -s expand_aliases"));
        assert!(invocation.args[2].contains(".bashrc"));
    }

    #[tokio::test]
    async fn resolve_tools_with_mcp_registry_includes_mcp_tools() {
        let tool_registry = rocode_tool::create_default_registry().await;
        let mcp_registry = rocode_mcp::McpToolRegistry::new();
        mcp_registry
            .register(rocode_mcp::McpTool::new(
                "github",
                "search",
                Some("Search GitHub".to_string()),
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    }
                }),
            ))
            .await;

        let tools = resolve_tools_with_mcp_registry(&tool_registry, Some(&mcp_registry)).await;
        assert!(tools.iter().any(|t| t.name == "github_search"));
    }

    #[tokio::test]
    async fn execute_tool_calls_ignores_empty_tool_name() {
        let tool_registry = Arc::new(rocode_tool::ToolRegistry::new());
        tool_registry.register(NoArgEchoTool).await;

        let mut session = Session::new(".");
        let sid = session.id.clone();
        session
            .messages
            .push(SessionMessage::user(sid.clone(), "run tools"));

        let mut assistant = SessionMessage::assistant(sid);
        assistant.parts.push(crate::MessagePart {
            id: format!("prt_{}", uuid::Uuid::new_v4()),
            part_type: PartType::ToolCall {
                id: "call_empty".to_string(),
                name: " ".to_string(),
                input: serde_json::json!({}),
                status: crate::ToolCallStatus::Running,
                raw: None,
                state: None,
            },
            created_at: chrono::Utc::now(),
            message_id: None,
        });
        assistant.add_tool_call("call_ok", "noarg_echo", serde_json::json!({}));
        session.messages.push(assistant);

        let provider: Arc<dyn Provider> =
            Arc::new(StaticModelProvider::with_model("test-model", 8192, 1024));
        let ctx = ToolContext::new(session.id.clone(), "msg_test".to_string(), ".".to_string());

        SessionPrompt::execute_tool_calls(
            &mut session,
            tool_registry,
            ctx,
            provider,
            "mock",
            "test-model",
        )
        .await
        .expect("execute_tool_calls should succeed");

        let tool_msg = session
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, Role::Tool))
            .expect("tool message should exist");
        let result_ids: Vec<&str> = tool_msg
            .parts
            .iter()
            .filter_map(|part| match &part.part_type {
                PartType::ToolResult { tool_call_id, .. } => Some(tool_call_id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(result_ids, vec!["call_ok"]);
    }

    #[tokio::test]
    async fn execute_tool_calls_runs_no_arg_tool() {
        let tool_registry = Arc::new(rocode_tool::ToolRegistry::new());
        tool_registry.register(NoArgEchoTool).await;

        let mut session = Session::new(".");
        let sid = session.id.clone();
        session
            .messages
            .push(SessionMessage::user(sid.clone(), "run noarg"));
        let mut assistant = SessionMessage::assistant(sid);
        assistant.add_tool_call("call_noarg", "noarg_echo", serde_json::json!({}));
        session.messages.push(assistant);

        let provider: Arc<dyn Provider> =
            Arc::new(StaticModelProvider::with_model("test-model", 8192, 1024));
        let ctx = ToolContext::new(session.id.clone(), "msg_test".to_string(), ".".to_string());

        SessionPrompt::execute_tool_calls(
            &mut session,
            tool_registry,
            ctx,
            provider,
            "mock",
            "test-model",
        )
        .await
        .expect("execute_tool_calls should succeed");

        let tool_msg = session
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, Role::Tool))
            .expect("tool message should exist");

        let (content, is_error) = tool_msg
            .parts
            .iter()
            .find_map(|part| match &part.part_type {
                PartType::ToolResult {
                    tool_call_id,
                    content,
                    is_error,
                    ..
                } if tool_call_id == "call_noarg" => Some((content.clone(), *is_error)),
                _ => None,
            })
            .expect("noarg result should exist");

        assert!(!is_error);
        assert_eq!(content, "{}");
    }

    #[tokio::test]
    async fn execute_tool_calls_routes_invalid_arguments_to_invalid_tool() {
        let tool_registry = Arc::new(rocode_tool::ToolRegistry::new());
        tool_registry.register(AlwaysInvalidArgsTool).await;
        tool_registry
            .register(rocode_tool::invalid::InvalidTool)
            .await;

        let mut session = Session::new(".");
        let sid = session.id.clone();
        session
            .messages
            .push(SessionMessage::user(sid.clone(), "run invalid"));
        let mut assistant = SessionMessage::assistant(sid);
        assistant.add_tool_call("call_invalid", "needs_path", serde_json::json!({}));
        session.messages.push(assistant);

        let provider: Arc<dyn Provider> =
            Arc::new(StaticModelProvider::with_model("test-model", 8192, 1024));
        let ctx = ToolContext::new(session.id.clone(), "msg_test".to_string(), ".".to_string());

        SessionPrompt::execute_tool_calls(
            &mut session,
            tool_registry,
            ctx,
            provider,
            "mock",
            "test-model",
        )
        .await
        .expect("execute_tool_calls should succeed");

        let assistant_msg = session
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, Role::Assistant))
            .expect("assistant message should exist");
        let tool_call = assistant_msg
            .parts
            .iter()
            .find_map(|part| match &part.part_type {
                PartType::ToolCall {
                    id,
                    name,
                    input,
                    status,
                    ..
                } if id == "call_invalid" => Some((name, input, status)),
                _ => None,
            })
            .expect("tool call should exist");
        assert_eq!(tool_call.0, "invalid");
        assert_eq!(
            tool_call.1.get("tool").and_then(|v| v.as_str()),
            Some("needs_path")
        );
        assert!(tool_call.1.get("receivedArgs").is_none());
        assert!(matches!(tool_call.2, crate::ToolCallStatus::Completed));

        let tool_msg = session
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, Role::Tool))
            .expect("tool message should exist");
        let (content, is_error) = tool_msg
            .parts
            .iter()
            .find_map(|part| match &part.part_type {
                PartType::ToolResult {
                    tool_call_id,
                    content,
                    is_error,
                    ..
                } if tool_call_id == "call_invalid" => Some((content.clone(), *is_error)),
                _ => None,
            })
            .expect("invalid fallback result should exist");
        assert!(!is_error);
        assert!(content.contains("The arguments provided to the tool are invalid:"));
    }

    #[tokio::test]
    async fn execute_tool_calls_only_runs_running_tool_calls() {
        let tool_registry = Arc::new(rocode_tool::ToolRegistry::new());
        tool_registry.register(NoArgEchoTool).await;

        let mut session = Session::new(".");
        let sid = session.id.clone();
        session
            .messages
            .push(SessionMessage::user(sid.clone(), "run running only"));
        let mut assistant = SessionMessage::assistant(sid);
        assistant.parts.push(crate::MessagePart {
            id: format!("prt_{}", uuid::Uuid::new_v4()),
            part_type: PartType::ToolCall {
                id: "call_pending".to_string(),
                name: "noarg_echo".to_string(),
                input: serde_json::json!({}),
                status: crate::ToolCallStatus::Pending,
                raw: Some("{".to_string()),
                state: Some(rocode_message::ToolState::Pending {
                    input: serde_json::json!({}),
                    raw: "{".to_string(),
                }),
            },
            created_at: chrono::Utc::now(),
            message_id: None,
        });
        assistant.add_tool_call("call_running", "noarg_echo", serde_json::json!({}));
        session.messages.push(assistant);

        let provider: Arc<dyn Provider> =
            Arc::new(StaticModelProvider::with_model("test-model", 8192, 1024));
        let ctx = ToolContext::new(session.id.clone(), "msg_test".to_string(), ".".to_string());

        SessionPrompt::execute_tool_calls(
            &mut session,
            tool_registry,
            ctx,
            provider,
            "mock",
            "test-model",
        )
        .await
        .expect("execute_tool_calls should succeed");

        let tool_msg = session
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, Role::Tool))
            .expect("tool message should exist");
        let result_ids: Vec<&str> = tool_msg
            .parts
            .iter()
            .filter_map(|part| match &part.part_type {
                PartType::ToolResult { tool_call_id, .. } => Some(tool_call_id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(result_ids, vec!["call_running"]);
    }

    #[tokio::test]
    async fn execute_tool_calls_reused_call_id_in_new_turn_still_executes() {
        let tool_registry = Arc::new(rocode_tool::ToolRegistry::new());
        tool_registry.register(NoArgEchoTool).await;

        let mut session = Session::new(".");
        let sid = session.id.clone();

        session
            .messages
            .push(SessionMessage::user(sid.clone(), "turn one"));
        let mut assistant_1 = SessionMessage::assistant(sid.clone());
        assistant_1.add_tool_call("tool-call-0", "noarg_echo", serde_json::json!({}));
        session.messages.push(assistant_1);
        let mut tool_msg_1 = SessionMessage::tool(sid.clone());
        tool_msg_1.add_tool_result("tool-call-0", "{}", false);
        session.messages.push(tool_msg_1);

        session
            .messages
            .push(SessionMessage::user(sid.clone(), "turn two"));
        let mut assistant_2 = SessionMessage::assistant(sid);
        assistant_2.add_tool_call("tool-call-0", "noarg_echo", serde_json::json!({}));
        session.messages.push(assistant_2);

        let provider: Arc<dyn Provider> =
            Arc::new(StaticModelProvider::with_model("test-model", 8192, 1024));
        let ctx = ToolContext::new(session.id.clone(), "msg_test".to_string(), ".".to_string());

        SessionPrompt::execute_tool_calls(
            &mut session,
            tool_registry,
            ctx,
            provider,
            "mock",
            "test-model",
        )
        .await
        .expect("execute_tool_calls should succeed");

        let tool_msgs: Vec<&SessionMessage> = session
            .messages
            .iter()
            .filter(|m| matches!(m.role, Role::Tool))
            .collect();
        assert!(
            tool_msgs.len() >= 2,
            "expected a second tool message for the new turn"
        );

        let last_tool_msg = tool_msgs.last().expect("latest tool message should exist");
        let second_turn_result_count = last_tool_msg
            .parts
            .iter()
            .filter(|part| {
                matches!(
                    &part.part_type,
                    PartType::ToolResult { tool_call_id, .. } if tool_call_id == "tool-call-0"
                )
            })
            .count();
        assert_eq!(second_turn_result_count, 1);
    }

    // ── PartInput serde round-trip tests ──

    #[test]
    fn part_input_text_round_trip() {
        let part = PartInput::Text {
            text: "hello".to_string(),
        };
        let json = serde_json::to_value(&part).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["text"], "hello");

        let back: PartInput = serde_json::from_value(json).unwrap();
        assert!(matches!(back, PartInput::Text { text } if text == "hello"));
    }

    #[test]
    fn part_input_file_round_trip() {
        let part = PartInput::File {
            url: "file:///tmp/test.rs".to_string(),
            filename: Some("test.rs".to_string()),
            mime: Some("text/plain".to_string()),
        };
        let json = serde_json::to_value(&part).unwrap();
        assert_eq!(json["type"], "file");
        assert_eq!(json["url"], "file:///tmp/test.rs");
        assert_eq!(json["filename"], "test.rs");

        let back: PartInput = serde_json::from_value(json).unwrap();
        assert!(matches!(back, PartInput::File { url, .. } if url == "file:///tmp/test.rs"));
    }

    #[test]
    fn part_input_agent_round_trip() {
        let part = PartInput::Agent {
            name: "explore".to_string(),
        };
        let json = serde_json::to_value(&part).unwrap();
        assert_eq!(json["type"], "agent");
        assert_eq!(json["name"], "explore");

        let back: PartInput = serde_json::from_value(json).unwrap();
        assert!(matches!(back, PartInput::Agent { name } if name == "explore"));
    }

    #[test]
    fn part_input_subtask_round_trip() {
        let part = PartInput::Subtask {
            prompt: "do stuff".to_string(),
            description: Some("stuff".to_string()),
            agent: "build".to_string(),
        };
        let json = serde_json::to_value(&part).unwrap();
        assert_eq!(json["type"], "subtask");
        assert_eq!(json["agent"], "build");

        let back: PartInput = serde_json::from_value(json).unwrap();
        assert!(matches!(back, PartInput::Subtask { agent, .. } if agent == "build"));
    }

    #[test]
    fn part_input_try_from_value() {
        let val = serde_json::json!({"type": "text", "text": "hi"});
        let part = PartInput::try_from(val).unwrap();
        assert!(matches!(part, PartInput::Text { text } if text == "hi"));
    }

    #[test]
    fn part_input_try_from_invalid_value() {
        let val = serde_json::json!({"type": "unknown", "data": 42});
        assert!(PartInput::try_from(val).is_err());
    }

    #[test]
    fn part_input_parse_array_mixed() {
        let arr = serde_json::json!([
            {"type": "text", "text": "hello"},
            {"type": "agent", "name": "explore"},
            {"type": "bogus"},
            {"type": "file", "url": "file:///x", "filename": "x", "mime": "text/plain"}
        ]);
        let parts = PartInput::parse_array(&arr);
        assert_eq!(parts.len(), 3); // bogus entry skipped
        assert!(matches!(&parts[0], PartInput::Text { text } if text == "hello"));
        assert!(matches!(&parts[1], PartInput::Agent { name } if name == "explore"));
        assert!(matches!(&parts[2], PartInput::File { url, .. } if url == "file:///x"));
    }

    #[test]
    fn part_input_parse_array_non_array() {
        let val = serde_json::json!("not an array");
        assert!(PartInput::parse_array(&val).is_empty());
    }

    #[test]
    fn part_input_file_skips_none_fields_in_json() {
        let part = PartInput::File {
            url: "file:///tmp/x".to_string(),
            filename: None,
            mime: None,
        };
        let json = serde_json::to_value(&part).unwrap();
        assert!(json.get("filename").is_none());
        assert!(json.get("mime").is_none());
    }

    // ── resolve_prompt_parts tests ──

    #[tokio::test]
    async fn resolve_prompt_parts_plain_text() {
        let parts =
            resolve_prompt_parts("just plain text", std::path::Path::new("/tmp"), &[]).await;
        assert_eq!(parts.len(), 1);
        assert!(matches!(&parts[0], PartInput::Text { text } if text == "just plain text"));
    }

    #[tokio::test]
    async fn resolve_prompt_parts_agent_fallback() {
        // @explore doesn't exist as a file, but is a known agent
        let agents = vec!["explore".to_string(), "build".to_string()];
        let parts = resolve_prompt_parts(
            "check @explore for details",
            std::path::Path::new("/tmp"),
            &agents,
        )
        .await;
        assert_eq!(parts.len(), 2);
        assert!(matches!(&parts[0], PartInput::Text { .. }));
        assert!(matches!(&parts[1], PartInput::Agent { name } if name == "explore"));
    }

    #[tokio::test]
    async fn resolve_prompt_parts_deduplicates() {
        let parts = resolve_prompt_parts(
            "see @explore and @explore again",
            std::path::Path::new("/tmp"),
            &["explore".to_string()],
        )
        .await;
        // text + one agent (deduplicated)
        assert_eq!(parts.len(), 2);
    }

    #[tokio::test]
    async fn resolve_prompt_parts_real_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.rs");
        tokio::fs::write(&file, "fn main() {}").await.unwrap();

        let parts = resolve_prompt_parts("look at @test.rs", dir.path(), &[]).await;
        assert_eq!(parts.len(), 2);
        assert!(
            matches!(&parts[1], PartInput::File { mime, .. } if mime.as_deref() == Some("text/plain"))
        );
    }

    #[tokio::test]
    async fn resolve_prompt_parts_directory() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("src");
        tokio::fs::create_dir(&sub).await.unwrap();

        let parts = resolve_prompt_parts("look at @src", dir.path(), &[]).await;
        assert_eq!(parts.len(), 2);
        assert!(
            matches!(&parts[1], PartInput::File { mime, .. } if mime.as_deref() == Some("application/x-directory"))
        );
    }

    /// Regression test for the prompt loop early-exit bug:
    /// When the assistant message has text + tool calls and finish="tool-calls",
    /// the loop must NOT break at the top-of-loop check.
    /// Previously, the check used `has_finish = !text.is_empty()` which caused
    /// premature exit when models emit text before tool calls.
    #[test]
    fn early_exit_does_not_break_on_tool_calls_finish() {
        // Simulate: user message at index 0, assistant at index 1
        let user = SessionMessage::user("s1", "hello");
        let mut assistant = SessionMessage::assistant("s1");
        // Assistant has text content (model explained before calling tools)
        assistant.parts.push(MessagePart {
            id: "prt_text".to_string(),
            part_type: PartType::Text {
                text: "Let me read those files for you.".to_string(),
                synthetic: None,
                ignored: None,
            },
            created_at: chrono::Utc::now(),
            message_id: None,
        });
        // finish_reason is "tool-calls" — loop should continue, not break
        assistant.finish = Some("tool-calls".to_string());

        let messages = [user, assistant];

        let last_user_idx = messages
            .iter()
            .rposition(|m| matches!(m.role, Role::User))
            .unwrap();
        let last_assistant_idx = messages
            .iter()
            .rposition(|m| matches!(m.role, Role::Assistant));

        // The early-exit check from the prompt loop
        let should_break = if let Some(assistant_idx) = last_assistant_idx {
            let assistant = &messages[assistant_idx];
            let is_terminal = assistant
                .finish
                .as_deref()
                .is_some_and(|f| !matches!(f, "tool-calls" | "tool_calls" | "unknown"));
            is_terminal && last_user_idx < assistant_idx
        } else {
            false
        };

        assert!(
            !should_break,
            "early-exit must NOT trigger when finish='tool-calls'"
        );
    }

    /// Verify that the early-exit check DOES break when finish is terminal
    /// (e.g. "stop") and assistant is after the last user message.
    #[test]
    fn early_exit_breaks_on_terminal_finish() {
        let user = SessionMessage::user("s1", "hello");
        let mut assistant = SessionMessage::assistant("s1");
        assistant.parts.push(MessagePart {
            id: "prt_text".to_string(),
            part_type: PartType::Text {
                text: "Here is my response.".to_string(),
                synthetic: None,
                ignored: None,
            },
            created_at: chrono::Utc::now(),
            message_id: None,
        });
        assistant.finish = Some("stop".to_string());

        let messages = [user, assistant];

        let last_user_idx = messages
            .iter()
            .rposition(|m| matches!(m.role, Role::User))
            .unwrap();
        let last_assistant_idx = messages
            .iter()
            .rposition(|m| matches!(m.role, Role::Assistant));

        let should_break = if let Some(assistant_idx) = last_assistant_idx {
            let assistant = &messages[assistant_idx];
            let is_terminal = assistant
                .finish
                .as_deref()
                .is_some_and(|f| !matches!(f, "tool-calls" | "tool_calls" | "unknown"));
            is_terminal && last_user_idx < assistant_idx
        } else {
            false
        };

        assert!(should_break, "early-exit MUST trigger when finish='stop'");
    }

    /// Verify that the early-exit check does NOT break when finish is None
    /// (assistant message still streaming / no FinishStep received yet).
    #[test]
    fn early_exit_does_not_break_when_finish_is_none() {
        let user = SessionMessage::user("s1", "hello");
        let mut assistant = SessionMessage::assistant("s1");
        assistant.parts.push(MessagePart {
            id: "prt_text".to_string(),
            part_type: PartType::Text {
                text: "partial response...".to_string(),
                synthetic: None,
                ignored: None,
            },
            created_at: chrono::Utc::now(),
            message_id: None,
        });
        // finish is None — still streaming
        assistant.finish = None;

        let messages = [user, assistant];

        let last_user_idx = messages
            .iter()
            .rposition(|m| matches!(m.role, Role::User))
            .unwrap();
        let last_assistant_idx = messages
            .iter()
            .rposition(|m| matches!(m.role, Role::Assistant));

        let should_break = if let Some(assistant_idx) = last_assistant_idx {
            let assistant = &messages[assistant_idx];
            let is_terminal = assistant
                .finish
                .as_deref()
                .is_some_and(|f| !matches!(f, "tool-calls" | "tool_calls" | "unknown"));
            is_terminal && last_user_idx < assistant_idx
        } else {
            false
        };

        assert!(
            !should_break,
            "early-exit must NOT trigger when finish is None"
        );
    }

    #[test]
    fn chat_message_hook_not_triggered_on_user_message_creation() {
        let source = include_str!("mod.rs");
        let create_user_fn = source
            .find("async fn create_user_message")
            .expect("create_user_message should exist");
        // Narrow the search to the body of create_user_message only,
        // stopping at the next standalone method definition so we don't
        // accidentally match helper methods defined between the two markers.
        let rest = &source[create_user_fn..];
        let next_method = rest[1..]
            .find("\n    async fn ")
            .or_else(|| rest[1..].find("\n    pub async fn "))
            .map(|offset| offset + 1)
            .unwrap_or(rest.len());
        let create_user_section = &rest[..next_method];
        assert!(
            !create_user_section.contains("HookEvent::ChatMessage"),
            "ChatMessage hook should not be in create_user_message"
        );
    }
}
