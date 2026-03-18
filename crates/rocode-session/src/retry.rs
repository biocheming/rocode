use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

pub const RETRY_INITIAL_DELAY: u64 = 2000;
pub const RETRY_BACKOFF_FACTOR: u64 = 2;
pub const RETRY_MAX_DELAY_NO_HEADERS: u64 = 30_000;
pub const RETRY_MAX_DELAY: u64 = 2_147_483_647;

#[derive(Debug, thiserror::Error)]
#[error("Sleep cancelled")]
pub struct SleepCancelled;

pub async fn sleep_with_cancel(ms: u64, cancel: CancellationToken) -> Result<(), SleepCancelled> {
    let duration = Duration::from_millis(ms.min(RETRY_MAX_DELAY));

    tokio::select! {
        _ = sleep(duration) => Ok(()),
        _ = cancel.cancelled() => Err(SleepCancelled),
    }
}

pub fn delay(attempt: u32, error: Option<&ApiErrorInfo>) -> u64 {
    if let Some(err) = error {
        if let Some(ref headers) = err.response_headers {
            if let Some(retry_after_ms) = headers.get("retry-after-ms") {
                if let Ok(parsed_ms) = retry_after_ms.parse::<f64>() {
                    return parsed_ms as u64;
                }
            }

            if let Some(retry_after) = headers.get("retry-after") {
                if let Ok(parsed_seconds) = retry_after.parse::<f64>() {
                    return (parsed_seconds * 1000.0).ceil() as u64;
                }

                if let Ok(parsed_date) = chrono::DateTime::parse_from_rfc2822(retry_after) {
                    let now = chrono::Utc::now();
                    let diff = parsed_date.with_timezone(&chrono::Utc) - now;
                    if diff.num_milliseconds() > 0 {
                        return diff.num_milliseconds() as u64;
                    }
                }
            }

            return RETRY_INITIAL_DELAY * RETRY_BACKOFF_FACTOR.pow(attempt - 1);
        }
    }

    let calculated = RETRY_INITIAL_DELAY * RETRY_BACKOFF_FACTOR.pow(attempt - 1);
    calculated.min(RETRY_MAX_DELAY_NO_HEADERS)
}

#[derive(Debug, Clone)]
pub struct ApiErrorInfo {
    pub message: String,
    pub is_retryable: bool,
    pub response_headers: Option<std::collections::HashMap<String, String>>,
    pub response_body: Option<String>,
}

fn deserialize_opt_string_lossy<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(value)) => Some(value),
        _ => None,
    })
}

pub fn retryable(error: &crate::MessageError) -> Option<String> {
    match error {
        crate::MessageError::ContextOverflowError { .. } => None,
        crate::MessageError::ApiError {
            message,
            is_retryable,
            response_body,
            ..
        } => {
            if !is_retryable {
                return None;
            }

            if let Some(body) = response_body {
                if body.contains("FreeUsageLimitError") {
                    return Some(
                        "Free usage exceeded, add credits https://opencode.ai/zen".to_string(),
                    );
                }
            }

            if message.contains("Overloaded") {
                Some("Provider is overloaded".to_string())
            } else {
                Some(message.clone())
            }
        }
        crate::MessageError::Unknown { message } => {
            #[derive(Debug, Deserialize, Default)]
            struct UnknownErrorDetailWire {
                #[serde(
                    default,
                    rename = "type",
                    deserialize_with = "deserialize_opt_string_lossy"
                )]
                kind: Option<String>,
                #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
                code: Option<String>,
            }

            #[derive(Debug, Deserialize, Default)]
            struct UnknownErrorWire {
                #[serde(default, deserialize_with = "deserialize_opt_string_lossy")]
                code: Option<String>,
                #[serde(
                    default,
                    rename = "type",
                    deserialize_with = "deserialize_opt_string_lossy"
                )]
                kind: Option<String>,
                #[serde(default)]
                error: Option<UnknownErrorDetailWire>,
            }

            if let Ok(parsed) = serde_json::from_str::<UnknownErrorWire>(message) {
                let code = parsed.code.as_deref().unwrap_or("");

                if parsed.kind.as_deref() == Some("error") {
                    if let Some(error_obj) = parsed.error {
                        if error_obj.kind.as_deref() == Some("too_many_requests") {
                            return Some("Too Many Requests".to_string());
                        }
                        if error_obj
                            .code
                            .as_deref()
                            .map(|c| c.contains("rate_limit"))
                            .unwrap_or(false)
                        {
                            return Some("Rate Limited".to_string());
                        }
                    }
                }

                if code.contains("exhausted") || code.contains("unavailable") {
                    return Some("Provider is overloaded".to_string());
                }

                return Some(message.clone());
            }
            None
        }
        _ => None,
    }
}

pub struct RetryState {
    pub attempt: u32,
    pub max_attempts: u32,
    pub last_error: Option<String>,
}

impl RetryState {
    pub fn new(max_attempts: u32) -> Self {
        Self {
            attempt: 0,
            max_attempts,
            last_error: None,
        }
    }

    pub fn should_retry(&self) -> bool {
        self.attempt < self.max_attempts
    }

    pub fn increment(&mut self, error: String) {
        self.attempt += 1;
        self.last_error = Some(error);
    }

    pub fn next_delay(&self, error_info: Option<&ApiErrorInfo>) -> u64 {
        delay(self.attempt, error_info)
    }
}
