use crate::{ui_command_argument_kind, ResolvedUiCommand, UiActionId};

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
    ConnectProvider(Option<String>),
    ListThemes,
    ListPresets,
    SelectPreset(String),
    ListSessions,
    ParentSession,
    ListChildSessions,
    FocusChildSession(String),
    FocusNextChildSession,
    FocusPreviousChildSession,
    BackToRootSession,
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
    /// `/inspect [stage_id]` — show stage event log for current session.
    InspectStage(Option<String>),
    /// User typed an unknown /command — we should warn, not treat as prompt.
    Unknown(String),
}

impl InteractiveCommand {
    pub fn ui_action_invocation(&self) -> Option<ResolvedUiCommand> {
        let (action_id, argument) = match self {
            Self::Exit => (UiActionId::Exit, None),
            Self::ShowHelp => (UiActionId::ShowHelp, None),
            Self::Abort => (UiActionId::AbortExecution, None),
            Self::ShowRecovery => (UiActionId::OpenRecoveryList, None),
            Self::NewSession => (UiActionId::NewSession, None),
            Self::ShowStatus => (UiActionId::ShowStatus, None),
            Self::ListModels => (UiActionId::OpenModelList, None),
            Self::SelectModel(model_ref) => (UiActionId::OpenModelList, Some(model_ref.clone())),
            Self::ListProviders => (UiActionId::ConnectProvider, None),
            Self::ConnectProvider(query) => (UiActionId::ConnectProvider, query.clone()),
            Self::ListThemes => (UiActionId::OpenThemeList, None),
            Self::ListPresets => (UiActionId::OpenPresetList, None),
            Self::SelectPreset(name) => (UiActionId::OpenPresetList, Some(name.clone())),
            Self::ListSessions => (UiActionId::OpenSessionList, None),
            Self::ParentSession => (UiActionId::NavigateParentSession, None),
            Self::ListTasks => (UiActionId::ListTasks, None),
            Self::Compact => (UiActionId::CompactSession, None),
            Self::Copy => (UiActionId::CopySession, None),
            Self::ListAgents => (UiActionId::OpenAgentList, None),
            Self::SelectAgent(name) => (UiActionId::OpenAgentList, Some(name.clone())),
            Self::ToggleSidebar => (UiActionId::ToggleSidebar, None),
            _ => return None,
        };

        Some(ResolvedUiCommand {
            action_id,
            argument_kind: ui_command_argument_kind(action_id),
            argument,
        })
    }

    pub fn ui_action_id(&self) -> Option<UiActionId> {
        self.ui_action_invocation()
            .map(|invocation| invocation.action_id)
    }
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
        "provider" | "connect" => Some(InteractiveCommand::ConnectProvider(
            (!arg.is_empty()).then_some(arg),
        )),
        "theme" | "themes" => Some(InteractiveCommand::ListThemes),
        "preset" | "presets" => {
            if arg.is_empty() {
                Some(InteractiveCommand::ListPresets)
            } else {
                Some(InteractiveCommand::SelectPreset(arg))
            }
        }
        "session" | "sessions" | "resume" | "continue" => Some(InteractiveCommand::ListSessions),
        "parent" | "back" => Some(InteractiveCommand::ParentSession),
        "child" | "children" => {
            if arg.is_empty() {
                Some(InteractiveCommand::ListChildSessions)
            } else {
                let mut sub_parts = arg.split_whitespace();
                let sub_cmd = sub_parts.next().unwrap_or("");
                let sub_arg = sub_parts.collect::<Vec<_>>().join(" ");
                match sub_cmd {
                    "list" => Some(InteractiveCommand::ListChildSessions),
                    "focus" if !sub_arg.is_empty() => {
                        Some(InteractiveCommand::FocusChildSession(sub_arg))
                    }
                    "focus" => Some(InteractiveCommand::ListChildSessions),
                    "next" => Some(InteractiveCommand::FocusNextChildSession),
                    "prev" | "previous" => Some(InteractiveCommand::FocusPreviousChildSession),
                    "back" | "root" => Some(InteractiveCommand::BackToRootSession),
                    _ => Some(InteractiveCommand::ListChildSessions),
                }
            }
        }
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
        "inspect" | "stage" | "stages" => {
            if arg.is_empty() {
                Some(InteractiveCommand::InspectStage(None))
            } else {
                Some(InteractiveCommand::InspectStage(Some(arg)))
            }
        }
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
    use crate::{ResolvedUiCommand, UiActionId, UiCommandArgumentKind};

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
            parse_interactive_command("/connect"),
            Some(InteractiveCommand::ConnectProvider(None))
        );
        assert_eq!(
            parse_interactive_command("/connect openrouter"),
            Some(InteractiveCommand::ConnectProvider(Some(
                "openrouter".to_string()
            )))
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
            parse_interactive_command("/parent"),
            Some(InteractiveCommand::ParentSession)
        );
        assert_eq!(
            parse_interactive_command("/back"),
            Some(InteractiveCommand::ParentSession)
        );
        assert_eq!(
            parse_interactive_command("/child"),
            Some(InteractiveCommand::ListChildSessions)
        );
        assert_eq!(
            parse_interactive_command("/children"),
            Some(InteractiveCommand::ListChildSessions)
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

    #[test]
    fn parses_child_session_commands() {
        assert_eq!(
            parse_interactive_command("/child list"),
            Some(InteractiveCommand::ListChildSessions)
        );
        assert_eq!(
            parse_interactive_command("/child focus child-42"),
            Some(InteractiveCommand::FocusChildSession(
                "child-42".to_string()
            ))
        );
        assert_eq!(
            parse_interactive_command("/child focus"),
            Some(InteractiveCommand::ListChildSessions)
        );
        assert_eq!(
            parse_interactive_command("/child next"),
            Some(InteractiveCommand::FocusNextChildSession)
        );
        assert_eq!(
            parse_interactive_command("/child prev"),
            Some(InteractiveCommand::FocusPreviousChildSession)
        );
        assert_eq!(
            parse_interactive_command("/child previous"),
            Some(InteractiveCommand::FocusPreviousChildSession)
        );
        assert_eq!(
            parse_interactive_command("/child back"),
            Some(InteractiveCommand::BackToRootSession)
        );
        assert_eq!(
            parse_interactive_command("/child root"),
            Some(InteractiveCommand::BackToRootSession)
        );
    }

    #[test]
    fn parses_inspect_commands() {
        assert_eq!(
            parse_interactive_command("/inspect"),
            Some(InteractiveCommand::InspectStage(None))
        );
        assert_eq!(
            parse_interactive_command("/inspect stg_abc"),
            Some(InteractiveCommand::InspectStage(Some(
                "stg_abc".to_string()
            )))
        );
        assert_eq!(
            parse_interactive_command("/stage"),
            Some(InteractiveCommand::InspectStage(None))
        );
        assert_eq!(
            parse_interactive_command("/stages"),
            Some(InteractiveCommand::InspectStage(None))
        );
    }

    #[test]
    fn maps_non_parameterized_commands_to_ui_actions() {
        assert_eq!(
            InteractiveCommand::Exit.ui_action_id(),
            Some(UiActionId::Exit)
        );
        assert_eq!(
            InteractiveCommand::ListModels.ui_action_id(),
            Some(UiActionId::OpenModelList)
        );
        assert_eq!(
            InteractiveCommand::ToggleSidebar.ui_action_id(),
            Some(UiActionId::ToggleSidebar)
        );
        assert_eq!(
            InteractiveCommand::Compact.ui_action_id(),
            Some(UiActionId::CompactSession)
        );
        assert_eq!(
            InteractiveCommand::ListPresets.ui_action_id(),
            Some(UiActionId::OpenPresetList)
        );
        assert_eq!(
            InteractiveCommand::Copy.ui_action_id(),
            Some(UiActionId::CopySession)
        );
        assert_eq!(InteractiveCommand::ListChildSessions.ui_action_id(), None);
        assert_eq!(
            InteractiveCommand::Abort.ui_action_id(),
            Some(UiActionId::AbortExecution)
        );
        assert_eq!(
            InteractiveCommand::SelectModel("foo".to_string()).ui_action_id(),
            Some(UiActionId::OpenModelList)
        );
    }

    #[test]
    fn maps_parameterized_commands_to_ui_action_invocations() {
        assert_eq!(
            InteractiveCommand::SelectModel("openai/gpt-5".to_string()).ui_action_invocation(),
            Some(ResolvedUiCommand {
                action_id: UiActionId::OpenModelList,
                argument_kind: UiCommandArgumentKind::ModelRef,
                argument: Some("openai/gpt-5".to_string()),
            })
        );
        assert_eq!(
            InteractiveCommand::SelectAgent("build".to_string()).ui_action_invocation(),
            Some(ResolvedUiCommand {
                action_id: UiActionId::OpenAgentList,
                argument_kind: UiCommandArgumentKind::AgentRef,
                argument: Some("build".to_string()),
            })
        );
        assert_eq!(
            InteractiveCommand::SelectPreset("atlas".to_string()).ui_action_invocation(),
            Some(ResolvedUiCommand {
                action_id: UiActionId::OpenPresetList,
                argument_kind: UiCommandArgumentKind::PresetRef,
                argument: Some("atlas".to_string()),
            })
        );
        assert_eq!(
            InteractiveCommand::ConnectProvider(Some("glm".to_string())).ui_action_invocation(),
            Some(ResolvedUiCommand {
                action_id: UiActionId::ConnectProvider,
                argument_kind: UiCommandArgumentKind::Text,
                argument: Some("glm".to_string()),
            })
        );
    }
}
