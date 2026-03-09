use crate::runtime::policy::{LoopPolicy, ToolDedupScope};
use crate::skill_list::SkillListOrchestrator;
use crate::traits::{Orchestrator, ToolExecutor};
use crate::{
    AgentDescriptor, OrchestratorContext, OrchestratorError, OrchestratorOutput, ToolExecError,
    ToolOutput, ToolRunner,
};
use async_trait::async_trait;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub const READ_ONLY_STAGE_TOOLS: &[&str] = &["read", "glob", "grep", "ls", "ast_grep_search"];
pub const PROMETHEUS_PLANNER_TOOLS: &[&str] =
    &["read", "glob", "grep", "ls", "ast_grep_search", "question"];
pub const PROMETHEUS_PLANNER_WRITE_TOOLS: &[&str] = &["write", "edit"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageToolPolicy {
    AllowAll,
    AllowReadOnly,
    PrometheusPlanning,
    DisableAll,
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
            StageToolPolicy::PrometheusPlanning => Some(
                PROMETHEUS_PLANNER_TOOLS
                    .iter()
                    .chain(PROMETHEUS_PLANNER_WRITE_TOOLS.iter())
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
            StageToolPolicy::PrometheusPlanning => {
                validate_prometheus_planning_tool_call(tool_name, arguments, exec_ctx)
            }
            _ => Ok(()),
        }
    }
}

fn validate_prometheus_planning_tool_call(
    tool_name: &str,
    arguments: &serde_json::Value,
    exec_ctx: &crate::ExecutionContext,
) -> Result<(), ToolExecError> {
    match tool_name.to_ascii_lowercase().as_str() {
        "write" | "edit" => {
            let raw_path = extract_tool_file_path(arguments).ok_or_else(|| {
                ToolExecError::InvalidArguments(format!(
                    "tool `{tool_name}` requires a file_path when used in Prometheus planning stages"
                ))
            })?;
            validate_prometheus_artifact_path(raw_path, exec_ctx)
        }
        _ => Ok(()),
    }
}

fn extract_tool_file_path(arguments: &serde_json::Value) -> Option<&str> {
    arguments
        .get("file_path")
        .or_else(|| arguments.get("filePath"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn validate_prometheus_artifact_path(
    raw_path: &str,
    exec_ctx: &crate::ExecutionContext,
) -> Result<(), ToolExecError> {
    let workdir = Path::new(&exec_ctx.workdir);
    let candidate = Path::new(raw_path);
    let resolved = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        workdir.join(candidate)
    };

    let normalized = normalize_path(&resolved);
    let normalized_workdir = normalize_path(workdir);

    if !normalized.starts_with(&normalized_workdir) {
        return Err(ToolExecError::PermissionDenied(format!(
            "Prometheus planning stages may only write artifacts inside the session workdir: {raw_path}"
        )));
    }

    let relative = normalized
        .strip_prefix(&normalized_workdir)
        .ok()
        .unwrap_or(&normalized);

    let mut components = relative.components();
    let under_sisyphus = matches!(
        components.next(),
        Some(Component::Normal(first)) if first == std::ffi::OsStr::new(".sisyphus")
    );
    if !under_sisyphus {
        return Err(ToolExecError::PermissionDenied(format!(
            "Prometheus planning stages may only write markdown artifacts under .sisyphus/: {raw_path}"
        )));
    }

    let is_markdown = normalized
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or(false);
    if !is_markdown {
        return Err(ToolExecError::PermissionDenied(format!(
            "Prometheus planning stages may only write markdown artifacts (*.md): {raw_path}"
        )));
    }

    Ok(())
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
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
    async fn prometheus_planning_allows_question_and_markdown_artifacts() {
        let executor = FilteredToolExecutor::new(
            Arc::new(TestToolExecutor),
            StageToolPolicy::PrometheusPlanning,
        );

        let ids = executor.list_ids().await;
        assert_eq!(
            ids,
            vec![
                "read".to_string(),
                "edit".to_string(),
                "write".to_string(),
                "grep".to_string(),
                "question".to_string(),
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

        executor
            .execute(
                "write",
                json!({"file_path": "/repo/.sisyphus/plans/plan.md", "content": "# Plan"}),
                &exec_ctx(),
            )
            .await
            .expect("markdown artifact write should be allowed");
    }

    #[tokio::test]
    async fn prometheus_planning_rejects_non_sisyphus_writes() {
        let executor = FilteredToolExecutor::new(
            Arc::new(TestToolExecutor),
            StageToolPolicy::PrometheusPlanning,
        );
        let err = executor
            .execute(
                "write",
                json!({"file_path": "/repo/src/main.rs", "content": "fn main() {}"}),
                &exec_ctx(),
            )
            .await
            .expect_err("code write should be blocked");
        assert!(err.to_string().contains(".sisyphus"));
    }

    #[tokio::test]
    async fn prometheus_planning_rejects_non_markdown_artifacts() {
        let executor = FilteredToolExecutor::new(
            Arc::new(TestToolExecutor),
            StageToolPolicy::PrometheusPlanning,
        );
        let err = executor
            .execute(
                "edit",
                json!({
                    "file_path": "/repo/.sisyphus/plans/plan.json",
                    "old_string": "{}",
                    "new_string": "[]"
                }),
                &exec_ctx(),
            )
            .await
            .expect_err("non-markdown artifact should be blocked");
        assert!(err.to_string().contains("*.md"));
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
