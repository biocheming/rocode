use crate::HookEvent;

/// Map a plugin hook name string to the corresponding [`HookEvent`] variant.
///
/// Hook names follow the TypeScript/OpenCode plugin host conventions.
pub fn hook_name_to_event(name: &str) -> Option<HookEvent> {
    match name {
        // Core lifecycle
        "config.loaded" => Some(HookEvent::ConfigLoaded),
        "session.start" => Some(HookEvent::SessionStart),
        "session.end" => Some(HookEvent::SessionEnd),
        "tool.call" => Some(HookEvent::ToolCall),
        "tool.result" => Some(HookEvent::ToolResult),
        "message.sent" => Some(HookEvent::MessageSent),
        "message.received" => Some(HookEvent::MessageReceived),
        "error" => Some(HookEvent::Error),
        "file.change" => Some(HookEvent::FileChange),
        "provider.change" => Some(HookEvent::ProviderChange),

        // TS/OpenCode-compatible hooks
        "chat.headers" => Some(HookEvent::ChatHeaders),
        "chat.params" => Some(HookEvent::ChatParams),
        "chat.message" => Some(HookEvent::ChatMessage),
        "tool.execute.before" => Some(HookEvent::ToolExecuteBefore),
        "tool.execute.after" => Some(HookEvent::ToolExecuteAfter),
        "tool.definition" => Some(HookEvent::ToolDefinition),
        "permission.ask" => Some(HookEvent::PermissionAsk),
        "command.execute.before" => Some(HookEvent::CommandExecuteBefore),
        "shell.env" => Some(HookEvent::ShellEnv),
        "experimental.chat.system.transform" => Some(HookEvent::ChatSystemTransform),
        "experimental.chat.messages.transform" => Some(HookEvent::ChatMessagesTransform),
        "experimental.session.compacting" => Some(HookEvent::SessionCompacting),
        "experimental.text.complete" => Some(HookEvent::TextComplete),
        _ => None,
    }
}
