use async_trait::async_trait;
use serde::Deserialize;
use std::str::FromStr;
use std::sync::Arc;

use rocode_agent::{AgentInfo, AgentMode, AgentRegistry};
use rocode_command::output_blocks::{MessageBlock, MessageRole as OutputMessageRole, OutputBlock};
use rocode_config::{Config as AppConfig, SkillTreeNodeConfig};
use rocode_orchestrator::output_metadata::output_usage;
use rocode_orchestrator::{
    resolve_skill_markdown_repo, scheduler_orchestrator_from_profile, scheduler_plan_from_profile,
    scheduler_request_defaults_from_file, scheduler_request_defaults_from_plan, AgentResolver,
    AvailableAgentMeta, AvailableCategoryMeta, ExecutionContext as OrchestratorExecutionContext,
    ModelRef as OrchestratorModelRef, ModelResolver, Orchestrator, OrchestratorContext,
    OrchestratorError, SchedulerConfig, SchedulerPresetKind, SchedulerProfileConfig,
    SchedulerRequestDefaults, SkillTreeNode, SkillTreeRequestPlan,
    ToolExecError as OrchestratorToolExecError, ToolExecutor as OrchestratorToolExecutor,
    ToolOutput as OrchestratorToolOutput, ToolRunner,
};
use tokio_util::sync::CancellationToken;

use crate::request_options::{resolve_compiled_execution_request, ExecutionResolutionContext};
use crate::runtime_control::SessionRunStatus;
use crate::session_runtime::events::{
    broadcast_session_updated, emit_output_block_via_hook, server_output_block_hook,
};
use crate::session_runtime::{
    assistant_visible_text, ensure_default_session_title,
    finalize_active_scheduler_stage_cancelled, first_user_message_text, ModelPricing,
    SessionSchedulerLifecycleHook,
};
use crate::{Result, ServerState};
use rocode_session::prompt::{OutputBlockEvent, OutputBlockHook};

use super::super::permission::request_permission;
use super::super::tui::request_question_answers;
use super::cancel::is_scheduler_cancellation_error;
use super::messages::resolve_provider_and_model;
use super::session_crud::{resolved_session_directory, set_session_run_status};

use super::cancel::abort_session_execution;

fn to_orchestrator_skill_tree(node: &SkillTreeNodeConfig) -> SkillTreeNode {
    SkillTreeNode {
        node_id: node.node_id.clone(),
        markdown_path: node.markdown_path.clone(),
        children: node
            .children
            .iter()
            .map(to_orchestrator_skill_tree)
            .collect(),
    }
}

fn resolve_builtin_scheduler_request_defaults(
    requested_profile: Option<&str>,
) -> Option<SchedulerRequestDefaults> {
    let profile_name = requested_profile
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let preset = SchedulerPresetKind::from_str(profile_name).ok()?;
    let profile = SchedulerProfileConfig {
        orchestrator: Some(preset.as_str().to_string()),
        ..Default::default()
    };
    let plan = scheduler_plan_from_profile(Some(profile_name.to_string()), &profile).ok()?;
    Some(scheduler_request_defaults_from_plan(&plan))
}

pub(crate) fn resolve_scheduler_request_defaults(
    config: &AppConfig,
    requested_profile: Option<&str>,
) -> Option<SchedulerRequestDefaults> {
    if let Some(defaults) = resolve_builtin_scheduler_request_defaults(requested_profile) {
        return Some(defaults);
    }

    let scheduler_path = config
        .scheduler_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    if let Some(profile_name) = requested_profile
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let scheduler_config = match SchedulerConfig::load_from_file(scheduler_path) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(path = %scheduler_path, %error, "failed to load scheduler config");
                return None;
            }
        };
        let profile = match scheduler_config.profile(profile_name) {
            Ok(profile) => profile,
            Err(error) => {
                tracing::warn!(path = %scheduler_path, profile = %profile_name, %error, "failed to resolve requested scheduler profile");
                return None;
            }
        };
        let plan = match scheduler_plan_from_profile(Some(profile_name.to_string()), profile) {
            Ok(plan) => plan,
            Err(error) => {
                tracing::warn!(path = %scheduler_path, profile = %profile_name, %error, "failed to build requested scheduler profile plan");
                return None;
            }
        };
        return Some(scheduler_request_defaults_from_plan(&plan));
    }

    match scheduler_request_defaults_from_file(scheduler_path) {
        Ok(defaults) => Some(defaults),
        Err(error) => {
            tracing::warn!(path = %scheduler_path, %error, "failed to load scheduler request defaults");
            None
        }
    }
}

pub(super) fn scheduler_system_prompt_preview(
    profile_name: &str,
    profile: &SchedulerProfileConfig,
) -> String {
    let orchestrator = profile.orchestrator.as_deref().unwrap_or(profile_name);
    SchedulerPresetKind::from_str(orchestrator)
        .ok()
        .map(|preset| preset.definition().system_prompt_preview().to_string())
        .unwrap_or_else(|| {
            format!(
                "You are the `{profile_name}` scheduler profile.
Bias: follow its configured stages and orchestration contract faithfully.
Boundary: preserve the profile's execution constraints and role semantics."
            )
        })
}

pub(super) fn scheduler_mode_kind(profile_name: &str) -> &'static str {
    if SchedulerPresetKind::from_str(profile_name).is_ok() {
        "preset"
    } else {
        "profile"
    }
}

pub(crate) struct PromptRequestConfigInput<'a> {
    pub state: &'a Arc<ServerState>,
    pub config: &'a AppConfig,
    pub session_id: &'a str,
    pub requested_agent: Option<&'a str>,
    pub requested_scheduler_profile: Option<&'a str>,
    pub request_model: Option<&'a str>,
    pub request_variant: Option<&'a str>,
    pub route: &'static str,
}

pub(super) fn resolve_scheduler_profile_config(
    config: &AppConfig,
    requested_profile: Option<&str>,
) -> Option<(String, SchedulerProfileConfig)> {
    let profile_name = requested_profile
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    if let Ok(preset) = SchedulerPresetKind::from_str(profile_name) {
        return Some((
            profile_name.to_string(),
            SchedulerProfileConfig {
                orchestrator: Some(preset.as_str().to_string()),
                ..Default::default()
            },
        ));
    }

    let scheduler_path = config
        .scheduler_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let scheduler_config = match SchedulerConfig::load_from_file(scheduler_path) {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!(path = %scheduler_path, %error, "failed to load scheduler profile config");
            return None;
        }
    };
    let profile = match scheduler_config.profile(profile_name) {
        Ok(profile) => profile.clone(),
        Err(error) => {
            tracing::warn!(path = %scheduler_path, profile = %profile_name, %error, "failed to resolve scheduler profile config");
            return None;
        }
    };
    Some((profile_name.to_string(), profile))
}

#[derive(Clone)]
pub(super) struct SchedulerAgentResolver {
    pub(super) registry: Arc<AgentRegistry>,
}

impl AgentResolver for SchedulerAgentResolver {
    fn resolve(&self, name: &str) -> Option<rocode_orchestrator::AgentDescriptor> {
        self.registry
            .get(name)
            .map(to_orchestrator_agent_descriptor)
    }
}

fn to_orchestrator_agent_descriptor(info: &AgentInfo) -> rocode_orchestrator::AgentDescriptor {
    rocode_orchestrator::AgentDescriptor {
        name: info.name.clone(),
        system_prompt: info.system_prompt.clone(),
        model: info
            .model
            .as_ref()
            .map(|model| rocode_orchestrator::ModelRef {
                provider_id: model.provider_id.clone(),
                model_id: model.model_id.clone(),
            }),
        max_steps: info.max_steps,
        temperature: info.temperature,
        allowed_tools: info.allowed_tools.clone(),
    }
}

pub(crate) fn to_task_agent_info(info: &AgentInfo) -> rocode_tool::TaskAgentInfo {
    rocode_tool::TaskAgentInfo {
        name: info.name.clone(),
        model: info.model.as_ref().map(|m| rocode_tool::TaskAgentModel {
            provider_id: m.provider_id.clone(),
            model_id: m.model_id.clone(),
        }),
        can_use_task: info.is_tool_allowed("task"),
        steps: info.max_steps,
        execution: Some(rocode_orchestrator::ExecutionRequestContext {
            provider_id: info.model.as_ref().map(|m| m.provider_id.clone()),
            model_id: info.model.as_ref().map(|m| m.model_id.clone()),
            max_tokens: info.max_tokens,
            temperature: info.temperature,
            top_p: info.top_p,
            variant: info.variant.clone(),
            provider_options: (!info.options.is_empty()).then_some(info.options.clone()),
        }),
        max_tokens: info.max_tokens,
        temperature: info.temperature,
        top_p: info.top_p,
        variant: info.variant.clone(),
    }
}

#[derive(Clone)]
pub(super) struct SessionSchedulerModelResolver {
    pub(super) state: Arc<ServerState>,
    pub(super) fallback_provider_id: String,
    pub(super) fallback_model_id: String,
    pub(super) fallback_request: rocode_orchestrator::CompiledExecutionRequest,
}

#[async_trait]
impl ModelResolver for SessionSchedulerModelResolver {
    async fn chat_stream(
        &self,
        model: Option<&OrchestratorModelRef>,
        messages: Vec<rocode_provider::Message>,
        tools: Vec<rocode_provider::ToolDefinition>,
        _exec_ctx: &OrchestratorExecutionContext,
    ) -> std::result::Result<rocode_provider::StreamResult, OrchestratorError> {
        let (provider_id, model_id) = model
            .map(|model| (model.provider_id.clone(), model.model_id.clone()))
            .unwrap_or_else(|| {
                (
                    self.fallback_provider_id.clone(),
                    self.fallback_model_id.clone(),
                )
            });

        let provider = {
            let providers = self.state.providers.read().await;
            providers
                .get_provider(&provider_id)
                .map_err(|error| OrchestratorError::ModelError(error.to_string()))?
        };

        let request = self
            .fallback_request
            .with_model(model_id)
            .to_chat_request(messages, tools, true);
        provider
            .chat_stream(request)
            .await
            .map_err(|error| OrchestratorError::ModelError(error.to_string()))
    }
}

#[derive(Clone)]
pub(super) struct SessionSchedulerToolExecutor {
    pub(super) state: Arc<ServerState>,
    pub(super) session_id: String,
    pub(super) message_id: String,
    pub(super) directory: String,
    pub(super) abort_token: CancellationToken,
    pub(super) current_model: Option<String>,
    pub(super) tool_runtime_config: rocode_tool::ToolRuntimeConfig,
    pub(super) agent_registry: Arc<AgentRegistry>,
}

#[derive(Clone)]
pub(super) struct SchedulerRunCancelToken {
    pub(super) token: CancellationToken,
}

impl rocode_orchestrator::runtime::events::CancelToken for SchedulerRunCancelToken {
    fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}

impl SessionSchedulerToolExecutor {
    fn build_tool_context(
        &self,
        exec_ctx: &OrchestratorExecutionContext,
    ) -> rocode_tool::ToolContext {
        let mut base_ctx = rocode_tool::ToolContext::new(
            self.session_id.clone(),
            self.message_id.clone(),
            self.directory.clone(),
        )
        .with_agent(exec_ctx.agent_name.clone())
        .with_abort(self.abort_token.clone())
        .with_tool_runtime_config(self.tool_runtime_config.clone())
        .with_registry(self.state.tool_registry.clone())
        .with_get_last_model({
            let current_model = self.current_model.clone();
            move |_session_id| {
                let current_model = current_model.clone();
                async move { Ok(current_model.clone()) }
            }
        })
        .with_get_agent_info({
            let agent_registry = self.agent_registry.clone();
            move |name| {
                let agent_registry = agent_registry.clone();
                async move { Ok(agent_registry.get(&name).map(to_task_agent_info)) }
            }
        })
        .with_ask_question({
            let state = self.state.clone();
            let session_id = self.session_id.clone();
            move |questions| {
                let state = state.clone();
                let session_id = session_id.clone();
                async move { request_question_answers(state, session_id, questions).await }
            }
        })
        .with_ask({
            let state = self.state.clone();
            let session_id = self.session_id.clone();
            move |request| {
                let state = state.clone();
                let session_id = session_id.clone();
                async move { request_permission(state, session_id, request).await }
            }
        })
        .with_resolve_category({
            let category_registry = self.state.category_registry.clone();
            move |category| {
                let registry = category_registry.clone();
                async move {
                    Ok(registry
                        .resolve(&category)
                        .map(|def| rocode_tool::TaskCategoryInfo {
                            name: category,
                            description: def.description.clone(),
                            model: def.model.as_ref().map(|m| rocode_tool::TaskAgentModel {
                                provider_id: m.provider_id.clone(),
                                model_id: m.model_id.clone(),
                            }),
                            prompt_suffix: def.prompt_suffix.clone(),
                            variant: def.variant.clone(),
                        }))
                }
            }
        });

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
        struct OrchestratorExecutionMetadataWire {
            #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
            call_id: Option<String>,
        }

        let wire = serde_json::to_value(&exec_ctx.metadata)
            .ok()
            .and_then(|value| {
                serde_json::from_value::<OrchestratorExecutionMetadataWire>(value).ok()
            })
            .unwrap_or_default();
        base_ctx.call_id = wire.call_id;
        base_ctx.extra = exec_ctx.metadata.clone();
        Self::with_agent_task_publish_bus(base_ctx, self.state.clone())
    }

    /// Wire `publish_bus` to route `agent_task.*` events to
    /// [`RuntimeControlRegistry`] so spawned agent tasks appear in the
    /// execution topology with correct parent links.
    fn with_agent_task_publish_bus(
        ctx: rocode_tool::ToolContext,
        state: Arc<ServerState>,
    ) -> rocode_tool::ToolContext {
        let session_id = ctx.session_id.clone();
        ctx.with_publish_bus(move |event_type, properties| {
            let state = state.clone();
            let session_id = session_id.clone();
            async move {
                match event_type.as_str() {
                    "agent_task.registered" => {
                        let task_id = properties["task_id"].as_str().unwrap_or_default();
                        let agent_name = properties["agent_name"].as_str().unwrap_or_default();
                        let parent_tool_call_id = properties["parent_tool_call_id"].as_str().map(
                            crate::runtime_control::RuntimeControlRegistry::tool_call_execution_id,
                        );
                        // Resolve stage_id from the parent execution's record.
                        let stage_id = if let Some(ref pid) = parent_tool_call_id {
                            state.runtime_control.resolve_stage_id(pid).await
                        } else {
                            None
                        };
                        state
                            .runtime_control
                            .register_agent_task(
                                task_id,
                                &session_id,
                                agent_name,
                                parent_tool_call_id,
                                stage_id.clone(),
                            )
                            .await;
                        // Update agent counts on the stage message.
                        if let Some(ref sid) = stage_id {
                            update_stage_agent_counts(&state, &session_id, sid).await;
                        }
                    }
                    "agent_task.completed" => {
                        let task_id = properties["task_id"].as_str().unwrap_or_default();
                        // Resolve stage_id before finishing (record still exists).
                        let exec_id =
                            crate::runtime_control::RuntimeControlRegistry::agent_task_execution_id(
                                task_id,
                            );
                        let stage_id = state.runtime_control.resolve_stage_id(&exec_id).await;
                        state.runtime_control.finish_agent_task(task_id).await;
                        // Update agent counts on the stage message.
                        if let Some(ref sid) = stage_id {
                            update_stage_agent_counts(&state, &session_id, sid).await;
                        }
                    }
                    _ => {}
                }
            }
        })
    }
}

/// Update `scheduler_stage_done_agent_count` and `scheduler_stage_total_agent_count`
/// in the stage's session message metadata so all three frontends can display agent progress.
async fn update_stage_agent_counts(
    state: &crate::server::ServerState,
    session_id: &str,
    stage_id: &str,
) {
    let (done, total) = state.runtime_control.count_stage_agents(stage_id).await;
    let mut sessions = state.sessions.lock().await;
    let Some(mut session) = sessions.get(session_id).cloned() else {
        return;
    };
    // The stage_id is also used as the message_id for the stage message.
    if let Some(message) = session.get_message_mut(stage_id) {
        message.metadata.insert(
            "scheduler_stage_done_agent_count".to_string(),
            serde_json::json!(done),
        );
        message.metadata.insert(
            "scheduler_stage_total_agent_count".to_string(),
            serde_json::json!(total),
        );
        session.touch();
        sessions.update(session);
    }
}

#[async_trait]
impl OrchestratorToolExecutor for SessionSchedulerToolExecutor {
    async fn execute(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
        exec_ctx: &OrchestratorExecutionContext,
    ) -> std::result::Result<OrchestratorToolOutput, OrchestratorToolExecError> {
        let ctx = self.build_tool_context(exec_ctx);
        let result = self
            .state
            .tool_registry
            .execute(tool_name, arguments, ctx)
            .await
            .map_err(|error| match error {
                rocode_tool::ToolError::InvalidArguments(message) => {
                    OrchestratorToolExecError::InvalidArguments(message)
                }
                rocode_tool::ToolError::PermissionDenied(message) => {
                    OrchestratorToolExecError::PermissionDenied(message)
                }
                rocode_tool::ToolError::Cancelled => {
                    OrchestratorToolExecError::ExecutionError("cancelled".to_string())
                }
                other => OrchestratorToolExecError::ExecutionError(other.to_string()),
            })?;
        Ok(OrchestratorToolOutput {
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
        self.state.tool_registry.list_ids().await
    }

    async fn list_definitions(
        &self,
        _exec_ctx: &OrchestratorExecutionContext,
    ) -> Vec<rocode_provider::ToolDefinition> {
        let mut tools: Vec<rocode_provider::ToolDefinition> = self
            .state
            .tool_registry
            .list_schemas()
            .await
            .into_iter()
            .map(|schema| rocode_provider::ToolDefinition {
                name: schema.name,
                description: Some(schema.description),
                parameters: schema.parameters,
            })
            .collect();
        rocode_session::prioritize_tool_definitions(&mut tools);
        tools
    }
}

pub(crate) fn resolve_config_default_agent_name(config: &AppConfig) -> String {
    config
        .default_agent
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("build")
        .to_string()
}

pub(crate) fn resolve_request_skill_tree_plan(
    config: &AppConfig,
    scheduler_defaults: Option<&SchedulerRequestDefaults>,
) -> Option<SkillTreeRequestPlan> {
    if let Some(plan) = scheduler_defaults.and_then(|defaults| defaults.skill_tree_plan.clone()) {
        return Some(plan);
    }

    let skill_tree = config.composition.as_ref()?.skill_tree.as_ref()?;
    if matches!(skill_tree.enabled, Some(false)) {
        return None;
    }

    let root = skill_tree.root.as_ref()?;
    let root = to_orchestrator_skill_tree(root);
    let markdown_repo = resolve_skill_markdown_repo(&config.skill_paths);

    match SkillTreeRequestPlan::from_tree_with_separator(
        &root,
        &markdown_repo,
        skill_tree.separator.as_deref(),
    ) {
        Ok(plan) => plan,
        Err(error) => {
            tracing::warn!(%error, "failed to build request skill tree plan");
            None
        }
    }
}

pub(crate) struct ResolvedPromptRequestConfig {
    pub scheduler_applied: bool,
    pub scheduler_profile_name: Option<String>,
    pub scheduler_root_agent: Option<String>,
    pub scheduler_skill_tree_applied: bool,
    pub resolved_agent: Option<AgentInfo>,
    pub provider: Arc<dyn rocode_provider::Provider>,
    pub provider_id: String,
    pub model_id: String,
    pub agent_system_prompt: Option<String>,
    pub compiled_request: rocode_orchestrator::CompiledExecutionRequest,
}

pub(super) fn resolve_request_model_inputs(
    scheduler_applied: bool,
    agent_model: Option<&str>,
    scheduler_profile: Option<&SchedulerProfileConfig>,
    request_model: Option<&str>,
    config_model: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>) {
    if scheduler_applied {
        if let Some(agent_model) = agent_model {
            return (None, Some(agent_model.to_string()), None);
        }

        if let Some(model) = scheduler_profile.and_then(|profile| profile.model.as_ref()) {
            return (
                None,
                Some(model.model_id.clone()),
                Some(model.provider_id.clone()),
            );
        }

        return (
            request_model.map(str::to_string),
            config_model.map(str::to_string),
            None,
        );
    }

    (
        request_model.map(str::to_string),
        agent_model.or(config_model).map(str::to_string),
        None,
    )
}

fn build_execution_resolution_context(
    session_id: &str,
    provider_id: &str,
    model_id: &str,
    request_variant: Option<&str>,
    resolved_agent: Option<&AgentInfo>,
) -> ExecutionResolutionContext {
    ExecutionResolutionContext {
        session_id: session_id.to_string(),
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
        max_tokens: resolved_agent.and_then(|agent| agent.max_tokens),
        temperature: resolved_agent.and_then(|agent| agent.temperature),
        top_p: resolved_agent.and_then(|agent| agent.top_p),
        variant: request_variant
            .map(str::to_string)
            .or_else(|| resolved_agent.and_then(|agent| agent.variant.clone())),
    }
}

pub(crate) async fn resolve_prompt_request_config(
    input: PromptRequestConfigInput<'_>,
) -> Result<ResolvedPromptRequestConfig> {
    let PromptRequestConfigInput {
        state,
        config,
        session_id,
        requested_agent,
        requested_scheduler_profile,
        request_model,
        request_variant,
        route,
    } = input;

    let scheduler_defaults =
        resolve_scheduler_request_defaults(config, requested_scheduler_profile);
    let scheduler_applied = scheduler_defaults.is_some();
    let scheduler_profile_name = scheduler_defaults
        .as_ref()
        .and_then(|defaults| defaults.profile_name.clone());
    let scheduler_root_agent = scheduler_defaults
        .as_ref()
        .and_then(|defaults| defaults.root_agent_name.clone());
    let scheduler_skill_tree_applied = scheduler_defaults
        .as_ref()
        .and_then(|defaults| defaults.skill_tree_plan.as_ref())
        .is_some();
    let scheduler_agent_name = if requested_agent.is_none() {
        scheduler_root_agent.clone()
    } else {
        None
    };
    let fallback_agent_name =
        if requested_agent.is_none() && scheduler_agent_name.is_none() && !scheduler_applied {
            Some(resolve_config_default_agent_name(config))
        } else {
            None
        };

    let agent_registry = AgentRegistry::from_config(config);
    let selected_agent_name = requested_agent
        .or(scheduler_agent_name.as_deref())
        .or(fallback_agent_name.as_deref());
    let resolved_agent = selected_agent_name.and_then(|name| agent_registry.get(name).cloned());
    if requested_agent.is_some() && resolved_agent.is_none() {
        tracing::warn!(
            route,
            requested_agent = ?requested_agent,
            scheduler_agent = ?scheduler_agent_name,
            fallback_agent = ?fallback_agent_name,
            "requested agent not found in registry; proceeding without agent-specific overrides"
        );
    } else if scheduler_agent_name.is_some() && resolved_agent.is_none() {
        tracing::warn!(
            route,
            scheduler_agent = ?scheduler_agent_name,
            "scheduler root agent not found in registry; proceeding without agent-specific overrides"
        );
    }

    let scheduler_profile_config = scheduler_profile_name
        .as_deref()
        .and_then(|profile_name| resolve_scheduler_profile_config(config, Some(profile_name)))
        .map(|(_, profile)| profile);
    let scheduler_profile_model = scheduler_profile_config
        .as_ref()
        .and_then(|profile| profile.model.as_ref())
        .map(|model| format!("{}/{}", model.provider_id, model.model_id));
    let agent_model = resolved_agent
        .as_ref()
        .and_then(|agent| agent.model.as_ref())
        .map(|model| format!("{}/{}", model.provider_id, model.model_id));
    let (request_model_input, config_model_input, config_provider_input) =
        resolve_request_model_inputs(
            scheduler_applied,
            agent_model.as_deref(),
            scheduler_profile_config.as_ref(),
            request_model,
            config.model.as_deref(),
        );
    let (provider, provider_id, model_id) = resolve_provider_and_model(
        state,
        request_model_input.as_deref(),
        config_model_input.as_deref(),
        config_provider_input.as_deref(),
    )
    .await?;

    let request_skill_tree_plan =
        resolve_request_skill_tree_plan(config, scheduler_defaults.as_ref());
    let mut agent_system_prompt = resolved_agent
        .as_ref()
        .and_then(|agent| agent.resolved_system_prompt());
    if let Some(plan) = request_skill_tree_plan.as_ref() {
        agent_system_prompt = plan.compose_system_prompt(agent_system_prompt.as_deref());
    }

    let compiled_request = resolve_compiled_execution_request(
        config,
        &build_execution_resolution_context(
            session_id,
            &provider_id,
            &model_id,
            request_variant,
            resolved_agent.as_ref(),
        ),
    )
    .await;
    tracing::info!(
        route,
        requested_agent = ?requested_agent,
        scheduler_agent = ?scheduler_agent_name,
        scheduler_applied,
        scheduler_profile = ?scheduler_profile_name,
        scheduler_root_agent = ?scheduler_root_agent,
        scheduler_skill_tree_applied,
        request_skill_tree_applied = request_skill_tree_plan.is_some(),
        fallback_agent = ?fallback_agent_name,
        resolved_agent = ?resolved_agent.as_ref().map(|agent| agent.name.as_str()),
        agent_model = ?agent_model,
        scheduler_profile_model = ?scheduler_profile_model,
        request_model_input = ?request_model_input,
        config_model_input = ?config_model_input,
        config_provider_input = ?config_provider_input,
        system_prompt_applied = agent_system_prompt.is_some(),
        "resolved request prompt agent configuration"
    );

    Ok(ResolvedPromptRequestConfig {
        scheduler_applied,
        scheduler_profile_name,
        scheduler_root_agent,
        scheduler_skill_tree_applied,
        resolved_agent,
        provider,
        provider_id,
        model_id,
        agent_system_prompt,
        compiled_request,
    })
}

#[derive(Debug, Clone)]
pub struct LocalSchedulerPromptRequest {
    pub session_id: Option<String>,
    pub directory: String,
    pub prompt_text: String,
    pub display_prompt_text: String,
    pub scheduler_profile: String,
    pub model: Option<String>,
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LocalSchedulerPromptOutcome {
    pub session_id: String,
    pub assistant_text: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cancelled: bool,
}

pub async fn run_local_scheduler_prompt(
    state: Arc<ServerState>,
    req: LocalSchedulerPromptRequest,
    output_hook: Option<OutputBlockHook>,
) -> anyhow::Result<LocalSchedulerPromptOutcome> {
    let output_hook = output_hook.or_else(|| Some(server_output_block_hook(state.clone())));
    let config = state.config_store.config();
    let session_id = {
        let mut sessions = state.sessions.lock().await;
        match req
            .session_id
            .as_deref()
            .and_then(|id| sessions.get(id).cloned())
        {
            Some(existing) => existing.id,
            None => {
                sessions
                    .create("rocode-cli", resolved_session_directory(&req.directory))
                    .id
            }
        }
    };
    let request_config = resolve_prompt_request_config(PromptRequestConfigInput {
        state: &state,
        config: &config,
        session_id: &session_id,
        requested_agent: None,
        requested_scheduler_profile: Some(req.scheduler_profile.as_str()),
        request_model: req.model.as_deref(),
        request_variant: req.variant.as_deref(),
        route: "cli-local",
    })
    .await
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;

    let profile_name = request_config
        .scheduler_profile_name
        .clone()
        .ok_or_else(|| anyhow::anyhow!("scheduler profile was not resolved"))?;
    let mut profile_config = resolve_scheduler_profile_config(&config, Some(&profile_name))
        .map(|(_, profile)| profile)
        .ok_or_else(|| anyhow::anyhow!("scheduler profile config not found: {}", profile_name))?;

    let mut session = {
        let sessions = state.sessions.lock().await;
        sessions
            .get(&session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("failed to initialize local scheduler session"))?
    };
    let normalized_directory = resolved_session_directory(&session.directory);
    if session.directory != normalized_directory {
        session.directory = normalized_directory;
    }

    let scheduler_applied = request_config.scheduler_applied;
    let scheduler_root_agent = request_config.scheduler_root_agent.clone();
    let scheduler_skill_tree_applied = request_config.scheduler_skill_tree_applied;
    let provider = request_config.provider.clone();
    let provider_id = request_config.provider_id.clone();
    let model_id = request_config.model_id.clone();
    let fallback_request = resolve_compiled_execution_request(
        &config,
        &ExecutionResolutionContext {
            session_id: session_id.clone(),
            provider_id: provider_id.clone(),
            model_id: model_id.clone(),
            variant: req.variant.clone(),
            ..Default::default()
        },
    )
    .await;

    set_session_run_status(&state, &session_id, SessionRunStatus::Busy).await;

    session.metadata.insert(
        "model_provider".to_string(),
        serde_json::json!(&provider_id),
    );
    session
        .metadata
        .insert("model_id".to_string(), serde_json::json!(&model_id));
    session.metadata.insert(
        "scheduler_applied".to_string(),
        serde_json::json!(scheduler_applied),
    );
    session.metadata.insert(
        "scheduler_skill_tree_applied".to_string(),
        serde_json::json!(scheduler_skill_tree_applied),
    );
    session.metadata.insert(
        "scheduler_profile".to_string(),
        serde_json::json!(profile_name.clone()),
    );
    if let Some(root_agent) = scheduler_root_agent.as_deref() {
        session.metadata.insert(
            "scheduler_root_agent".to_string(),
            serde_json::json!(root_agent),
        );
    } else {
        session.metadata.remove("scheduler_root_agent");
    }

    let mode_kind = scheduler_mode_kind(&profile_name);
    let resolved_system_prompt = scheduler_system_prompt_preview(&profile_name, &profile_config);
    let user_message_id = {
        let user_message = session.add_user_message(req.display_prompt_text.clone());
        user_message.metadata.insert(
            "resolved_scheduler_profile".to_string(),
            serde_json::json!(profile_name.clone()),
        );
        user_message.metadata.insert(
            "resolved_execution_mode_kind".to_string(),
            serde_json::json!(mode_kind),
        );
        user_message.metadata.insert(
            "resolved_system_prompt".to_string(),
            serde_json::json!(resolved_system_prompt.clone()),
        );
        user_message.metadata.insert(
            "resolved_system_prompt_preview".to_string(),
            serde_json::json!(resolved_system_prompt.clone()),
        );
        user_message.metadata.insert(
            "resolved_system_prompt_applied".to_string(),
            serde_json::json!(true),
        );
        user_message.metadata.insert(
            "resolved_user_prompt".to_string(),
            serde_json::json!(req.prompt_text.clone()),
        );
        user_message.id.clone()
    };
    let assistant_message_id = session.add_assistant_message().id.clone();

    if session.is_default_title() {
        if let Some(first_text) = first_user_message_text(&session) {
            let immediate = rocode_session::generate_session_title(&first_text);
            if !immediate.is_empty() && immediate != "New Session" {
                session.set_auto_title(immediate);
            }
        }
    }

    {
        let mut sessions = state.sessions.lock().await;
        sessions.update(session.clone());
    }

    let agent_registry = Arc::new(AgentRegistry::from_config(&config));
    if profile_config.available_agents.is_empty() {
        profile_config.available_agents = agent_registry
            .list()
            .iter()
            .filter(|a| !a.hidden && matches!(a.mode, AgentMode::Subagent | AgentMode::All))
            .map(|a| AvailableAgentMeta {
                name: a.name.clone(),
                description: a.description.clone().unwrap_or_default(),
                mode: match a.mode {
                    AgentMode::Primary => "primary".to_string(),
                    AgentMode::Subagent => "subagent".to_string(),
                    AgentMode::All => "all".to_string(),
                },
                cost: if a.name == "oracle" {
                    "EXPENSIVE".to_string()
                } else {
                    "CHEAP".to_string()
                },
            })
            .collect();
    }
    if profile_config.available_categories.is_empty() {
        profile_config.available_categories = state
            .category_registry
            .category_descriptions()
            .into_iter()
            .map(|(name, description)| AvailableCategoryMeta { name, description })
            .collect();
    }
    if profile_config.skill_list.is_empty() {
        profile_config.skill_list = rocode_tool::skill::list_available_skills()
            .into_iter()
            .map(|(name, _description)| name)
            .collect();
    }

    let current_model = Some(format!("{}:{}", provider_id, model_id));
    let scheduler_abort_token = CancellationToken::new();
    state
        .runtime_control
        .register_scheduler_run(
            &session_id,
            scheduler_abort_token.clone(),
            Some(profile_name.clone()),
        )
        .await;
    let tool_executor: Arc<dyn OrchestratorToolExecutor> = Arc::new(SessionSchedulerToolExecutor {
        state: state.clone(),
        session_id: session_id.clone(),
        message_id: assistant_message_id.clone(),
        directory: session.directory.clone(),
        abort_token: scheduler_abort_token.clone(),
        current_model,
        tool_runtime_config: rocode_tool::ToolRuntimeConfig::from_config(&config),
        agent_registry: agent_registry.clone(),
    });
    let tool_runner = ToolRunner::new(tool_executor.clone());
    let model_resolver: Arc<dyn ModelResolver> = Arc::new(SessionSchedulerModelResolver {
        state: state.clone(),
        fallback_provider_id: provider_id.clone(),
        fallback_model_id: model_id.clone(),
        fallback_request: fallback_request.clone(),
    });
    let exec_ctx = OrchestratorExecutionContext {
        session_id: session_id.clone(),
        workdir: session.directory.clone(),
        agent_name: profile_name.clone(),
        metadata: std::collections::HashMap::from([
            (
                "message_id".to_string(),
                serde_json::json!(assistant_message_id.clone()),
            ),
            (
                "user_message_id".to_string(),
                serde_json::json!(user_message_id.clone()),
            ),
            (
                "scheduler_profile".to_string(),
                serde_json::json!(profile_name.clone()),
            ),
        ]),
    };
    let model_pricing = {
        let providers = state.providers.read().await;
        providers
            .find_model(&model_id)
            .map(|(_, info)| ModelPricing::from_model_info(&info))
    };
    let lifecycle_hook = Arc::new(
        SessionSchedulerLifecycleHook::new(state.clone(), session_id.clone(), profile_name.clone())
            .with_model_pricing(model_pricing)
            .with_output_hook(output_hook.clone()),
    );
    let ctx = OrchestratorContext {
        agent_resolver: Arc::new(SchedulerAgentResolver {
            registry: agent_registry.clone(),
        }),
        model_resolver,
        tool_executor,
        lifecycle_hook,
        cancel_token: Arc::new(SchedulerRunCancelToken {
            token: scheduler_abort_token.clone(),
        }),
        exec_ctx,
    };

    let orchestrator_result = match scheduler_orchestrator_from_profile(
        Some(profile_name.clone()),
        &profile_config,
        tool_runner,
    ) {
        Ok(mut orchestrator) => orchestrator.execute(&req.prompt_text, &ctx).await,
        Err(error) => Err(OrchestratorError::Other(error.to_string())),
    };
    state
        .runtime_control
        .finish_scheduler_run(&session_id)
        .await;

    session = {
        let sessions = state.sessions.lock().await;
        sessions
            .get(&session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("scheduler session vanished"))?
    };

    let mut prompt_tokens = 0;
    let mut completion_tokens = 0;
    let mut cancelled = false;
    if let Some(assistant) = session.get_message_mut(&assistant_message_id) {
        assistant.metadata.insert(
            "model_provider".to_string(),
            serde_json::json!(&provider_id),
        );
        assistant
            .metadata
            .insert("model_id".to_string(), serde_json::json!(&model_id));
        assistant.metadata.insert(
            "scheduler_profile".to_string(),
            serde_json::json!(profile_name.clone()),
        );
        assistant.metadata.insert(
            "resolved_scheduler_profile".to_string(),
            serde_json::json!(profile_name.clone()),
        );
        assistant.metadata.insert(
            "resolved_execution_mode_kind".to_string(),
            serde_json::json!(mode_kind),
        );
        assistant
            .metadata
            .insert("mode".to_string(), serde_json::json!(profile_name.clone()));
        assistant.metadata.insert(
            "scheduler_applied".to_string(),
            serde_json::json!(scheduler_applied),
        );
        match orchestrator_result {
            Ok(output) => {
                cancelled = output.is_cancelled();
                if cancelled {
                    let _ = finalize_active_scheduler_stage_cancelled(&state, &session_id).await;
                    assistant.finish = Some("cancelled".to_string());
                    assistant
                        .metadata
                        .insert("finish_reason".to_string(), serde_json::json!("cancelled"));
                } else {
                    assistant.finish = Some("stop".to_string());
                }
                assistant.metadata.insert(
                    "scheduler_steps".to_string(),
                    serde_json::json!(output.steps),
                );
                assistant.metadata.insert(
                    "scheduler_tool_calls".to_string(),
                    serde_json::json!(output.tool_calls_count),
                );
                if let Some(usage) = output_usage(&output.metadata) {
                    prompt_tokens = usage.prompt_tokens;
                    completion_tokens = usage.completion_tokens;
                    let cost = model_pricing
                        .map(|p| {
                            p.compute(
                                usage.prompt_tokens,
                                usage.completion_tokens,
                                usage.cache_read_tokens,
                                usage.cache_write_tokens,
                            )
                        })
                        .unwrap_or(0.0);
                    assistant.usage = Some(rocode_session::MessageUsage {
                        input_tokens: usage.prompt_tokens,
                        output_tokens: usage.completion_tokens,
                        reasoning_tokens: usage.reasoning_tokens,
                        cache_read_tokens: usage.cache_read_tokens,
                        cache_write_tokens: usage.cache_write_tokens,
                        total_cost: cost,
                    });
                }
                assistant.add_text(output.content);
            }
            Err(error) => {
                cancelled = is_scheduler_cancellation_error(&error);
                if cancelled {
                    let _ = finalize_active_scheduler_stage_cancelled(&state, &session_id).await;
                    assistant.finish = Some("cancelled".to_string());
                    assistant
                        .metadata
                        .insert("finish_reason".to_string(), serde_json::json!("cancelled"));
                    assistant.add_text("Scheduler cancelled.");
                } else {
                    assistant.finish = Some("error".to_string());
                    assistant
                        .metadata
                        .insert("error".to_string(), serde_json::json!(error.to_string()));
                    assistant.add_text(format!("Scheduler error: {}", error));
                }
            }
        }
    }

    ensure_default_session_title(&mut session, provider.clone(), &model_id).await;
    let assistant_text = session
        .get_message(&assistant_message_id)
        .map(assistant_visible_text)
        .unwrap_or_default();

    {
        let mut sessions = state.sessions.lock().await;
        sessions.update(session);
    }
    broadcast_session_updated(state.as_ref(), session_id.clone(), "prompt.completed");
    set_session_run_status(&state, &session_id, SessionRunStatus::Idle).await;

    if let Some(output_hook) = output_hook {
        if !assistant_text.trim().is_empty() {
            emit_output_block_via_hook(
                Some(&output_hook),
                OutputBlockEvent {
                    session_id: session_id.clone(),
                    block: OutputBlock::Message(MessageBlock::full(
                        OutputMessageRole::Assistant,
                        assistant_text.clone(),
                    )),
                    id: Some(assistant_message_id.clone()),
                },
            )
            .await;
        }
    }

    Ok(LocalSchedulerPromptOutcome {
        session_id,
        assistant_text,
        prompt_tokens,
        completion_tokens,
        cancelled,
    })
}

pub async fn abort_local_session_execution(
    state: Arc<ServerState>,
    session_id: &str,
    scheduler_stage_only: bool,
) -> serde_json::Value {
    abort_session_execution(&state, session_id, scheduler_stage_only).await
}
