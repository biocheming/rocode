use std::collections::HashMap;
use std::sync::Arc;

use rocode_orchestrator::session_title_request;
use rocode_provider::{Content, Message, Provider, Role as ProviderRole, ToolDefinition};
use serde::Deserialize;

use crate::{sanitize_display_text, PartType, Role, Session, SessionMessage};

use super::MAX_STEPS;

// --- Structured Output ---

const STRUCTURED_OUTPUT_DESCRIPTION: &str = r#"Use this tool to return your final response in the requested structured format.

IMPORTANT:
- You MUST call this tool exactly once at the end of your response
- The input must be valid JSON matching the required schema
- Complete all necessary research and tool calls BEFORE calling this tool
- This tool provides your final answer - no further actions are taken after calling it"#;

const STRUCTURED_OUTPUT_SYSTEM_PROMPT: &str = r#"IMPORTANT: The user has requested structured output. You MUST use the StructuredOutput tool to provide your final response. Do NOT respond with plain text - you MUST call the StructuredOutput tool with your answer formatted according to the schema."#;

pub struct StructuredOutputConfig {
    pub schema: serde_json::Value,
}

pub fn create_structured_output_tool(schema: serde_json::Value) -> ToolDefinition {
    let mut tool_schema = schema;
    if let Some(obj) = tool_schema.as_object_mut() {
        obj.remove("$schema");
    }

    ToolDefinition {
        name: "StructuredOutput".to_string(),
        description: Some(STRUCTURED_OUTPUT_DESCRIPTION.to_string()),
        parameters: tool_schema,
    }
}

pub fn structured_output_system_prompt() -> String {
    STRUCTURED_OUTPUT_SYSTEM_PROMPT.to_string()
}

pub fn extract_structured_output(parts: &[crate::MessagePart]) -> Option<serde_json::Value> {
    for part in parts {
        if let PartType::ToolCall { name, input, .. } = &part.part_type {
            if name == "StructuredOutput" {
                return Some(input.clone());
            }
        }
    }
    None
}

// --- Plan Mode ---

const PROMPT_PLAN: &str = r#"You are in PLAN mode. The user wants you to create a plan before executing.

## Your task:
1. Understand the user's request thoroughly
2. Explore the codebase to understand the current state
3. Create a detailed plan in the plan file
4. Use the plan_exit tool when done planning

## Important:
- Do NOT make any edits or run commands (except read operations)
- Only create/modify the plan file
- Ask clarifying questions if needed
- Use explore subagent to understand the codebase"#;

const BUILD_SWITCH: &str = r#"The user has approved your plan and wants you to execute it.

## Your task:
1. Execute the plan step by step
2. Make the necessary changes to the codebase
3. Test your changes
4. Verify the implementation matches the plan

## Important:
- You may now use all tools including edit, write, bash
- Follow the plan closely but adapt as needed
- Report progress to the user"#;

pub fn insert_reminders(
    messages: &[SessionMessage],
    agent_name: &str,
    was_plan: bool,
) -> Vec<SessionMessage> {
    let last_user_idx = messages.iter().rposition(|m| matches!(m.role, Role::User));

    if let Some(idx) = last_user_idx {
        let mut messages = messages.to_vec();

        if agent_name == "plan" {
            let reminder_text = PROMPT_PLAN.to_string();
            messages[idx].parts.push(crate::MessagePart {
                id: format!("prt_{}", uuid::Uuid::new_v4()),
                part_type: PartType::Text {
                    text: reminder_text,
                    synthetic: None,
                    ignored: None,
                },
                created_at: chrono::Utc::now(),
                message_id: None,
            });
        }

        if was_plan && agent_name == "build" {
            let reminder_text = BUILD_SWITCH.to_string();
            messages[idx].parts.push(crate::MessagePart {
                id: format!("prt_{}", uuid::Uuid::new_v4()),
                part_type: PartType::Text {
                    text: reminder_text,
                    synthetic: None,
                    ignored: None,
                },
                created_at: chrono::Utc::now(),
                message_id: None,
            });
        }

        messages
    } else {
        messages.to_vec()
    }
}

pub fn was_plan_agent(messages: &[SessionMessage]) -> bool {
    fn deserialize_opt_string_lossy<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
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
    struct AgentMetadataWire {
        #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
        agent: Option<String>,
    }

    fn agent_metadata_wire(metadata: &HashMap<String, serde_json::Value>) -> AgentMetadataWire {
        let Ok(value) = serde_json::to_value(metadata) else {
            return AgentMetadataWire::default();
        };
        serde_json::from_value::<AgentMetadataWire>(value).unwrap_or_default()
    }

    messages
        .iter()
        .any(|m| agent_metadata_wire(&m.metadata).agent.as_deref() == Some("plan"))
}

// --- Tool Resolution ---

pub struct ResolvedTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

fn preferred_tool_order_key(name: &str) -> (u8, &str) {
    match name {
        "task_flow" => (0, name),
        "task" => (1, name),
        _ => (2, name),
    }
}

pub fn prioritize_tool_definitions(tools: &mut [ToolDefinition]) {
    tools.sort_by(|a, b| preferred_tool_order_key(&a.name).cmp(&preferred_tool_order_key(&b.name)));
}

pub fn merge_tool_definitions(
    base: Vec<ToolDefinition>,
    extra: Vec<ToolDefinition>,
) -> Vec<ToolDefinition> {
    let mut merged: HashMap<String, ToolDefinition> = HashMap::new();
    for tool in base.into_iter().chain(extra) {
        merged.insert(tool.name.clone(), tool);
    }

    let mut tools: Vec<ToolDefinition> = merged.into_values().collect();
    prioritize_tool_definitions(&mut tools);
    tools
}

pub async fn resolve_tools_with_mcp(
    tool_registry: &rocode_tool::ToolRegistry,
    mcp_tools: Vec<ToolDefinition>,
) -> Vec<ToolDefinition> {
    let base = tool_registry
        .list_schemas()
        .await
        .into_iter()
        .map(|s| ToolDefinition {
            name: s.name,
            description: Some(s.description),
            parameters: s.parameters,
        })
        .collect();

    merge_tool_definitions(base, mcp_tools)
}

pub async fn resolve_tools_with_mcp_registry(
    tool_registry: &rocode_tool::ToolRegistry,
    mcp_registry: Option<&rocode_mcp::McpToolRegistry>,
) -> Vec<ToolDefinition> {
    let dynamic_mcp_tools = if let Some(registry) = mcp_registry {
        registry
            .list()
            .await
            .into_iter()
            .map(|tool| ToolDefinition {
                name: tool.full_name,
                description: tool.description,
                parameters: tool.input_schema,
            })
            .collect()
    } else {
        Vec::new()
    };

    resolve_tools_with_mcp(tool_registry, dynamic_mcp_tools).await
}

pub async fn resolve_tools(tool_registry: &rocode_tool::ToolRegistry) -> Vec<ToolDefinition> {
    resolve_tools_with_mcp_registry(tool_registry, None).await
}

#[cfg(test)]
mod title_tests {
    use super::*;

    #[test]
    fn prioritize_tool_definitions_prefers_task_flow_over_task() {
        let mut tools = vec![
            ToolDefinition {
                name: "websearch".to_string(),
                description: None,
                parameters: serde_json::json!({}),
            },
            ToolDefinition {
                name: "task".to_string(),
                description: None,
                parameters: serde_json::json!({}),
            },
            ToolDefinition {
                name: "task_flow".to_string(),
                description: None,
                parameters: serde_json::json!({}),
            },
        ];

        prioritize_tool_definitions(&mut tools);
        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(names, vec!["task_flow", "task", "websearch"]);
    }
}

// --- Misc ---

pub fn max_steps_for_agent(agent_steps: Option<u32>) -> u32 {
    agent_steps.unwrap_or(MAX_STEPS)
}

pub fn generate_session_title(first_user_message: &str) -> String {
    let first_line = first_user_message.lines().next().unwrap_or("").trim();

    if first_line.chars().count() > 100 {
        format!("{}...", first_line.chars().take(97).collect::<String>())
    } else if first_line.is_empty() {
        "New Session".to_string()
    } else {
        first_line.to_string()
    }
}

fn trim_title_source(text: &str, max_chars: usize) -> String {
    let normalized = sanitize_display_text(text).trim().to_string();
    if normalized.chars().count() <= max_chars {
        normalized
    } else {
        normalized.chars().take(max_chars).collect::<String>()
    }
}

pub fn compose_session_title_source(session: &Session) -> Option<(String, String)> {
    let first_user = session
        .messages
        .iter()
        .find(|message| matches!(message.role, Role::User))
        .map(SessionMessage::get_text)
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())?;

    let fallback = generate_session_title(&first_user);
    let mut sections = vec![format!(
        "User request:\n{}",
        trim_title_source(&first_user, 400)
    )];

    if let Some(assistant_text) = session
        .messages
        .iter()
        .rev()
        .filter(|message| matches!(message.role, Role::Assistant))
        .map(SessionMessage::get_text)
        .map(|text| trim_title_source(&text, 600))
        .find(|text| !text.trim().is_empty())
    {
        sections.push(format!("Assistant outcome:\n{}", assistant_text));
    }

    Some((sections.join("\n\n"), fallback))
}

/// Generate a refined session title from the session's first-turn context.
/// Uses the first user request and, when available, the latest assistant
/// outcome already persisted in the session.
pub async fn generate_session_title_for_session(
    session: &Session,
    provider: Arc<dyn Provider>,
    model_id: &str,
) -> String {
    let Some((title_source, fallback)) = compose_session_title_source(session) else {
        return "New Session".to_string();
    };

    let request = session_title_request(model_id).to_chat_request_with_system(
        vec![Message {
            role: ProviderRole::User,
            content: Content::Text(format!(
                "Generate a short session title (under 80 chars) for this conversation.\n\
                 Base it on the actual task and outcome, not the user's raw wording.\n\
                 Reply with ONLY the title, no quotes or explanation.\n\n{}",
                title_source
            )),
            cache_control: None,
            provider_options: None,
        }],
        vec![],
        None,
        Some(
            "You generate concise conversation titles. Prefer compact task-focused summaries. Reply with only the title."
                .to_string(),
        ),
    );

    match provider.chat(request).await {
        Ok(response) => {
            let text = response
                .choices
                .first()
                .map(|c| match &c.message.content {
                    Content::Text(t) => t.clone(),
                    Content::Parts(parts) => parts
                        .iter()
                        .filter_map(|p| p.text.clone())
                        .collect::<Vec<_>>()
                        .join(""),
                })
                .unwrap_or_default();

            let cleaned = text
                .replace(['"', '\''], "")
                .lines()
                .map(|l| l.trim())
                .find(|l| !l.is_empty() && !l.starts_with("<think>"))
                .unwrap_or("")
                .to_string();

            if cleaned.is_empty() {
                fallback
            } else if cleaned.chars().count() > 100 {
                format!("{}...", cleaned.chars().take(97).collect::<String>())
            } else {
                cleaned
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to generate title via LLM, using fallback");
            fallback
        }
    }
}

/// Generate a session title using an LLM (matching TS `ensureTitle`).
/// Falls back to `generate_session_title` on any failure.
pub async fn generate_session_title_llm(
    first_user_message: &str,
    provider: Arc<dyn Provider>,
    model_id: &str,
) -> String {
    let fallback = generate_session_title(first_user_message);

    let request = session_title_request(model_id).to_chat_request_with_system(
        vec![Message {
            role: Role::User,
            content: Content::Text(format!(
                "Generate a short title (under 80 chars) for this conversation. \
                     Reply with ONLY the title, no quotes or explanation.\n\n{}",
                first_user_message
            )),
            cache_control: None,
            provider_options: None,
        }],
        vec![],
        None,
        Some("You generate concise conversation titles. Reply with only the title.".to_string()),
    );

    match provider.chat(request).await {
        Ok(response) => {
            // Extract text from the first choice
            let text = response
                .choices
                .first()
                .map(|c| match &c.message.content {
                    Content::Text(t) => t.clone(),
                    Content::Parts(parts) => parts
                        .iter()
                        .filter_map(|p| p.text.clone())
                        .collect::<Vec<_>>()
                        .join(""),
                })
                .unwrap_or_default();

            // Clean up: remove thinking tags, take first non-empty line
            let cleaned = text
                .replace(['"', '\''], "")
                .lines()
                .map(|l| l.trim())
                .find(|l| !l.is_empty() && !l.starts_with("<think>"))
                .unwrap_or("")
                .to_string();

            if cleaned.is_empty() {
                fallback
            } else if cleaned.chars().count() > 100 {
                format!("{}...", cleaned.chars().take(97).collect::<String>())
            } else {
                cleaned
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to generate title via LLM, using fallback");
            fallback
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream;
    use rocode_provider::{
        ChatRequest, ChatResponse, Choice, Message as ProviderMessage, ModelInfo, ProviderError,
        StreamResult,
    };
    use std::sync::{Arc, Mutex};

    #[derive(Debug)]
    struct CaptureProvider {
        title: String,
        last_prompt: Arc<Mutex<Option<String>>>,
    }

    #[async_trait]
    impl Provider for CaptureProvider {
        fn id(&self) -> &str {
            "capture"
        }

        fn name(&self) -> &str {
            "Capture"
        }

        fn models(&self) -> Vec<ModelInfo> {
            Vec::new()
        }

        fn get_model(&self, _id: &str) -> Option<&ModelInfo> {
            None
        }

        async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
            let text = request
                .messages
                .first()
                .map(|message| match &message.content {
                    Content::Text(text) => text.clone(),
                    Content::Parts(parts) => parts
                        .iter()
                        .filter_map(|part| part.text.clone())
                        .collect::<Vec<_>>()
                        .join(" "),
                })
                .unwrap_or_default();
            *self.last_prompt.lock().expect("capture prompt") = Some(text);
            Ok(ChatResponse {
                id: "capture-response".to_string(),
                model: "capture-model".to_string(),
                choices: vec![Choice {
                    index: 0,
                    message: ProviderMessage {
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
    fn compose_session_title_source_includes_assistant_outcome() {
        let mut session = Session::new(".");
        session.add_user_message("根据 ./t.html 文件，设计一个科技感更加浓重的网页");
        session
            .add_assistant_message()
            .add_text("已完成首页重构，强化了深色科技风、发光边框和分层卡片布局。");

        let (source, fallback) =
            compose_session_title_source(&session).expect("title source should exist");
        assert!(source.contains("User request:"));
        assert!(source.contains("Assistant outcome:"));
        assert!(source.contains("已完成首页重构"));
        assert_eq!(fallback, "根据 ./t.html 文件，设计一个科技感更加浓重的网页");
    }

    #[tokio::test]
    async fn generate_session_title_for_session_uses_assistant_context() {
        let mut session = Session::new(".");
        session.add_user_message("Fix the scheduler session title flow after first reply");
        session
            .add_assistant_message()
            .add_text("Implemented refined title regeneration based on the first completed turn.");

        let last_prompt = Arc::new(Mutex::new(None));
        let provider = Arc::new(CaptureProvider {
            title: "Refine Session Titles After First Reply".to_string(),
            last_prompt: last_prompt.clone(),
        });

        let title = generate_session_title_for_session(&session, provider, "mock-model").await;
        assert_eq!(title, "Refine Session Titles After First Reply");

        let captured = last_prompt
            .lock()
            .expect("capture prompt")
            .clone()
            .unwrap_or_default();
        assert!(captured.contains("User request:"));
        assert!(captured.contains("Assistant outcome:"));
        assert!(captured.contains("Implemented refined title regeneration"));
    }
}
