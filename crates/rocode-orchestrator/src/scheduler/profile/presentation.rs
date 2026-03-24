use super::super::prompt_support::build_capabilities_summary;
use super::super::{SchedulerLoopBudget, SchedulerSessionProjection};
use super::{SchedulerProfilePlan, SchedulerStageKind};

pub(super) fn markdown_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("- {value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(in crate::scheduler) fn skill_tree_context(plan: &SchedulerProfilePlan) -> Option<&str> {
    plan.skill_tree
        .as_ref()
        .map(|tree| tree.context_markdown.trim())
        .filter(|context| !context.is_empty())
}

pub(in crate::scheduler) fn render_plan_snapshot(plan: &SchedulerProfilePlan) -> String {
    let mut lines = Vec::new();
    if let Some(profile_name) = &plan.profile_name {
        lines.push(format!("profile: {profile_name}"));
    }
    if let Some(orchestrator) = &plan.orchestrator {
        lines.push(format!("orchestrator: {orchestrator}"));
    }
    if let Some(description) = plan
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        lines.push(format!("description: {description}"));
    }
    lines.push(format!("stages: {}", render_stage_sequence(&plan.stages)));
    if !plan.skill_list.is_empty() {
        lines.push(format!("skills: {}", plan.skill_list.join(", ")));
    }
    if let Some(agent_tree) = &plan.agent_tree {
        lines.push(format!("root-agent: {}", agent_tree.agent.name));
    }
    if plan.skill_graph.is_some() {
        lines.push("review-graph: enabled".to_string());
    }
    lines.join("\n")
}

pub(super) fn render_stage_sequence(stages: &[SchedulerStageKind]) -> String {
    stages
        .iter()
        .map(|stage| match stage {
            SchedulerStageKind::RequestAnalysis => "request-analysis",
            SchedulerStageKind::Route => "route",
            SchedulerStageKind::Interview => "interview",
            SchedulerStageKind::Plan => "plan",
            SchedulerStageKind::Delegation => "delegation",
            SchedulerStageKind::Review => "review",
            SchedulerStageKind::ExecutionOrchestration => "execution-orchestration",
            SchedulerStageKind::Synthesis => "synthesis",
            SchedulerStageKind::Handoff => "handoff",
        })
        .collect::<Vec<_>>()
        .join(" -> ")
}

pub(in crate::scheduler) fn profile_prompt_suffix(
    plan: &SchedulerProfilePlan,
    stage: Option<SchedulerStageKind>,
) -> String {
    let mut sections = Vec::new();

    if let Some(profile_name) = plan
        .profile_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Profile: {profile_name}"));
    }

    if let Some(orchestrator) = plan
        .orchestrator
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Orchestrator: {orchestrator}"));
    }

    if let Some(description) = plan
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("Description: {description}"));
    }

    let stage_caps = stage.and_then(|stage| plan.stage_capabilities_override(stage));
    let effective_skill_list = stage_caps
        .as_ref()
        .map(|caps| caps.skill_list.as_slice())
        .unwrap_or(&plan.skill_list);
    let effective_agents = stage_caps
        .as_ref()
        .map(|caps| caps.agents.as_slice())
        .unwrap_or(&[]);

    if !effective_skill_list.is_empty() {
        sections.push(format!(
            "Active Skills:\n{}",
            markdown_list(effective_skill_list)
        ));
    }

    if !effective_agents.is_empty() && stage_caps.is_some() {
        sections.push(format!(
            "### Available Capabilities\n\n**Agents:** {}",
            effective_agents.join(", ")
        ));
    } else {
        let capabilities = build_capabilities_summary(
            &plan.available_agents,
            &plan.available_categories,
            effective_skill_list,
        );
        if !capabilities.is_empty() {
            sections.push(capabilities);
        }
    }

    if let Some(context) = skill_tree_context(plan) {
        sections.push(format!("Skill Tree Context:\n{context}"));
    }

    if sections.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n## Scheduler Profile Context\n{}",
            sections.join("\n\n")
        )
    }
}

pub(super) fn parse_loop_budget(s: &str) -> SchedulerLoopBudget {
    if let Some(rest) = s.strip_prefix("step-limit:") {
        if let Ok(n) = rest.trim().parse::<u32>() {
            return SchedulerLoopBudget::StepLimit(n);
        }
    }
    SchedulerLoopBudget::Unbounded
}

pub(super) fn parse_session_projection(s: &str) -> SchedulerSessionProjection {
    match s {
        "hidden" => SchedulerSessionProjection::Hidden,
        _ => SchedulerSessionProjection::Transcript,
    }
}
