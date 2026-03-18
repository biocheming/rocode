//! Slash Commands System
//!
//! Provides a command system for loading and executing slash commands from:
//! - `.opencode/commands/*.md` files
//! - MCP prompts
//! - Built-in commands
pub mod actions;
pub mod agent_presenter;
pub mod branding;
pub mod cli_markdown;
pub mod cli_panel;
pub mod cli_permission;
pub mod cli_prompt;
pub mod cli_select;
pub mod cli_spinner;
pub mod cli_style;
pub mod governance_fixtures;
#[cfg(test)]
mod governance_tests;
pub mod interactive;
pub mod output_blocks;
pub mod stage_protocol;
pub use actions::{
    ui_command_argument_kind, UiActionId, UiCommandArgumentKind, UiCommandCategory, UiCommandSpec,
    UiSlashCommandSpec,
};
pub use stage_protocol::*;
mod start_work;

use rocode_plugin::{HookContext, HookEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Command metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub template: String,
    #[serde(default)]
    pub scheduler_profile: Option<String>,
    pub source: CommandSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CommandSource {
    File(PathBuf),
    Builtin,
    Mcp { server: String, prompt: String },
    Skill { name: String },
}

/// Command execution context
#[derive(Debug, Clone)]
pub struct CommandContext {
    pub arguments: Vec<String>,
    pub variables: HashMap<String, String>,
    pub working_directory: PathBuf,
}

impl CommandContext {
    pub fn new(working_directory: PathBuf) -> Self {
        Self {
            arguments: Vec::new(),
            variables: HashMap::new(),
            working_directory,
        }
    }

    pub fn with_arguments(mut self, args: Vec<String>) -> Self {
        self.arguments = args;
        self
    }

    pub fn with_variable(mut self, key: String, value: String) -> Self {
        self.variables.insert(key, value);
        self
    }
}

/// Command registry for loading and executing commands
pub struct CommandRegistry {
    commands: HashMap<String, Command>,
    ui_commands: Vec<UiCommandSpec>,
    ui_command_by_action: HashMap<UiActionId, usize>,
    ui_slash_aliases: HashMap<String, UiActionId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedUiCommand {
    pub action_id: UiActionId,
    pub argument_kind: UiCommandArgumentKind,
    pub argument: Option<String>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            commands: HashMap::new(),
            ui_commands: Vec::new(),
            ui_command_by_action: HashMap::new(),
            ui_slash_aliases: HashMap::new(),
        };
        registry.register_builtin_commands();
        registry.register_builtin_ui_commands();
        registry
    }

    /// Register built-in commands
    fn register_builtin_commands(&mut self) {
        self.register(Command {
            name: "init".to_string(),
            description: "Initialize OpenCode in the current project".to_string(),
            template: include_str!("../commands/init.md").to_string(),
            scheduler_profile: None,
            source: CommandSource::Builtin,
        });

        self.register(Command {
            name: "review".to_string(),
            description: "Review the current changes in the project".to_string(),
            template: include_str!("../commands/review.md").to_string(),
            scheduler_profile: None,
            source: CommandSource::Builtin,
        });

        self.register(Command {
            name: "commit".to_string(),
            description: "Create a git commit with the current changes".to_string(),
            template: include_str!("../commands/commit.md").to_string(),
            scheduler_profile: None,
            source: CommandSource::Builtin,
        });

        self.register(Command {
            name: "test".to_string(),
            description: "Run tests for the project".to_string(),
            template: include_str!("../commands/test.md").to_string(),
            scheduler_profile: None,
            source: CommandSource::Builtin,
        });

        self.register(Command {
            name: "start-work".to_string(),
            description: "Start Atlas execution session from Prometheus plan".to_string(),
            template: include_str!("../commands/start-work.md").to_string(),
            scheduler_profile: Some("atlas".to_string()),
            source: CommandSource::Builtin,
        });
    }

    fn register_builtin_ui_commands(&mut self) {
        for command in actions::builtin_ui_commands() {
            self.register_ui_command(command);
        }
    }

    fn register_ui_command(&mut self, command: UiCommandSpec) {
        let action_id = command.action_id;
        let idx = self.ui_commands.len();
        if let Some(slash) = &command.slash {
            self.ui_slash_aliases
                .insert(slash.name.to_string(), action_id);
            for alias in slash.aliases {
                self.ui_slash_aliases
                    .insert((*alias).to_string(), action_id);
            }
        }
        self.ui_command_by_action.insert(action_id, idx);
        self.ui_commands.push(command);
    }

    /// Register a new command
    pub fn register(&mut self, command: Command) {
        self.commands.insert(command.name.clone(), command);
    }

    /// Get a command by name
    pub fn get(&self, name: &str) -> Option<&Command> {
        self.commands.get(name)
    }

    /// List all available commands
    pub fn list(&self) -> Vec<&Command> {
        self.commands.values().collect()
    }

    /// Load commands from .rocode/commands directory
    pub fn load_from_directory(&mut self, project_dir: &Path) -> anyhow::Result<()> {
        let commands_dir = project_dir.join(".rocode/commands");

        if !commands_dir.exists() {
            return Ok(());
        }

        let pattern = commands_dir.join("*.md");
        let pattern_str = pattern.to_string_lossy();

        for entry in glob::glob(&pattern_str)? {
            let path = entry?;
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            let template = std::fs::read_to_string(&path)?;
            let description = extract_description(&template)
                .unwrap_or_else(|| format!("Custom command: {}", name));

            self.register(Command {
                name: name.clone(),
                description,
                template,
                scheduler_profile: None,
                source: CommandSource::File(path),
            });
        }

        Ok(())
    }

    pub fn parse(&self, input: &str) -> Option<(&Command, Vec<String>)> {
        let input = input.trim_start();

        if !input.starts_with('/') {
            return None;
        }

        let input = &input[1..];
        let parts: Vec<&str> = input.split_whitespace().collect();

        if parts.is_empty() {
            return None;
        }

        let name = parts[0];
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

        self.commands.get(name).map(|cmd| (cmd, args))
    }

    /// Execute a command and return the rendered template
    pub fn execute(&self, name: &str, ctx: CommandContext) -> anyhow::Result<String> {
        let command = self
            .commands
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Command not found: {}", name))?;

        self.render_command(command, ctx)
    }

    /// Execute a command with plugin hooks (async version)
    pub async fn execute_with_hooks(
        &self,
        name: &str,
        ctx: CommandContext,
    ) -> anyhow::Result<String> {
        let command = self
            .commands
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Command not found: {}", name))?;

        let mut rendered = self.render_command(command, ctx.clone())?;

        // Plugin hook: command.execute.before
        let hook_outputs = rocode_plugin::trigger_collect(
            HookContext::new(HookEvent::CommandExecuteBefore)
                .with_data("command", serde_json::json!(name))
                .with_data("source", serde_json::json!(format!("{:?}", command.source)))
                .with_data("arguments", serde_json::json!(ctx.arguments.join(" ")))
                .with_data(
                    "parts",
                    serde_json::json!([{
                        "type": "text",
                        "text": rendered
                    }]),
                ),
        )
        .await;
        for output in hook_outputs {
            let Some(payload) = output.payload.as_ref() else {
                continue;
            };
            apply_command_hook_payload(&mut rendered, payload);
        }

        Ok(rendered)
    }

    fn render_command(&self, command: &Command, ctx: CommandContext) -> anyhow::Result<String> {
        let rendered = self.render_template(&command.template, ctx.clone());
        match command.name.as_str() {
            "start-work" => start_work::render(rendered, &ctx),
            _ => Ok(rendered),
        }
    }

    fn render_template(&self, template: &str, ctx: CommandContext) -> String {
        let mut result = template.to_string();

        for (i, arg) in ctx.arguments.iter().enumerate() {
            let placeholder = format!("${}", i + 1);
            result = result.replace(&placeholder, arg);
        }

        let all_args = ctx.arguments.join(" ");
        result = result.replace("$ARGUMENTS", &all_args);

        for (key, value) in &ctx.variables {
            let placeholder = format!("${{{}}}", key);
            result = result.replace(&placeholder, value);
        }

        for (key, value) in std::env::vars() {
            let placeholder = format!("$ENV_{}", key);
            result = result.replace(&placeholder, &value);
        }

        result
    }

    pub fn ui_command(&self, action_id: UiActionId) -> Option<&UiCommandSpec> {
        self.ui_command_by_action
            .get(&action_id)
            .and_then(|idx| self.ui_commands.get(*idx))
    }

    pub fn ui_commands(&self) -> &[UiCommandSpec] {
        &self.ui_commands
    }

    pub fn ui_palette_commands(&self) -> Vec<&UiCommandSpec> {
        self.ui_commands
            .iter()
            .filter(|command| command.include_in_palette)
            .collect()
    }

    pub fn ui_slash_command(&self, name: &str) -> Option<&UiCommandSpec> {
        self.ui_slash_aliases
            .get(name)
            .and_then(|action_id| self.ui_command(*action_id))
    }

    pub fn ui_all_slash_commands(&self) -> Vec<&UiCommandSpec> {
        self.ui_commands
            .iter()
            .filter(|command| command.slash.is_some())
            .collect()
    }

    pub fn ui_suggested_slash_commands(&self) -> Vec<&UiCommandSpec> {
        self.ui_commands
            .iter()
            .filter(|command| command.slash.as_ref().is_some_and(|slash| slash.suggested))
            .collect()
    }

    pub fn resolve_ui_slash_input(&self, input: &str) -> Option<ResolvedUiCommand> {
        let trimmed = input.trim();
        if !trimmed.starts_with('/') {
            return None;
        }

        let body = trimmed[1..].trim();
        if body.is_empty() {
            return None;
        }

        let mut parts = body.split_whitespace();
        let name = parts.next()?.to_ascii_lowercase();
        let argument = parts.collect::<Vec<_>>().join(" ");
        let action_id = self.ui_slash_aliases.get(&format!("/{}", name))?;
        let command = self.ui_command(*action_id)?;
        let argument_kind = command.argument_kind();
        let argument = (!argument.trim().is_empty()).then_some(argument.trim().to_string());
        if matches!(argument_kind, UiCommandArgumentKind::None) && argument.is_some() {
            return None;
        }

        Some(ResolvedUiCommand {
            action_id: command.action_id,
            argument_kind,
            argument,
        })
    }
}

fn command_payload_object(
    payload: &serde_json::Value,
) -> Option<&serde_json::Map<String, serde_json::Value>> {
    payload
        .get("output")
        .and_then(|value| value.as_object())
        .or_else(|| payload.as_object())
        .or_else(|| payload.get("data").and_then(|value| value.as_object()))
}

fn apply_command_hook_payload(rendered: &mut String, payload: &serde_json::Value) {
    let Some(object) = command_payload_object(payload) else {
        return;
    };

    #[derive(Debug, Default, Deserialize)]
    struct CommandHookPartWire {
        #[serde(
            default,
            rename = "type",
            deserialize_with = "rocode_types::deserialize_opt_string_lossy"
        )]
        kind: Option<String>,
        #[serde(
            default,
            deserialize_with = "rocode_types::deserialize_opt_string_lossy"
        )]
        text: Option<String>,
    }

    fn deserialize_parts_lossy<'de, D>(
        deserializer: D,
    ) -> Result<Vec<CommandHookPartWire>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Option::<serde_json::Value>::deserialize(deserializer)?;
        let Some(serde_json::Value::Array(values)) = value else {
            return Ok(Vec::new());
        };
        Ok(values
            .into_iter()
            .filter_map(|value| serde_json::from_value::<CommandHookPartWire>(value).ok())
            .collect())
    }

    #[derive(Debug, Default, Deserialize)]
    struct CommandHookPayloadWire {
        #[serde(
            default,
            deserialize_with = "rocode_types::deserialize_opt_string_lossy"
        )]
        output: Option<String>,
        #[serde(
            default,
            deserialize_with = "rocode_types::deserialize_opt_string_lossy"
        )]
        template: Option<String>,
        #[serde(default, deserialize_with = "deserialize_parts_lossy")]
        parts: Vec<CommandHookPartWire>,
    }

    let wire = serde_json::to_value(object)
        .ok()
        .and_then(|value| serde_json::from_value::<CommandHookPayloadWire>(value).ok())
        .unwrap_or_default();

    if let Some(text) = wire.output.or(wire.template) {
        if !text.is_empty() {
            *rendered = text;
        }
        return;
    }

    let text = wire
        .parts
        .into_iter()
        .filter(|part| part.kind.as_deref() == Some("text"))
        .filter_map(|part| part.text)
        .collect::<Vec<_>>()
        .join("\n");

    if !text.is_empty() {
        *rendered = text;
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract description from markdown (first line after # if present)
fn extract_description(content: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            return Some(trimmed.trim_start_matches('#').trim().to_string());
        }
        if !trimmed.is_empty() && !trimmed.starts_with("<!--") {
            return Some(format!(
                "Command: {}",
                trimmed.chars().take(50).collect::<String>()
            ));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_command() {
        let registry = CommandRegistry::new();

        let result = registry.parse("/init my-project");
        assert!(result.is_some());

        let (cmd, args) = result.unwrap();
        assert_eq!(cmd.name, "init");
        assert_eq!(args, vec!["my-project"]);
    }

    #[test]
    fn test_render_template() {
        let registry = CommandRegistry::new();
        let ctx = CommandContext::new(PathBuf::from("/tmp"))
            .with_arguments(vec!["arg1".to_string(), "arg2".to_string()])
            .with_variable("PROJECT".to_string(), "test-project".to_string());

        let result = registry.render_template("Hello $1 and $2. Project: ${PROJECT}", ctx);
        assert_eq!(result, "Hello arg1 and arg2. Project: test-project");
    }

    #[test]
    fn start_work_builtin_sets_atlas_profile() {
        let registry = CommandRegistry::new();
        let command = registry.get("start-work").unwrap();
        assert_eq!(command.scheduler_profile.as_deref(), Some("atlas"));
    }

    #[test]
    fn ui_command_aliases_resolve_to_same_action() {
        let registry = CommandRegistry::new();
        let abort = registry.ui_slash_command("/abort").expect("abort command");
        let primary = registry
            .ui_slash_command("/command")
            .expect("primary slash command");
        let alias = registry.ui_slash_command("/palette").expect("alias");
        assert_eq!(abort.action_id, UiActionId::AbortExecution);
        assert_eq!(primary.action_id, UiActionId::ToggleCommandPalette);
        assert_eq!(alias.action_id, UiActionId::ToggleCommandPalette);

        let preset = registry
            .ui_slash_command("/preset")
            .expect("preset command");
        let agent = registry.ui_slash_command("/agent").expect("agent command");
        let mode = registry.ui_slash_command("/mode").expect("mode command");
        assert_eq!(preset.action_id, UiActionId::OpenPresetList);
        assert_eq!(agent.action_id, UiActionId::OpenAgentList);
        assert_eq!(mode.action_id, UiActionId::OpenModeList);
    }

    #[test]
    fn ui_palette_commands_include_prompt_submission() {
        let registry = CommandRegistry::new();
        assert!(registry
            .ui_palette_commands()
            .iter()
            .any(|command| command.action_id == UiActionId::SubmitPrompt));
    }

    #[test]
    fn ui_command_argument_kinds_match_shared_semantics() {
        let registry = CommandRegistry::new();
        let model = registry.ui_slash_command("/model").expect("model command");
        let preset = registry
            .ui_slash_command("/preset")
            .expect("preset command");
        let sessions = registry
            .ui_slash_command("/session")
            .expect("session command");
        let copy = registry.ui_slash_command("/copy").expect("copy command");

        assert_eq!(model.argument_kind(), UiCommandArgumentKind::ModelRef);
        assert_eq!(preset.argument_kind(), UiCommandArgumentKind::PresetRef);
        assert_eq!(
            sessions.argument_kind(),
            UiCommandArgumentKind::SessionTarget
        );
        assert_eq!(copy.argument_kind(), UiCommandArgumentKind::None);
    }

    #[test]
    fn resolve_ui_slash_input_returns_action_and_argument() {
        let registry = CommandRegistry::new();
        let resolved = registry
            .resolve_ui_slash_input("/model openai/gpt-5")
            .expect("resolved command");

        assert_eq!(resolved.action_id, UiActionId::OpenModelList);
        assert_eq!(resolved.argument_kind, UiCommandArgumentKind::ModelRef);
        assert_eq!(resolved.argument.as_deref(), Some("openai/gpt-5"));
    }

    #[test]
    fn resolve_ui_slash_input_rejects_stray_arguments_for_non_parameterized_actions() {
        let registry = CommandRegistry::new();

        assert_eq!(registry.resolve_ui_slash_input("/rename demo"), None);
        assert_eq!(registry.resolve_ui_slash_input("/copy extra"), None);
    }
}
