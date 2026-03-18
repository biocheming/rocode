use crate::{MessageRole, SessionMessage};
use rocode_core::contracts::{
    plugin_hooks,
    session::MessageRoleWire,
    wire::{self, fields as wire_fields},
};

pub(crate) fn hook_payload_object(
    payload: &serde_json::Value,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    payload
        .get(plugin_hooks::keys::OUTPUT)
        .and_then(|value| value.as_object())
        .or_else(|| payload.as_object())
        .or_else(|| {
            payload
                .get(plugin_hooks::keys::DATA)
                .and_then(|value| value.as_object())
        })
}

pub(crate) fn session_message_hook_payload(message: &SessionMessage) -> serde_json::Value {
    let mut payload = serde_json::to_value(message).unwrap_or_else(|_| serde_json::json!({}));
    let Some(object) = payload.as_object_mut() else {
        return payload;
    };

    object.insert(
        plugin_hooks::keys::INFO.to_string(),
        serde_json::json!({
            plugin_hooks::keys::ID: message.id,
            wire::keys::SESSION_ID: message.session_id,
            wire_fields::ROLE: hook_message_role(&message.role),
            "time": { "created": message.created_at.timestamp_millis() },
        }),
    );

    payload
}

pub(crate) fn hook_message_role(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::User => MessageRoleWire::User.as_str(),
        MessageRole::Assistant => MessageRoleWire::Assistant.as_str(),
        MessageRole::System => MessageRoleWire::System.as_str(),
        MessageRole::Tool => MessageRoleWire::Tool.as_str(),
    }
}

pub(crate) fn apply_chat_messages_hook_outputs(
    messages: &mut Vec<SessionMessage>,
    hook_outputs: Vec<rocode_plugin::HookOutput>,
) {
    for output in hook_outputs {
        let Some(payload) = output.payload.as_ref() else {
            continue;
        };
        let Some(object) = hook_payload_object(payload) else {
            continue;
        };
        let Some(next_messages) = object
            .get(plugin_hooks::keys::MESSAGES)
            .and_then(|value| value.as_array())
        else {
            continue;
        };
        let parsed = serde_json::from_value::<Vec<SessionMessage>>(serde_json::Value::Array(
            next_messages.clone(),
        ));
        if let Ok(next) = parsed {
            *messages = next;
        }
    }
}

pub(crate) fn apply_chat_message_hook_outputs(
    message: &mut SessionMessage,
    hook_outputs: Vec<rocode_plugin::HookOutput>,
) {
    for output in hook_outputs {
        let Some(payload) = output.payload.as_ref() else {
            continue;
        };
        let Some(object) = hook_payload_object(payload) else {
            continue;
        };
        if let Some(next_message) = object.get(plugin_hooks::keys::MESSAGE) {
            if let Ok(parsed) = serde_json::from_value::<SessionMessage>(next_message.clone()) {
                *message = parsed;
            }
        }
        if let Some(next_parts) = object
            .get(plugin_hooks::keys::PARTS)
            .and_then(|value| value.as_array())
        {
            let parsed = serde_json::from_value::<Vec<crate::MessagePart>>(
                serde_json::Value::Array(next_parts.clone()),
            );
            if let Ok(parts) = parsed {
                message.parts = parts;
            }
        }
    }
}
