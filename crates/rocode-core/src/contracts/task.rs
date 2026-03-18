/// Text envelope contract used by delegated task tools.
///
/// This is intentionally plain-text so it can be displayed directly in
/// terminals while still being machine-parseable by UIs.

/// Task/tool metadata keys shared across task/task_flow/UI layers.
pub mod metadata_keys {
    pub const TASK_STATUS: &str = "taskStatus";
    pub const HAS_TEXT_OUTPUT: &str = "hasTextOutput";
    pub const MODEL: &str = "model";
    pub const LOADED_SKILL_COUNT: &str = "loadedSkillCount";

    pub const MODEL_PROVIDER_ID_CAMEL: &str = "providerID";
    pub const MODEL_PROVIDER_ID_SNAKE: &str = "provider_id";
    pub const MODEL_ID_CAMEL: &str = "modelID";
    pub const MODEL_ID_SNAKE: &str = "model_id";
}

pub const TASK_ID_PREFIX: &str = "task_id:";
pub const TASK_STATUS_PREFIX: &str = "task_status:";

pub const TASK_RESULT_TAG_OPEN: &str = "<task_result>";
pub const TASK_RESULT_TAG_CLOSE: &str = "</task_result>";

pub const TASK_ID_RESUME_SUFFIX: &str = "(for resuming to continue this task if needed)";

pub const TASK_NO_TEXT_OUTPUT_MESSAGE: &str =
    "Task completed successfully. No textual output was returned by subagent.";

/// Task result status label used inside `task_status: ...`.
pub const TASK_STATUS_COMPLETED: &str = "completed";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskResultEnvelope {
    pub task_id: Option<String>,
    pub task_status: Option<String>,
    pub body: String,
}

impl TaskResultEnvelope {
    pub fn parse(result_text: &str) -> Self {
        let mut envelope = TaskResultEnvelope::default();
        for line in result_text.lines() {
            let trimmed = line.trim();
            if let Some(raw) = trimmed.strip_prefix(TASK_ID_PREFIX) {
                let id = raw
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !id.is_empty() {
                    envelope.task_id = Some(id);
                }
                continue;
            }
            if let Some(raw) = trimmed.strip_prefix(TASK_STATUS_PREFIX) {
                let status = raw.trim().to_string();
                if !status.is_empty() {
                    envelope.task_status = Some(status);
                }
            }
        }

        if let (Some(start), Some(end)) = (
            result_text.find(TASK_RESULT_TAG_OPEN),
            result_text.find(TASK_RESULT_TAG_CLOSE),
        ) {
            if end > start {
                let body = &result_text[start + TASK_RESULT_TAG_OPEN.len()..end];
                envelope.body = body.trim().to_string();
                return envelope;
            }
        }

        envelope.body = result_text.trim().to_string();
        envelope
    }

    pub fn format(task_id: &str, task_status: &str, body: &str) -> String {
        format!(
            "{TASK_ID_PREFIX} {task_id} {TASK_ID_RESUME_SUFFIX}\n{TASK_STATUS_PREFIX} {task_status}\n\n{TASK_RESULT_TAG_OPEN}\n{body}\n{TASK_RESULT_TAG_CLOSE}",
        )
    }

    pub fn format_completed(task_id: &str, body: &str) -> String {
        Self::format(task_id, TASK_STATUS_COMPLETED, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_task_envelope_with_tags() {
        let text = TaskResultEnvelope::format_completed("session-1", "hello\nworld");
        let parsed = TaskResultEnvelope::parse(&text);
        assert_eq!(parsed.task_id.as_deref(), Some("session-1"));
        assert_eq!(parsed.task_status.as_deref(), Some(TASK_STATUS_COMPLETED));
        assert_eq!(parsed.body, "hello\nworld");
    }

    #[test]
    fn parses_task_envelope_without_tags() {
        let text = format!("{TASK_ID_PREFIX} s1\n\nhello");
        let parsed = TaskResultEnvelope::parse(&text);
        assert_eq!(parsed.task_id.as_deref(), Some("s1"));
        assert_eq!(parsed.body, text);
    }
}
