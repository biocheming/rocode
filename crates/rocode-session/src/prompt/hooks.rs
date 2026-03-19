use crate::{Role, SessionMessage};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct HookMessageInfoTime {
    created: i64,
}

#[derive(Debug, Serialize)]
struct HookMessageInfo {
    id: String,
    #[serde(rename = "sessionID")]
    session_id: String,
    role: &'static str,
    time: HookMessageInfoTime,
}

pub(crate) fn parse_hook_payload<T: DeserializeOwned>(payload: &serde_json::Value) -> Option<T> {
    #[derive(Debug, Deserialize)]
    #[serde(untagged)]
    enum HookEnvelope<T> {
        Output { output: T },
        Data { data: T },
        Direct(T),
    }

    let envelope: HookEnvelope<T> = serde_json::from_value(payload.clone()).ok()?;
    Some(match envelope {
        HookEnvelope::Output { output } => output,
        HookEnvelope::Data { data } => data,
        HookEnvelope::Direct(value) => value,
    })
}

pub(crate) fn session_message_hook_payload(message: &SessionMessage) -> serde_json::Value {
    let mut payload = serde_json::to_value(message).unwrap_or(serde_json::Value::Null);
    let Some(object) = payload.as_object_mut() else {
        return payload;
    };

    let info = HookMessageInfo {
        id: message.id.clone(),
        session_id: message.session_id.clone(),
        role: hook_message_role(&message.role),
        time: HookMessageInfoTime {
            created: message.created_at.timestamp_millis(),
        },
    };

    object.insert(
        "info".to_string(),
        serde_json::to_value(info).unwrap_or(serde_json::Value::Null),
    );

    payload
}

pub(crate) fn hook_message_role(role: &Role) -> &'static str {
    role.as_str()
}

pub(crate) fn apply_chat_messages_hook_outputs(
    messages: &mut Vec<SessionMessage>,
    hook_outputs: Vec<rocode_plugin::HookOutput>,
) {
    fn deserialize_opt_session_messages_lossy<'de, D>(
        deserializer: D,
    ) -> Result<Option<Vec<SessionMessage>>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Option::<serde_json::Value>::deserialize(deserializer)?;
        let Some(value) = value else {
            return Ok(None);
        };
        Ok(serde_json::from_value::<Vec<SessionMessage>>(value).ok())
    }

    #[derive(Debug, Deserialize, Default)]
    struct ChatMessagesHookWire {
        #[serde(default, deserialize_with = "deserialize_opt_session_messages_lossy")]
        messages: Option<Vec<SessionMessage>>,
    }

    for output in hook_outputs {
        let Some(payload) = output.payload.as_ref() else {
            continue;
        };
        let Some(parsed) = parse_hook_payload::<ChatMessagesHookWire>(payload) else {
            continue;
        };
        if let Some(next) = parsed.messages {
            *messages = next;
        }
    }
}

pub(crate) fn apply_chat_message_hook_outputs(
    message: &mut SessionMessage,
    hook_outputs: Vec<rocode_plugin::HookOutput>,
) {
    fn deserialize_opt_session_message_lossy<'de, D>(
        deserializer: D,
    ) -> Result<Option<SessionMessage>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Option::<serde_json::Value>::deserialize(deserializer)?;
        let Some(value) = value else {
            return Ok(None);
        };
        Ok(serde_json::from_value::<SessionMessage>(value).ok())
    }

    fn deserialize_opt_message_parts_lossy<'de, D>(
        deserializer: D,
    ) -> Result<Option<Vec<crate::MessagePart>>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Option::<serde_json::Value>::deserialize(deserializer)?;
        let Some(value) = value else {
            return Ok(None);
        };
        Ok(serde_json::from_value::<Vec<crate::MessagePart>>(value).ok())
    }

    #[derive(Debug, Deserialize, Default)]
    struct ChatMessageHookWire {
        #[serde(default, deserialize_with = "deserialize_opt_session_message_lossy")]
        message: Option<SessionMessage>,
        #[serde(default, deserialize_with = "deserialize_opt_message_parts_lossy")]
        parts: Option<Vec<crate::MessagePart>>,
    }

    for output in hook_outputs {
        let Some(payload) = output.payload.as_ref() else {
            continue;
        };
        let Some(parsed) = parse_hook_payload::<ChatMessageHookWire>(payload) else {
            continue;
        };
        if let Some(next_message) = parsed.message {
            *message = next_message;
        }
        if let Some(next_parts) = parsed.parts {
            message.parts = next_parts;
        }
    }
}
