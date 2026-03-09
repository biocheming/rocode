use crate::error::{OrchestratorError, ToolExecError};
use crate::types::{
    AgentDescriptor, ExecutionContext, ModelRef, OrchestratorContext, OrchestratorOutput,
    ToolOutput,
};
use async_trait::async_trait;

#[async_trait]
pub trait AgentResolver: Send + Sync {
    fn resolve(&self, name: &str) -> Option<AgentDescriptor>;
}

#[async_trait]
pub trait ModelResolver: Send + Sync {
    async fn chat_stream(
        &self,
        model: Option<&ModelRef>,
        messages: Vec<rocode_provider::Message>,
        tools: Vec<rocode_provider::ToolDefinition>,
        exec_ctx: &ExecutionContext,
    ) -> Result<rocode_provider::StreamResult, OrchestratorError>;
}

#[async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
        exec_ctx: &ExecutionContext,
    ) -> Result<ToolOutput, ToolExecError>;

    async fn list_ids(&self) -> Vec<String>;

    async fn list_definitions(
        &self,
        exec_ctx: &ExecutionContext,
    ) -> Vec<rocode_provider::ToolDefinition>;
}

#[async_trait]
pub trait LifecycleHook: Send + Sync {
    async fn on_orchestration_start(
        &self,
        agent_name: &str,
        max_steps: Option<u32>,
        exec_ctx: &ExecutionContext,
    );

    async fn on_step_start(
        &self,
        agent_name: &str,
        model_id: &str,
        step: u32,
        exec_ctx: &ExecutionContext,
    );

    async fn on_orchestration_end(&self, agent_name: &str, steps: u32, exec_ctx: &ExecutionContext);

    async fn on_scheduler_stage_start(
        &self,
        _agent_name: &str,
        _stage_name: &str,
        _stage_index: u32,
        _exec_ctx: &ExecutionContext,
    ) {
    }

    async fn on_scheduler_stage_end(
        &self,
        _agent_name: &str,
        _stage_name: &str,
        _stage_index: u32,
        _stage_total: u32,
        _content: &str,
        _exec_ctx: &ExecutionContext,
    ) {
    }

    async fn on_scheduler_stage_content(
        &self,
        _stage_name: &str,
        _stage_index: u32,
        _content_delta: &str,
        _exec_ctx: &ExecutionContext,
    ) {
    }
}

pub struct NoopLifecycleHook;

#[async_trait]
impl LifecycleHook for NoopLifecycleHook {
    async fn on_orchestration_start(&self, _: &str, _: Option<u32>, _: &ExecutionContext) {}

    async fn on_step_start(&self, _: &str, _: &str, _: u32, _: &ExecutionContext) {}

    async fn on_orchestration_end(&self, _: &str, _: u32, _: &ExecutionContext) {}
}

#[async_trait]
pub trait Orchestrator: Send + Sync {
    async fn execute(
        &mut self,
        input: &str,
        ctx: &OrchestratorContext,
    ) -> Result<OrchestratorOutput, OrchestratorError>;
}
