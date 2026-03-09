use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::ServerState;
use rocode_orchestrator::{ExecutionContext as OrchestratorExecutionContext, LifecycleHook};
use rocode_provider::Provider;
use rocode_session::{MessageRole, Session};

#[derive(Clone)]
pub(crate) struct SessionSchedulerLifecycleHook {
    state: Arc<ServerState>,
    session_id: String,
    scheduler_profile: String,
    /// Tracks the message ID of the currently streaming stage message.
    active_stage_message_id: Arc<Mutex<Option<String>>>,
}

impl SessionSchedulerLifecycleHook {
    pub(crate) fn new(
        state: Arc<ServerState>,
        session_id: String,
        scheduler_profile: String,
    ) -> Self {
        Self {
            state,
            session_id,
            scheduler_profile,
            active_stage_message_id: Arc::new(Mutex::new(None)),
        }
    }

    async fn emit_stage_message(
        &self,
        stage_name: &str,
        stage_index: u32,
        stage_total: u32,
        content: &str,
        exec_ctx: &OrchestratorExecutionContext,
    ) {
        emit_scheduler_stage_message(
            &self.state,
            &self.session_id,
            &self.scheduler_profile,
            stage_name,
            stage_index,
            stage_total,
            content,
            exec_ctx,
        )
        .await;
    }
}

#[async_trait]
impl LifecycleHook for SessionSchedulerLifecycleHook {
    async fn on_orchestration_start(
        &self,
        _: &str,
        _: Option<u32>,
        _: &OrchestratorExecutionContext,
    ) {
    }

    async fn on_step_start(&self, _: &str, _: &str, _: u32, _: &OrchestratorExecutionContext) {}

    async fn on_orchestration_end(&self, _: &str, _: u32, _: &OrchestratorExecutionContext) {}

    async fn on_scheduler_stage_start(
        &self,
        _agent_name: &str,
        stage_name: &str,
        stage_index: u32,
        exec_ctx: &OrchestratorExecutionContext,
    ) {
        let mut sessions = self.state.sessions.lock().await;
        let Some(mut session) = sessions.get(&self.session_id).cloned() else {
            return;
        };

        let message = session.add_assistant_message();
        let message_id = message.id.clone();
        message.metadata.insert(
            "scheduler_profile".to_string(),
            serde_json::json!(&self.scheduler_profile),
        );
        message.metadata.insert(
            "resolved_scheduler_profile".to_string(),
            serde_json::json!(&self.scheduler_profile),
        );
        message.metadata.insert(
            "scheduler_stage".to_string(),
            serde_json::json!(stage_name),
        );
        message.metadata.insert(
            "scheduler_stage_index".to_string(),
            serde_json::json!(stage_index),
        );
        message.metadata.insert(
            "scheduler_stage_emitted".to_string(),
            serde_json::json!(true),
        );
        message.metadata.insert(
            "scheduler_stage_agent".to_string(),
            serde_json::json!(&exec_ctx.agent_name),
        );
        message.metadata.insert(
            "scheduler_stage_streaming".to_string(),
            serde_json::json!(true),
        );
        // Start with just the stage title; body will be streamed incrementally.
        message.add_text(format!(
            "## {}\n\n",
            scheduler_stage_title(&self.scheduler_profile, stage_name),
        ));

        session.touch();
        sessions.update(session);
        drop(sessions);

        *self.active_stage_message_id.lock().await = Some(message_id);

        self.state.broadcast(
            &serde_json::json!({
                "type": "session.updated",
                "sessionID": &self.session_id,
                "source": "prompt.scheduler.stage.start",
            })
            .to_string(),
        );
    }

    async fn on_scheduler_stage_content(
        &self,
        _stage_name: &str,
        _stage_index: u32,
        content_delta: &str,
        _exec_ctx: &OrchestratorExecutionContext,
    ) {
        let message_id = {
            let guard = self.active_stage_message_id.lock().await;
            match guard.as_ref() {
                Some(id) => id.clone(),
                None => return,
            }
        };

        let mut sessions = self.state.sessions.lock().await;
        let Some(mut session) = sessions.get(&self.session_id).cloned() else {
            return;
        };

        if let Some(message) = session.get_message_mut(&message_id) {
            message.append_text(content_delta);
        }
        session.touch();
        sessions.update(session);
        drop(sessions);

        self.state.broadcast(
            &serde_json::json!({
                "type": "session.updated",
                "sessionID": &self.session_id,
                "source": "prompt.scheduler.stage.content",
            })
            .to_string(),
        );
    }

    async fn on_scheduler_stage_end(
        &self,
        _: &str,
        stage_name: &str,
        stage_index: u32,
        stage_total: u32,
        content: &str,
        exec_ctx: &OrchestratorExecutionContext,
    ) {
        let message_id = self.active_stage_message_id.lock().await.take();

        match message_id {
            Some(msg_id) => {
                // Finalize the streaming message: replace content with final version.
                let body = content.trim();
                let mut sessions = self.state.sessions.lock().await;
                let Some(mut session) = sessions.get(&self.session_id).cloned() else {
                    return;
                };
                if let Some(message) = session.get_message_mut(&msg_id) {
                    message.set_text(format!(
                        "## {}\n\n{}",
                        scheduler_stage_title(&self.scheduler_profile, stage_name),
                        body
                    ));
                    message.metadata.insert(
                        "scheduler_stage_total".to_string(),
                        serde_json::json!(stage_total),
                    );
                    message
                        .metadata
                        .remove("scheduler_stage_streaming");
                }
                session.touch();
                sessions.update(session);
                drop(sessions);

                self.state.broadcast(
                    &serde_json::json!({
                        "type": "session.updated",
                        "sessionID": &self.session_id,
                        "source": "prompt.scheduler.stage",
                    })
                    .to_string(),
                );
            }
            None => {
                // Fallback: no streaming message was created, emit full message.
                self.emit_stage_message(stage_name, stage_index, stage_total, content, exec_ctx)
                    .await;
            }
        }
    }
}

pub(crate) fn scheduler_stage_title(scheduler_profile: &str, stage_name: &str) -> String {
    format!(
        "{} · {}",
        scheduler_profile,
        pretty_scheduler_stage_name(stage_name)
    )
}

fn pretty_scheduler_stage_name(stage_name: &str) -> String {
    stage_name
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) async fn emit_scheduler_stage_message(
    state: &Arc<ServerState>,
    session_id: &str,
    scheduler_profile: &str,
    stage_name: &str,
    stage_index: u32,
    stage_total: u32,
    content: &str,
    exec_ctx: &OrchestratorExecutionContext,
) {
    let body = content.trim();
    if body.is_empty() {
        return;
    }

    let mut sessions = state.sessions.lock().await;
    let Some(mut session) = sessions.get(session_id).cloned() else {
        return;
    };

    let message = session.add_assistant_message();
    message.metadata.insert(
        "scheduler_profile".to_string(),
        serde_json::json!(scheduler_profile),
    );
    message.metadata.insert(
        "resolved_scheduler_profile".to_string(),
        serde_json::json!(scheduler_profile),
    );
    message
        .metadata
        .insert("scheduler_stage".to_string(), serde_json::json!(stage_name));
    message.metadata.insert(
        "scheduler_stage_index".to_string(),
        serde_json::json!(stage_index),
    );
    message.metadata.insert(
        "scheduler_stage_total".to_string(),
        serde_json::json!(stage_total),
    );
    message.metadata.insert(
        "scheduler_stage_emitted".to_string(),
        serde_json::json!(true),
    );
    message.metadata.insert(
        "scheduler_stage_agent".to_string(),
        serde_json::json!(exec_ctx.agent_name.clone()),
    );
    message.add_text(format!(
        "## {}\n\n{}",
        scheduler_stage_title(scheduler_profile, stage_name),
        body
    ));
    session.touch();
    sessions.update(session);
    drop(sessions);

    state.broadcast(
        &serde_json::json!({
            "type": "session.updated",
            "sessionID": session_id,
            "source": "prompt.scheduler.stage",
        })
        .to_string(),
    );
}

pub(crate) fn first_user_message_text(session: &Session) -> Option<String> {
    session
        .messages
        .iter()
        .find(|message| matches!(message.role, MessageRole::User))
        .map(|message| message.get_text())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

pub(crate) async fn ensure_default_session_title(
    session: &mut Session,
    provider: Arc<dyn Provider>,
    model_id: &str,
) {
    if !session.is_default_title() {
        return;
    }

    let Some(first_user_text) = first_user_message_text(session) else {
        return;
    };

    let generated_title =
        rocode_session::generate_session_title_llm(&first_user_text, provider, model_id).await;
    if !generated_title.trim().is_empty() {
        session.set_title(generated_title);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use rocode_provider::{
        ChatRequest, ChatResponse, Choice, Content, Message, ModelInfo, Provider, ProviderError,
        Role, StreamResult,
    };
    use std::collections::HashMap;

    #[derive(Debug)]
    struct MockProvider {
        title: String,
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn id(&self) -> &str {
            "mock"
        }

        fn name(&self) -> &str {
            "Mock"
        }

        fn models(&self) -> Vec<ModelInfo> {
            Vec::new()
        }

        fn get_model(&self, _id: &str) -> Option<&ModelInfo> {
            None
        }

        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, ProviderError> {
            Ok(ChatResponse {
                id: "mock-response".to_string(),
                model: "mock-model".to_string(),
                choices: vec![Choice {
                    index: 0,
                    message: Message {
                        role: Role::Assistant,
                        content: Content::Text(self.title.clone()),
                        cache_control: None,
                        provider_options: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
            })
        }

        async fn chat_stream(&self, _request: ChatRequest) -> Result<StreamResult, ProviderError> {
            Ok(Box::pin(stream::iter(Vec::<
                Result<rocode_provider::StreamEvent, ProviderError>,
            >::new())))
        }
    }

    #[test]
    fn scheduler_stage_title_prettifies_hyphenated_stage_names() {
        assert_eq!(
            scheduler_stage_title("prometheus", "execution-orchestration"),
            "prometheus · Execution Orchestration"
        );
    }

    #[test]
    fn first_user_message_text_uses_first_real_user_message() {
        let mut session = Session::new("project", ".");
        session.add_assistant_message().add_text("hello");
        session.add_user_message("  First prompt  ");
        session.add_user_message("Second prompt");

        assert_eq!(
            first_user_message_text(&session).as_deref(),
            Some("First prompt")
        );
    }

    #[tokio::test]
    async fn emit_scheduler_stage_message_appends_assistant_stage_message() {
        let state = Arc::new(ServerState::new());
        let session_id = {
            let mut sessions = state.sessions.lock().await;
            sessions.create("project", ".").id
        };
        let exec_ctx = OrchestratorExecutionContext {
            session_id: session_id.clone(),
            workdir: ".".to_string(),
            agent_name: "prometheus".to_string(),
            metadata: HashMap::new(),
        };

        emit_scheduler_stage_message(
            &state,
            &session_id,
            "prometheus",
            "plan",
            3,
            4,
            "## Plan\n- step",
            &exec_ctx,
        )
        .await;

        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).expect("session should exist");
        let message = session.messages.last().expect("stage message should exist");
        assert!(message.get_text().contains("prometheus · Plan"));
        assert_eq!(
            message
                .metadata
                .get("scheduler_stage")
                .and_then(|value| value.as_str()),
            Some("plan")
        );
    }

    #[tokio::test]
    async fn ensure_default_session_title_updates_default_title_only() {
        let mut session = Session::new("project", ".");
        session.add_user_message("Fix the scheduler event flow");
        ensure_default_session_title(
            &mut session,
            Arc::new(MockProvider {
                title: "Scheduler Event Flow".to_string(),
            }),
            "mock-model",
        )
        .await;
        assert_eq!(session.title, "Scheduler Event Flow");

        let mut named = Session::new("project", ".");
        named.set_title("Pinned Title");
        named.add_user_message("Ignored input");
        ensure_default_session_title(
            &mut named,
            Arc::new(MockProvider {
                title: "Should Not Replace".to_string(),
            }),
            "mock-model",
        )
        .await;
        assert_eq!(named.title, "Pinned Title");
    }
}
