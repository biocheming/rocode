use async_trait::async_trait;
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{delete, get, patch, post},
    Json, Router,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, RwLock};

use crate::session_runtime::{
    ensure_default_session_title, first_user_message_text, SessionSchedulerLifecycleHook,
};
use crate::{ApiError, Result, ServerState};
use rocode_agent::{AgentInfo, AgentMode, AgentRegistry};
use rocode_command::{CommandContext, CommandRegistry};
use rocode_config::{Config as AppConfig, SkillTreeNodeConfig};
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

use super::stream::stream_message;
use super::tui::request_question_answers;
use super::{
    apply_plugin_config_hooks, get_plugin_loader, plugin_auth::ensure_plugin_loader_active,
    should_apply_plugin_config_hooks,
};

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

fn scheduler_system_prompt_preview(profile_name: &str, profile: &SchedulerProfileConfig) -> String {
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

fn scheduler_mode_kind(profile_name: &str) -> &'static str {
    if SchedulerPresetKind::from_str(profile_name).is_ok() {
        "preset"
    } else {
        "profile"
    }
}

fn resolve_scheduler_profile_config(
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
struct SchedulerAgentResolver {
    registry: Arc<AgentRegistry>,
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

#[derive(Clone)]
struct SessionSchedulerModelResolver {
    state: Arc<ServerState>,
    fallback_provider_id: String,
    fallback_model_id: String,
    variant: Option<String>,
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

        let mut request = rocode_provider::ChatRequest::new(model_id, messages).with_tools(tools);
        request.variant = self.variant.clone();
        provider
            .chat_stream(request)
            .await
            .map_err(|error| OrchestratorError::ModelError(error.to_string()))
    }
}

#[derive(Clone)]
struct SessionSchedulerToolExecutor {
    state: Arc<ServerState>,
    session_id: String,
    message_id: String,
    directory: String,
    current_model: Option<String>,
    tool_runtime_config: rocode_tool::ToolRuntimeConfig,
    agent_registry: Arc<AgentRegistry>,
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
                async move {
                    Ok(agent_registry
                        .get(&name)
                        .map(|info| rocode_tool::TaskAgentInfo {
                            name: info.name.clone(),
                            model: info.model.as_ref().map(|m| rocode_tool::TaskAgentModel {
                                provider_id: m.provider_id.clone(),
                                model_id: m.model_id.clone(),
                            }),
                            can_use_task: info.is_tool_allowed("task"),
                            steps: info.max_steps,
                        }))
                }
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
        base_ctx.call_id = exec_ctx
            .metadata
            .get("call_id")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        base_ctx.extra = exec_ctx.metadata.clone();
        base_ctx
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
    pub agent_params: rocode_session::AgentParams,
}

fn resolve_request_model_inputs(
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

pub(crate) async fn resolve_prompt_request_config(
    state: &Arc<ServerState>,
    config: &AppConfig,
    requested_agent: Option<&str>,
    requested_scheduler_profile: Option<&str>,
    request_model: Option<&str>,
    _variant: Option<&str>,
    route: &'static str,
) -> Result<ResolvedPromptRequestConfig> {
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

    let agent_params = rocode_session::AgentParams {
        max_tokens: resolved_agent.as_ref().and_then(|agent| agent.max_tokens),
        temperature: resolved_agent.as_ref().and_then(|agent| agent.temperature),
        top_p: resolved_agent.as_ref().and_then(|agent| agent.top_p),
    };
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
        agent_params,
    })
}
pub(crate) fn session_routes() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/", get(list_sessions).post(create_session))
        .route("/status", get(session_status))
        .route(
            "/{id}",
            get(get_session)
                .patch(update_session)
                .delete(delete_session),
        )
        .route("/{id}/children", get(get_session_children))
        .route("/{id}/todo", get(get_session_todos))
        .route("/{id}/fork", post(fork_session))
        .route("/{id}/abort", post(abort_session))
        .route("/{id}/share", post(share_session).delete(unshare_session))
        .route("/{id}/archive", post(archive_session))
        .route("/{id}/title", patch(set_session_title))
        .route("/{id}/permission", patch(set_session_permission))
        .route(
            "/{id}/summary",
            get(get_session_summary).patch(set_session_summary),
        )
        .route(
            "/{id}/revert",
            post(session_revert).delete(clear_session_revert),
        )
        .route("/{id}/unrevert", post(session_unrevert))
        .route("/{id}/compaction", post(start_compaction))
        .route("/{id}/summarize", post(summarize_session))
        .route("/{id}/init", post(init_session))
        .route("/{id}/command", post(execute_command))
        .route("/{id}/shell", post(execute_shell))
        .route("/{id}/message", post(send_message).get(list_messages))
        .route(
            "/{id}/message/{msgID}",
            get(get_message).delete(delete_message),
        )
        .route("/{id}/message/{msgID}/part", post(add_message_part))
        .route(
            "/{id}/message/{msgID}/part/{partID}",
            delete(delete_part).patch(update_part),
        )
        .route("/{id}/tool/{tool_call_id}/cancel", post(cancel_tool_call))
        .route("/{id}/stream", post(stream_message))
        .route("/{id}/prompt", post(session_prompt))
        .route("/{id}/prompt/abort", post(abort_prompt))
        .route("/{id}/prompt_async", post(prompt_async))
        .route("/{id}/diff", get(get_session_diff))
}

#[derive(Debug, Deserialize)]
pub struct ListSessionsQuery {
    pub directory: Option<String>,
    pub roots: Option<bool>,
    pub start: Option<i64>,
    pub search: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub slug: String,
    pub project_id: String,
    pub directory: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub version: String,
    pub time: SessionTimeInfo,
    pub summary: Option<SessionSummaryInfo>,
    pub share: Option<SessionShareInfo>,
    pub revert: Option<SessionRevertInfo>,
    pub permission: Option<PermissionRulesetInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize)]
pub struct SessionTimeInfo {
    pub created: i64,
    pub updated: i64,
    pub compacting: Option<i64>,
    pub archived: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct SessionSummaryInfo {
    pub additions: u64,
    pub deletions: u64,
    pub files: u64,
}

#[derive(Debug, Serialize)]
pub struct SessionShareInfo {
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct SessionRevertInfo {
    pub message_id: String,
    pub part_id: Option<String>,
    pub snapshot: Option<String>,
    pub diff: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PermissionRulesetInfo {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
    pub mode: Option<String>,
}

fn session_to_info(session: &rocode_session::Session) -> SessionInfo {
    SessionInfo {
        id: session.id.clone(),
        slug: session.slug.clone(),
        project_id: session.project_id.clone(),
        directory: session.directory.clone(),
        parent_id: session.parent_id.clone(),
        title: session.title.clone(),
        version: session.version.clone(),
        time: SessionTimeInfo {
            created: session.time.created,
            updated: session.time.updated,
            compacting: session.time.compacting,
            archived: session.time.archived,
        },
        summary: session.summary.as_ref().map(|s| SessionSummaryInfo {
            additions: s.additions,
            deletions: s.deletions,
            files: s.files,
        }),
        share: session
            .share
            .as_ref()
            .map(|s| SessionShareInfo { url: s.url.clone() }),
        revert: session.revert.as_ref().map(|r| SessionRevertInfo {
            message_id: r.message_id.clone(),
            part_id: r.part_id.clone(),
            snapshot: r.snapshot.clone(),
            diff: r.diff.clone(),
        }),
        permission: session.permission.as_ref().map(|p| PermissionRulesetInfo {
            allow: p.allow.clone(),
            deny: p.deny.clone(),
            mode: p.mode.clone(),
        }),
        metadata: if session.metadata.is_empty() {
            None
        } else {
            Some(session.metadata.clone())
        },
    }
}

async fn persist_sessions_if_enabled(state: &Arc<ServerState>) {
    if let Err(err) = state.sync_sessions_to_storage().await {
        tracing::error!("failed to sync sessions to storage: {}", err);
    }
}

async fn list_sessions(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<ListSessionsQuery>,
) -> Result<Json<Vec<SessionInfo>>> {
    let filter = rocode_session::SessionFilter {
        directory: query.directory,
        roots: query.roots.unwrap_or(false),
        start: query.start,
        search: query.search,
        limit: query.limit,
    };
    let manager = state.sessions.lock().await;
    let sessions = manager.list_filtered(filter);
    let infos: Vec<SessionInfo> = sessions.into_iter().map(session_to_info).collect();
    Ok(Json(infos))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum SessionRunStatus {
    Idle,
    Busy,
    Retry {
        attempt: u32,
        message: String,
        next: i64,
    },
}

impl Default for SessionRunStatus {
    fn default() -> Self {
        Self::Idle
    }
}

static SESSION_RUN_STATUS: Lazy<RwLock<HashMap<String, SessionRunStatus>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

async fn set_session_run_status(
    state: &Arc<ServerState>,
    session_id: &str,
    status: SessionRunStatus,
) {
    {
        let mut statuses = SESSION_RUN_STATUS.write().await;
        match &status {
            SessionRunStatus::Idle => {
                statuses.remove(session_id);
            }
            _ => {
                statuses.insert(session_id.to_string(), status.clone());
            }
        }
    }

    state.broadcast(
        &serde_json::json!({
            "type": "session.status",
            "sessionID": session_id,
            "status": status,
        })
        .to_string(),
    );
}

/// Drop guard that sets session status to idle when the prompt task exits.
/// Mirrors the TS `defer(() => cancel(sessionID))` pattern to guarantee
/// the spinner stops even if the spawned task panics.
struct IdleGuard {
    state: Arc<ServerState>,
    session_id: Option<String>,
}

impl IdleGuard {
    /// Defuse the guard — the caller will handle cleanup explicitly.
    fn defuse(&mut self) {
        self.session_id = None;
    }
}

impl Drop for IdleGuard {
    fn drop(&mut self) {
        let Some(sid) = self.session_id.take() else {
            return;
        };
        let state = self.state.clone();
        tokio::spawn(async move {
            set_session_run_status(&state, &sid, SessionRunStatus::Idle).await;
        });
    }
}

async fn session_status(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<HashMap<String, SessionStatusInfo>>> {
    let run_status = SESSION_RUN_STATUS.read().await.clone();
    let manager = state.sessions.lock().await;
    let sessions = manager.list();
    let status: HashMap<String, SessionStatusInfo> = sessions
        .into_iter()
        .map(|s| {
            let lifecycle_status = match s.status {
                rocode_session::SessionStatus::Active => "active",
                rocode_session::SessionStatus::Completed => "completed",
                rocode_session::SessionStatus::Archived => "archived",
                rocode_session::SessionStatus::Compacting => "compacting",
            };
            let run = run_status.get(&s.id).cloned().unwrap_or_default();
            let (status, idle, busy, attempt, message, next) = match run {
                SessionRunStatus::Idle => {
                    (lifecycle_status.to_string(), true, false, None, None, None)
                }
                SessionRunStatus::Busy => ("busy".to_string(), false, true, None, None, None),
                SessionRunStatus::Retry {
                    attempt,
                    message,
                    next,
                } => (
                    "retry".to_string(),
                    false,
                    true,
                    Some(attempt),
                    Some(message),
                    Some(next),
                ),
            };
            (
                s.id.clone(),
                SessionStatusInfo {
                    status,
                    idle,
                    busy,
                    attempt,
                    message,
                    next,
                },
            )
        })
        .collect();
    Ok(Json(status))
}

#[derive(Debug, Serialize)]
pub struct SessionStatusInfo {
    pub status: String,
    pub idle: bool,
    pub busy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub parent_id: Option<String>,
    pub title: Option<String>,
    pub permission: Option<PermissionRulesetInput>,
    pub scheduler_profile: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PermissionRulesetInput {
    pub allow: Option<Vec<String>>,
    pub deny: Option<Vec<String>>,
    pub mode: Option<String>,
}

async fn create_session(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let mut session = if let Some(parent_id) = &req.parent_id {
        sessions
            .create_child(parent_id)
            .ok_or_else(|| ApiError::SessionNotFound(parent_id.clone()))?
    } else {
        sessions.create("default", resolved_session_directory("."))
    };
    let normalized_directory = resolved_session_directory(&session.directory);
    if session.directory != normalized_directory {
        session.directory = normalized_directory;
        sessions.update(session.clone());
    }
    if let Some(profile) = req
        .scheduler_profile
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        session
            .metadata
            .insert("scheduler_profile".to_string(), serde_json::json!(profile));
        session
            .metadata
            .insert("scheduler_applied".to_string(), serde_json::json!(true));
        sessions.update(session.clone());
    }
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(session_to_info(&session)))
}

pub(crate) fn resolved_session_directory(raw: &str) -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let trimmed = raw.trim();
    let candidate = if trimmed.is_empty() || trimmed == "." {
        cwd
    } else {
        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            path
        } else {
            cwd.join(path)
        }
    };
    candidate
        .canonicalize()
        .unwrap_or(candidate)
        .to_string_lossy()
        .to_string()
}

#[derive(Debug, Clone)]
struct ResolvedPromptPayload {
    display_text: String,
    execution_text: String,
    agent: Option<String>,
    scheduler_profile: Option<String>,
}

async fn resolve_prompt_payload(
    display_text: &str,
    session_id: &str,
    session_directory: &str,
) -> Result<ResolvedPromptPayload> {
    let mut registry = CommandRegistry::new();
    registry
        .load_from_directory(&PathBuf::from(session_directory))
        .map_err(|error| ApiError::BadRequest(format!("Failed to load commands: {}", error)))?;

    let Some((command, arguments)) = registry.parse(display_text) else {
        return Ok(ResolvedPromptPayload {
            display_text: display_text.to_string(),
            execution_text: display_text.to_string(),
            agent: None,
            scheduler_profile: None,
        });
    };

    let mut ctx = CommandContext::new(PathBuf::from(session_directory)).with_arguments(arguments);
    ctx = ctx
        .with_variable("SESSION_ID".to_string(), session_id.to_string())
        .with_variable("TIMESTAMP".to_string(), chrono::Utc::now().to_rfc3339());
    let execution_text = registry
        .execute_with_hooks(&command.name, ctx)
        .await
        .map_err(|error| {
            ApiError::BadRequest(format!(
                "Failed to execute command `/{}`: {}",
                command.name, error
            ))
        })?;

    Ok(ResolvedPromptPayload {
        display_text: display_text.to_string(),
        execution_text,
        agent: None,
        scheduler_profile: command.scheduler_profile.clone(),
    })
}

async fn get_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<SessionInfo>> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id))?;
    Ok(Json(session_to_info(session)))
}

async fn delete_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    state
        .sessions
        .lock()
        .await
        .delete(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    SESSION_RUN_STATUS.write().await.remove(&id);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

async fn get_session_children(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<SessionInfo>>> {
    let manager = state.sessions.lock().await;
    let children = manager.children(&id);
    Ok(Json(children.into_iter().map(session_to_info).collect()))
}

#[derive(Debug, Serialize)]
pub struct TodoInfo {
    pub id: String,
    pub content: String,
    pub status: String,
    pub priority: String,
}

static TODO_MANAGER: Lazy<rocode_session::TodoManager> =
    Lazy::new(rocode_session::TodoManager::new);

async fn get_session_todos(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<TodoInfo>>> {
    let sessions = state.sessions.lock().await;
    if sessions.get(&id).is_none() {
        return Err(ApiError::SessionNotFound(id));
    }
    drop(sessions);

    let todos = TODO_MANAGER.get(&id).await;
    let items = todos
        .into_iter()
        .enumerate()
        .map(|(idx, todo)| TodoInfo {
            id: format!("{}_{}", id, idx),
            content: todo.content,
            status: todo.status,
            priority: todo.priority,
        })
        .collect();
    Ok(Json(items))
}

#[derive(Debug, Deserialize)]
pub struct ForkSessionRequest {
    pub message_id: Option<String>,
}

async fn fork_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<ForkSessionRequest>,
) -> Result<Json<SessionInfo>> {
    let forked = state
        .sessions
        .lock()
        .await
        .fork(&id, req.message_id.as_deref())
        .ok_or_else(|| ApiError::SessionNotFound(id))?;
    persist_sessions_if_enabled(&state).await;
    Ok(Json(session_to_info(&forked)))
}

async fn share_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<SessionShareInfo>> {
    let mut sessions = state.sessions.lock().await;
    let share_url = format!("https://share.opencode.ai/{}", id);
    sessions
        .share(&id, share_url.clone())
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(SessionShareInfo { url: share_url }))
}

async fn unshare_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    sessions
        .unshare(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(serde_json::json!({ "unshared": true })))
}

#[derive(Debug, Deserialize)]
pub struct ArchiveSessionRequest {
    pub archive: Option<bool>,
}

async fn archive_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<ArchiveSessionRequest>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let info = if req.archive.unwrap_or(true) {
        let updated = sessions
            .set_archived(&id, None)
            .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
        session_to_info(&updated)
    } else {
        let session = sessions
            .get(&id)
            .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
        session_to_info(session)
    };
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

#[derive(Debug, Deserialize)]
pub struct SetTitleRequest {
    pub title: String,
}

async fn set_session_title(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<SetTitleRequest>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    session.set_title(&req.title);
    let updated = session.clone();
    sessions.update(updated.clone());
    let info = session_to_info(&updated);
    drop(sessions);
    state.broadcast(
        &serde_json::json!({
            "type": "session.updated",
            "sessionID": id,
            "source": "session.title.set",
        })
        .to_string(),
    );
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

async fn set_session_permission(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<PermissionRulesetInput>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let updated = sessions
        .set_permission(
            &id,
            rocode_session::PermissionRuleset {
                allow: req.allow.unwrap_or_default(),
                deny: req.deny.unwrap_or_default(),
                mode: req.mode,
            },
        )
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    let info = session_to_info(&updated);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

async fn get_session_summary(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<Option<SessionSummaryInfo>>> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    Ok(Json(session.summary.as_ref().map(|s| SessionSummaryInfo {
        additions: s.additions,
        deletions: s.deletions,
        files: s.files,
    })))
}

#[derive(Debug, Deserialize)]
pub struct SetSummaryRequest {
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub files: Option<u64>,
}

async fn set_session_summary(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<SetSummaryRequest>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let updated = sessions
        .set_summary(
            &id,
            rocode_session::SessionSummary {
                additions: req.additions.unwrap_or(0),
                deletions: req.deletions.unwrap_or(0),
                files: req.files.unwrap_or(0),
                diffs: None,
            },
        )
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    let info = session_to_info(&updated);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

#[derive(Debug, Deserialize)]
pub struct RevertRequest {
    pub message_id: String,
    pub part_id: Option<String>,
    pub snapshot: Option<String>,
    pub diff: Option<String>,
}

async fn session_revert(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<RevertRequest>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let updated = sessions
        .set_revert(
            &id,
            rocode_session::SessionRevert {
                message_id: req.message_id,
                part_id: req.part_id,
                snapshot: req.snapshot,
                diff: req.diff,
            },
        )
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    let info = session_to_info(&updated);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

async fn clear_session_revert(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let updated = sessions
        .clear_revert(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    let info = session_to_info(&updated);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

async fn start_compaction(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    session.start_compacting();
    let info = session_to_info(session);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub scheduler_profile: Option<String>,
    pub variant: Option<String>,
    pub stream: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct MessageInfo {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub parts: Vec<PartInfo>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub mode: Option<String>,
    pub finish: Option<String>,
    pub error: Option<String>,
    pub cost: f64,
    pub tokens: MessageTokensInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize, Default)]
pub struct MessageTokensInfo {
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

#[derive(Debug, Serialize)]
pub struct PartInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub part_type: String,
    pub text: Option<String>,
    pub file: Option<MessageFileInfo>,
    pub tool_call: Option<ToolCallInfo>,
    pub tool_result: Option<ToolResultInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthetic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignored: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct MessageFileInfo {
    pub url: String,
    pub filename: String,
    pub mime: String,
}

#[derive(Debug, Serialize)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct ToolResultInfo {
    pub tool_call_id: String,
    pub content: String,
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<serde_json::Value>>,
}

fn message_role_name(role: &rocode_session::MessageRole) -> &'static str {
    match role {
        rocode_session::MessageRole::User => "user",
        rocode_session::MessageRole::Assistant => "assistant",
        rocode_session::MessageRole::System => "system",
        rocode_session::MessageRole::Tool => "tool",
    }
}

fn part_type_name(part_type: &rocode_session::PartType) -> &'static str {
    match part_type {
        rocode_session::PartType::Text { .. } => "text",
        rocode_session::PartType::ToolCall { .. } => "tool_call",
        rocode_session::PartType::ToolResult { .. } => "tool_result",
        rocode_session::PartType::Reasoning { .. } => "reasoning",
        rocode_session::PartType::File { .. } => "file",
        rocode_session::PartType::StepStart { .. } => "step_start",
        rocode_session::PartType::StepFinish { .. } => "step_finish",
        rocode_session::PartType::Snapshot { .. } => "snapshot",
        rocode_session::PartType::Patch { .. } => "patch",
        rocode_session::PartType::Agent { .. } => "agent",
        rocode_session::PartType::Subtask { .. } => "subtask",
        rocode_session::PartType::Retry { .. } => "retry",
        rocode_session::PartType::Compaction { .. } => "compaction",
    }
}

fn part_to_info(part: &rocode_session::MessagePart) -> PartInfo {
    let (synthetic, ignored) = match &part.part_type {
        rocode_session::PartType::Text {
            synthetic, ignored, ..
        } => (*synthetic, *ignored),
        _ => (None, None),
    };
    PartInfo {
        id: part.id.clone(),
        part_type: part_type_name(&part.part_type).to_string(),
        text: match &part.part_type {
            rocode_session::PartType::Text { text, .. } => Some(text.clone()),
            rocode_session::PartType::Reasoning { text } => Some(text.clone()),
            rocode_session::PartType::Compaction { summary } => Some(summary.clone()),
            _ => None,
        },
        file: if let rocode_session::PartType::File {
            url,
            filename,
            mime,
        } = &part.part_type
        {
            Some(MessageFileInfo {
                url: url.clone(),
                filename: filename.clone(),
                mime: mime.clone(),
            })
        } else {
            None
        },
        tool_call: if let rocode_session::PartType::ToolCall {
            id,
            name,
            input,
            status,
            raw,
            state,
            ..
        } = &part.part_type
        {
            Some(ToolCallInfo {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
                status: Some(
                    match status {
                        rocode_session::ToolCallStatus::Pending => "pending",
                        rocode_session::ToolCallStatus::Running => "running",
                        rocode_session::ToolCallStatus::Completed => "completed",
                        rocode_session::ToolCallStatus::Error => "error",
                    }
                    .to_string(),
                ),
                raw: raw.clone(),
                state: state.as_ref().and_then(|s| serde_json::to_value(s).ok()),
            })
        } else {
            None
        },
        tool_result: if let rocode_session::PartType::ToolResult {
            tool_call_id,
            content,
            is_error,
            title,
            metadata,
            attachments,
        } = &part.part_type
        {
            Some(ToolResultInfo {
                tool_call_id: tool_call_id.clone(),
                content: content.clone(),
                is_error: *is_error,
                title: title.clone(),
                metadata: metadata.clone(),
                attachments: attachments.clone(),
            })
        } else {
            None
        },
        synthetic,
        ignored,
    }
}

fn message_to_info(session_id: &str, message: &rocode_session::SessionMessage) -> MessageInfo {
    let usage = message.usage.clone().unwrap_or_default();
    let model_id = message
        .metadata
        .get("model_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let model_provider = message
        .metadata
        .get("model_provider")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let model = match (model_provider.as_deref(), model_id.as_deref()) {
        (Some(provider), Some(model)) => Some(format!("{}/{}", provider, model)),
        (None, Some(model)) => Some(model.to_string()),
        _ => None,
    };
    let cost = if usage.total_cost > 0.0 {
        usage.total_cost
    } else {
        message
            .metadata
            .get("cost")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    };

    MessageInfo {
        id: message.id.clone(),
        session_id: session_id.to_string(),
        role: message_role_name(&message.role).to_string(),
        parts: message.parts.iter().map(part_to_info).collect(),
        created_at: message.created_at.timestamp_millis(),
        completed_at: message
            .metadata
            .get("completed_at")
            .and_then(|v| v.as_i64()),
        agent: message
            .metadata
            .get("agent")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        model,
        mode: message
            .metadata
            .get("mode")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        finish: message.finish.clone().or_else(|| {
            message
                .metadata
                .get("finish_reason")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }),
        error: message
            .metadata
            .get("error")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        cost,
        tokens: MessageTokensInfo {
            input: usage.input_tokens,
            output: usage.output_tokens,
            reasoning: usage.reasoning_tokens,
            cache_read: usage.cache_read_tokens,
            cache_write: usage.cache_write_tokens,
        },
        metadata: if message.metadata.is_empty() {
            None
        } else {
            Some(message.metadata.clone())
        },
    }
}

pub(crate) async fn resolve_provider_and_model(
    state: &ServerState,
    request_model: Option<&str>,
    config_model: Option<&str>,
    config_provider: Option<&str>,
) -> Result<(Arc<dyn rocode_provider::Provider>, String, String)> {
    let providers = state.providers.read().await;
    let resolve_from_model = |model: &str| -> Result<(String, String)> {
        providers
            .parse_model_string(model)
            .ok_or_else(|| ApiError::BadRequest(format!("Model not found: {}", model)))
    };

    let (provider_id, model_id) = if let Some(model) = request_model {
        resolve_from_model(model)?
    } else if let Some(model) = config_model {
        if let Some(provider_id) = config_provider {
            (provider_id.to_string(), model.to_string())
        } else {
            resolve_from_model(model)?
        }
    } else {
        let first = providers
            .list_models()
            .into_iter()
            .next()
            .ok_or_else(|| ApiError::BadRequest("No providers configured".to_string()))?;
        (first.provider, first.id)
    };

    let provider = providers
        .get_provider(&provider_id)
        .map_err(|e| ApiError::ProviderError(e.to_string()))?;
    if provider.get_model(&model_id).is_none() {
        return Err(ApiError::BadRequest(format!(
            "Model `{}` not found for provider `{}`",
            model_id, provider_id
        )));
    }

    Ok((provider, provider_id, model_id))
}

async fn send_message(
    State(state): State<Arc<ServerState>>,
    Path(session_id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Json<MessageInfo>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    session.add_user_message(&req.content);
    if let Some(variant) = req.variant.as_deref() {
        session
            .metadata
            .insert("model_variant".to_string(), serde_json::json!(variant));
    }
    let assistant_msg = session.add_assistant_message();
    let info = message_to_info(&session_id, assistant_msg);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

async fn list_messages(
    State(state): State<Arc<ServerState>>,
    Path(session_id): Path<String>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<Vec<MessageInfo>>> {
    state
        .api_perf
        .list_messages_calls
        .fetch_add(1, Ordering::Relaxed);
    if query.after.is_some() {
        state
            .api_perf
            .list_messages_incremental_calls
            .fetch_add(1, Ordering::Relaxed);
    } else {
        state
            .api_perf
            .list_messages_full_calls
            .fetch_add(1, Ordering::Relaxed);
    }

    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    let limit = query.limit.filter(|value| *value > 0);
    let mut started = query.after.is_none();
    let mut messages = Vec::new();

    for message in &session.messages {
        if !started {
            if query.after.as_deref() == Some(message.id.as_str()) {
                started = true;
            }
            continue;
        }
        messages.push(message_to_info(&session_id, message));
        if let Some(limit) = limit {
            if messages.len() >= limit {
                break;
            }
        }
    }

    // If the anchor message is unknown, fall back to a full list so clients can recover.
    if query.after.is_some() && !started {
        messages.clear();
        for message in &session.messages {
            messages.push(message_to_info(&session_id, message));
            if let Some(limit) = limit {
                if messages.len() >= limit {
                    break;
                }
            }
        }
    }

    Ok(Json(messages))
}

#[derive(Debug, Deserialize)]
pub struct ListMessagesQuery {
    pub after: Option<String>,
    pub limit: Option<usize>,
}

async fn delete_message(
    State(state): State<Arc<ServerState>>,
    Path((session_id, msg_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    session.remove_message(&msg_id);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[derive(Debug, Deserialize)]
pub struct AddPartRequest {
    #[serde(rename = "type")]
    pub part_type: String,
    pub text: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub tool_status: Option<String>,
    pub tool_raw_input: Option<String>,
    pub content: Option<String>,
    pub is_error: Option<bool>,
    pub title: Option<String>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub attachments: Option<Vec<serde_json::Value>>,
}

fn build_message_part(req: AddPartRequest, msg_id: &str) -> Result<rocode_session::MessagePart> {
    let part_type = match req.part_type.as_str() {
        "text" => rocode_session::PartType::Text {
            text: req.text.ok_or_else(|| {
                ApiError::BadRequest("Field `text` is required for text parts".to_string())
            })?,
            synthetic: None,
            ignored: None,
        },
        "reasoning" => rocode_session::PartType::Reasoning {
            text: req.text.ok_or_else(|| {
                ApiError::BadRequest("Field `text` is required for reasoning parts".to_string())
            })?,
        },
        "tool_call" => rocode_session::PartType::ToolCall {
            id: req.tool_call_id.ok_or_else(|| {
                ApiError::BadRequest(
                    "Field `tool_call_id` is required for tool_call parts".to_string(),
                )
            })?,
            name: req.tool_name.ok_or_else(|| {
                ApiError::BadRequest(
                    "Field `tool_name` is required for tool_call parts".to_string(),
                )
            })?,
            input: req.tool_input.unwrap_or_else(|| serde_json::json!({})),
            status: match req
                .tool_status
                .as_deref()
                .unwrap_or("pending")
                .to_ascii_lowercase()
                .as_str()
            {
                "running" => rocode_session::ToolCallStatus::Running,
                "completed" => rocode_session::ToolCallStatus::Completed,
                "error" => rocode_session::ToolCallStatus::Error,
                _ => rocode_session::ToolCallStatus::Pending,
            },
            raw: req.tool_raw_input,
            state: None,
        },
        "tool_result" => rocode_session::PartType::ToolResult {
            tool_call_id: req.tool_call_id.ok_or_else(|| {
                ApiError::BadRequest(
                    "Field `tool_call_id` is required for tool_result parts".to_string(),
                )
            })?,
            content: req.content.ok_or_else(|| {
                ApiError::BadRequest(
                    "Field `content` is required for tool_result parts".to_string(),
                )
            })?,
            is_error: req.is_error.unwrap_or(false),
            title: req.title,
            metadata: req.metadata,
            attachments: req.attachments,
        },
        unsupported => {
            return Err(ApiError::BadRequest(format!(
                "Unsupported part type: {}",
                unsupported
            )));
        }
    };

    Ok(rocode_session::MessagePart {
        id: format!("prt_{}", uuid::Uuid::new_v4().simple()),
        part_type,
        created_at: chrono::Utc::now(),
        message_id: Some(msg_id.to_string()),
    })
}

async fn add_message_part(
    State(state): State<Arc<ServerState>>,
    Path((session_id, msg_id)): Path<(String, String)>,
    Json(req): Json<AddPartRequest>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    let message = session
        .get_message_mut(&msg_id)
        .ok_or_else(|| ApiError::NotFound(format!("Message not found: {}", msg_id)))?;

    let part = build_message_part(req, &msg_id)?;
    let part_id = part.id.clone();
    message.parts.push(part);
    session.touch();
    drop(sessions);
    persist_sessions_if_enabled(&state).await;

    Ok(Json(serde_json::json!({
        "added": true,
        "session_id": session_id,
        "message_id": msg_id,
        "part_id": part_id,
    })))
}

async fn delete_part(
    State(state): State<Arc<ServerState>>,
    Path((session_id, msg_id, part_id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    let message = session
        .get_message_mut(&msg_id)
        .ok_or_else(|| ApiError::NotFound(format!("Message not found: {}", msg_id)))?;

    let before = message.parts.len();
    message.parts.retain(|part| part.id != part_id);
    if message.parts.len() == before {
        return Err(ApiError::NotFound(format!("Part not found: {}", part_id)));
    }
    session.touch();
    drop(sessions);
    persist_sessions_if_enabled(&state).await;

    Ok(Json(serde_json::json!({
        "deleted": true,
        "session_id": session_id,
        "message_id": msg_id,
        "part_id": part_id,
    })))
}

#[derive(Debug, Deserialize)]
pub struct SessionPromptRequest {
    pub message: Option<String>,
    pub model: Option<String>,
    pub variant: Option<String>,
    pub agent: Option<String>,
    pub scheduler_profile: Option<String>,
    pub command: Option<String>,
    pub arguments: Option<String>,
}

async fn session_prompt(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<SessionPromptRequest>,
) -> Result<Json<serde_json::Value>> {
    if req.agent.is_some() && req.scheduler_profile.is_some() {
        return Err(ApiError::BadRequest(
            "`agent` and `scheduler_profile` are mutually exclusive".to_string(),
        ));
    }

    let display_prompt_text = if let Some(message) = req.message.as_deref() {
        message.to_string()
    } else if let Some(command) = req.command.as_deref() {
        req.arguments
            .as_deref()
            .map(|args| format!("/{command} {args}"))
            .unwrap_or_else(|| format!("/{command}"))
    } else {
        return Err(ApiError::BadRequest(
            "Either `message` or `command` must be provided".to_string(),
        ));
    };

    let session_directory = {
        let sessions = state.sessions.lock().await;
        let Some(session) = sessions.get(&id) else {
            return Err(ApiError::SessionNotFound(id));
        };
        resolved_session_directory(&session.directory)
    };
    let _ = ensure_plugin_loader_active(&state).await?;

    let resolved_prompt =
        resolve_prompt_payload(&display_prompt_text, &id, &session_directory).await?;
    let prompt_text = resolved_prompt.execution_text.clone();
    let display_prompt_text = resolved_prompt.display_text.clone();
    let effective_agent = resolved_prompt.agent.clone().or(req.agent.clone());
    let effective_scheduler_profile = resolved_prompt
        .scheduler_profile
        .clone()
        .or(req.scheduler_profile.clone());

    let config = if let Some(loader) = get_plugin_loader() {
        if should_apply_plugin_config_hooks(&headers) {
            let mut cfg = (*state.config_store.config()).clone();
            apply_plugin_config_hooks(loader, &mut cfg).await;
            state.config_store.set_plugin_applied(cfg.clone()).await;
            Arc::new(cfg)
        } else {
            // Internal request: use cached plugin-applied config snapshot so that
            // plugin-injected agent configs (model/prompt/permission) are available.
            state
                .config_store
                .plugin_applied()
                .await
                .unwrap_or_else(|| state.config_store.config())
        }
    } else {
        state.config_store.config()
    };

    let request_config = resolve_prompt_request_config(
        &state,
        &config,
        effective_agent.as_deref(),
        effective_scheduler_profile.as_deref(),
        req.model.as_deref(),
        req.variant.as_deref(),
        "session",
    )
    .await?;
    let scheduler_applied = request_config.scheduler_applied;
    let scheduler_profile_name = request_config.scheduler_profile_name.clone();
    let scheduler_root_agent = request_config.scheduler_root_agent.clone();
    let scheduler_skill_tree_applied = request_config.scheduler_skill_tree_applied;
    let resolved_agent = request_config.resolved_agent.clone();
    let provider = request_config.provider.clone();
    let provider_id = request_config.provider_id.clone();
    let model_id = request_config.model_id.clone();
    let agent_system_prompt = request_config.agent_system_prompt.clone();
    let agent_params = request_config.agent_params.clone();

    let task_state = state.clone();
    let session_id = id.clone();
    let task_variant = req.variant.clone();
    let task_agent = resolved_agent.as_ref().map(|agent| agent.name.clone());
    let task_model = model_id.clone();
    let task_provider_client = provider.clone();
    let task_provider = provider_id.clone();
    let task_system_prompt = agent_system_prompt.clone();
    let task_agent_params = agent_params.clone();
    let task_scheduler_applied = scheduler_applied;
    let task_scheduler_profile_name = scheduler_profile_name.clone();
    let task_scheduler_root_agent = scheduler_root_agent.clone();
    let task_scheduler_skill_tree_applied = scheduler_skill_tree_applied;
    let task_config = config.clone();
    let task_scheduler_profile_config = task_scheduler_profile_name
        .as_deref()
        .and_then(|profile_name| resolve_scheduler_profile_config(&task_config, Some(profile_name)))
        .map(|(_, profile)| profile);
    tokio::spawn(async move {
        let mut session = {
            let sessions = task_state.sessions.lock().await;
            let Some(session) = sessions.get(&session_id).cloned() else {
                return;
            };
            session
        };
        let normalized_directory = resolved_session_directory(&session.directory);
        if session.directory != normalized_directory {
            session.directory = normalized_directory;
        }
        set_session_run_status(&task_state, &session_id, SessionRunStatus::Busy).await;

        // Safety guard: ensure status is always set to idle when this block
        // exits, mirroring the TS `defer(() => cancel(sessionID))` pattern.
        // This prevents the spinner from getting stuck if anything panics.
        let mut _idle_guard = IdleGuard {
            state: task_state.clone(),
            session_id: Some(session_id.clone()),
        };

        if let Some(variant) = task_variant.as_deref() {
            session
                .metadata
                .insert("model_variant".to_string(), serde_json::json!(variant));
        } else {
            session.metadata.remove("model_variant");
        }
        session.metadata.insert(
            "model_provider".to_string(),
            serde_json::json!(&task_provider),
        );
        session
            .metadata
            .insert("model_id".to_string(), serde_json::json!(&task_model));
        if let Some(agent) = task_agent.as_deref() {
            session
                .metadata
                .insert("agent".to_string(), serde_json::json!(agent));
        } else {
            session.metadata.remove("agent");
        }
        session.metadata.insert(
            "scheduler_applied".to_string(),
            serde_json::json!(task_scheduler_applied),
        );
        session.metadata.insert(
            "scheduler_skill_tree_applied".to_string(),
            serde_json::json!(task_scheduler_skill_tree_applied),
        );
        if let Some(profile) = task_scheduler_profile_name.as_deref() {
            session
                .metadata
                .insert("scheduler_profile".to_string(), serde_json::json!(profile));
        } else {
            session.metadata.remove("scheduler_profile");
        }
        if let Some(root_agent) = task_scheduler_root_agent.as_deref() {
            session.metadata.insert(
                "scheduler_root_agent".to_string(),
                serde_json::json!(root_agent),
            );
        } else {
            session.metadata.remove("scheduler_root_agent");
        }

        if let (Some(profile_name), Some(profile_config)) = (
            task_scheduler_profile_name.clone(),
            task_scheduler_profile_config.clone(),
        ) {
            let mode_kind = scheduler_mode_kind(&profile_name);
            let resolved_system_prompt =
                scheduler_system_prompt_preview(&profile_name, &profile_config);
            let user_message_id = {
                let user_message = session.add_user_message(display_prompt_text.clone());
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
                    serde_json::json!(prompt_text.clone()),
                );
                user_message.id.clone()
            };
            let assistant_message_id = session.add_assistant_message().id.clone();

            // Set an immediate title from the user message when the title is
            // still the auto-generated default, so frontends see a meaningful
            // label right away.  The LLM-generated title replaces it later.
            if session.is_default_title() {
                if let Some(first_text) = first_user_message_text(&session) {
                    let immediate = rocode_session::generate_session_title(&first_text);
                    if !immediate.is_empty() && immediate != "New Session" {
                        session.set_title(immediate);
                    }
                }
            }

            {
                let mut sessions = task_state.sessions.lock().await;
                sessions.update(session.clone());
            }
            task_state.broadcast(
                &serde_json::json!({
                    "type": "session.updated",
                    "sessionID": session_id,
                    "source": "prompt.scheduler.pending",
                })
                .to_string(),
            );

            let agent_registry = Arc::new(AgentRegistry::from_config(&task_config));

            // Inject runtime metadata into profile_config for dynamic prompt building
            let mut profile_config = profile_config;
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
                        } else if a.name == "explore"
                            || a.name == "code-explorer"
                            || a.name == "docs-researcher"
                        {
                            "CHEAP".to_string()
                        } else {
                            "CHEAP".to_string()
                        },
                    })
                    .collect();
            }
            if profile_config.available_categories.is_empty() {
                profile_config.available_categories = task_state
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

            let current_model = Some(format!("{}:{}", task_provider, task_model));
            let tool_executor: Arc<dyn OrchestratorToolExecutor> =
                Arc::new(SessionSchedulerToolExecutor {
                    state: task_state.clone(),
                    session_id: session_id.clone(),
                    message_id: assistant_message_id.clone(),
                    directory: session.directory.clone(),
                    current_model,
                    tool_runtime_config: rocode_tool::ToolRuntimeConfig::from_config(&task_config),
                    agent_registry: agent_registry.clone(),
                });
            let tool_runner = ToolRunner::new(tool_executor.clone());
            let model_resolver: Arc<dyn ModelResolver> = Arc::new(SessionSchedulerModelResolver {
                state: task_state.clone(),
                fallback_provider_id: task_provider.clone(),
                fallback_model_id: task_model.clone(),
                variant: task_variant.clone(),
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
            let ctx = OrchestratorContext {
                agent_resolver: Arc::new(SchedulerAgentResolver {
                    registry: agent_registry.clone(),
                }),
                model_resolver,
                tool_executor,
                lifecycle_hook: Arc::new(SessionSchedulerLifecycleHook::new(
                    task_state.clone(),
                    session_id.clone(),
                    profile_name.clone(),
                )),
                exec_ctx,
            };

            let orchestrator_result = match scheduler_orchestrator_from_profile(
                Some(profile_name.clone()),
                &profile_config,
                tool_runner,
            ) {
                Ok(mut orchestrator) => orchestrator.execute(&prompt_text, &ctx).await,
                Err(error) => Err(OrchestratorError::Other(error.to_string())),
            };

            session = {
                let sessions = task_state.sessions.lock().await;
                sessions.get(&session_id).cloned().unwrap_or(session)
            };

            ensure_default_session_title(&mut session, task_provider_client.clone(), &task_model)
                .await;

            // Extract handoff metadata before borrowing session mutably.
            let handoff_entries: Vec<(String, serde_json::Value)> =
                if let Ok(ref output) = orchestrator_result {
                    ["scheduler_handoff_mode", "scheduler_handoff_plan_path"]
                        .iter()
                        .filter_map(|key| {
                            output
                                .metadata
                                .get(*key)
                                .map(|v| (key.to_string(), v.clone()))
                        })
                        .collect()
                } else {
                    Vec::new()
                };

            if let Some(assistant) = session.get_message_mut(&assistant_message_id) {
                assistant.metadata.insert(
                    "model_provider".to_string(),
                    serde_json::json!(&task_provider),
                );
                assistant
                    .metadata
                    .insert("model_id".to_string(), serde_json::json!(&task_model));
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
                    serde_json::json!(task_scheduler_applied),
                );
                match orchestrator_result {
                    Ok(output) => {
                        assistant.finish = Some("stop".to_string());
                        assistant.metadata.insert(
                            "scheduler_steps".to_string(),
                            serde_json::json!(output.steps),
                        );
                        assistant.metadata.insert(
                            "scheduler_tool_calls".to_string(),
                            serde_json::json!(output.tool_calls_count),
                        );
                        assistant.add_text(output.content);
                    }
                    Err(error) => {
                        tracing::error!(
                            session_id = %session_id,
                            scheduler_profile = %profile_name,
                            %error,
                            "scheduler prompt failed"
                        );
                        assistant.finish = Some("error".to_string());
                        assistant
                            .metadata
                            .insert("error".to_string(), serde_json::json!(error.to_string()));
                        assistant.add_text(format!("Scheduler error: {}", error));
                    }
                }
            }
            // Propagate handoff metadata to session (outside message borrow).
            for (key, value) in handoff_entries {
                session.metadata.insert(key, value);
            }
            session.touch();
            {
                let mut sessions = task_state.sessions.lock().await;
                sessions.update(session.clone());
            }
            task_state.broadcast(
                &serde_json::json!({
                    "type": "session.updated",
                    "sessionID": session_id,
                    "source": "prompt.scheduler.completed",
                })
                .to_string(),
            );
            persist_sessions_if_enabled(&task_state).await;
            return;
        }

        let (update_tx, mut update_rx) =
            tokio::sync::mpsc::unbounded_channel::<rocode_session::Session>();
        let update_state = task_state.clone();
        let update_session_repo = task_state.session_repo.clone();
        let update_message_repo = task_state.message_repo.clone();

        // Coalescing persistence worker — only persists the latest snapshot, not every tick.
        let persist_latest: Arc<tokio::sync::Mutex<Option<rocode_session::Session>>> =
            Arc::new(tokio::sync::Mutex::new(None));
        let persist_notify = Arc::new(Notify::new());
        let persist_worker = {
            let latest = persist_latest.clone();
            let notify = persist_notify.clone();
            let s_repo = update_session_repo.clone();
            let m_repo = update_message_repo.clone();
            tokio::spawn(async move {
                loop {
                    notify.notified().await;
                    // Drain: grab the latest snapshot, leaving None.
                    let snapshot = latest.lock().await.take();
                    let Some(snapshot) = snapshot else { continue };
                    if let (Some(s_repo), Some(m_repo)) = (&s_repo, &m_repo) {
                        match serde_json::to_value(&snapshot) {
                            Ok(val) => match serde_json::from_value::<rocode_types::Session>(val) {
                                Ok(mut stored) => {
                                    let messages = std::mem::take(&mut stored.messages);
                                    if let Err(e) = s_repo.upsert(&stored).await {
                                        tracing::warn!(session_id = %stored.id, %e, "incremental session upsert failed");
                                    }
                                    for msg in messages {
                                        if let Err(e) = m_repo.upsert(&msg).await {
                                            tracing::warn!(message_id = %msg.id, %e, "incremental message upsert failed");
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(session_id = %snapshot.id, %e, "incremental persist: failed to deserialize session snapshot");
                                }
                            },
                            Err(e) => {
                                tracing::warn!(session_id = %snapshot.id, %e, "incremental persist: failed to serialize session snapshot");
                            }
                        }
                    }
                }
            })
        };

        let mut update_task = tokio::spawn(async move {
            while let Some(snapshot) = update_rx.recv().await {
                let snapshot_id = snapshot.id.clone();

                // 1. Update in-memory state + WebSocket broadcast FIRST (low latency).
                {
                    let mut sessions = update_state.sessions.lock().await;
                    sessions.update(snapshot.clone());
                }
                update_state.broadcast(
                    &serde_json::json!({
                        "type": "session.updated",
                        "sessionID": snapshot_id,
                        "source": "prompt.stream",
                    })
                    .to_string(),
                );

                // 2. Queue latest snapshot for async persistence (coalesced).
                *persist_latest.lock().await = Some(snapshot);
                persist_notify.notify_one();
            }
            // Channel closed — signal persist worker to flush final snapshot.
            persist_notify.notify_one();
        });
        // Keep persist_worker handle at this scope so the outer timeout path can abort it.
        let persist_worker_handle = persist_worker;
        let update_hook: rocode_session::SessionUpdateHook = Arc::new(move |snapshot| {
            let _ = update_tx.send(snapshot.clone());
        });

        let prompt_runner = rocode_session::SessionPrompt::new(Arc::new(RwLock::new(
            rocode_session::SessionStateManager::new(),
        )))
        .with_tool_runtime_config(rocode_tool::ToolRuntimeConfig::from_config(&config));
        let tool_defs = rocode_session::resolve_tools(task_state.tool_registry.as_ref()).await;
        let input = rocode_session::PromptInput {
            session_id: session_id.clone(),
            message_id: None,
            model: Some(rocode_session::prompt::ModelRef {
                provider_id: task_provider.clone(),
                model_id: task_model.clone(),
            }),
            agent: task_agent.clone(),
            no_reply: false,
            system: None,
            variant: task_variant.clone(),
            parts: vec![rocode_session::PartInput::Text { text: prompt_text }],
            tools: None,
        };

        let agent_registry = AgentRegistry::from_config(&config);
        let agent_lookup: Option<
            Arc<dyn Fn(&str) -> Option<rocode_tool::TaskAgentInfo> + Send + Sync>,
        > = {
            Some(Arc::new(move |name: &str| {
                agent_registry
                    .get(name)
                    .map(|info| rocode_tool::TaskAgentInfo {
                        name: info.name.clone(),
                        model: info.model.as_ref().map(|m| rocode_tool::TaskAgentModel {
                            provider_id: m.provider_id.clone(),
                            model_id: m.model_id.clone(),
                        }),
                        can_use_task: info.is_tool_allowed("task"),
                        steps: info.max_steps,
                    })
            }))
        };

        let ask_question_hook: Option<rocode_session::prompt::AskQuestionHook> = {
            let state = task_state.clone();
            Some(Arc::new(move |session_id, questions| {
                let state = state.clone();
                Box::pin(
                    async move { request_question_answers(state, session_id, questions).await },
                )
            }))
        };

        let event_broadcast: Option<rocode_session::prompt::EventBroadcastHook> = {
            let state = task_state.clone();
            Some(Arc::new(move |event| {
                state.broadcast(event);
            }))
        };

        if let Err(error) = prompt_runner
            .prompt_with_update_hook(
                input,
                &mut session,
                provider,
                task_system_prompt.clone(),
                tool_defs,
                task_agent_params.clone(),
                Some(update_hook),
                event_broadcast,
                agent_lookup,
                ask_question_hook,
            )
            .await
        {
            tracing::error!(
                session_id = %session_id,
                provider_id = %task_provider,
                model_id = %task_model,
                %error,
                "session prompt failed"
            );
            let assistant = session.add_assistant_message();
            assistant.finish = Some("error".to_string());
            assistant
                .metadata
                .insert("error".to_string(), serde_json::json!(error.to_string()));
            assistant
                .metadata
                .insert("finish_reason".to_string(), serde_json::json!("error"));
            assistant.metadata.insert(
                "model_provider".to_string(),
                serde_json::json!(&task_provider),
            );
            assistant
                .metadata
                .insert("model_id".to_string(), serde_json::json!(&task_model));
            if let Some(agent) = task_agent.as_deref() {
                assistant
                    .metadata
                    .insert("agent".to_string(), serde_json::json!(agent));
            }
            assistant.add_text(format!("Provider error: {}", error));
        }
        match tokio::time::timeout(Duration::from_secs(1), &mut update_task).await {
            Ok(joined) => {
                let _ = joined;
            }
            Err(_) => {
                update_task.abort();
                tracing::warn!(
                    session_id = %session_id,
                    "timed out waiting for prompt update task shutdown; aborted task"
                );
            }
        }
        // Always clean up the persist worker — it may still be alive if update_task was aborted.
        // Give it a brief window to flush the last queued snapshot, then abort.
        if !persist_worker_handle.is_finished() {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        persist_worker_handle.abort();

        {
            let mut sessions = task_state.sessions.lock().await;
            sessions.update(session);
        }
        task_state.broadcast(
            &serde_json::json!({
                "type": "session.updated",
                "sessionID": session_id,
                "source": "prompt.final",
            })
            .to_string(),
        );
        // Normal path reached — defuse the guard so we handle cleanup explicitly.
        _idle_guard.defuse();
        set_session_run_status(&task_state, &session_id, SessionRunStatus::Idle).await;
        // Only flush the current session — full sync is deferred to shutdown/startup.
        if let Err(err) = task_state.flush_session_to_storage(&session_id).await {
            tracing::error!(session_id = %session_id, %err, "failed to flush session to storage");
        }
    });

    Ok(Json(serde_json::json!({
        "status": "started",
        "model": format!("{}/{}", provider_id, model_id),
        "variant": req.variant,
    })))
}

async fn abort_prompt(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let sessions = state.sessions.lock().await;
    if sessions.get(&id).is_none() {
        return Err(ApiError::SessionNotFound(id));
    }
    Ok(Json(serde_json::json!({ "aborted": true })))
}

async fn abort_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let sessions = state.sessions.lock().await;
    if sessions.get(&id).is_none() {
        return Err(ApiError::SessionNotFound(id));
    }
    Ok(Json(serde_json::json!({ "aborted": true })))
}

#[derive(Debug, Deserialize)]
pub struct UpdateSessionTimeRequest {
    pub archived: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSessionRequest {
    pub title: Option<String>,
    pub time: Option<UpdateSessionTimeRequest>,
}

async fn update_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateSessionRequest>,
) -> Result<Json<SessionInfo>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;

    if let Some(title) = req.title {
        session.set_title(title);
    }
    if let Some(time) = req.time {
        if let Some(archived) = time.archived {
            session.set_archived(Some(archived));
        }
    }
    let info = session_to_info(session);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;
    Ok(Json(info))
}

async fn get_message(
    State(state): State<Arc<ServerState>>,
    Path((session_id, msg_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    let message = session
        .get_message(&msg_id)
        .ok_or_else(|| ApiError::NotFound(format!("Message not found: {}", msg_id)))?;

    let info = serde_json::json!({
        "id": message.id,
        "sessionID": session_id,
        "role": message_role_name(&message.role),
        "createdAt": message.created_at.timestamp_millis(),
    });
    Ok(Json(serde_json::json!({
        "info": info,
        "parts": message.parts.clone(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct UpdatePartRequest {
    pub part: serde_json::Value,
}

async fn update_part(
    State(state): State<Arc<ServerState>>,
    Path((session_id, msg_id, part_id)): Path<(String, String, String)>,
    Json(req): Json<UpdatePartRequest>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;
    let message = session
        .get_message_mut(&msg_id)
        .ok_or_else(|| ApiError::NotFound(format!("Message not found: {}", msg_id)))?;

    let mut part: rocode_session::MessagePart = serde_json::from_value(req.part)
        .map_err(|e| ApiError::BadRequest(format!("Invalid part payload: {}", e)))?;
    if part.id != part_id {
        return Err(ApiError::BadRequest(format!(
            "Part id mismatch: body has {}, path has {}",
            part.id, part_id
        )));
    }
    part.message_id = Some(msg_id.clone());

    let updated_part = {
        let target = message
            .parts
            .iter_mut()
            .find(|existing| existing.id == part_id)
            .ok_or_else(|| ApiError::NotFound(format!("Part not found: {}", part_id)))?;
        *target = part.clone();
        target.clone()
    };
    session.touch();
    drop(sessions);
    persist_sessions_if_enabled(&state).await;

    Ok(Json(serde_json::json!({
        "updated": true,
        "part": updated_part,
    })))
}

#[derive(Debug, Deserialize)]
pub struct ExecuteShellRequest {
    pub command: String,
    pub workdir: Option<String>,
}

async fn execute_shell(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<ExecuteShellRequest>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    session.add_user_message(format!("$ {}", req.command));
    let assistant = session.add_assistant_message();
    assistant.add_text(format!("Shell command queued: {}", req.command));
    let assistant_id = assistant.id.clone();
    drop(sessions);
    persist_sessions_if_enabled(&state).await;

    Ok(Json(serde_json::json!({
        "executed": true,
        "command": req.command,
        "workdir": req.workdir,
        "message_id": assistant_id,
    })))
}

#[derive(Debug, Deserialize)]
pub struct PromptAsyncRequest {
    pub message: Option<String>,
    pub model: Option<String>,
}

async fn prompt_async(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<PromptAsyncRequest>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    let text = req
        .message
        .as_deref()
        .ok_or_else(|| ApiError::BadRequest("Field `message` is required".to_string()))?;
    session.add_user_message(text);
    let assistant = session.add_assistant_message();
    let assistant_id = assistant.id.clone();
    drop(sessions);
    persist_sessions_if_enabled(&state).await;

    Ok(Json(serde_json::json!({
        "status": "queued",
        "message_id": assistant_id,
        "model": req.model,
    })))
}

#[derive(Debug, Deserialize)]
pub struct InitSessionRequest {
    pub force: Option<bool>,
}

async fn init_session(
    Path(_id): Path<String>,
    Json(_req): Json<InitSessionRequest>,
) -> Result<Json<serde_json::Value>> {
    Ok(Json(
        serde_json::json!({ "initialized": true, "message": "Session initialized successfully" }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct SummarizeSessionRequest {
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
}

async fn summarize_session(
    Path(_id): Path<String>,
    Json(_req): Json<SummarizeSessionRequest>,
) -> Result<Json<serde_json::Value>> {
    Ok(Json(
        serde_json::json!({ "summarized": true, "message": "Session summarized successfully" }),
    ))
}

async fn session_unrevert(Path(_id): Path<String>) -> Result<Json<serde_json::Value>> {
    Ok(Json(
        serde_json::json!({ "unreverted": true, "message": "Session unreverted successfully" }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct ExecuteCommandRequest {
    pub command: String,
    pub arguments: Option<String>,
    pub model: Option<String>,
    pub agent: Option<String>,
}

async fn execute_command(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
    Json(req): Json<ExecuteCommandRequest>,
) -> Result<Json<serde_json::Value>> {
    let mut sessions = state.sessions.lock().await;
    let session = sessions
        .get_mut(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id.clone()))?;
    let text = req
        .arguments
        .as_deref()
        .map(|args| format!("/{cmd} {args}", cmd = req.command))
        .unwrap_or_else(|| format!("/{}", req.command));
    session.add_user_message(text);
    let assistant = session.add_assistant_message();
    assistant.add_text(format!("Command queued: {}", req.command));
    let assistant_id = assistant.id.clone();
    let arguments = req
        .arguments
        .as_deref()
        .map(|value| {
            value
                .split_whitespace()
                .map(|item| item.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    sessions.publish_command_executed(&req.command, &id, arguments, &assistant_id);
    drop(sessions);
    persist_sessions_if_enabled(&state).await;

    Ok(Json(serde_json::json!({
        "executed": true,
        "command": req.command,
        "arguments": req.arguments,
        "model": req.model,
        "agent": req.agent,
        "message_id": assistant_id,
    })))
}

async fn get_session_diff(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<FileDiffInfo>>> {
    let sessions = state.sessions.lock().await;
    let session = sessions
        .get(&id)
        .ok_or_else(|| ApiError::SessionNotFound(id))?;
    let diffs = session
        .summary
        .as_ref()
        .and_then(|summary| summary.diffs.as_ref())
        .map(|items| {
            items
                .iter()
                .map(|diff| FileDiffInfo {
                    path: diff.path.clone(),
                    additions: diff.additions,
                    deletions: diff.deletions,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(Json(diffs))
}

async fn cancel_tool_call(
    State(state): State<Arc<ServerState>>,
    Path((session_id, tool_call_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    // Verify the tool call exists in the session (hold lock briefly).
    {
        let sessions = state.sessions.lock().await;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| ApiError::SessionNotFound(session_id.clone()))?;

        let found = session.messages.iter().any(|msg| {
            msg.parts.iter().any(|part| {
                matches!(
                    &part.part_type,
                    rocode_session::PartType::ToolCall { id, .. } if id == &tool_call_id
                )
            })
        });

        if !found {
            return Err(ApiError::NotFound(format!(
                "Tool call {} not found in session {}",
                tool_call_id, session_id
            )));
        }
    }

    // Look up the plugin request mapping from global tracking
    if let Some(tracking) = rocode_plugin::subprocess::get_tool_call_tracking(&tool_call_id).await {
        // Get the plugin loader and cancel the request
        if let Some(loader) = get_plugin_loader() {
            let clients = loader.clients().await;
            if let Some(plugin) = clients
                .iter()
                .find(|c| c.plugin_id() == tracking.plugin_name)
            {
                if let Err(e) = plugin.cancel_request(tracking.request_id).await {
                    tracing::warn!(
                        tool_call_id = %tool_call_id,
                        plugin_name = %tracking.plugin_name,
                        request_id = %tracking.request_id,
                        error = %e,
                        "Failed to send cancel request to plugin"
                    );
                    return Ok(Json(serde_json::json!({
                        "cancelled": false,
                        "message": format!("Failed to cancel: {}", e)
                    })));
                }

                // Remove from tracking
                rocode_plugin::subprocess::remove_tool_call_tracking(&tool_call_id).await;

                return Ok(Json(serde_json::json!({
                    "cancelled": true,
                    "message": "Cancel request sent to plugin"
                })));
            }
        }

        return Ok(Json(serde_json::json!({
            "cancelled": false,
            "message": "Plugin not found or not loaded"
        })));
    }

    Ok(Json(serde_json::json!({
        "cancelled": false,
        "message": "Tool call is not currently executing or not tracked"
    })))
}

#[derive(Debug, Serialize)]
pub struct FileDiffInfo {
    pub path: String,
    pub additions: u64,
    pub deletions: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_model_inputs_prefer_agent_override() {
        let profile = SchedulerProfileConfig {
            model: Some(OrchestratorModelRef {
                provider_id: "anthropic".to_string(),
                model_id: "claude-opus-4-6".to_string(),
            }),
            ..Default::default()
        };

        let (request_model, config_model, config_provider) = resolve_request_model_inputs(
            true,
            Some("openai/gpt-5"),
            Some(&profile),
            Some("anthropic/claude-sonnet-4-6"),
            Some("anthropic/claude-haiku-4-5-20251001"),
        );

        assert_eq!(request_model, None);
        assert_eq!(config_model.as_deref(), Some("openai/gpt-5"));
        assert_eq!(config_provider, None);
    }

    #[test]
    fn scheduler_model_inputs_prefer_profile_override_over_request_model() {
        let profile = SchedulerProfileConfig {
            model: Some(OrchestratorModelRef {
                provider_id: "anthropic".to_string(),
                model_id: "claude-opus-4-6".to_string(),
            }),
            ..Default::default()
        };

        let (request_model, config_model, config_provider) = resolve_request_model_inputs(
            true,
            None,
            Some(&profile),
            Some("openai/gpt-5"),
            Some("anthropic/claude-haiku-4-5-20251001"),
        );

        assert_eq!(request_model, None);
        assert_eq!(config_model.as_deref(), Some("claude-opus-4-6"));
        assert_eq!(config_provider.as_deref(), Some("anthropic"));
    }

    #[test]
    fn scheduler_model_inputs_fall_back_to_request_model_when_no_overrides_exist() {
        let (request_model, config_model, config_provider) = resolve_request_model_inputs(
            true,
            None,
            None,
            Some("openai/gpt-5"),
            Some("anthropic/claude-haiku-4-5-20251001"),
        );

        assert_eq!(request_model.as_deref(), Some("openai/gpt-5"));
        assert_eq!(
            config_model.as_deref(),
            Some("anthropic/claude-haiku-4-5-20251001")
        );
        assert_eq!(config_provider, None);
    }

    #[test]
    fn non_scheduler_model_inputs_keep_request_then_agent_precedence() {
        let (request_model, config_model, config_provider) = resolve_request_model_inputs(
            false,
            Some("anthropic/claude-opus-4-6"),
            None,
            Some("openai/gpt-5"),
            Some("anthropic/claude-haiku-4-5-20251001"),
        );

        assert_eq!(request_model.as_deref(), Some("openai/gpt-5"));
        assert_eq!(config_model.as_deref(), Some("anthropic/claude-opus-4-6"));
        assert_eq!(config_provider, None);
    }

    #[test]
    fn builtin_preset_defaults_resolve_without_external_scheduler_file() {
        let defaults = resolve_scheduler_request_defaults(&AppConfig::default(), Some("sisyphus"))
            .expect("builtin preset should resolve without schedulerPath");

        assert_eq!(defaults.profile_name.as_deref(), Some("sisyphus"));
    }

    #[test]
    fn builtin_presets_resolve_as_preset_modes() {
        let defaults = resolve_scheduler_request_defaults(&AppConfig::default(), Some("sisyphus"))
            .expect("builtin preset should resolve without schedulerPath");

        assert_eq!(defaults.profile_name.as_deref(), Some("sisyphus"));
        assert_eq!(scheduler_mode_kind("sisyphus"), "preset");
    }

    #[test]
    fn public_preset_preview_uses_compact_role_summary_and_no_first_person_intro() {
        let profile = SchedulerProfileConfig {
            orchestrator: Some("atlas".to_string()),
            ..Default::default()
        };

        let preview = scheduler_system_prompt_preview("atlas", &profile);
        assert!(preview.contains("You are Atlas — master orchestrator for plan execution."));
        assert!(!preview.contains("I'm Atlas"));
    }

    #[test]
    fn prometheus_preview_uses_compact_planner_summary() {
        let profile = SchedulerProfileConfig {
            orchestrator: Some("prometheus".to_string()),
            ..Default::default()
        };

        let preview = scheduler_system_prompt_preview("prometheus", &profile);
        assert!(preview.contains("You are Prometheus — strategic planning consultant."));
        assert!(preview.contains(
            "Bias: interview first, clarify scope, then produce one reviewed work plan."
        ));
        assert!(preview
            .contains("Boundary: planner-only; never execute code or modify non-markdown files."));
        assert!(!preview.contains("I'm Prometheus"));
    }
}
