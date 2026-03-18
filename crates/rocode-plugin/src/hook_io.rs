use serde_json::{Map, Value};

use crate::{HookContext, HookEvent};
use rocode_core::contracts::plugin_hooks::{aliases as hook_aliases, keys as hook_keys};
use rocode_core::contracts::permission::PermissionHookStatus;
use rocode_core::contracts::wire::keys as wire_keys;

/// Build (input, output) JSON payloads for script-style hooks.
///
/// This mirrors the TS plugin-host expectations where each hook receives
/// (input, output) objects with a small set of normalized keys.
pub(crate) fn hook_io_from_context(context: &HookContext) -> (Value, Value) {
    let source = context_values(context);
    let mut input = Map::new();
    let mut output = Map::new();

    match context.event {
        HookEvent::ToolDefinition => {
            copy_first(&source, &mut input, hook_keys::TOOL_ID, &[hook_keys::TOOL_ID]);
            copy_first(
                &source,
                &mut output,
                hook_keys::DESCRIPTION,
                &[hook_keys::DESCRIPTION],
            );
            copy_first(
                &source,
                &mut output,
                hook_keys::PARAMETERS,
                &[hook_keys::PARAMETERS],
            );
        }
        HookEvent::ToolExecuteBefore => {
            copy_first(&source, &mut input, hook_keys::TOOL, &[hook_keys::TOOL]);
            copy_first(
                &source,
                &mut input,
                wire_keys::SESSION_ID,
                &[wire_keys::SESSION_ID],
            );
            copy_first(
                &source,
                &mut input,
                hook_keys::CALL_ID,
                &[hook_keys::CALL_ID],
            );
            copy_first(&source, &mut output, hook_keys::ARGS, &[hook_keys::ARGS]);
        }
        HookEvent::ToolExecuteAfter => {
            copy_first(&source, &mut input, hook_keys::TOOL, &[hook_keys::TOOL]);
            copy_first(
                &source,
                &mut input,
                wire_keys::SESSION_ID,
                &[wire_keys::SESSION_ID],
            );
            copy_first(
                &source,
                &mut input,
                hook_keys::CALL_ID,
                &[hook_keys::CALL_ID],
            );
            copy_first(&source, &mut input, hook_keys::ARGS, &[hook_keys::ARGS]);
            copy_first(&source, &mut input, hook_keys::ERROR, &[hook_keys::ERROR]);
            copy_first(&source, &mut output, hook_keys::TITLE, &[hook_keys::TITLE]);
            copy_first(&source, &mut output, hook_keys::OUTPUT, &[hook_keys::OUTPUT]);
            copy_first(
                &source,
                &mut output,
                hook_keys::METADATA,
                &[hook_keys::METADATA],
            );
        }
        HookEvent::ChatSystemTransform => {
            copy_first(
                &source,
                &mut input,
                wire_keys::SESSION_ID,
                &[wire_keys::SESSION_ID],
            );
            copy_first(&source, &mut input, hook_keys::MODEL, &[hook_keys::MODEL]);
            if !input.contains_key(hook_keys::MODEL) {
                if let Some(model) = synthesize_model(&source) {
                    input.insert(hook_keys::MODEL.to_string(), model);
                }
            }
            copy_first(
                &source,
                &mut output,
                hook_keys::SYSTEM,
                &[hook_keys::SYSTEM],
            );
        }
        HookEvent::ChatMessagesTransform => {
            copy_first(
                &source,
                &mut output,
                hook_keys::MESSAGES,
                &[hook_keys::MESSAGES],
            );
        }
        HookEvent::ChatParams => {
            copy_first(
                &source,
                &mut input,
                wire_keys::SESSION_ID,
                &[wire_keys::SESSION_ID],
            );
            copy_first(&source, &mut input, hook_keys::AGENT, &[hook_keys::AGENT]);
            copy_first(&source, &mut input, hook_keys::MODEL, &[hook_keys::MODEL]);
            if !input.contains_key(hook_keys::MODEL) {
                if let Some(model) = synthesize_model(&source) {
                    input.insert(hook_keys::MODEL.to_string(), model);
                }
            }
            copy_first(
                &source,
                &mut input,
                hook_keys::PROVIDER,
                &[hook_keys::PROVIDER],
            );
            if !input.contains_key(hook_keys::PROVIDER) {
                if let Some(provider) = synthesize_provider(&source) {
                    input.insert(hook_keys::PROVIDER.to_string(), provider);
                }
            }
            copy_first(
                &source,
                &mut input,
                hook_keys::MESSAGE,
                &[hook_keys::MESSAGE],
            );
            copy_first(
                &source,
                &mut output,
                hook_keys::TEMPERATURE,
                &[hook_keys::TEMPERATURE],
            );
            copy_first(&source, &mut output, hook_keys::TOP_P, &[hook_keys::TOP_P]);
            copy_first(&source, &mut output, hook_keys::TOP_K, &[hook_keys::TOP_K]);
            copy_first(
                &source,
                &mut output,
                hook_keys::OPTIONS,
                &[hook_keys::OPTIONS],
            );
            copy_first(
                &source,
                &mut output,
                hook_keys::MAX_TOKENS,
                &[hook_keys::MAX_TOKENS],
            );
        }
        HookEvent::ChatHeaders => {
            copy_first(
                &source,
                &mut input,
                wire_keys::SESSION_ID,
                &[wire_keys::SESSION_ID],
            );
            copy_first(&source, &mut input, hook_keys::AGENT, &[hook_keys::AGENT]);
            copy_first(&source, &mut input, hook_keys::MODEL, &[hook_keys::MODEL]);
            if !input.contains_key(hook_keys::MODEL) {
                if let Some(model) = synthesize_model(&source) {
                    input.insert(hook_keys::MODEL.to_string(), model);
                }
            }
            copy_first(
                &source,
                &mut input,
                hook_keys::PROVIDER,
                &[hook_keys::PROVIDER],
            );
            if !input.contains_key(hook_keys::PROVIDER) {
                if let Some(provider) = synthesize_provider(&source) {
                    input.insert(hook_keys::PROVIDER.to_string(), provider);
                }
            }
            copy_first(
                &source,
                &mut input,
                hook_keys::MESSAGE,
                &[hook_keys::MESSAGE],
            );
            copy_first(
                &source,
                &mut output,
                hook_keys::HEADERS,
                &[hook_keys::HEADERS],
            );
        }
        HookEvent::ChatMessage => {
            copy_first(
                &source,
                &mut input,
                wire_keys::SESSION_ID,
                &[wire_keys::SESSION_ID],
            );
            copy_first(&source, &mut input, hook_keys::AGENT, &[hook_keys::AGENT]);
            copy_first(&source, &mut input, hook_keys::MODEL, &[hook_keys::MODEL]);
            if !input.contains_key(hook_keys::MODEL) {
                if let Some(model) = synthesize_model(&source) {
                    input.insert(hook_keys::MODEL.to_string(), model);
                }
            }
            copy_first(
                &source,
                &mut input,
                wire_keys::MESSAGE_ID,
                &[wire_keys::MESSAGE_ID],
            );
            copy_first(
                &source,
                &mut input,
                hook_keys::VARIANT,
                &[hook_keys::VARIANT],
            );
            copy_first(
                &source,
                &mut input,
                hook_keys::HAS_TOOL_CALLS,
                &[hook_keys::HAS_TOOL_CALLS],
            );
            copy_first(
                &source,
                &mut output,
                hook_keys::MESSAGE,
                &[hook_keys::MESSAGE],
            );
            copy_first(&source, &mut output, hook_keys::PARTS, &[hook_keys::PARTS]);
        }
        HookEvent::SessionCompacting => {
            copy_first(
                &source,
                &mut input,
                wire_keys::SESSION_ID,
                &[wire_keys::SESSION_ID],
            );
            copy_first(&source, &mut input, hook_keys::AUTO, &[hook_keys::AUTO]);
            copy_first(
                &source,
                &mut input,
                hook_keys::COMPLETED,
                &[hook_keys::COMPLETED],
            );
            copy_first(
                &source,
                &mut output,
                hook_keys::CONTEXT,
                &[hook_keys::CONTEXT],
            );
            copy_first(
                &source,
                &mut output,
                hook_keys::PROMPT,
                &[hook_keys::PROMPT],
            );
        }
        HookEvent::TextComplete => {
            copy_first(
                &source,
                &mut input,
                wire_keys::SESSION_ID,
                &[wire_keys::SESSION_ID],
            );
            copy_first(
                &source,
                &mut input,
                wire_keys::MESSAGE_ID,
                &[wire_keys::MESSAGE_ID],
            );
            copy_first(
                &source,
                &mut input,
                hook_keys::PART_ID,
                &[hook_keys::PART_ID],
            );
            copy_first(&source, &mut output, hook_keys::TEXT, &[hook_keys::TEXT]);
        }
        HookEvent::ShellEnv => {
            copy_first(&source, &mut input, hook_keys::CWD, &[hook_keys::CWD]);
            copy_first(
                &source,
                &mut input,
                wire_keys::SESSION_ID,
                &[wire_keys::SESSION_ID],
            );
            copy_first(
                &source,
                &mut input,
                hook_keys::CALL_ID,
                &[hook_keys::CALL_ID],
            );
            copy_first(&source, &mut output, hook_keys::ENV, &[hook_keys::ENV]);
        }
        HookEvent::CommandExecuteBefore => {
            copy_first(
                &source,
                &mut input,
                hook_keys::COMMAND,
                &[hook_keys::COMMAND],
            );
            copy_first(
                &source,
                &mut input,
                wire_keys::SESSION_ID,
                &[wire_keys::SESSION_ID],
            );
            copy_first(
                &source,
                &mut input,
                hook_keys::ARGUMENTS,
                &[hook_keys::ARGUMENTS],
            );
            copy_first(
                &source,
                &mut input,
                hook_keys::SOURCE,
                &[hook_keys::SOURCE],
            );
            copy_first(&source, &mut output, hook_keys::PARTS, &[hook_keys::PARTS]);
        }
        HookEvent::PermissionAsk => {
            copy_first(
                &source,
                &mut input,
                hook_keys::PERMISSION,
                &[hook_keys::PERMISSION],
            );
            copy_first(
                &source,
                &mut input,
                hook_keys::PERMISSION_TYPE,
                &[hook_keys::PERMISSION_TYPE, hook_aliases::PERMISSION_TYPE_CAMEL],
            );
            copy_first(
                &source,
                &mut input,
                hook_keys::PERMISSION_ID,
                &[hook_keys::PERMISSION_ID, hook_aliases::PERMISSION_ID_CAMEL],
            );
            copy_first(
                &source,
                &mut output,
                hook_keys::STATUS,
                &[hook_keys::STATUS],
            );
        }
        _ => {
            input = source.clone();
            output = source;
        }
    }

    seed_hook_output(context.event.clone(), &mut output);

    (Value::Object(input), Value::Object(output))
}

fn context_values(context: &HookContext) -> Map<String, Value> {
    let mut values = Map::new();
    for (key, value) in &context.data {
        values.insert(key.clone(), value.clone());
        let normalized = normalize_hook_key(key);
        if normalized != *key {
            values.entry(normalized).or_insert_with(|| value.clone());
        }
    }
    if let Some(session_id) = &context.session_id {
        values
            .entry(wire_keys::SESSION_ID.to_string())
            .or_insert_with(|| Value::String(session_id.clone()));
    }
    values
}

fn first_value(source: &Map<String, Value>, keys: &[&str]) -> Option<Value> {
    keys.iter().find_map(|key| source.get(*key).cloned())
}

fn copy_first(
    source: &Map<String, Value>,
    target: &mut Map<String, Value>,
    target_key: &str,
    candidate_keys: &[&str],
) {
    if let Some(value) = first_value(source, candidate_keys) {
        target.insert(target_key.to_string(), value);
    }
}

fn synthesize_model(source: &Map<String, Value>) -> Option<Value> {
    let model_id = first_value(source, &[hook_keys::MODEL_ID, hook_aliases::MODEL_ID_SNAKE])?;
    let mut model = Map::new();
    model.insert(hook_keys::MODEL_ID.to_string(), model_id.clone());
    model.insert(hook_keys::ID.to_string(), model_id);
    if let Some(provider_id) =
        first_value(source, &[hook_keys::PROVIDER_ID, hook_aliases::PROVIDER_ID_SNAKE])
    {
        model.insert(hook_keys::PROVIDER_ID.to_string(), provider_id);
    }
    Some(Value::Object(model))
}

fn synthesize_provider(source: &Map<String, Value>) -> Option<Value> {
    let provider_id = first_value(source, &[hook_keys::PROVIDER_ID, hook_aliases::PROVIDER_ID_SNAKE])?;
    let mut provider = Map::new();
    provider.insert(hook_keys::ID.to_string(), provider_id.clone());
    provider.insert(
        hook_keys::INFO.to_string(),
        Value::Object(Map::from_iter([(
            hook_keys::ID.to_string(),
            provider_id,
        )])),
    );
    Some(Value::Object(provider))
}

fn normalize_hook_key(key: &str) -> String {
    match key {
        hook_aliases::TOOL_ID_SNAKE => hook_keys::TOOL_ID.to_string(),
        hook_aliases::CALL_ID_SNAKE => hook_keys::CALL_ID.to_string(),
        hook_aliases::MODEL_ID_SNAKE => hook_keys::MODEL_ID.to_string(),
        hook_aliases::PROVIDER_ID_SNAKE => hook_keys::PROVIDER_ID.to_string(),
        hook_aliases::MESSAGE_ID_SNAKE => wire_keys::MESSAGE_ID.to_string(),
        hook_aliases::PART_ID_SNAKE => hook_keys::PART_ID.to_string(),
        hook_aliases::MAX_TOKENS_SNAKE => hook_keys::MAX_TOKENS.to_string(),
        _ => key.to_string(),
    }
}

fn ensure_default(map: &mut Map<String, Value>, key: &str, value: Value) {
    map.entry(key.to_string()).or_insert(value);
}

fn ensure_object(map: &mut Map<String, Value>, key: &str) {
    ensure_default(map, key, Value::Object(Map::new()));
}

fn ensure_array(map: &mut Map<String, Value>, key: &str) {
    ensure_default(map, key, Value::Array(Vec::new()));
}

fn seed_hook_output(event: HookEvent, output: &mut Map<String, Value>) {
    match event {
        HookEvent::ToolDefinition => {
            ensure_default(
                output,
                hook_keys::DESCRIPTION,
                Value::String(String::new()),
            );
            ensure_object(output, hook_keys::PARAMETERS);
        }
        HookEvent::ToolExecuteBefore => {
            ensure_object(output, hook_keys::ARGS);
        }
        HookEvent::ToolExecuteAfter => {
            ensure_default(output, hook_keys::TITLE, Value::String(String::new()));
            ensure_default(output, hook_keys::OUTPUT, Value::String(String::new()));
            ensure_object(output, hook_keys::METADATA);
        }
        HookEvent::ChatHeaders => {
            ensure_object(output, hook_keys::HEADERS);
        }
        HookEvent::ChatParams => {
            ensure_default(output, hook_keys::TEMPERATURE, Value::Null);
            ensure_default(output, hook_keys::TOP_P, Value::Null);
            ensure_default(output, hook_keys::TOP_K, Value::Null);
            ensure_object(output, hook_keys::OPTIONS);
        }
        HookEvent::ChatMessage => {
            ensure_default(output, hook_keys::MESSAGE, Value::Null);
            ensure_array(output, hook_keys::PARTS);
        }
        HookEvent::ChatMessagesTransform => {
            ensure_array(output, hook_keys::MESSAGES);
        }
        HookEvent::ChatSystemTransform => {
            ensure_array(output, hook_keys::SYSTEM);
        }
        HookEvent::SessionCompacting => {
            ensure_array(output, hook_keys::CONTEXT);
        }
        HookEvent::TextComplete => {
            ensure_default(output, hook_keys::TEXT, Value::String(String::new()));
        }
        HookEvent::ShellEnv => {
            ensure_object(output, hook_keys::ENV);
        }
        HookEvent::CommandExecuteBefore => {
            ensure_array(output, hook_keys::PARTS);
        }
        HookEvent::PermissionAsk => {
            ensure_default(
                output,
                hook_keys::STATUS,
                Value::String(PermissionHookStatus::Ask.as_str().to_string()),
            );
        }
        _ => {}
    }
}
