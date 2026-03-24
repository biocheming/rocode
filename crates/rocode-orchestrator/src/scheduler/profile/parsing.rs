use super::super::{SchedulerExecutionGateDecision, SchedulerExecutionGateStatus};
use serde_json::Value;

pub fn parse_execution_gate_decision(output: &str) -> Option<SchedulerExecutionGateDecision> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }

    for candidate in profile_json_candidates(trimmed) {
        if let Some(decision) = parse_execution_gate_candidate(&candidate) {
            return Some(decision);
        }
    }

    None
}

fn parse_execution_gate_candidate(candidate: &str) -> Option<SchedulerExecutionGateDecision> {
    if let Ok(decision) = serde_json::from_str::<SchedulerExecutionGateDecision>(candidate) {
        return Some(normalize_execution_gate_decision(decision));
    }

    let value = serde_json::from_str::<Value>(candidate).ok()?;
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .or_else(|| value.get("gate_decision").and_then(Value::as_str))
        .and_then(parse_execution_gate_status_token)?;

    let summary = first_non_empty_string(&[
        value.get("summary").and_then(Value::as_str),
        value.get("reasoning").and_then(Value::as_str),
        value.get("execution_fidelity").and_then(Value::as_str),
    ])
    .unwrap_or_default()
    .to_string();

    let next_input = first_non_empty_string(&[
        value.get("next_input").and_then(Value::as_str),
        joined_string_array(value.get("next_actions")).as_deref(),
    ])
    .map(str::to_string);

    let final_response = first_non_empty_string(&[
        value.get("final_response").and_then(Value::as_str),
        build_legacy_gate_details_markdown(&value).as_deref(),
    ])
    .map(str::to_string);

    Some(normalize_execution_gate_decision(
        SchedulerExecutionGateDecision {
            status,
            summary,
            next_input,
            final_response,
        },
    ))
}

fn parse_execution_gate_status_token(token: &str) -> Option<SchedulerExecutionGateStatus> {
    match token.trim().to_ascii_lowercase().as_str() {
        "done" | "complete" | "completed" | "finish" | "finished" => {
            Some(SchedulerExecutionGateStatus::Done)
        }
        "continue" | "retry" | "again" => Some(SchedulerExecutionGateStatus::Continue),
        "blocked" | "block" | "stop" => Some(SchedulerExecutionGateStatus::Blocked),
        _ => None,
    }
}

fn first_non_empty_string<'a>(candidates: &[Option<&'a str>]) -> Option<&'a str> {
    candidates
        .iter()
        .flatten()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
}

fn joined_string_array(value: Option<&Value>) -> Option<String> {
    let items = value?.as_array()?;
    let lines = items
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("- {value}"))
        .collect::<Vec<_>>();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn build_legacy_gate_details_markdown(value: &Value) -> Option<String> {
    let mut sections = Vec::new();

    if let Some(summary) = value
        .get("verification_summary")
        .and_then(Value::as_object)
        .filter(|summary| !summary.is_empty())
    {
        let mut lines = Vec::new();
        for (key, raw) in summary {
            let rendered = match raw {
                Value::String(text) => text.clone(),
                _ => raw.to_string(),
            };
            lines.push(format!("- {}: {}", key.replace('_', " "), rendered));
        }
        if !lines.is_empty() {
            sections.push(format!("### Verification Summary\n{}", lines.join("\n")));
        }
    }

    if let Some(task_status) = value
        .get("task_status")
        .and_then(Value::as_object)
        .filter(|status| !status.is_empty())
    {
        let mut lines = Vec::new();
        for (key, raw) in task_status {
            let rendered = raw.as_str().unwrap_or_default().trim();
            if !rendered.is_empty() {
                lines.push(format!("- {}: {}", key.replace('_', " "), rendered));
            }
        }
        if !lines.is_empty() {
            sections.push(format!("### Task Status\n{}", lines.join("\n")));
        }
    }

    if let Some(execution_fidelity) = value
        .get("execution_fidelity")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        sections.push(format!("### Execution Fidelity\n{}", execution_fidelity));
    }

    if let Some(minor_issues) = joined_string_array(value.get("minor_issues")) {
        sections.push(format!("### Minor Issues\n{}", minor_issues));
    }

    if let Some(next_actions) = joined_string_array(value.get("next_actions")) {
        sections.push(format!("### Next Actions\n{}", next_actions));
    }

    (!sections.is_empty()).then(|| sections.join("\n\n"))
}

pub fn normalize_execution_gate_decision(
    mut decision: SchedulerExecutionGateDecision,
) -> SchedulerExecutionGateDecision {
    decision.summary = decision.summary.trim().to_string();
    decision.next_input = decision
        .next_input
        .take()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    decision.final_response = decision
        .final_response
        .take()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if matches!(decision.status, SchedulerExecutionGateStatus::Continue)
        && decision.next_input.is_none()
    {
        let fallback = if decision.summary.is_empty() {
            "continue the bounded retry on the unresolved gap and collect concrete verification evidence"
                .to_string()
        } else {
            decision.summary.clone()
        };
        decision.next_input = Some(fallback);
    }

    decision
}

fn profile_json_candidates(output: &str) -> Vec<String> {
    let mut candidates = Vec::new();

    for marker in ["```json", "```JSON", "```"] {
        let mut remaining = output;
        while let Some(start) = remaining.find(marker) {
            let after = &remaining[start + marker.len()..];
            if let Some(end) = after.find("```") {
                let candidate = after[..end].trim();
                if !candidate.is_empty() {
                    candidates.push(candidate.to_string());
                }
                remaining = &after[end + 3..];
            } else {
                break;
            }
        }
    }

    if let Some((start, end)) = profile_find_balanced_json_object(output) {
        let candidate = output[start..end].trim();
        if !candidate.is_empty() {
            candidates.push(candidate.to_string());
        }
    }

    if candidates.is_empty() {
        candidates.push(output.trim().to_string());
    }

    candidates
}

fn profile_find_balanced_json_object(input: &str) -> Option<(usize, usize)> {
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in input.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(idx);
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    return start.map(|s| (s, idx + ch.len_utf8()));
                }
            }
            _ => {}
        }
    }

    None
}
