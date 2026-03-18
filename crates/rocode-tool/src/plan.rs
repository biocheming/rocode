use async_trait::async_trait;
use rocode_core::contracts::{
    output_blocks::MessageRoleWire,
    session::MessagePartTypeWire,
    task::metadata_keys as task_metadata_keys,
    tools::{arg_keys as tool_arg_keys, BuiltinToolName},
    wire::{self, aliases as wire_aliases},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{Metadata, QuestionDef, QuestionOption, Tool, ToolContext, ToolError, ToolResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanEnterParams {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanExitParams {}

pub struct PlanEnterTool;

pub struct PlanExitTool;

const PLAN_FILE: &str = "PLAN.md";

/// Create a user message and a synthetic text part via the ToolContext callbacks,
/// matching the TS `Session.updateMessage()` + `Session.updatePart()` pattern.
async fn create_user_message_with_part(
    ctx: &ToolContext,
    agent: &str,
    model: &Option<String>,
    text: &str,
) -> Result<(), ToolError> {
    let now = chrono::Utc::now().timestamp_millis();
    let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
    let part_id = format!("prt_{}", uuid::Uuid::new_v4().simple());

    // Build the MessageV2.User info matching the TS MessageInfo::User shape
    let mut user_msg = serde_json::json!({
        "id": message_id,
        "role": MessageRoleWire::User.as_str(),
        "time": { "created": now },
        "agent": agent,
    });
    user_msg[wire::keys::SESSION_ID] = serde_json::json!(ctx.session_id);
    if let Some(ref m) = model {
        user_msg[task_metadata_keys::MODEL] = serde_json::json!(m);
    }

    // Persist the message (mirrors TS Session.updateMessage)
    ctx.do_update_message(user_msg).await?;

    // Build the synthetic text part matching the TS MessageV2.TextPart shape
    let mut text_part = serde_json::json!({
        "id": part_id,
        "type": MessagePartTypeWire::Text.as_str(),
        "text": text,
        "synthetic": true,
    });
    text_part[wire::keys::MESSAGE_ID] = serde_json::json!(message_id);
    text_part[wire::keys::SESSION_ID] = serde_json::json!(ctx.session_id);

    // Persist the part (mirrors TS Session.updatePart)
    ctx.do_update_part(text_part).await?;

    Ok(())
}

#[async_trait]
impl Tool for PlanEnterTool {
    fn id(&self) -> &str {
        BuiltinToolName::PlanEnter.as_str()
    }

    fn description(&self) -> &str {
        "Switch to plan mode for research and planning. In plan mode, you can read files and create plans but cannot make changes. Use this when you need to thoroughly analyze a problem before implementing."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let plan_path = get_plan_path(&ctx);
        let plan_display = plan_path.display();

        let questions = vec![QuestionDef {
            question: format!(
                "Would you like to switch to the plan agent and create a plan saved to {}?",
                plan_display
            ),
            header: Some("Plan Mode".to_string()),
            options: vec![
                QuestionOption {
                    label: "Yes".to_string(),
                    description: Some("Switch to plan agent for research and planning".to_string()),
                },
                QuestionOption {
                    label: "No".to_string(),
                    description: Some(
                        "Stay with build agent to continue making changes".to_string(),
                    ),
                },
            ],
            multiple: false,
        }];

        let answers = ctx.question(questions).await?;

        let answer = answers
            .first()
            .and_then(|a| a.first())
            .map(|s| s.as_str())
            .unwrap_or("No");

        if answer == "No" {
            return Err(ToolError::QuestionRejected(
                "User rejected plan mode switch".to_string(),
            ));
        }

        let model = ctx.do_get_last_model().await;

        // Create a user message + synthetic part (mirrors TS Session.updateMessage + updatePart)
        let synthetic_text =
            "User has requested to enter plan mode. Switch to plan mode and begin planning.";
        create_user_message_with_part(&ctx, "plan", &model, synthetic_text).await?;

        ctx.do_switch_agent("plan".to_string(), model.clone())
            .await?;

        let mut metadata = Metadata::new();
        metadata.insert(tool_arg_keys::AGENT.to_string(), serde_json::json!("plan"));
        metadata.insert(
            wire_aliases::SESSION_ID_SNAKE.to_string(),
            serde_json::json!(ctx.session_id),
        );
        if let Some(ref m) = model {
            metadata.insert(task_metadata_keys::MODEL.to_string(), serde_json::json!(m));
        }

        Ok(ToolResult {
            output: format!(
                "User confirmed to switch to plan mode. A new message has been created to switch you to plan mode. The plan file will be at {}. Begin planning.",
                plan_display
            ),
            title: "Switching to plan agent".to_string(),
            metadata,
            truncated: false,
        })
    }
}

#[async_trait]
impl Tool for PlanExitTool {
    fn id(&self) -> &str {
        BuiltinToolName::PlanExit.as_str()
    }

    fn description(&self) -> &str {
        "Exit plan mode and switch to build mode for implementation. Use this when you have completed your plan and are ready to make file changes."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let plan_path = get_plan_path(&ctx);
        let plan_display = plan_path.display();

        let questions = vec![QuestionDef {
            question: format!("Plan at {} is complete. Would you like to switch to the build agent and start implementing?", plan_display),
            header: Some("Build Agent".to_string()),
            options: vec![
                QuestionOption {
                    label: "Yes".to_string(),
                    description: Some("Switch to build agent and start implementing the plan".to_string()),
                },
                QuestionOption {
                    label: "No".to_string(),
                    description: Some("Stay with plan agent to continue refining the plan".to_string()),
                },
            ],
            multiple: false,
        }];

        let answers = ctx.question(questions).await?;

        let answer = answers
            .first()
            .and_then(|a| a.first())
            .map(|s| s.as_str())
            .unwrap_or("No");

        if answer == "No" {
            return Err(ToolError::QuestionRejected(
                "User rejected build mode switch".to_string(),
            ));
        }

        let model = ctx.do_get_last_model().await;

        let plan_path = get_plan_path(&ctx);
        let plan_display = plan_path.display();

        // Create a user message + synthetic part (mirrors TS Session.updateMessage + updatePart)
        let synthetic_text = format!(
            "The plan at {} has been approved, you can now edit files. Execute the plan.",
            plan_display
        );
        create_user_message_with_part(&ctx, "build", &model, &synthetic_text).await?;

        ctx.do_switch_agent("build".to_string(), model.clone())
            .await?;

        let mut metadata = Metadata::new();
        metadata.insert(tool_arg_keys::AGENT.to_string(), serde_json::json!("build"));
        metadata.insert(
            wire_aliases::SESSION_ID_SNAKE.to_string(),
            serde_json::json!(ctx.session_id),
        );
        if let Some(ref m) = model {
            metadata.insert(task_metadata_keys::MODEL.to_string(), serde_json::json!(m));
        }

        Ok(ToolResult {
            output: "User approved switching to build agent. Wait for further instructions."
                .to_string(),
            title: "Switching to build agent".to_string(),
            metadata,
            truncated: false,
        })
    }
}

fn get_plan_path(ctx: &ToolContext) -> PathBuf {
    PathBuf::from(&ctx.worktree)
        .join(".opencode")
        .join(PLAN_FILE)
}
