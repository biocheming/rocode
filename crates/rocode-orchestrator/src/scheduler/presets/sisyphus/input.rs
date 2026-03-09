use crate::scheduler::SchedulerExecutionOrchestrationStageInput;

use super::build_sisyphus_dynamic_prompt;

fn push_optional_section(sections: &mut Vec<String>, title: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        sections.push(format!("## {title}\n{value}"));
    }
}

pub fn compose_sisyphus_execution_orchestration_input(
    input: SchedulerExecutionOrchestrationStageInput<'_>,
) -> String {
    let mut sections = Vec::new();
    sections.push(
        "## Stage
execution-orchestration"
            .to_string(),
    );
    sections.push(format!(
        "## Original Request
{}",
        input.original_request
    ));
    sections.push(format!(
        "## Request Brief
{}",
        input.request_brief
    ));
    push_optional_section(&mut sections, "Route Summary", input.route_summary);
    push_optional_section(&mut sections, "Planning Output", input.planning_output);
    push_optional_section(
        &mut sections,
        "Skill Tree Context",
        input.skill_tree_context,
    );
    sections.push(
        "## Execution Frame
- This is Sisyphus single-loop execution orchestration, not an interview-first planning workflow.
- Re-state the detected intent class, choose the execution path, and then act.
- Default bias: delegate; work directly only when the task is genuinely super simple.
- The value here comes from action plus verification, not from a reviewed planning handoff."
            .to_string(),
    );
    sections.push(
        "## Execution Priorities
- Assess codebase shape before following local patterns blindly.
- Run explore/librarian research in parallel before committing on non-trivial repo questions.
- Match delegation to the best specialist, category, and loaded skills.
- Keep explicit task tracking for bounded delegated work.
- Finish only with evidence-backed verification and concrete outcomes."
            .to_string(),
    );
    sections.push(build_sisyphus_dynamic_prompt(
        input.available_agents,
        input.available_categories,
        input.skill_list,
    ));
    sections.join("\n\n")
}
