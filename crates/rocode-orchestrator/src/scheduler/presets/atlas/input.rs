use crate::scheduler::{
    SchedulerCoordinationGateStageInput, SchedulerCoordinationVerificationStageInput,
    SchedulerExecutionOrchestrationStageInput, SchedulerSynthesisStageInput,
};

use super::build_atlas_dynamic_prompt;

fn push_optional_section(sections: &mut Vec<String>, title: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        sections.push(format!("## {title}\n{value}"));
    }
}

pub fn compose_atlas_execution_orchestration_input(
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
- This is Atlas coordination-loop orchestration, not a planner-only handoff and not a single autonomous executor.
- Read the current work plan or task list, decompose it into bounded work items, and coordinate the next worker round.
- Delegate one bounded task per worker unless a parallel wave is clearly independent and safe.
- Atlas never writes the implementation itself; it coordinates, verifies, and tracks task completion."
            .to_string(),
    );
    sections.push(
        "## Execution Priorities
- Build a parallelization map before dispatching workers.
- Keep explicit task boundaries and terminal status evidence for every item.
- Treat worker completion claims as untrusted until concrete artifacts are checked.
- Use verification as the QA gate after each delegation round.
- Reach synthesis only when every required task is complete with evidence or a concrete blocker is confirmed."
            .to_string(),
    );
    sections.push(build_atlas_dynamic_prompt(
        input.available_agents,
        input.available_categories,
        input.skill_list,
    ));
    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::{AvailableAgentMeta, AvailableCategoryMeta};

    #[test]
    fn atlas_execution_input_carries_coordination_loop_semantics() {
        let input = SchedulerExecutionOrchestrationStageInput {
            original_request: "ship the migration cleanup plan",
            request_brief: "Coordinate the remaining migration tasks",
            route_summary: Some("coordination-heavy task list"),
            planning_output: Some("1. update schema\n2. verify migration"),
            skill_tree_context: Some("inherits rust + db context"),
            available_agents: &[AvailableAgentMeta {
                name: "oracle".into(),
                description: "High-IQ reasoning specialist.".into(),
                mode: "subagent".into(),
                cost: "EXPENSIVE".into(),
            }],
            available_categories: &[AvailableCategoryMeta {
                name: "rust".into(),
                description: "Rust implementation and debugging tasks".into(),
            }],
            skill_list: &["review-pr".into()],
        };

        let composed = compose_atlas_execution_orchestration_input(input);
        assert!(composed.contains("Atlas coordination-loop orchestration"));
        assert!(composed.contains("decompose it into bounded work items"));
        assert!(composed.contains("parallelization map"));
        assert!(composed.contains("worker completion claims as untrusted"));
        assert!(composed.contains("6-Section Prompt Structure"));
        assert!(composed.contains("`rust` — Rust implementation and debugging tasks"));
        assert!(composed.contains("Oracle_Usage"));
    }

    #[test]
    fn atlas_synthesis_input_carries_structured_delivery_contract() {
        let composed = compose_atlas_synthesis_input(SchedulerSynthesisStageInput {
            original_request: "ship the migration cleanup plan",
            request_brief: "Coordinate remaining migration tasks",
            current_plan: "request-analysis -> execution-orchestration -> synthesis",
            route_decision_json: Some("{\"preset\":\"atlas\"}"),
            planning_output: Some("- task A\n- task B"),
            delegation_output: Some("worker claims task A done"),
            review_output: Some("task A verified"),
            saved_planning_artifact: Some("artifact.md"),
        });
        assert!(composed.contains("## Stage\nsynthesis"));
        assert!(composed.contains("## Delivery Summary"));
        assert!(composed.contains("**Task Status**"));
        assert!(composed.contains("prefer reviewed verification over worker claims"));
    }

    #[test]
    fn atlas_coordination_verification_input_carries_task_level_verification_contract() {
        let composed = compose_atlas_coordination_verification_input(
            SchedulerCoordinationVerificationStageInput {
                original_request: "ship the migration cleanup plan",
                request_brief: "Coordinate remaining migration tasks",
                round: 2,
                execution_output: "worker round output",
                planning_output: Some("- task A\n- task B"),
                skill_tree_context: Some("inherits rust + db context"),
            },
        );
        assert!(composed.contains("## Stage\ncoordination-verification"));
        assert!(composed.contains("Audit each Atlas task item individually"));
        assert!(composed.contains("task boundary"));
    }

    #[test]
    fn atlas_coordination_gate_input_carries_task_ledger_contract() {
        let composed = compose_atlas_coordination_gate_input(SchedulerCoordinationGateStageInput {
            original_request: "ship the migration cleanup plan",
            request_brief: "Coordinate remaining migration tasks",
            current_plan: "request-analysis -> execution-orchestration -> synthesis",
            round: 2,
            execution_output: "worker round output",
            verification_output: Some("task A verified, task B weak"),
        });
        assert!(composed.contains("## Stage\ncoordination-gate"));
        assert!(composed.contains("Judge completion by task boundary"));
        assert!(composed.contains("weakly-verified task items"));
        assert!(composed.contains("**Task Status**"));
    }
}

pub fn compose_atlas_synthesis_input(input: SchedulerSynthesisStageInput<'_>) -> String {
    let mut sections = Vec::new();
    sections.push(
        "## Stage
synthesis"
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
    sections.push(format!(
        "## Current Plan
{}",
        input.current_plan
    ));
    push_optional_section(&mut sections, "Route Decision", input.route_decision_json);
    push_optional_section(&mut sections, "Planning Output", input.planning_output);
    push_optional_section(&mut sections, "Delegation Output", input.delegation_output);
    push_optional_section(&mut sections, "Review Output", input.review_output);
    push_optional_section(
        &mut sections,
        "Saved Planning Artifact",
        input.saved_planning_artifact,
    );
    sections.push(
        "## Synthesis Charter
Return the final Atlas delivery in this exact top-level order: `## Delivery Summary` -> `**Task Status**` -> `**Verification**` -> `**Blockers or Risks**` -> `**Next Actions**`. Report by task boundary, prefer reviewed verification over worker claims, and keep explicit evidence in the final answer."
            .to_string(),
    );
    sections.join("\n\n")
}

pub fn compose_atlas_coordination_gate_input(
    input: SchedulerCoordinationGateStageInput<'_>,
) -> String {
    let mut sections = Vec::new();
    sections.push(
        "## Stage
coordination-gate"
            .to_string(),
    );
    sections.push(format!(
        "## Round
{}",
        input.round
    ));
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
    sections.push(format!(
        "## Execution Output
{}",
        input.execution_output
    ));
    push_optional_section(
        &mut sections,
        "Verification Output",
        input.verification_output,
    );
    sections.push(format!(
        "## Current Plan
{}",
        input.current_plan
    ));
    sections.push(
        "## Coordination Decision Contract
Judge completion by task boundary. Return JSON only. Use `done` only when every required task item is complete with evidence. Use `continue` only when you can name the exact unfinished or weakly-verified task items for the next worker round. Use `blocked` only for a concrete blocker. If `final_response` is present, format it as `## Delivery Summary`, `**Task Status**`, `**Verification**`, `**Blockers or Risks**`, `**Next Actions**`."
            .to_string(),
    );
    sections.join("\n\n")
}

pub fn compose_atlas_coordination_verification_input(
    input: SchedulerCoordinationVerificationStageInput<'_>,
) -> String {
    let mut sections = Vec::new();
    sections.push(
        "## Stage
coordination-verification"
            .to_string(),
    );
    sections.push(format!(
        "## Round
{}",
        input.round
    ));
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
    sections.push(format!(
        "## Execution Output
{}",
        input.execution_output
    ));
    push_optional_section(&mut sections, "Planning Output", input.planning_output);
    push_optional_section(
        &mut sections,
        "Skill Tree Context",
        input.skill_tree_context,
    );
    sections.push(
        "## Verification Charter
Audit each Atlas task item individually against execution evidence. Mark items complete only when the worker output proves the required task boundary. Surface incomplete, conflicting, and blocked items explicitly, and do not rewrite implementation here."
            .to_string(),
    );
    sections.join("\n\n")
}
