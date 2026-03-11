#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractiveCommand {
    Exit,
    ShowHelp,
    Abort,
    ShowRecovery,
    ExecuteRecovery(String),
    NewSession,
    ClearScreen,
    ShowStatus,
    ListModels,
    SelectModel(String),
    ListProviders,
    ListThemes,
    ListPresets,
    SelectPreset(String),
    ListSessions,
    ListTasks,
    ShowTask(String),
    KillTask(String),
    Compact,
    Copy,
    ListAgents,
    SelectAgent(String),
    ToggleSidebar,
    ToggleActive,
    ScrollUp,
    ScrollDown,
    ScrollBottom,
    /// User typed an unknown /command — we should warn, not treat as prompt.
    Unknown(String),
}

pub fn parse_interactive_command(input: &str) -> Option<InteractiveCommand> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let plain = trimmed.to_ascii_lowercase();
    match plain.as_str() {
        "exit" | "quit" => return Some(InteractiveCommand::Exit),
        "help" => return Some(InteractiveCommand::ShowHelp),
        "clear" => return Some(InteractiveCommand::ClearScreen),
        "stats" => return Some(InteractiveCommand::ShowStatus),
        "models" => return Some(InteractiveCommand::ListModels),
        "providers" => return Some(InteractiveCommand::ListProviders),
        _ => {}
    }

    if !trimmed.starts_with('/') {
        return None;
    }

    let body = trimmed[1..].trim();
    if body.is_empty() {
        return None;
    }

    let mut parts = body.split_whitespace();
    let name = parts.next()?.to_ascii_lowercase();
    let arg = parts.collect::<Vec<_>>().join(" ");

    match name.as_str() {
        "help" | "commands" => Some(InteractiveCommand::ShowHelp),
        "exit" | "quit" | "q" => Some(InteractiveCommand::Exit),
        "abort" => Some(InteractiveCommand::Abort),
        "recover" | "recovery" => {
            if arg.is_empty() {
                Some(InteractiveCommand::ShowRecovery)
            } else {
                Some(InteractiveCommand::ExecuteRecovery(arg))
            }
        }
        "new" => Some(InteractiveCommand::NewSession),
        "clear" => Some(InteractiveCommand::ClearScreen),
        "status" | "stats" => Some(InteractiveCommand::ShowStatus),
        "models" => Some(InteractiveCommand::ListModels),
        "model" => {
            if arg.is_empty() {
                Some(InteractiveCommand::ListModels)
            } else {
                Some(InteractiveCommand::SelectModel(arg))
            }
        }
        "providers" => Some(InteractiveCommand::ListProviders),
        "theme" | "themes" => Some(InteractiveCommand::ListThemes),
        "preset" | "presets" => {
            if arg.is_empty() {
                Some(InteractiveCommand::ListPresets)
            } else {
                Some(InteractiveCommand::SelectPreset(arg))
            }
        }
        "session" | "sessions" | "resume" | "continue" => Some(InteractiveCommand::ListSessions),
        "compact" => Some(InteractiveCommand::Compact),
        "copy" => Some(InteractiveCommand::Copy),
        "agent" | "agents" => {
            if arg.is_empty() {
                Some(InteractiveCommand::ListAgents)
            } else {
                Some(InteractiveCommand::SelectAgent(arg))
            }
        }
        "sidebar" => Some(InteractiveCommand::ToggleSidebar),
        "active" => Some(InteractiveCommand::ToggleActive),
        "up" | "pageup" => Some(InteractiveCommand::ScrollUp),
        "down" | "pagedown" => Some(InteractiveCommand::ScrollDown),
        "bottom" | "end" => Some(InteractiveCommand::ScrollBottom),
        "tasks" | "task" => {
            if arg.is_empty() {
                Some(InteractiveCommand::ListTasks)
            } else {
                let mut sub_parts = arg.split_whitespace();
                let sub_cmd = sub_parts.next().unwrap_or("");
                let sub_arg = sub_parts.collect::<Vec<_>>().join(" ");
                match sub_cmd {
                    "show" if !sub_arg.is_empty() => Some(InteractiveCommand::ShowTask(sub_arg)),
                    "kill" | "cancel" if !sub_arg.is_empty() => {
                        Some(InteractiveCommand::KillTask(sub_arg))
                    }
                    _ => Some(InteractiveCommand::ListTasks),
                }
            }
        }
        _ => Some(InteractiveCommand::Unknown(name)),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_interactive_command, InteractiveCommand};

    #[test]
    fn parses_plain_commands() {
        assert_eq!(
            parse_interactive_command("help"),
            Some(InteractiveCommand::ShowHelp)
        );
        assert_eq!(
            parse_interactive_command("models"),
            Some(InteractiveCommand::ListModels)
        );
        assert_eq!(
            parse_interactive_command("providers"),
            Some(InteractiveCommand::ListProviders)
        );
        assert_eq!(
            parse_interactive_command("clear"),
            Some(InteractiveCommand::ClearScreen)
        );
    }

    #[test]
    fn parses_slash_commands() {
        assert_eq!(
            parse_interactive_command("/help"),
            Some(InteractiveCommand::ShowHelp)
        );
        assert_eq!(
            parse_interactive_command("/abort"),
            Some(InteractiveCommand::Abort)
        );
        assert_eq!(
            parse_interactive_command("/recover"),
            Some(InteractiveCommand::ShowRecovery)
        );
        assert_eq!(
            parse_interactive_command("/recover retry"),
            Some(InteractiveCommand::ExecuteRecovery("retry".to_string()))
        );
        assert_eq!(
            parse_interactive_command("/themes"),
            Some(InteractiveCommand::ListThemes)
        );
        assert_eq!(
            parse_interactive_command("/preset"),
            Some(InteractiveCommand::ListPresets)
        );
        assert_eq!(
            parse_interactive_command("/session"),
            Some(InteractiveCommand::ListSessions)
        );
        assert_eq!(
            parse_interactive_command("/compact"),
            Some(InteractiveCommand::Compact)
        );
        assert_eq!(
            parse_interactive_command("/copy"),
            Some(InteractiveCommand::Copy)
        );
        assert_eq!(
            parse_interactive_command("/new"),
            Some(InteractiveCommand::NewSession)
        );
        assert_eq!(
            parse_interactive_command("/clear"),
            Some(InteractiveCommand::ClearScreen)
        );
    }

    #[test]
    fn parses_model_selection() {
        assert_eq!(
            parse_interactive_command("/model openai/gpt-4.1"),
            Some(InteractiveCommand::SelectModel(
                "openai/gpt-4.1".to_string()
            ))
        );
        assert_eq!(
            parse_interactive_command("/model"),
            Some(InteractiveCommand::ListModels)
        );
    }

    #[test]
    fn parses_agent_commands() {
        assert_eq!(
            parse_interactive_command("/agent"),
            Some(InteractiveCommand::ListAgents)
        );
        assert_eq!(
            parse_interactive_command("/agents"),
            Some(InteractiveCommand::ListAgents)
        );
        assert_eq!(
            parse_interactive_command("/agent build"),
            Some(InteractiveCommand::SelectAgent("build".to_string()))
        );
        assert_eq!(
            parse_interactive_command("/preset prometheus"),
            Some(InteractiveCommand::SelectPreset("prometheus".to_string()))
        );
    }

    #[test]
    fn unknown_slash_command_returns_unknown() {
        assert_eq!(
            parse_interactive_command("/foo"),
            Some(InteractiveCommand::Unknown("foo".to_string()))
        );
        assert_eq!(
            parse_interactive_command("/nonexistent"),
            Some(InteractiveCommand::Unknown("nonexistent".to_string()))
        );
    }

    #[test]
    fn parses_toggle_commands() {
        assert_eq!(
            parse_interactive_command("/sidebar"),
            Some(InteractiveCommand::ToggleSidebar)
        );
        assert_eq!(
            parse_interactive_command("/active"),
            Some(InteractiveCommand::ToggleActive)
        );
    }

    #[test]
    fn parses_scroll_commands() {
        assert_eq!(
            parse_interactive_command("/up"),
            Some(InteractiveCommand::ScrollUp)
        );
        assert_eq!(
            parse_interactive_command("/down"),
            Some(InteractiveCommand::ScrollDown)
        );
        assert_eq!(
            parse_interactive_command("/bottom"),
            Some(InteractiveCommand::ScrollBottom)
        );
        assert_eq!(
            parse_interactive_command("/pageup"),
            Some(InteractiveCommand::ScrollUp)
        );
        assert_eq!(
            parse_interactive_command("/end"),
            Some(InteractiveCommand::ScrollBottom)
        );
    }

    #[test]
    fn ignores_non_commands() {
        assert_eq!(parse_interactive_command(""), None);
        assert_eq!(parse_interactive_command("hello rocode"), None);
    }

    #[test]
    fn parses_tasks_commands() {
        assert_eq!(
            parse_interactive_command("/tasks"),
            Some(InteractiveCommand::ListTasks)
        );
        assert_eq!(
            parse_interactive_command("/task"),
            Some(InteractiveCommand::ListTasks)
        );
        assert_eq!(
            parse_interactive_command("/tasks show a1"),
            Some(InteractiveCommand::ShowTask("a1".to_string()))
        );
        assert_eq!(
            parse_interactive_command("/tasks kill a1"),
            Some(InteractiveCommand::KillTask("a1".to_string()))
        );
        assert_eq!(
            parse_interactive_command("/tasks cancel a2"),
            Some(InteractiveCommand::KillTask("a2".to_string()))
        );
    }
}
