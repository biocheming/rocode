use async_trait::async_trait;
use rocode_core::contracts::{
    tools::{arg_keys as tool_arg_keys, BuiltinToolName, ToolCallStatusWire},
    wire,
};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

use crate::attachment_metadata::{
    collect_attachments_from_metadata, strip_attachments_from_metadata,
};
use crate::{Metadata, Tool, ToolContext, ToolError, ToolResult};

const MAX_BATCH_SIZE: usize = 25;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchParams {
    #[serde(default, alias = "toolCalls")]
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResult {
    pub tool: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<serde_json::Value>,
}

pub struct BatchTool;

type BatchFuture = Pin<Box<dyn Future<Output = BatchResult> + Send>>;

#[async_trait]
impl Tool for BatchTool {
    fn id(&self) -> &str {
        BuiltinToolName::Batch.as_str()
    }

    fn description(&self) -> &str {
        "Execute multiple tool calls in parallel. Maximum 25 tools per batch. Use this for optimal performance when you need to run multiple independent operations."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                (tool_arg_keys::TOOL_CALLS_CAMEL): {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 25,
                    "items": {
                        "type": "object",
                        "properties": {
                            (tool_arg_keys::TOOL): {
                                "type": "string",
                                "description": "The name of the tool to execute"
                            },
                            (tool_arg_keys::PARAMETERS): {
                                "type": "object",
                                "description": "Parameters for the tool"
                            }
                        },
                        "required": [tool_arg_keys::TOOL, tool_arg_keys::PARAMETERS]
                    },
                    "description": "Array of tool calls to execute in parallel"
                }
            },
            "required": [tool_arg_keys::TOOL_CALLS_CAMEL]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let params: BatchParams = serde_json::from_value(args)
            .map_err(|e| ToolError::InvalidArguments(format!("Invalid parameters: {}", e)))?;

        let total_calls = params.tool_calls.len();
        let tool_calls: Vec<_> = params.tool_calls.into_iter().take(MAX_BATCH_SIZE).collect();
        let discarded_count = total_calls.saturating_sub(MAX_BATCH_SIZE);

        if tool_calls.is_empty() {
            return Err(ToolError::ValidationError(
                "Provide at least one tool call".to_string(),
            ));
        }

        let registry = match &ctx.registry {
            Some(r) => r.clone(),
            None => {
                return Err(ToolError::ExecutionError(
                    "Tool registry not available. Batch execution requires registry access."
                        .to_string(),
                ));
            }
        };

        let mut futures: Vec<BatchFuture> = Vec::new();

        for call in tool_calls {
            if BuiltinToolName::parse(&call.tool).is_some_and(|tool| tool == BuiltinToolName::Batch)
            {
                let tool_name = call.tool.clone();
                let err_msg = format!(
                    "Tool '{}' is not allowed in batch. Disallowed: {}",
                    tool_name,
                    BuiltinToolName::Batch.as_str(),
                );
                futures.push(Box::pin(async move {
                    BatchResult {
                        tool: tool_name,
                        success: false,
                        error: Some(err_msg),
                        attachments: Vec::new(),
                    }
                }) as BatchFuture);
                continue;
            }

            let registry = registry.clone();
            let tool_name = call.tool.clone();
            let tool_params = call.parameters.clone();
            let ctx_clone = ctx.clone();
            let session_id = ctx.session_id.clone();
            let message_id = ctx.message_id.clone();
            let call_id = uuid::Uuid::new_v4().to_string();
            let call_start_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;

            futures.push(Box::pin(async move {
                let mut running_part = serde_json::json!({
                        "id": call_id,
                        "type": "tool",
                        "tool": tool_name,
                        "callID": call_id,
                        "state": {
                            "status": ToolCallStatusWire::Running.as_str(),
                            "input": tool_params,
                            "time": {
                                "start": call_start_time
                            }
                        }
                    });
                running_part[wire::keys::MESSAGE_ID] = serde_json::json!(message_id);
                running_part[wire::keys::SESSION_ID] = serde_json::json!(session_id);
                let _ = ctx_clone.do_update_part(running_part).await;

                let result = match registry.get(&tool_name).await {
                    Some(tool) => {
                        match tool.execute(tool_params.clone(), ctx_clone.clone()).await {
                            Ok(res) => {
                                let call_end_time = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis()
                                    as u64;

                                let mut completed_part = serde_json::json!({
                                        "id": call_id,
                                        "type": "tool",
                                        "tool": tool_name,
                                        "callID": call_id,
                                        "state": {
                                            "status": ToolCallStatusWire::Completed.as_str(),
                                            "input": tool_params,
                                            (tool_arg_keys::OUTPUT): res.output,
                                            "title": res.title,
                                            "metadata": strip_attachments_from_metadata(&res.metadata),
                                            "attachments": collect_attachments_from_metadata(&res.metadata),
                                            "time": {
                                                "start": call_start_time,
                                                "end": call_end_time
                                            }
                                        }
                                    });
                                completed_part[wire::keys::MESSAGE_ID] =
                                    serde_json::json!(message_id);
                                completed_part[wire::keys::SESSION_ID] =
                                    serde_json::json!(session_id);
                                let _ = ctx_clone.do_update_part(completed_part).await;

                                let attachments = collect_attachments_from_metadata(&res.metadata);

                                BatchResult {
                                    tool: tool_name,
                                    success: true,
                                    error: None,
                                    attachments,
                                }
                            }
                            Err(e) => {
                                let call_end_time = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis()
                                    as u64;

                                let mut error_part = serde_json::json!({
                                        "id": call_id,
                                        "type": "tool",
                                        "tool": tool_name,
                                        "callID": call_id,
                                        "state": {
                                            "status": ToolCallStatusWire::Error.as_str(),
                                            "input": tool_params,
                                            (tool_arg_keys::ERROR): e.to_string(),
                                            "time": {
                                                "start": call_start_time,
                                                "end": call_end_time
                                            }
                                        }
                                    });
                                error_part[wire::keys::MESSAGE_ID] = serde_json::json!(message_id);
                                error_part[wire::keys::SESSION_ID] = serde_json::json!(session_id);
                                let _ = ctx_clone.do_update_part(error_part).await;

                                BatchResult {
                                    tool: tool_name,
                                    success: false,
                                    error: Some(e.to_string()),
                                    attachments: Vec::new(),
                                }
                            }
                        }
                    }
                    None => {
                        let available = registry.suggest_tools(&tool_name).await;
                        let err_msg = format!(
                            "Tool '{}' not in registry. External tools (MCP, environment) cannot be batched - call them directly. Available tools: {}",
                            tool_name,
                            available.join(", ")
                        );
                        BatchResult {
                            tool: tool_name.clone(),
                            success: false,
                            error: Some(err_msg),
                            attachments: Vec::new(),
                        }
                    }
                };

                result
            }) as BatchFuture);
        }

        let results: Vec<BatchResult> = futures::future::join_all(futures).await;

        let mut final_results = results;

        if discarded_count > 0 {
            final_results.push(BatchResult {
                tool: BuiltinToolName::Batch.as_str().to_string(),
                success: false,
                error: Some(format!(
                    "{} additional calls discarded (max {} per batch)",
                    discarded_count, MAX_BATCH_SIZE
                )),
                attachments: Vec::new(),
            });
        }

        let successful = final_results.iter().filter(|r| r.success).count();
        let failed = final_results.len() - successful;

        let output = if failed > 0 {
            format!(
                "Executed {}/{} tools successfully. {} failed.",
                successful,
                final_results.len(),
                failed
            )
        } else {
            format!(
                "All {} tools executed successfully.\n\nKeep using the batch tool for optimal performance in your next response!",
                successful
            )
        };

        let tools_list: Vec<&str> = final_results.iter().map(|r| r.tool.as_str()).collect();
        let aggregated_attachments: Vec<serde_json::Value> = final_results
            .iter()
            .filter(|r| r.success)
            .flat_map(|r| r.attachments.clone())
            .collect();

        let mut metadata = Metadata::new();
        metadata.insert("total".to_string(), serde_json::json!(final_results.len()));
        metadata.insert("successful".to_string(), serde_json::json!(successful));
        metadata.insert("failed".to_string(), serde_json::json!(failed));
        metadata.insert("tools".to_string(), serde_json::json!(tools_list));
        metadata.insert(
            "details".to_string(),
            serde_json::json!(final_results
                .iter()
                .map(|r| serde_json::json!({
                    (tool_arg_keys::TOOL): r.tool,
                    (tool_arg_keys::SUCCESS): r.success
                }))
                .collect::<Vec<_>>()),
        );
        if !aggregated_attachments.is_empty() {
            metadata.insert(
                "attachments".to_string(),
                serde_json::Value::Array(aggregated_attachments),
            );
        }

        Ok(ToolResult {
            output,
            title: format!("Batch execution ({}/{})", successful, final_results.len()),
            metadata,
            truncated: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::BatchParams;
    use rocode_core::contracts::tools::BuiltinToolName;

    #[test]
    fn batch_params_accepts_camel_case_tool_calls() {
        let value = serde_json::json!({
            "toolCalls": [
                { "tool": BuiltinToolName::Read.as_str(), "parameters": { "file_path": "index.html" } }
            ]
        });
        let parsed: BatchParams = serde_json::from_value(value).expect("should parse toolCalls");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].tool, BuiltinToolName::Read.as_str());
    }

    #[test]
    fn batch_params_defaults_tool_calls_when_missing() {
        let parsed: BatchParams =
            serde_json::from_value(serde_json::json!({})).expect("should parse empty object");
        assert!(parsed.tool_calls.is_empty());
    }
}
