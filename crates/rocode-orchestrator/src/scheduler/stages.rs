use crate::runtime::policy::{LoopPolicy, ToolDedupScope};
use crate::skill_list::SkillListOrchestrator;
use crate::traits::{Orchestrator, ToolExecutor};
use crate::{
    AgentDescriptor, OrchestratorContext, OrchestratorError, OrchestratorOutput, ToolExecError,
    ToolOutput, ToolRunner,
};
use async_trait::async_trait;
use rocode_core::contracts::tools::BuiltinToolName;
use std::collections::HashSet;
use std::sync::Arc;

pub const READ_ONLY_STAGE_TOOLS: &[&str] = &[
    BuiltinToolName::Read.as_str(),
    BuiltinToolName::Glob.as_str(),
    BuiltinToolName::Grep.as_str(),
    BuiltinToolName::Ls.as_str(),
    BuiltinToolName::AstGrepSearch.as_str(),
];

pub type StageToolArgumentValidator =
    fn(&str, &serde_json::Value, &crate::ExecutionContext) -> Result<(), ToolExecError>;

#[derive(Debug, Clone, Copy)]
pub struct StageToolConstraint {
    pub allowed_tools: &'static [&'static str],
    pub validator_id: Option<&'static str>,
    pub argument_validator: Option<StageToolArgumentValidator>,
}

impl StageToolConstraint {
    pub const fn new(
        allowed_tools: &'static [&'static str],
        validator_id: Option<&'static str>,
        argument_validator: Option<StageToolArgumentValidator>,
    ) -> Self {
        Self {
            allowed_tools,
            validator_id,
            argument_validator,
        }
    }
}

impl PartialEq for StageToolConstraint {
    fn eq(&self, other: &Self) -> bool {
        self.allowed_tools == other.allowed_tools && self.validator_id == other.validator_id
    }
}

impl Eq for StageToolConstraint {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageToolPolicy {
    AllowAll,
    AllowReadOnly,
    Restricted(StageToolConstraint),
    DisableAll,
}

impl StageToolPolicy {
    pub fn label(self) -> String {
        match self {
            Self::AllowAll => "allow-all".to_string(),
            Self::AllowReadOnly => "allow-read-only".to_string(),
            Self::Restricted(constraint) => {
                format!("restricted:{}", constraint.validator_id.unwrap_or("custom"))
            }
            Self::DisableAll => "disable-all".to_string(),
        }
    }
}

pub fn stage_agent(name: &str, system_prompt: String, max_steps: u32) -> AgentDescriptor {
    stage_agent_with_limit(name, system_prompt, Some(max_steps))
}

pub fn stage_agent_unbounded(name: &str, system_prompt: String) -> AgentDescriptor {
    stage_agent_with_limit(name, system_prompt, None)
}

fn stage_agent_with_limit(
    name: &str,
    system_prompt: String,
    max_steps: Option<u32>,
) -> AgentDescriptor {
    AgentDescriptor {
        name: name.to_string(),
        system_prompt: Some(system_prompt),
        model: None,
        max_steps,
        temperature: Some(0.2),
        allowed_tools: Vec::new(),
    }
}

pub async fn execute_stage_agent(
    input: &str,
    ctx: &OrchestratorContext,
    agent: AgentDescriptor,
    policy: StageToolPolicy,
    stage_context: Option<(String, u32)>,
) -> Result<OrchestratorOutput, OrchestratorError> {
    let loop_policy = LoopPolicy {
        max_steps: agent.max_steps,
        tool_dedup: ToolDedupScope::PerStep,
        ..Default::default()
    };
    let (stage_ctx, runner) = filtered_stage_context(ctx, policy);
    let mut orchestrator = SkillListOrchestrator::new(agent, runner).with_loop_policy(loop_policy);
    if let Some((stage_name, stage_index)) = stage_context {
        orchestrator.set_stage_context(stage_name, stage_index);
    }
    orchestrator.execute(input, &stage_ctx).await
}

fn filtered_stage_context(
    ctx: &OrchestratorContext,
    policy: StageToolPolicy,
) -> (OrchestratorContext, ToolRunner) {
    let filtered_executor: Arc<dyn ToolExecutor> =
        Arc::new(FilteredToolExecutor::new(ctx.tool_executor.clone(), policy));
    let stage_ctx = OrchestratorContext {
        agent_resolver: ctx.agent_resolver.clone(),
        model_resolver: ctx.model_resolver.clone(),
        tool_executor: filtered_executor.clone(),
        lifecycle_hook: ctx.lifecycle_hook.clone(),
        cancel_token: ctx.cancel_token.clone(),
        exec_ctx: ctx.exec_ctx.clone(),
    };
    (stage_ctx, ToolRunner::new(filtered_executor))
}

struct FilteredToolExecutor {
    inner: Arc<dyn ToolExecutor>,
    allowed_tools: Option<HashSet<String>>,
    policy: StageToolPolicy,
}

impl FilteredToolExecutor {
    fn new(inner: Arc<dyn ToolExecutor>, policy: StageToolPolicy) -> Self {
        let allowed_tools = match policy {
            StageToolPolicy::AllowAll => None,
            StageToolPolicy::AllowReadOnly => Some(
                READ_ONLY_STAGE_TOOLS
                    .iter()
                    .map(|tool| (*tool).to_string())
                    .collect(),
            ),
            StageToolPolicy::Restricted(constraint) => Some(
                constraint
                    .allowed_tools
                    .iter()
                    .map(|tool| (*tool).to_string())
                    .collect(),
            ),
            StageToolPolicy::DisableAll => Some(HashSet::new()),
        };
        Self {
            inner,
            allowed_tools,
            policy,
        }
    }

    fn is_allowed(&self, tool_name: &str) -> bool {
        match &self.allowed_tools {
            None => true,
            Some(allowed) => {
                allowed.contains(tool_name) || allowed.contains(&tool_name.to_ascii_lowercase())
            }
        }
    }

    fn validate_arguments(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
        exec_ctx: &crate::ExecutionContext,
    ) -> Result<(), ToolExecError> {
        match self.policy {
            StageToolPolicy::Restricted(constraint) => constraint
                .argument_validator
                .map(|validator| validator(tool_name, arguments, exec_ctx))
                .unwrap_or(Ok(())),
            _ => Ok(()),
        }
    }
}

#[async_trait]
impl ToolExecutor for FilteredToolExecutor {
    async fn execute(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
        exec_ctx: &crate::ExecutionContext,
    ) -> Result<ToolOutput, ToolExecError> {
        if !self.is_allowed(tool_name) {
            return Err(ToolExecError::PermissionDenied(format!(
                "tool `{tool_name}` is not available in this scheduler stage"
            )));
        }
        self.validate_arguments(tool_name, &arguments, exec_ctx)?;
        self.inner.execute(tool_name, arguments, exec_ctx).await
    }

    async fn list_ids(&self) -> Vec<String> {
        let mut ids = self.inner.list_ids().await;
        if self.allowed_tools.is_some() {
            ids.retain(|tool| self.is_allowed(tool));
        }
        ids
    }

    async fn list_definitions(
        &self,
        exec_ctx: &crate::ExecutionContext,
    ) -> Vec<rocode_provider::ToolDefinition> {
        let mut defs = self.inner.list_definitions(exec_ctx).await;
        if self.allowed_tools.is_some() {
            defs.retain(|tool| self.is_allowed(&tool.name));
        }
        defs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TestToolExecutor;

    #[async_trait]
    impl ToolExecutor for TestToolExecutor {
        async fn execute(
            &self,
            tool_name: &str,
            _arguments: serde_json::Value,
            _exec_ctx: &crate::ExecutionContext,
        ) -> Result<ToolOutput, ToolExecError> {
            Ok(ToolOutput {
                output: format!("ran:{tool_name}"),
                is_error: false,
                title: None,
                metadata: None,
            })
        }

        async fn list_ids(&self) -> Vec<String> {
            vec![
                "read".to_string(),
                "edit".to_string(),
                "write".to_string(),
                "grep".to_string(),
                "question".to_string(),
            ]
        }

        async fn list_definitions(
            &self,
            _exec_ctx: &crate::ExecutionContext,
        ) -> Vec<rocode_provider::ToolDefinition> {
            vec![
                rocode_provider::ToolDefinition {
                    name: "read".to_string(),
                    description: None,
                    parameters: json!({"type": "object"}),
                },
                rocode_provider::ToolDefinition {
                    name: "edit".to_string(),
                    description: None,
                    parameters: json!({"type": "object"}),
                },
                rocode_provider::ToolDefinition {
                    name: "write".to_string(),
                    description: None,
                    parameters: json!({"type": "object"}),
                },
                rocode_provider::ToolDefinition {
                    name: "question".to_string(),
                    description: None,
                    parameters: json!({"type": "object"}),
                },
            ]
        }
    }

    fn exec_ctx() -> crate::ExecutionContext {
        crate::ExecutionContext {
            session_id: "test".to_string(),
            workdir: "/repo".to_string(),
            agent_name: "scheduler-stage".to_string(),
            metadata: Default::default(),
        }
    }

    static VALIDATOR_CALLED: AtomicU32 = AtomicU32::new(0);

    fn record_validator(
        tool_name: &str,
        _arguments: &serde_json::Value,
        _exec_ctx: &crate::ExecutionContext,
    ) -> Result<(), ToolExecError> {
        VALIDATOR_CALLED.fetch_add(1, Ordering::SeqCst);
        if tool_name.eq_ignore_ascii_case("write") {
            return Err(ToolExecError::PermissionDenied(
                "write is blocked by the test validator".to_string(),
            ));
        }
        Ok(())
    }

    #[tokio::test]
    async fn allow_read_only_filters_tool_inventory() {
        let executor =
            FilteredToolExecutor::new(Arc::new(TestToolExecutor), StageToolPolicy::AllowReadOnly);
        let ids = executor.list_ids().await;
        assert_eq!(ids, vec!["read".to_string(), "grep".to_string()]);
        let defs = executor.list_definitions(&exec_ctx()).await;
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "read");
    }

    #[tokio::test]
    async fn restricted_policy_allows_custom_tool_inventory() {
        let before = VALIDATOR_CALLED.load(Ordering::SeqCst);
        let executor = FilteredToolExecutor::new(
            Arc::new(TestToolExecutor),
            StageToolPolicy::Restricted(StageToolConstraint::new(
                &["read", "write", "question"],
                Some("record-validator"),
                Some(record_validator),
            )),
        );

        let ids = executor.list_ids().await;
        assert_eq!(
            ids,
            vec![
                "read".to_string(),
                "write".to_string(),
                "question".to_string()
            ]
        );

        executor
            .execute(
                "question",
                json!({"questions": [{"question": "Continue?"}]}),
                &exec_ctx(),
            )
            .await
            .expect("question should be allowed");
        assert!(
            VALIDATOR_CALLED.load(Ordering::SeqCst) > before,
            "validator was not called"
        );
    }

    #[tokio::test]
    async fn restricted_policy_applies_custom_argument_validator() {
        let before = VALIDATOR_CALLED.load(Ordering::SeqCst);
        let executor = FilteredToolExecutor::new(
            Arc::new(TestToolExecutor),
            StageToolPolicy::Restricted(StageToolConstraint::new(
                &["read", "write"],
                Some("record-validator"),
                Some(record_validator),
            )),
        );
        let err = executor
            .execute(
                "write",
                json!({"file_path": "/repo/notes.md", "content": "# Notes"}),
                &exec_ctx(),
            )
            .await
            .expect_err("validator should block write");
        assert!(err.to_string().contains("test validator"));
        assert!(
            VALIDATOR_CALLED.load(Ordering::SeqCst) > before,
            "validator was not called"
        );
    }

    #[tokio::test]
    async fn disable_all_rejects_execution() {
        let executor =
            FilteredToolExecutor::new(Arc::new(TestToolExecutor), StageToolPolicy::DisableAll);
        let err = executor
            .execute("read", json!({}), &exec_ctx())
            .await
            .expect_err("tool should be blocked");
        assert!(err.to_string().contains("not available"));
    }
}
