use crate::scheduler::{
    SchedulerAutonomousGateStageInput, SchedulerAutonomousVerificationStageInput,
    SchedulerExecutionOrchestrationStageInput,
};

use super::build_hephaestus_dynamic_prompt;

fn push_optional_section(sections: &mut Vec<String>, title: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        sections.push(format!("## {title}\n{value}"));
    }
}

pub fn compose_hephaestus_execution_orchestration_input(
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
- This is Hephaestus autonomous deep-worker execution, not a planning handoff and not a coordination-heavy multi-worker loop by default.
- Start acting in the same turn: explore, plan, decide, execute, verify.
- Prefer direct execution over discussion, and ask only as a last resort after repo exploration.
- Delegate only when that clearly improves quality, but remain responsible for the final verified result."
            .to_string(),
    );
    sections.push(
        "## Execution Priorities
- Extract the user's true intent before acting on the surface request.
- Search the repo and gather evidence before asking for missing context.
- Follow the full EXPLORE -> PLAN -> DECIDE -> EXECUTE -> VERIFY loop.
- Run concrete diagnostics, builds, tests, or artifact checks when applicable.
- Do not finish until the request is substantively complete, verified, and proven, or a real blocker is identified."
            .to_string(),
    );
    sections.push(build_hephaestus_dynamic_prompt(
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
    fn hephaestus_execution_input_carries_autonomous_loop_semantics() {
        let input = SchedulerExecutionOrchestrationStageInput {
            original_request: "fix the failing lsp diagnostics path",
            request_brief: "Autonomously diagnose and fix the diagnostics path",
            route_summary: Some("autonomous deep-worker execution"),
            planning_output: None,
            skill_tree_context: Some("inherits rust debugging context"),
            available_agents: &[AvailableAgentMeta {
                name: "explore".into(),
                description: "Exploration subagent for searching code.".into(),
                mode: "subagent".into(),
                cost: "CHEAP".into(),
            }],
            available_categories: &[AvailableCategoryMeta {
                name: "rust".into(),
                description: "Rust implementation and debugging tasks".into(),
            }],
            skill_list: &["debug".into()],
        };

        let composed = compose_hephaestus_execution_orchestration_input(input);
        assert!(composed.contains("Hephaestus autonomous deep-worker execution"));
        assert!(composed.contains("Start acting in the same turn"));
        assert!(composed.contains("EXPLORE -> PLAN -> DECIDE -> EXECUTE -> VERIFY"));
        assert!(composed.contains("substantively complete, verified, and proven"));
        assert!(composed.contains("Completion Guarantee (NON-NEGOTIABLE)"));
        assert!(composed.contains("`explore` agent — **CHEAP**"));
        assert!(composed.contains("**Active Skills**: debug"));
    }

    #[test]
    fn hephaestus_autonomous_verification_input_carries_loop_proof_contract() {
        let composed = compose_hephaestus_autonomous_verification_input(
            SchedulerAutonomousVerificationStageInput {
                original_request: "fix the failing lsp diagnostics path",
                request_brief: "Autonomously diagnose and fix the diagnostics path",
                current_plan: "request-analysis -> execution-orchestration",
                round: 1,
                execution_output: "fixed the path",
            },
        );
        assert!(composed.contains("## Stage\nautonomous-verification"));
        assert!(composed.contains("proof of EXPLORE -> PLAN -> DECIDE -> EXECUTE -> VERIFY"));
        assert!(composed.contains("changed artifacts"));
    }

    #[test]
    fn hephaestus_autonomous_gate_input_carries_verified_finish_contract() {
        let composed =
            compose_hephaestus_autonomous_gate_input(SchedulerAutonomousGateStageInput {
                original_request: "fix the failing lsp diagnostics path",
                request_brief: "Autonomously diagnose and fix the diagnostics path",
                current_plan: "request-analysis -> execution-orchestration",
                round: 1,
                execution_output: "fixed the path",
                verification_output: Some("targeted check passed"),
            });
        assert!(composed.contains("## Stage\nautonomous-finish-gate"));
        assert!(composed.contains("proved completion"));
        assert!(composed.contains("bounded retry"));
        assert!(composed.contains("**What Changed**"));
    }
}

pub fn compose_hephaestus_autonomous_gate_input(
    input: SchedulerAutonomousGateStageInput<'_>,
) -> String {
    let mut sections = Vec::new();
    sections.push(
        "## Stage
autonomous-finish-gate"
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
        "## Executor Output
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
        "## Finish Gate Contract
Judge whether the autonomous loop actually proved completion. Return JSON only. Use `done` only when the result is substantively complete and verification confirms it. Use `continue` only when one more bounded retry can close a concrete critical gap. Use `blocked` only for an actual external blocker. If `final_response` is present, format it as `## Delivery Summary`, `**What Changed**`, `**Verification**`, `**Risks or Follow-ups**`."
            .to_string(),
    );
    sections.join("\n\n")
}

pub fn compose_hephaestus_autonomous_verification_input(
    input: SchedulerAutonomousVerificationStageInput<'_>,
) -> String {
    let mut sections = Vec::new();
    sections.push(
        "## Stage
autonomous-verification"
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
        "## Executor Output
{}",
        input.execution_output
    ));
    sections.push(format!(
        "## Current Plan
{}",
        input.current_plan
    ));
    sections.push(
        "## Verification Charter
Audit the Hephaestus execution loop for proof of EXPLORE -> PLAN -> DECIDE -> EXECUTE -> VERIFY. Prefer concrete evidence of completion, changed artifacts, and verification over tone or confidence."
            .to_string(),
    );
    sections.join("\n\n")
}
