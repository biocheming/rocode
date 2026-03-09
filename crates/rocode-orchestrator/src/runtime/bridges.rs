use crate::runtime::events::{LoopError, LoopRequest, ToolCallReady, ToolResult};
use crate::runtime::traits::{ModelCaller, ToolDispatcher};
use crate::tool_runner::{ToolCallInput, ToolRunner};
use crate::traits::{ModelResolver, ToolExecutor};
use crate::types::{ExecutionContext, ModelRef};
use async_trait::async_trait;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// ModelCallerBridge – adapts orchestrator ModelResolver → runtime ModelCaller.
//
// Captures model identity and execution context so that run_loop does not
// need to know about orchestrator-level concerns.
// ---------------------------------------------------------------------------

pub struct ModelCallerBridge {
    model_resolver: Arc<dyn ModelResolver>,
    model: Option<ModelRef>,
    exec_ctx: ExecutionContext,
}

impl ModelCallerBridge {
    pub fn new(
        model_resolver: Arc<dyn ModelResolver>,
        model: Option<ModelRef>,
        exec_ctx: ExecutionContext,
    ) -> Self {
        Self {
            model_resolver,
            model,
            exec_ctx,
        }
    }
}

#[async_trait]
impl ModelCaller for ModelCallerBridge {
    async fn call_stream(
        &self,
        req: LoopRequest,
    ) -> Result<rocode_provider::StreamResult, LoopError> {
        self.model_resolver
            .chat_stream(self.model.as_ref(), req.messages, req.tools, &self.exec_ctx)
            .await
            .map_err(|e| LoopError::ModelError(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// ToolDispatcherBridge – adapts orchestrator ToolRunner → runtime ToolDispatcher.
//
// Preserves ToolRunner's name-repair and "invalid" fallback logic.
// Captures execution context for tool dispatch and definition listing.
// ---------------------------------------------------------------------------

pub struct ToolDispatcherBridge {
    tool_runner: ToolRunner,
    tool_executor: Arc<dyn ToolExecutor>,
    exec_ctx: ExecutionContext,
}

impl ToolDispatcherBridge {
    pub fn new(
        tool_runner: ToolRunner,
        tool_executor: Arc<dyn ToolExecutor>,
        exec_ctx: ExecutionContext,
    ) -> Self {
        Self {
            tool_runner,
            tool_executor,
            exec_ctx,
        }
    }
}

#[async_trait]
impl ToolDispatcher for ToolDispatcherBridge {
    async fn execute(&self, call: &ToolCallReady) -> ToolResult {
        let input = ToolCallInput {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        };
        let mut exec_ctx = self.exec_ctx.clone();
        exec_ctx
            .metadata
            .insert("call_id".to_string(), serde_json::json!(call.id));
        let output = self.tool_runner.execute_tool_call(input, &exec_ctx).await;
        ToolResult {
            tool_call_id: output.tool_call_id,
            tool_name: output.tool_name,
            output: output.content,
            is_error: output.is_error,
            title: output.title,
            metadata: output.metadata,
        }
    }

    async fn list_definitions(&self) -> Vec<rocode_provider::ToolDefinition> {
        self.tool_executor.list_definitions(&self.exec_ctx).await
    }
}
