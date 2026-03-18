use super::*;
use rocode_core::contracts::tools::BuiltinToolName;

pub fn sisyphus_junior() -> AgentInfo {
    let tools: Vec<BuiltinToolName> = PRIMARY_BUILTIN_TOOLS
        .iter()
        .copied()
        .filter(|tool| !matches!(tool, BuiltinToolName::Task | BuiltinToolName::TaskFlow))
        .collect();
    let mut agent = base_allowlist_agent("sisyphus-junior", AgentMode::Subagent, &tools);
    agent.description = Some("Category-dispatched executor for domain-specific tasks.".to_string());
    agent.max_steps = Some(50);
    agent
}
