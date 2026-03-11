use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;

use rocode_orchestrator::{
    ExecutionContext, ModelRef as OrchestratorModelRef, ModelResolver as OrchestratorModelResolver,
    OrchestratorError, ToolExecError as OrchestratorToolExecError,
    ToolExecutor as OrchestratorToolExecutor, ToolOutput as OrchestratorToolOutput,
};
use rocode_provider::{ChatRequest, ProviderRegistry};
use rocode_tool::{ToolContext, ToolRegistry};

use super::{attach_subsession_callbacks, map_tool_error, SubsessionState};
use crate::AgentInfo;

fn preferred_tool_order_key(name: &str) -> (u8, &str) {
    match name {
        "task_flow" => (0, name),
        "task" => (1, name),
        _ => (2, name),
    }
}

fn prioritize_tool_definitions(tools: &mut [rocode_provider::ToolDefinition]) {
    tools.sort_by(|a, b| preferred_tool_order_key(&a.name).cmp(&preferred_tool_order_key(&b.name)));
}

fn map_tool_result(result: rocode_tool::ToolResult) -> OrchestratorToolOutput {
    OrchestratorToolOutput {
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
    }
}

pub(super) struct ToolRegistryAdapter {
    tools: Arc<ToolRegistry>,
    agent: AgentInfo,
    disabled_tools: HashSet<String>,
    providers: Arc<ProviderRegistry>,
    subsessions: Arc<Mutex<HashMap<String, SubsessionState>>>,
    agent_registry: Arc<crate::AgentRegistry>,
    tool_runtime_config: rocode_tool::ToolRuntimeConfig,
    question_callback: Option<rocode_tool::QuestionCallback>,
    ask_callback: Option<rocode_tool::AskCallback>,
}

impl ToolRegistryAdapter {
    pub(super) fn new(
        tools: Arc<ToolRegistry>,
        agent: AgentInfo,
        disabled_tools: HashSet<String>,
        providers: Arc<ProviderRegistry>,
        subsessions: Arc<Mutex<HashMap<String, SubsessionState>>>,
        agent_registry: Arc<crate::AgentRegistry>,
        tool_runtime_config: rocode_tool::ToolRuntimeConfig,
        question_callback: Option<rocode_tool::QuestionCallback>,
        ask_callback: Option<rocode_tool::AskCallback>,
    ) -> Self {
        Self {
            tools,
            agent,
            disabled_tools,
            providers,
            subsessions,
            agent_registry,
            tool_runtime_config,
            question_callback,
            ask_callback,
        }
    }

    fn ensure_tool_allowed(&self, tool_name: &str) -> Result<(), OrchestratorToolExecError> {
        self.agent
            .ensure_tool_allowed(tool_name)
            .map_err(OrchestratorToolExecError::PermissionDenied)
    }

    fn current_model_string(&self) -> Option<String> {
        if let Some(model) = self.agent.model.as_ref() {
            return Some(format!("{}:{}", model.provider_id, model.model_id));
        }

        let provider = self.providers.list().into_iter().next()?;
        let model_id = provider.models().first().map(|m| m.id.clone())?;
        Some(format!("{}:{}", provider.id(), model_id))
    }

    fn build_tool_context(&self, exec_ctx: &ExecutionContext) -> ToolContext {
        let directory = if exec_ctx.workdir.is_empty() {
            std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        } else {
            exec_ctx.workdir.clone()
        };

        let current_model = self.current_model_string();
        let mut base_ctx = ToolContext::new(
            exec_ctx.session_id.clone(),
            "default".to_string(),
            directory,
        )
        .with_agent(self.agent.name.clone())
        .with_tool_runtime_config(self.tool_runtime_config.clone())
        .with_get_last_model({
            let current_model = current_model.clone();
            move |_session_id| {
                let current_model = current_model.clone();
                async move { Ok(current_model) }
            }
        });

        // Attach question callback if available
        if let Some(ref callback) = self.question_callback {
            base_ctx.ask_question = Some(callback.clone());
        }

        // Attach permission approval callback if available
        if let Some(ref callback) = self.ask_callback {
            base_ctx.ask = Some(callback.clone());
        }

        base_ctx.call_id = exec_ctx
            .metadata
            .get("call_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        base_ctx.extra = exec_ctx.metadata.clone();

        attach_subsession_callbacks(
            base_ctx,
            self.subsessions.clone(),
            self.providers.clone(),
            self.tools.clone(),
            self.agent_registry.clone(),
        )
    }
}

#[async_trait::async_trait]
impl OrchestratorToolExecutor for ToolRegistryAdapter {
    async fn execute(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
        exec_ctx: &ExecutionContext,
    ) -> Result<OrchestratorToolOutput, OrchestratorToolExecError> {
        if self.disabled_tools.contains(tool_name) {
            return Err(OrchestratorToolExecError::PermissionDenied(format!(
                "Tool '{}' is disabled for this subagent session",
                tool_name
            )));
        }

        self.ensure_tool_allowed(tool_name)?;
        let ctx = self.build_tool_context(exec_ctx);
        let result = self
            .tools
            .execute(tool_name, arguments, ctx)
            .await
            .map_err(map_tool_error)?;
        Ok(map_tool_result(result))
    }

    async fn list_ids(&self) -> Vec<String> {
        self.tools.list_ids().await
    }

    async fn list_definitions(
        &self,
        _exec_ctx: &ExecutionContext,
    ) -> Vec<rocode_provider::ToolDefinition> {
        let mut tools: Vec<rocode_provider::ToolDefinition> = self
            .tools
            .list_schemas()
            .await
            .into_iter()
            .filter(|schema| !self.disabled_tools.contains(&schema.name))
            .map(|schema| rocode_provider::ToolDefinition {
                name: schema.name,
                description: Some(schema.description),
                parameters: schema.parameters,
            })
            .collect();
        prioritize_tool_definitions(&mut tools);
        tools
    }
}

pub(super) struct ProviderModelResolver {
    pub(super) providers: Arc<ProviderRegistry>,
}

#[async_trait::async_trait]
impl OrchestratorModelResolver for ProviderModelResolver {
    async fn chat_stream(
        &self,
        model: Option<&OrchestratorModelRef>,
        messages: Vec<rocode_provider::Message>,
        tools: Vec<rocode_provider::ToolDefinition>,
        _exec_ctx: &ExecutionContext,
    ) -> Result<rocode_provider::StreamResult, OrchestratorError> {
        let (provider, model_id) = if let Some(model) = model {
            let provider = self
                .providers
                .get(&model.provider_id)
                .ok_or(OrchestratorError::NoProvider)?;
            (provider, model.model_id.clone())
        } else {
            let provider = self
                .providers
                .list()
                .into_iter()
                .next()
                .ok_or(OrchestratorError::NoProvider)?;
            let model_id = provider
                .models()
                .first()
                .map(|m| m.id.clone())
                .unwrap_or_default();
            (provider, model_id)
        };

        let request = ChatRequest::new(model_id, messages).with_tools(tools);
        provider
            .chat_stream(request)
            .await
            .map_err(|error| OrchestratorError::ModelError(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prioritize_tool_definitions_prefers_task_flow_over_task() {
        let mut tools = vec![
            rocode_provider::ToolDefinition {
                name: "task".to_string(),
                description: None,
                parameters: serde_json::json!({}),
            },
            rocode_provider::ToolDefinition {
                name: "websearch".to_string(),
                description: None,
                parameters: serde_json::json!({}),
            },
            rocode_provider::ToolDefinition {
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
