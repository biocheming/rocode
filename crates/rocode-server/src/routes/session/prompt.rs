use std::path::{Path as FsPath, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::recovery::RecoveryExecutionContext;
use crate::routes::permission::request_permission;
use crate::runtime_control::SessionRunStatus;
use crate::session_runtime::events::{
    broadcast_session_updated, server_output_block_hook, ServerEvent,
};
use crate::session_runtime::{
    ensure_default_session_title, finalize_active_scheduler_stage_cancelled,
    first_user_message_text, ModelPricing, SessionSchedulerLifecycleHook,
};
use crate::{ApiError, Result, ServerState};
use rocode_agent::{AgentMode, AgentRegistry};
use rocode_command::{CommandContext, CommandRegistry};
use rocode_orchestrator::output_metadata::output_usage;
use rocode_orchestrator::{
    scheduler_orchestrator_from_profile, AvailableAgentMeta, AvailableCategoryMeta,
    ExecutionContext as OrchestratorExecutionContext, ModelResolver, Orchestrator,
    OrchestratorContext, OrchestratorError, ToolExecutor as OrchestratorToolExecutor, ToolRunner,
};

use super::super::tui::request_question_answers;
use super::super::{
    apply_plugin_config_hooks, get_plugin_loader, plugin_auth::ensure_plugin_loader_active,
    should_apply_plugin_config_hooks,
};
use super::cancel::is_scheduler_cancellation_error;
use super::messages::{prompt_display_text, prompt_text_from_parts};
use super::scheduler::{
    resolve_prompt_request_config, resolve_scheduler_profile_config, scheduler_mode_kind,
    scheduler_system_prompt_preview, to_task_agent_info, SchedulerAgentResolver,
    SchedulerRunCancelToken, SessionSchedulerModelResolver, SessionSchedulerToolExecutor,
};
use super::session_crud::{
    persist_sessions_if_enabled, resolved_session_directory, set_session_run_status, IdleGuard,
};

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

#[derive(Debug, Deserialize)]
pub(super) struct SessionPromptRequest {
    pub message: Option<String>,
    #[serde(default)]
    pub parts: Option<Vec<rocode_session::prompt::PartInput>>,
    pub model: Option<String>,
    pub variant: Option<String>,
    pub agent: Option<String>,
    pub scheduler_profile: Option<String>,
    pub command: Option<String>,
    pub arguments: Option<String>,
    #[serde(default)]
    pub(super) recovery: Option<RecoveryExecutionContext>,
}

pub(super) struct SchedulerUserMessageContext<'a> {
    pub(super) display_prompt_text: &'a str,
    pub(super) resolved_user_prompt: &'a str,
    pub(super) profile_name: &'a str,
    pub(super) mode_kind: &'a str,
    pub(super) resolved_system_prompt: &'a str,
    pub(super) recovery: Option<&'a RecoveryExecutionContext>,
}

pub(super) async fn create_scheduler_user_message(
    prompt_runner: &rocode_session::SessionPrompt,
    session: &mut rocode_session::Session,
    input: &rocode_session::PromptInput,
    ctx: SchedulerUserMessageContext<'_>,
) -> Result<String> {
    prompt_runner
        .create_user_message(input, session)
        .await
        .map_err(|error| {
            ApiError::BadRequest(format!("Failed to create scheduler user message: {}", error))
        })?;

    let Some(user_message) = session
        .messages
        .iter_mut()
        .rfind(|message| matches!(message.role, rocode_session::MessageRole::User))
    else {
        return Err(ApiError::InternalError(
            "Scheduler prompt did not create a user message".to_string(),
        ));
    };

    if prompt_text_from_parts(&input.parts).trim().is_empty()
        && !ctx.display_prompt_text.trim().is_empty()
    {
        if let Some(rocode_session::PartType::Text { text, .. }) = user_message
            .parts
            .iter_mut()
            .find_map(|part| match &mut part.part_type {
                rocode_session::PartType::Text { .. } => Some(&mut part.part_type),
                _ => None,
            })
        {
            *text = ctx.display_prompt_text.to_string();
        }
    }

    user_message.metadata.insert(
        "resolved_scheduler_profile".to_string(),
        serde_json::json!(ctx.profile_name),
    );
    user_message.metadata.insert(
        "resolved_execution_mode_kind".to_string(),
        serde_json::json!(ctx.mode_kind),
    );
    user_message.metadata.insert(
        "resolved_system_prompt".to_string(),
        serde_json::json!(ctx.resolved_system_prompt),
    );
    user_message.metadata.insert(
        "resolved_system_prompt_preview".to_string(),
        serde_json::json!(ctx.resolved_system_prompt),
    );
    user_message.metadata.insert(
        "resolved_system_prompt_applied".to_string(),
        serde_json::json!(true),
    );
    user_message.metadata.insert(
        "resolved_user_prompt".to_string(),
        serde_json::json!(ctx.resolved_user_prompt),
    );

    if let Some(recovery) = ctx.recovery {
        if let Some(action) = recovery.action.as_ref() {
            user_message
                .metadata
                .insert("recovery_action".to_string(), serde_json::json!(action));
        }
        if let Some(target_id) = recovery.target_id.as_deref() {
            user_message.metadata.insert(
                "recovery_target_id".to_string(),
                serde_json::json!(target_id),
            );
        }
        if let Some(target_kind) = recovery.target_kind.as_deref() {
            user_message.metadata.insert(
                "recovery_target_kind".to_string(),
                serde_json::json!(target_kind),
            );
        }
        if let Some(target_label) = recovery.target_label.as_deref() {
            user_message.metadata.insert(
                "recovery_target_label".to_string(),
                serde_json::json!(target_label),
            );
        }
    }

    Ok(user_message.id.clone())
}

pub(super) async fn session_prompt(
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
    if req.command.is_some() && req.parts.is_some() {
        return Err(ApiError::BadRequest(
            "`command` and `parts` are mutually exclusive".to_string(),
        ));
    }

    let request_parts = req.parts.clone().filter(|parts| !parts.is_empty());
    let display_prompt_text = if let Some(parts) = request_parts.as_ref() {
        prompt_display_text(parts)
    } else if let Some(message) = req.message.as_deref() {
        message.to_string()
    } else if let Some(command) = req.command.as_deref() {
        req.arguments
            .as_deref()
            .map(|args| format!("/{command} {args}"))
            .unwrap_or_else(|| format!("/{command}"))
    } else {
        return Err(ApiError::BadRequest(
            "Either `message`, `parts`, or `command` must be provided".to_string(),
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
    let known_agents = AgentRegistry::from_config(&config)
        .list_all()
        .into_iter()
        .map(|agent| agent.name.clone())
        .collect::<Vec<_>>();

    let resolved_prompt = if let Some(parts) = request_parts.as_ref() {
        ResolvedPromptPayload {
            display_text: prompt_display_text(parts),
            execution_text: prompt_text_from_parts(parts),
            agent: None,
            scheduler_profile: None,
        }
    } else {
        resolve_prompt_payload(&display_prompt_text, &id, &session_directory).await?
    };
    let prompt_text = resolved_prompt.execution_text.clone();
    let display_prompt_text = resolved_prompt.display_text.clone();
    let prompt_parts = if let Some(parts) = request_parts.clone() {
        parts
    } else {
        rocode_session::resolve_prompt_parts(
            &prompt_text,
            FsPath::new(&session_directory),
            &known_agents,
        )
        .await
    };
    let effective_agent = resolved_prompt.agent.clone().or(req.agent.clone());
    let effective_scheduler_profile = resolved_prompt
        .scheduler_profile
        .clone()
        .or(req.scheduler_profile.clone());

    let request_config =
        resolve_prompt_request_config(super::scheduler::PromptRequestConfigInput {
            state: &state,
            config: &config,
            session_id: &id,
            requested_agent: effective_agent.as_deref(),
            requested_scheduler_profile: effective_scheduler_profile.as_deref(),
            request_model: req.model.as_deref(),
            request_variant: req.variant.as_deref(),
            route: "session",
        })
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
    let task_compiled_request = request_config.compiled_request.clone();

    let task_state = state.clone();
    let session_id = id.clone();
    let task_variant = req.variant.clone();
    let task_agent = resolved_agent.as_ref().map(|agent| agent.name.clone());
    let task_model = model_id.clone();
    let task_provider_client = provider.clone();
    let task_provider = provider_id.clone();
    let task_system_prompt = agent_system_prompt.clone();
    let task_scheduler_applied = scheduler_applied;
    let task_scheduler_profile_name = scheduler_profile_name.clone();
    let task_scheduler_root_agent = scheduler_root_agent.clone();
    let task_scheduler_skill_tree_applied = scheduler_skill_tree_applied;
    let task_config = config.clone();
    let task_recovery = req.recovery.clone();
    let task_prompt_parts = prompt_parts.clone();
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
        if let Some(recovery) = task_recovery.as_ref() {
            if let Some(action) = recovery.action.as_ref() {
                session.metadata.insert(
                    "last_recovery_action".to_string(),
                    serde_json::json!(action),
                );
            }
            if let Some(target_id) = recovery.target_id.as_deref() {
                session.metadata.insert(
                    "last_recovery_target_id".to_string(),
                    serde_json::json!(target_id),
                );
            } else {
                session.metadata.remove("last_recovery_target_id");
            }
            if let Some(target_kind) = recovery.target_kind.as_deref() {
                session.metadata.insert(
                    "last_recovery_target_kind".to_string(),
                    serde_json::json!(target_kind),
                );
            } else {
                session.metadata.remove("last_recovery_target_kind");
            }
            if let Some(target_label) = recovery.target_label.as_deref() {
                session.metadata.insert(
                    "last_recovery_target_label".to_string(),
                    serde_json::json!(target_label),
                );
            } else {
                session.metadata.remove("last_recovery_target_label");
            }
        }

        if let (Some(profile_name), Some(profile_config)) = (
            task_scheduler_profile_name.clone(),
            task_scheduler_profile_config.clone(),
        ) {
            let mode_kind = scheduler_mode_kind(&profile_name);
            let resolved_system_prompt =
                scheduler_system_prompt_preview(&profile_name, &profile_config);
            let scheduler_input = rocode_session::PromptInput {
                session_id: session_id.clone(),
                message_id: None,
                model: None,
                agent: None,
                no_reply: false,
                system: None,
                variant: task_variant.clone(),
                parts: task_prompt_parts.clone(),
                tools: None,
            };
            let user_message_id = match create_scheduler_user_message(
                task_state.prompt_runner.as_ref(),
                &mut session,
                &scheduler_input,
                SchedulerUserMessageContext {
                    display_prompt_text: &display_prompt_text,
                    resolved_user_prompt: &prompt_text,
                    profile_name: &profile_name,
                    mode_kind,
                    resolved_system_prompt: &resolved_system_prompt,
                    recovery: task_recovery.as_ref(),
                },
            )
            .await
            {
                Ok(message_id) => message_id,
                Err(error) => {
                    tracing::warn!(
                        session_id = %session_id,
                        scheduler_profile = %profile_name,
                        %error,
                        "failed to create scheduler user message"
                    );
                    let assistant = session.add_assistant_message();
                    assistant.finish = Some("error".to_string());
                    assistant
                        .metadata
                        .insert("error".to_string(), serde_json::json!(error.to_string()));
                    assistant.add_text(format!("Scheduler input error: {}", error));
                    session.touch();
                    {
                        let mut sessions = task_state.sessions.lock().await;
                        sessions.update(session.clone());
                    }
                    broadcast_session_updated(
                        task_state.as_ref(),
                        session_id.clone(),
                        "prompt.scheduler.error",
                    );
                    persist_sessions_if_enabled(&task_state).await;
                    return;
                }
            };
            let assistant_message_id = session.add_assistant_message().id.clone();

            // Set an immediate title from the user message when the title is
            // still the auto-generated default, so frontends see a meaningful
            // label right away.  The LLM-generated title replaces it later.
            if session.is_default_title() {
                if let Some(first_text) = first_user_message_text(&session) {
                    let immediate = rocode_session::generate_session_title(&first_text);
                    if !immediate.is_empty() && immediate != "New Session" {
                        session.set_auto_title(immediate);
                    }
                }
            }

            {
                let mut sessions = task_state.sessions.lock().await;
                sessions.update(session.clone());
            }
            broadcast_session_updated(
                task_state.as_ref(),
                session_id.clone(),
                "prompt.scheduler.pending",
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
            let scheduler_abort_token = CancellationToken::new();
            task_state
                .runtime_control
                .register_scheduler_run(
                    &session_id,
                    scheduler_abort_token.clone(),
                    Some(profile_name.clone()),
                )
                .await;
            let tool_executor: Arc<dyn OrchestratorToolExecutor> =
                Arc::new(SessionSchedulerToolExecutor {
                    state: task_state.clone(),
                    session_id: session_id.clone(),
                    message_id: assistant_message_id.clone(),
                    directory: session.directory.clone(),
                    abort_token: scheduler_abort_token.clone(),
                    current_model,
                    tool_runtime_config: rocode_tool::ToolRuntimeConfig::from_config(&task_config),
                    agent_registry: agent_registry.clone(),
                });
            let tool_runner = ToolRunner::new(tool_executor.clone());
            let model_resolver: Arc<dyn ModelResolver> = Arc::new(SessionSchedulerModelResolver {
                state: task_state.clone(),
                fallback_provider_id: task_provider.clone(),
                fallback_model_id: task_model.clone(),
                fallback_request: task_compiled_request.clone(),
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
            let task_model_pricing = {
                let providers = task_state.providers.read().await;
                providers
                    .find_model(&task_model)
                    .map(|(_, info)| ModelPricing::from_model_info(&info))
            };
            let lifecycle_hook = Arc::new(
                SessionSchedulerLifecycleHook::new(
                    task_state.clone(),
                    session_id.clone(),
                    profile_name.clone(),
                )
                .with_model_pricing(task_model_pricing),
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
                Ok(mut orchestrator) => orchestrator.execute(&prompt_text, &ctx).await,
                Err(error) => Err(OrchestratorError::Other(error.to_string())),
            };
            task_state
                .runtime_control
                .finish_scheduler_run(&session_id)
                .await;

            session = {
                let sessions = task_state.sessions.lock().await;
                sessions.get(&session_id).cloned().unwrap_or(session)
            };

            // Extract handoff metadata before borrowing session mutably.
            let handoff_entries: Vec<(String, serde_json::Value)> =
                if let Ok(ref output) = orchestrator_result {
                    [
                        "scheduler_handoff_mode",
                        "scheduler_handoff_plan_path",
                        "scheduler_handoff_command",
                    ]
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
                        if output.is_cancelled() {
                            let _ =
                                finalize_active_scheduler_stage_cancelled(&task_state, &session_id)
                                    .await;
                            assistant.finish = Some("cancelled".to_string());
                            assistant.metadata.insert(
                                "finish_reason".to_string(),
                                serde_json::json!("cancelled"),
                            );
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
                            let cost = task_model_pricing
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
                        if is_scheduler_cancellation_error(&error) {
                            let _ =
                                finalize_active_scheduler_stage_cancelled(&task_state, &session_id)
                                    .await;
                            assistant.finish = Some("cancelled".to_string());
                            assistant.metadata.insert(
                                "finish_reason".to_string(),
                                serde_json::json!("cancelled"),
                            );
                            assistant.add_text("Scheduler cancelled.");
                        } else {
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
            }
            ensure_default_session_title(&mut session, task_provider_client.clone(), &task_model)
                .await;
            // Propagate handoff metadata to session (outside message borrow).
            for (key, value) in handoff_entries {
                session.metadata.insert(key, value);
            }
            session.touch();
            {
                let mut sessions = task_state.sessions.lock().await;
                sessions.update(session.clone());
            }
            broadcast_session_updated(
                task_state.as_ref(),
                session_id.clone(),
                "prompt.scheduler.completed",
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
                {
                    let mut sessions = update_state.sessions.lock().await;
                    sessions.update(snapshot.clone());
                }

                *persist_latest.lock().await = Some(snapshot);
                persist_notify.notify_one();
            }
            persist_notify.notify_one();
        });
        // Keep persist_worker handle at this scope so the outer timeout path can abort it.
        let persist_worker_handle = persist_worker;
        let update_hook: rocode_session::SessionUpdateHook = Arc::new(move |snapshot| {
            let _ = update_tx.send(snapshot.clone());
        });

        let prompt_runner = task_state.prompt_runner.clone();
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
            parts: task_prompt_parts.clone(),
            tools: None,
        };

        let agent_registry = AgentRegistry::from_config(&config);
        let agent_lookup: Option<rocode_session::prompt::AgentLookup> = {
            Some(Arc::new(move |name: &str| {
                agent_registry.get(name).map(to_task_agent_info)
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
        let ask_permission_hook: Option<rocode_session::prompt::AskPermissionHook> = {
            let state = task_state.clone();
            Some(Arc::new(move |session_id, request| {
                let state = state.clone();
                Box::pin(async move { request_permission(state, session_id, request).await })
            }))
        };

        let event_broadcast: Option<rocode_session::prompt::EventBroadcastHook> = {
            let state = task_state.clone();
            Some(Arc::new(move |event| {
                if let Ok(server_event) = serde_json::from_value::<ServerEvent>(event) {
                    if let Some(payload) = server_event.to_json_string() {
                        state.broadcast(&payload);
                    } else {
                        tracing::warn!(
                            "failed to serialize ServerEvent from prompt event_broadcast"
                        );
                    }
                } else {
                    tracing::warn!("ignored non-ServerEvent payload in prompt event_broadcast");
                }
            }))
        };
        let output_block_hook: Option<rocode_session::prompt::OutputBlockHook> =
            { Some(server_output_block_hook(task_state.clone())) };

        let publish_bus_hook: Option<rocode_session::prompt::PublishBusHook> = {
            let state = task_state.clone();
            let session_id = session_id.clone();
            Some(Arc::new(
                move |event_type: String, properties: serde_json::Value| {
                    let state = state.clone();
                    let session_id = session_id.clone();
                    Box::pin(async move {
                        match event_type.as_str() {
                            "agent_task.registered" => {
                                let task_id = properties["task_id"].as_str().unwrap_or_default();
                                let agent_name =
                                    properties["agent_name"].as_str().unwrap_or_default();
                                let parent_tool_call_id = properties["parent_tool_call_id"]
                                .as_str()
                                .map(
                                    crate::runtime_control::RuntimeControlRegistry::tool_call_execution_id,
                                );
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
                                        stage_id,
                                    )
                                    .await;
                            }
                            "agent_task.completed" => {
                                let task_id = properties["task_id"].as_str().unwrap_or_default();
                                state.runtime_control.finish_agent_task(task_id).await;
                            }
                            _ => {}
                        }
                    }) as Pin<Box<dyn std::future::Future<Output = ()> + Send>>
                },
            ))
        };

        if let Err(error) = prompt_runner
            .prompt_with_update_hook(
                input,
                &mut session,
                rocode_session::prompt::PromptRequestContext {
                    provider,
                    system_prompt: task_system_prompt.clone(),
                    tools: tool_defs,
                    compiled_request: task_compiled_request.clone(),
                    hooks: rocode_session::prompt::PromptHooks {
                        update_hook: Some(update_hook),
                        event_broadcast,
                        output_block_hook,
                        agent_lookup,
                        ask_question_hook,
                        ask_permission_hook,
                        publish_bus_hook,
                    },
                },
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
        broadcast_session_updated(task_state.as_ref(), session_id.clone(), "prompt.final");
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

#[cfg(test)]
mod tests {
    use super::*;
    use rocode_session::{PartType, Session, SessionStateManager};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn test_prompt_runner() -> rocode_session::SessionPrompt {
        rocode_session::SessionPrompt::new(Arc::new(RwLock::new(SessionStateManager::new())))
    }

    fn text_parts(message: &rocode_session::SessionMessage) -> Vec<&str> {
        message
            .parts
            .iter()
            .filter_map(|part| match &part.part_type {
                PartType::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn scheduler_user_message_preserves_attachment_only_parts() {
        let prompt_runner = test_prompt_runner();
        let mut session = Session::new("project", "/tmp");
        let input = rocode_session::PromptInput {
            session_id: session.id.clone(),
            message_id: None,
            model: None,
            agent: None,
            no_reply: false,
            system: None,
            variant: None,
            parts: vec![rocode_session::PartInput::File {
                url: "data:text/plain;base64,SGVsbG8=".to_string(),
                filename: Some("note.txt".to_string()),
                mime: Some("text/plain".to_string()),
            }],
            tools: None,
        };

        let message_id = create_scheduler_user_message(
            &prompt_runner,
            &mut session,
            &input,
            SchedulerUserMessageContext {
                display_prompt_text: "[1 attachment]",
                resolved_user_prompt: "",
                profile_name: "atlas",
                mode_kind: "preset",
                resolved_system_prompt: "You are Atlas.",
                recovery: None,
            },
        )
        .await
        .expect("scheduler attachment-only user message should be created");

        let message = session
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .expect("user message should exist");
        assert!(
            text_parts(message).contains(&"[1 attachment]"),
            "attachment-only scheduler prompt should retain a visible summary text part"
        );
        assert!(message.parts.iter().any(|part| matches!(
            &part.part_type,
            PartType::File { filename, mime, .. }
            if filename == "note.txt" && mime == "text/plain"
        )));
        assert_eq!(
            message.metadata.get("resolved_scheduler_profile"),
            Some(&serde_json::json!("atlas"))
        );
    }

    #[tokio::test]
    async fn scheduler_user_message_keeps_text_and_file_parts_together() {
        let prompt_runner = test_prompt_runner();
        let mut session = Session::new("project", "/tmp");
        let input = rocode_session::PromptInput {
            session_id: session.id.clone(),
            message_id: None,
            model: None,
            agent: None,
            no_reply: false,
            system: None,
            variant: None,
            parts: vec![
                rocode_session::PartInput::Text {
                    text: "Inspect @note.txt".to_string(),
                },
                rocode_session::PartInput::File {
                    url: "data:text/plain;base64,SGVsbG8=".to_string(),
                    filename: Some("note.txt".to_string()),
                    mime: Some("text/plain".to_string()),
                },
            ],
            tools: None,
        };

        let message_id = create_scheduler_user_message(
            &prompt_runner,
            &mut session,
            &input,
            SchedulerUserMessageContext {
                display_prompt_text: "Inspect @note.txt",
                resolved_user_prompt: "Inspect @note.txt",
                profile_name: "atlas",
                mode_kind: "preset",
                resolved_system_prompt: "You are Atlas.",
                recovery: None,
            },
        )
        .await
        .expect("scheduler text+attachment user message should be created");

        let message = session
            .messages
            .iter()
            .find(|message| message.id == message_id)
            .expect("user message should exist");
        assert!(
            text_parts(message).contains(&"Inspect @note.txt"),
            "scheduler prompt text should remain visible alongside attachment parts"
        );
        assert!(message.parts.iter().any(|part| matches!(
            &part.part_type,
            PartType::File { filename, .. } if filename == "note.txt"
        )));
        assert_eq!(
            message.metadata.get("resolved_user_prompt"),
            Some(&serde_json::json!("Inspect @note.txt"))
        );
    }
}
