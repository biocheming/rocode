use crate::scheduler::{SchedulerDraftArtifactInput, SchedulerPlanningArtifactInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrometheusArtifactKind {
    Planning,
    Draft,
}

pub struct PrometheusDraftContext<'a> {
    pub original_input: &'a str,
    pub request_brief: &'a str,
    pub route_summary: Option<&'a str>,
    pub interview_output: Option<&'a str>,
    pub metis_review: Option<&'a str>,
    pub current_plan: Option<&'a str>,
    pub momus_review: Option<&'a str>,
    pub handoff_choice: Option<&'a str>,
    pub planning_artifact_path: Option<&'a str>,
    pub draft_artifact_path: Option<&'a str>,
}

pub struct PrometheusPlanningArtifactContext<'a> {
    pub request_brief: &'a str,
    pub route_summary: Option<&'a str>,
    pub interview_output: Option<&'a str>,
    pub metis_review: Option<&'a str>,
    pub planning_output: &'a str,
    pub planning_artifact_path: Option<&'a str>,
}

pub fn build_prometheus_artifact_relative_path(
    kind: PrometheusArtifactKind,
    session_id: &str,
) -> String {
    let (directory, prefix) = match kind {
        PrometheusArtifactKind::Planning => (".sisyphus/plans", "plan"),
        PrometheusArtifactKind::Draft => (".sisyphus/drafts", "draft"),
    };
    let session_slug = slugify_artifact_component(session_id, 32);
    format!("{directory}/{prefix}-{session_slug}.md")
}

#[cfg(test)]
pub fn build_planning_artifact_relative_path(session_id: &str) -> String {
    build_prometheus_artifact_relative_path(PrometheusArtifactKind::Planning, session_id)
}

#[cfg(test)]
pub fn build_draft_artifact_relative_path(session_id: &str) -> String {
    build_prometheus_artifact_relative_path(PrometheusArtifactKind::Draft, session_id)
}

pub fn compose_prometheus_planning_artifact(input: SchedulerPlanningArtifactInput<'_>) -> String {
    render_prometheus_plan_artifact(PrometheusPlanningArtifactContext {
        request_brief: input.request_brief,
        route_summary: input.route_summary,
        interview_output: input.interview_output,
        metis_review: input.metis_review,
        planning_output: input.planning_output,
        planning_artifact_path: input.planning_artifact_path,
    })
}

pub fn compose_prometheus_draft_artifact(input: SchedulerDraftArtifactInput<'_>) -> String {
    render_prometheus_draft(PrometheusDraftContext {
        original_input: input.original_request,
        request_brief: input.request_brief,
        route_summary: input.route_summary,
        interview_output: input.interview_output,
        metis_review: input.metis_review,
        current_plan: input.current_plan,
        momus_review: input.momus_review,
        handoff_choice: input.handoff_choice,
        planning_artifact_path: input.planning_artifact_path,
        draft_artifact_path: input.draft_artifact_path,
    })
}

pub fn render_prometheus_plan_artifact(context: PrometheusPlanningArtifactContext<'_>) -> String {
    let planning_output = context.planning_output.trim();
    if planning_output.is_empty() {
        return String::new();
    }
    if prometheus_plan_artifact_has_omo_shape(planning_output) {
        return planning_output.to_string();
    }

    let title = artifact_title(
        context.planning_artifact_path,
        planning_output,
        "prometheus-plan",
    );
    let deliverables = extract_markdown_list_items(planning_output, 3);
    let deliverables = if deliverables.is_empty() {
        vec![
            "Produce a single reviewed execution plan artifact.".to_string(),
            "Preserve planner-only handoff to `/start-work`.".to_string(),
        ]
    } else {
        deliverables
    };
    let interview_summary = context
        .interview_output
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(context.request_brief);
    let metis_items = context
        .metis_review
        .map(|text| extract_markdown_list_items(text, 6))
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| {
            vec!["No explicit Metis guardrails were captured in this pass.".to_string()]
        });

    let todos_body = if looks_like_task_breakdown(planning_output) {
        planning_output.to_string()
    } else {
        format!(
            "- [ ] Refine the generated planning body into concrete execution tasks.

### Generated Plan Body
{}",
            planning_output
        )
    };

    [
        format!("# {title}"),
        "## TL;DR

> **Quick Summary**: Prometheus generated a planner-only work plan for the request below.
>
> **Deliverables**:".to_string(),
        deliverables
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("
"),
        ">
> **Estimated Effort**: TBD
> **Parallel Execution**: TBD
> **Critical Path**: To be finalized by the task breakdown.".to_string(),
        "---".to_string(),
        format!("## Context

### Request Brief
{}

### Interview Summary
{}

### Metis Review
{}",
            context.request_brief,
            interview_summary,
            metis_items.iter().map(|item| format!("- {item}")).collect::<Vec<_>>().join("
")
        ),
        format!("## Work Objectives

### Core Objective
{}

### Concrete Deliverables
{}

### Definition of Done
- [ ] Plan saved as markdown under `.sisyphus/plans/*.md`
- [ ] Tasks include concrete acceptance criteria
- [ ] Tasks include Agent-Executed QA Scenarios

### Must Have
- Preserve Prometheus as planner-only
- Keep one consolidated work plan

### Must NOT Have (Guardrails)
- No claims that implementation is already complete
- No non-markdown file edits in this workflow
{}",
            context.request_brief,
            deliverables.iter().map(|item| format!("- {item}")).collect::<Vec<_>>().join("
"),
            context.route_summary.map(|summary| format!("- {summary}")).unwrap_or_default(),
        ),
        "## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.
> Acceptance criteria requiring manual user confirmation are forbidden.

### Test Decision
- **Infrastructure exists**: Determine from repo exploration
- **Automated tests**: TDD / tests-after / none, but still include agent QA
- **Framework**: Use the repo's actual test harness

### QA Policy
Every task must include agent-executed QA scenarios and evidence paths under `.sisyphus/evidence/`.".to_string(),
        "## Execution Strategy

### Parallel Execution Waves
- Group independent tasks into parallel waves when the scope supports it
- Keep dependencies explicit

### Dependency Matrix
- Derive concrete dependencies from the TODO section below

### Agent Dispatch Summary
- Choose agent profiles per task domain and evidence needs".to_string(),
        format!("## TODOs

{todos_body}"),
        "## Final Verification Wave
- Plan compliance audit
- Scope fidelity check
- Execution readiness review".to_string(),
        "## Commit Strategy
- Decide during execution; do not fabricate commit details during planning.".to_string(),
        "## Success Criteria
- The plan is concrete, bounded, and execution-ready
- Remaining decisions are explicit
- `/start-work` can begin from this artifact without re-interviewing the user".to_string(),
    ]
    .join("

")
}

pub fn render_prometheus_draft(context: PrometheusDraftContext<'_>) -> String {
    let title = artifact_title(
        context
            .planning_artifact_path
            .or(context.draft_artifact_path),
        context.original_input,
        "prometheus-session",
    );

    let mut requirements = vec![format!(
        "Original request: {}",
        context.original_input.trim()
    )];
    if !context.request_brief.trim().is_empty() {
        requirements.push(format!("Request brief: {}", context.request_brief.trim()));
    }
    requirements.extend(extract_markdown_list_items(
        context.interview_output.unwrap_or_default(),
        4,
    ));
    dedup_preserve(&mut requirements);

    let mut technical_decisions = Vec::new();
    if let Some(route_summary) = context
        .route_summary
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        technical_decisions.push(route_summary.to_string());
    }
    if context
        .current_plan
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
    {
        technical_decisions.push("A planning snapshot exists and should remain the single working plan candidate until handoff.".to_string());
    }
    if let Some(choice) = context
        .handoff_choice
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        technical_decisions.push(format!("Current handoff preference: {choice}"));
    }
    if technical_decisions.is_empty() {
        technical_decisions.push("Prometheus remains in planner-only mode.".to_string());
    }

    let mut research_findings = context
        .metis_review
        .map(|text| extract_markdown_list_items(text, 6))
        .unwrap_or_default();
    if research_findings.is_empty() {
        research_findings.push("No external research findings recorded yet.".to_string());
    }

    let mut open_questions = collect_decision_needed_lines(&[
        context.interview_output,
        context.current_plan,
        context.momus_review,
    ]);
    if open_questions.is_empty() {
        open_questions.push("None recorded yet.".to_string());
    }

    let include_scope = context.request_brief.trim();
    let mut sections = Vec::new();
    sections.push(format!("# Draft: {title}"));
    sections.push(format_markdown_list_section(
        "Requirements (confirmed)",
        &requirements,
    ));
    sections.push(format_markdown_list_section(
        "Technical Decisions",
        &technical_decisions,
    ));
    sections.push(format_markdown_list_section(
        "Research Findings",
        &research_findings,
    ));
    sections.push(format_markdown_list_section(
        "Open Questions",
        &open_questions,
    ));
    sections.push(format!(
        "## Scope Boundaries
- INCLUDE: {}
- EXCLUDE: Code execution or implementation claims inside the Prometheus workflow.",
        if include_scope.is_empty() {
            "Scope still being clarified."
        } else {
            include_scope
        }
    ));
    if let Some(path) = context
        .planning_artifact_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!(
            "## Planning Artifact
- Target plan path: `{path}`"
        ));
    }
    sections.join(
        "

",
    )
}

pub fn append_artifact_note(content: String, artifact_path: Option<&str>) -> String {
    let Some(artifact_path) = artifact_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return content;
    };

    if content.contains(artifact_path) {
        return content;
    }

    let trimmed = content.trim_end();
    if trimmed.is_empty() {
        format!("Plan saved to: `{artifact_path}`")
    } else {
        format!(
            "{trimmed}

Plan saved to: `{artifact_path}`"
        )
    }
}

pub fn slugify_artifact_component(input: &str, max_len: usize) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for ch in input.chars() {
        let normalized = ch.to_ascii_lowercase();
        if normalized.is_ascii_alphanumeric() {
            slug.push(normalized);
            last_was_dash = false;
        } else if !slug.is_empty() && !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }

        if slug.len() >= max_len {
            break;
        }
    }

    while slug.ends_with('-') {
        slug.pop();
    }

    if slug.is_empty() {
        "plan".to_string()
    } else {
        slug
    }
}

fn artifact_title(path_hint: Option<&str>, fallback_source: &str, default_title: &str) -> String {
    path_hint
        .and_then(|path| std::path::Path::new(path).file_stem())
        .and_then(|value| value.to_str())
        .or_else(|| first_markdown_heading(fallback_source))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .trim_start_matches("plan-")
                .trim_start_matches("draft-")
                .to_string()
        })
        .unwrap_or_else(|| default_title.to_string())
}

fn first_markdown_heading(text: &str) -> Option<&str> {
    text.lines()
        .map(str::trim)
        .find(|line| line.starts_with("# "))
        .map(|line| line.trim_start_matches("# ").trim())
}

fn prometheus_plan_artifact_has_omo_shape(text: &str) -> bool {
    [
        "## TL;DR",
        "## Context",
        "## Work Objectives",
        "## Verification Strategy",
        "## Execution Strategy",
        "## TODOs",
        "## Success Criteria",
    ]
    .iter()
    .all(|heading| text.contains(heading))
}

fn looks_like_task_breakdown(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        line.starts_with("- [ ]")
            || line.starts_with("- ")
            || line
                .chars()
                .next()
                .map(|ch| ch.is_ascii_digit())
                .unwrap_or(false)
    })
}

fn extract_markdown_list_items(text: &str, max: usize) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| {
            line.starts_with("- ")
                || line.starts_with("* ")
                || line
                    .chars()
                    .next()
                    .map(|ch| ch.is_ascii_digit())
                    .unwrap_or(false)
        })
        .map(|line| {
            line.trim_start_matches(|ch: char| {
                ch == '-'
                    || ch == '*'
                    || ch.is_ascii_digit()
                    || ch == '.'
                    || ch == ' '
                    || ch == '['
                    || ch == ']'
            })
            .trim()
            .to_string()
        })
        .filter(|line| !line.is_empty())
        .take(max)
        .collect()
}

fn collect_decision_needed_lines(sources: &[Option<&str>]) -> Vec<String> {
    let mut items = Vec::new();
    for source in sources.iter().flatten() {
        for line in source.lines().map(str::trim) {
            let lower = line.to_ascii_lowercase();
            if lower.contains("[decision needed:") || lower.contains("decision needed") {
                items.push(line.to_string());
            }
        }
    }
    dedup_preserve(&mut items);
    items
}

fn format_markdown_list_section(title: &str, items: &[String]) -> String {
    let body = if items.is_empty() {
        "- None.".to_string()
    } else {
        items
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    };
    format!(
        "## {title}
{body}"
    )
}

fn dedup_preserve(items: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    items.retain(|item| seen.insert(item.clone()));
}
