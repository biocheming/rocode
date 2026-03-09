use std::collections::{BTreeSet, HashMap};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rocode_agent::{
    AgentExecutor, AgentInfo, AgentMessage, AgentRegistry, Conversation, MessageRole,
    PersistedSubsessionState,
};
use rocode_command::cli_markdown::MarkdownStreamer;
use rocode_command::cli_permission::build_cli_permission_callback;
use rocode_command::cli_prompt::{read_prompt_line, PromptFrame, PromptHistory, PromptResult};
use rocode_command::cli_select::{
    interactive_multi_select, interactive_select, SelectOption, SelectResult,
};
use rocode_command::cli_spinner::Spinner;
use rocode_command::cli_style::CliStyle;
use rocode_command::interactive::{parse_interactive_command, InteractiveCommand};
use rocode_command::output_blocks::{
    render_cli_block_rich, MessagePhase, OutputBlock, StatusBlock,
};
use rocode_command::{CommandContext, CommandRegistry};
use rocode_config::loader::load_config;
use rocode_config::{Config, SkillTreeNodeConfig};
use rocode_core::agent_task_registry::{global_task_registry, AgentTaskStatus};
use rocode_orchestrator::{
    resolve_skill_markdown_repo, scheduler_plan_from_profile, scheduler_request_defaults_from_plan,
    SchedulerConfig, SchedulerPresetKind, SchedulerProfileConfig, SchedulerRequestDefaults,
    SkillTreeNode, SkillTreeRequestPlan,
};
use rocode_session::system::{EnvironmentContext, SystemPrompt};
use rocode_tool::registry::create_default_registry;

use crate::agent_stream_adapter::stream_prompt_to_blocks;
use crate::cli::RunOutputFormat;
use crate::providers::{setup_providers, show_help};
use crate::remote::run_non_interactive_attach;
use crate::util::{
    append_cli_file_attachments, collect_run_input, parse_model_and_provider, truncate_text,
};

fn to_orchestrator_skill_tree(node: &SkillTreeNodeConfig) -> SkillTreeNode {
    SkillTreeNode {
        node_id: node.node_id.clone(),
        markdown_path: node.markdown_path.clone(),
        children: node
            .children
            .iter()
            .map(to_orchestrator_skill_tree)
            .collect(),
    }
}

fn resolve_request_skill_tree_plan(
    config: &Config,
    scheduler_defaults: Option<&SchedulerRequestDefaults>,
) -> Option<SkillTreeRequestPlan> {
    if let Some(plan) = scheduler_defaults.and_then(|defaults| defaults.skill_tree_plan.clone()) {
        return Some(plan);
    }

    let skill_tree = config.composition.as_ref()?.skill_tree.as_ref()?;
    if matches!(skill_tree.enabled, Some(false)) {
        return None;
    }

    let root = skill_tree.root.as_ref()?;
    let root = to_orchestrator_skill_tree(root);
    let markdown_repo = resolve_skill_markdown_repo(&config.skill_paths);

    match SkillTreeRequestPlan::from_tree_with_separator(
        &root,
        &markdown_repo,
        skill_tree.separator.as_deref(),
    ) {
        Ok(plan) => plan,
        Err(error) => {
            tracing::warn!(%error, "failed to build request skill tree plan");
            None
        }
    }
}

fn resolve_requested_agent_name(
    config: &Config,
    requested_agent: Option<&str>,
    scheduler_defaults: Option<&SchedulerRequestDefaults>,
) -> String {
    if let Some(agent) = requested_agent
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return agent.to_string();
    }

    if let Some(agent) = scheduler_defaults.and_then(|defaults| defaults.root_agent_name.clone()) {
        return agent;
    }

    if let Some(agent) = config
        .default_agent
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return agent.to_string();
    }

    "build".to_string()
}
pub(crate) async fn run_non_interactive(
    message: Vec<String>,
    command: Option<String>,
    continue_last: bool,
    session: Option<String>,
    fork: bool,
    share: bool,
    model: Option<String>,
    requested_agent: Option<String>,
    requested_scheduler_profile: Option<String>,
    files: Vec<PathBuf>,
    format: RunOutputFormat,
    title: Option<String>,
    attach: Option<String>,
    dir: Option<PathBuf>,
    _port: Option<u16>,
    variant: Option<String>,
    _thinking: bool,
) -> anyhow::Result<()> {
    if let Some(dir) = dir {
        std::env::set_current_dir(&dir).map_err(|e| {
            anyhow::anyhow!("Failed to change directory to {}: {}", dir.display(), e)
        })?;
    }

    if fork && !continue_last && session.is_none() {
        anyhow::bail!("--fork requires --continue or --session");
    }

    let mut input = collect_run_input(message)?;
    append_cli_file_attachments(&mut input, &files)?;

    if let Some(base_url) = attach {
        return run_non_interactive_attach(
            base_url,
            input,
            command,
            continue_last,
            session,
            fork,
            share,
            model,
            requested_scheduler_profile,
            variant,
            format,
            title,
        )
        .await;
    }

    if continue_last || session.is_some() || fork || share {
        println!(
            "Note: session/share flags are currently applied when using `run --attach <server>`."
        );
    }

    if let Some(command_name) = command {
        let cwd = std::env::current_dir()?;
        let mut registry = CommandRegistry::new();
        let _ = registry.load_from_directory(&cwd);
        let args = if input.trim().is_empty() {
            Vec::new()
        } else {
            input
                .split_whitespace()
                .map(|part| part.to_string())
                .collect::<Vec<_>>()
        };
        let rendered =
            registry.execute(&command_name, CommandContext::new(cwd).with_arguments(args))?;
        input = rendered;
    }

    if input.trim().is_empty() {
        let (provider, model_id) = parse_model_and_provider(model);
        return run_chat_session(
            model_id,
            provider,
            requested_agent,
            requested_scheduler_profile,
            None,
            false,
        )
        .await;
    }

    let (provider, model_id) = parse_model_and_provider(model);
    run_chat_session(
        model_id,
        provider,
        requested_agent,
        requested_scheduler_profile,
        Some(input.clone()),
        true,
    )
    .await?;

    match format {
        RunOutputFormat::Default => {
            println!("{}", input);
        }
        RunOutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "message": input,
                    "format": "json",
                    "title": title,
                })
            );
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Default)]
struct CliRunSelection {
    model: Option<String>,
    provider: Option<String>,
    requested_agent: Option<String>,
    requested_scheduler_profile: Option<String>,
}

struct CliExecutionRuntime {
    executor: AgentExecutor,
    resolved_agent_name: String,
    resolved_scheduler_profile_name: Option<String>,
    resolved_model_label: String,
}

#[derive(Debug, Clone, Default)]
struct CliSchedulerResolution {
    defaults: Option<SchedulerRequestDefaults>,
    profile_model: Option<(String, String)>,
}

fn resolve_scheduler_profile_config(
    config: &Config,
    requested_scheduler_profile: Option<&str>,
) -> Option<(String, SchedulerProfileConfig)> {
    let requested = requested_scheduler_profile
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let scheduler_path = config
        .scheduler_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if let Some(path) = scheduler_path {
        let scheduler_config = match SchedulerConfig::load_from_file(path) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(path = %path, %error, "failed to load scheduler config");
                return requested.and_then(|name| {
                    SchedulerPresetKind::from_str(name).ok().map(|_| {
                        (
                            name.to_string(),
                            SchedulerProfileConfig {
                                orchestrator: Some(name.to_string()),
                                ..Default::default()
                            },
                        )
                    })
                });
            }
        };

        if let Some(name) = requested {
            if let Ok(profile) = scheduler_config.profile(name) {
                return Some((name.to_string(), profile.clone()));
            }
            return SchedulerPresetKind::from_str(name).ok().map(|_| {
                (
                    name.to_string(),
                    SchedulerProfileConfig {
                        orchestrator: Some(name.to_string()),
                        ..Default::default()
                    },
                )
            });
        }

        if let Some(name) = scheduler_config.default_profile_key() {
            if let Ok(profile) = scheduler_config.profile(name) {
                return Some((name.to_string(), profile.clone()));
            }
        }
        return None;
    }

    requested.and_then(|name| {
        SchedulerPresetKind::from_str(name).ok().map(|_| {
            (
                name.to_string(),
                SchedulerProfileConfig {
                    orchestrator: Some(name.to_string()),
                    ..Default::default()
                },
            )
        })
    })
}

fn resolve_scheduler_runtime(
    config: &Config,
    requested_scheduler_profile: Option<&str>,
) -> CliSchedulerResolution {
    let Some((profile_name, profile)) =
        resolve_scheduler_profile_config(config, requested_scheduler_profile)
    else {
        return CliSchedulerResolution::default();
    };

    let defaults = scheduler_plan_from_profile(Some(profile_name.clone()), &profile)
        .ok()
        .map(|plan| scheduler_request_defaults_from_plan(&plan));
    let profile_model = profile
        .model
        .as_ref()
        .map(|model| (model.provider_id.clone(), model.model_id.clone()));

    CliSchedulerResolution {
        defaults,
        profile_model,
    }
}

fn apply_system_prompt_to_conversation(
    conversation: Option<Conversation>,
    system_prompt: &str,
) -> Conversation {
    let mut conversation = conversation.unwrap_or_else(Conversation::new);
    if let Some(first) = conversation.messages.first_mut() {
        if matches!(first.role, MessageRole::System) {
            first.content = system_prompt.to_string();
            return conversation;
        }
    }
    conversation
        .messages
        .insert(0, AgentMessage::system(system_prompt.to_string()));
    conversation
}

fn compose_executor_system_prompt(agent_info: &AgentInfo, current_dir: &Path) -> String {
    let (model_api_id, provider_id) = match &agent_info.model {
        Some(m) => (m.model_id.clone(), m.provider_id.clone()),
        None => (
            "claude-sonnet-4-20250514".to_string(),
            "anthropic".to_string(),
        ),
    };
    let mut sections = Vec::new();
    if let Some(agent_prompt) = agent_info.resolved_system_prompt() {
        if !agent_prompt.trim().is_empty() {
            sections.push(agent_prompt);
        }
    }
    sections.push(SystemPrompt::for_model(&model_api_id).to_string());
    let env_ctx = EnvironmentContext::from_current(
        &model_api_id,
        &provider_id,
        current_dir.to_string_lossy().as_ref(),
    );
    sections.push(SystemPrompt::environment(&env_ctx));
    sections.join("\n\n")
}

async fn build_cli_execution_runtime(
    config: &Config,
    current_dir: &Path,
    provider_registry: Arc<rocode_provider::ProviderRegistry>,
    tool_registry: Arc<rocode_tool::registry::ToolRegistry>,
    agent_registry: Arc<AgentRegistry>,
    selection: &CliRunSelection,
    prior_conversation: Option<Conversation>,
    prior_subsessions: Option<HashMap<String, PersistedSubsessionState>>,
) -> anyhow::Result<CliExecutionRuntime> {
    let scheduler_resolution =
        resolve_scheduler_runtime(config, selection.requested_scheduler_profile.as_deref());
    let scheduler_defaults = scheduler_resolution.defaults.clone();
    let scheduler_profile_name = scheduler_defaults
        .as_ref()
        .and_then(|defaults| defaults.profile_name.clone());
    let scheduler_root_agent = scheduler_defaults
        .as_ref()
        .and_then(|defaults| defaults.root_agent_name.clone());
    let request_skill_tree_plan =
        resolve_request_skill_tree_plan(config, scheduler_defaults.as_ref());
    let agent_name = resolve_requested_agent_name(
        config,
        selection.requested_agent.as_deref(),
        scheduler_defaults.as_ref(),
    );

    let mut agent_info = agent_registry
        .get(&agent_name)
        .cloned()
        .unwrap_or_else(AgentInfo::build);

    if let Some(ref model_id) = selection.model {
        let provider_id = selection.provider.clone().unwrap_or_else(|| {
            if model_id.starts_with("claude") {
                "anthropic".to_string()
            } else {
                "openai".to_string()
            }
        });
        agent_info = agent_info.with_model(model_id.clone(), provider_id);
    } else if let Some((provider_id, model_id)) = scheduler_resolution.profile_model.clone() {
        agent_info = agent_info.with_model(model_id, provider_id);
    }

    let resolved_model_label = agent_info
        .model
        .as_ref()
        .map(|m| format!("{}/{}", m.provider_id, m.model_id))
        .unwrap_or_else(|| "auto".to_string());

    let mut executor = AgentExecutor::new(
        agent_info.clone(),
        provider_registry,
        tool_registry,
        agent_registry.clone(),
    )
    .with_tool_runtime_config(rocode_tool::ToolRuntimeConfig::from_config(config))
    .with_ask_question(cli_ask_question)
    .with_ask_permission(build_cli_permission_callback());

    if let Some(states) = prior_subsessions {
        executor = executor.with_persisted_subsessions(states);
    }

    let full_prompt = compose_executor_system_prompt(&agent_info, current_dir);
    let conversation = apply_system_prompt_to_conversation(prior_conversation, &full_prompt);
    *executor.conversation_mut() = conversation;

    if let Some(plan) = request_skill_tree_plan {
        executor = executor.with_request_skill_tree_plan(plan);
    }

    tracing::info!(
        requested_agent = ?selection.requested_agent,
        requested_scheduler_profile = ?selection.requested_scheduler_profile,
        resolved_agent = %agent_name,
        scheduler_profile = ?scheduler_profile_name,
        scheduler_root_agent = ?scheduler_root_agent,
        resolved_model = %resolved_model_label,
        "resolved cli runtime execution configuration"
    );

    Ok(CliExecutionRuntime {
        executor,
        resolved_agent_name: agent_name,
        resolved_scheduler_profile_name: scheduler_profile_name,
        resolved_model_label,
    })
}

fn cli_available_presets(config: &Config) -> Vec<String> {
    let mut names = BTreeSet::new();
    for preset in SchedulerPresetKind::public_presets() {
        names.insert(preset.as_str().to_string());
    }

    if let Some(path) = config
        .scheduler_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Ok(scheduler_config) = SchedulerConfig::load_from_file(path) {
            for name in scheduler_config.profiles.keys() {
                names.insert(name.clone());
            }
        }
    }

    names.into_iter().collect()
}

fn cli_list_presets(config: &Config, active_profile: Option<&str>) {
    let style = CliStyle::detect();
    println!(
        "\n  {} {}\n",
        style.bold_cyan(style.bullet()),
        style.bold("Available Presets")
    );
    for preset in cli_available_presets(config) {
        let active = if active_profile == Some(preset.as_str()) {
            " ← active"
        } else {
            ""
        };
        println!("    {}{}", preset, active);
    }
    println!();
}

fn cli_has_preset(config: &Config, name: &str) -> bool {
    cli_available_presets(config)
        .iter()
        .any(|preset| preset.eq_ignore_ascii_case(name))
}

fn cli_switch_message(kind: &str, value: &str) {
    let style = CliStyle::detect();
    println!(
        "  {} Switched {} to {}.",
        style.bold_cyan(style.bullet()),
        kind,
        value
    );
}

fn cli_prompt_frame(runtime: &CliExecutionRuntime, style: &CliStyle) -> PromptFrame {
    let mode_label = match runtime.resolved_scheduler_profile_name.as_deref() {
        Some(profile) => format!("Preset {}", profile),
        None => format!("Agent {}", runtime.resolved_agent_name),
    };
    let model_label = format!("Model {}", runtime.resolved_model_label);
    PromptFrame::boxed(&mode_label, &model_label, style)
}

fn cli_mode_summary(runtime: &CliExecutionRuntime) -> (String, &'static str) {
    match runtime.resolved_scheduler_profile_name.as_deref() {
        Some(profile) => (format!("preset {}", profile), "/preset to change"),
        None => (
            format!("agent {}", runtime.resolved_agent_name),
            "/agent to change",
        ),
    }
}

fn display_path_for_cli(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(stripped) = path.strip_prefix(&home) {
            let suffix = stripped.display().to_string();
            return if suffix.is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", suffix)
            };
        }
    }
    path.display().to_string()
}

fn pad_cli_card_text(text: &str, width: usize) -> String {
    let visible = text.chars().count();
    if visible >= width {
        text.to_string()
    } else {
        format!("{}{}", text, " ".repeat(width - visible))
    }
}

fn format_card_line(label: &str, value: &str, hint: Option<&str>, width: usize) -> String {
    let left = format!("{} {}", label, value);
    match hint {
        Some(hint_text) => {
            let left_len = left.chars().count();
            let hint_len = hint_text.chars().count();
            if left_len + hint_len + 1 <= width {
                format!(
                    "{}{}{}",
                    left,
                    " ".repeat(width - left_len - hint_len),
                    hint_text
                )
            } else {
                pad_cli_card_text(&truncate_text(&left, width), width)
            }
        }
        None => pad_cli_card_text(&truncate_text(&left, width), width),
    }
}

fn render_cli_welcome_card(
    style: &CliStyle,
    current_dir: &Path,
    runtime: &CliExecutionRuntime,
    prompt_frame: &PromptFrame,
) {
    let version = env!("CARGO_PKG_VERSION");
    let title = format!(">_ ROCode ({})", version);
    let (mode_value, mode_hint) = cli_mode_summary(runtime);
    let directory = display_path_for_cli(current_dir);

    let inner_width = prompt_frame.content_width();

    let top_border = if style.color {
        format!(
            "{}{}{}",
            style.cyan("╭"),
            style.cyan(&"─".repeat(inner_width + 2)),
            style.cyan("╮")
        )
    } else {
        format!("╭{}╮", "─".repeat(inner_width + 2))
    };
    let bottom_border = if style.color {
        format!(
            "{}{}{}",
            style.cyan("╰"),
            style.cyan(&"─".repeat(inner_width + 2)),
            style.cyan("╯")
        )
    } else {
        format!("╰{}╯", "─".repeat(inner_width + 2))
    };
    let title_row = if style.color {
        format!(
            "{} {} {}",
            style.cyan("│"),
            style.bold_cyan(&pad_cli_card_text(&title, inner_width)),
            style.cyan("│")
        )
    } else {
        format!("│ {} │", pad_cli_card_text(&title, inner_width))
    };
    let empty_row = if style.color {
        format!(
            "{} {} {}",
            style.cyan("│"),
            " ".repeat(inner_width),
            style.cyan("│")
        )
    } else {
        format!("│ {} │", " ".repeat(inner_width))
    };

    println!();
    println!("{}", top_border);
    println!("{}", title_row);
    println!("{}", empty_row);
    for row in [
        format_card_line("mode:", &mode_value, Some(mode_hint), inner_width),
        format_card_line(
            "model:",
            &runtime.resolved_model_label,
            Some("/model to change"),
            inner_width,
        ),
        format_card_line("directory:", &directory, None, inner_width),
    ] {
        if style.color {
            println!("{} {} {}", style.cyan("│"), row, style.cyan("│"));
        } else {
            println!("│ {} │", row);
        }
    }
    println!("{}", bottom_border);
    println!();
    if style.color {
        println!(
            "  {}",
            style.dim("Tip: Use /help, /agent, /preset, or /model to adjust the session.")
        );
    } else {
        println!("  Tip: Use /help, /agent, /preset, or /model to adjust the session.");
    }
    println!();
}

async fn run_chat_session(
    model: Option<String>,
    provider: Option<String>,
    requested_agent: Option<String>,
    requested_scheduler_profile: Option<String>,
    initial_prompt: Option<String>,
    single_shot: bool,
) -> anyhow::Result<()> {
    let current_dir = std::env::current_dir()?;
    let config = load_config(&current_dir)?;
    let provider_registry = Arc::new(setup_providers(&config).await?);

    if provider_registry.list().is_empty() {
        eprintln!("Error: No API keys configured.");
        println!("Set one of the following environment variables:");
        eprintln!("  - ANTHROPIC_API_KEY");
        eprintln!("  - OPENAI_API_KEY");
        eprintln!("  - OPENROUTER_API_KEY");
        eprintln!("  - GOOGLE_API_KEY");
        eprintln!("  - MISTRAL_API_KEY");
        eprintln!("  - GROQ_API_KEY");
        eprintln!("  - XAI_API_KEY");
        eprintln!("  - DEEPSEEK_API_KEY");
        eprintln!("  - COHERE_API_KEY");
        eprintln!("  - TOGETHER_API_KEY");
        eprintln!("  - PERPLEXITY_API_KEY");
        eprintln!("  - CEREBRAS_API_KEY");
        eprintln!("  - DEEPINFRA_API_KEY");
        eprintln!("  - VERCEL_API_KEY");
        eprintln!("  - GITLAB_TOKEN");
        eprintln!("  - GITHUB_COPILOT_TOKEN");
        eprintln!("  - GOOGLE_VERTEX_API_KEY + GOOGLE_VERTEX_PROJECT_ID + GOOGLE_VERTEX_LOCATION");
        std::process::exit(1);
    }

    let tool_registry = Arc::new(create_default_registry().await);
    let agent_registry_arc = Arc::new(AgentRegistry::from_config(&config));
    let mut selection = CliRunSelection {
        model,
        provider,
        requested_agent,
        requested_scheduler_profile,
    };

    let mut runtime = build_cli_execution_runtime(
        &config,
        &current_dir,
        provider_registry.clone(),
        tool_registry.clone(),
        agent_registry_arc.clone(),
        &selection,
        None,
        None,
    )
    .await?;
    let repl_style = CliStyle::detect();
    let initial_prompt_frame = cli_prompt_frame(&runtime, &repl_style);
    render_cli_welcome_card(&repl_style, &current_dir, &runtime, &initial_prompt_frame);

    if let Some(prompt_text) = initial_prompt {
        println!("User: {}", prompt_text);
        process_message(&mut runtime.executor, &prompt_text).await?;
        if single_shot {
            return Ok(());
        }
    }

    let mut prompt_history = PromptHistory::new(200);

    loop {
        let prompt_frame = cli_prompt_frame(&runtime, &repl_style);
        let result = read_prompt_line(&prompt_frame, &prompt_history, &repl_style)?;
        let trimmed = match result {
            PromptResult::Line(ref s) => s.trim().to_string(),
            PromptResult::Eof => break,
            PromptResult::Interrupt => continue,
        };

        if trimmed.is_empty() {
            continue;
        }

        if let Some(cmd) = parse_interactive_command(&trimmed) {
            match cmd {
                InteractiveCommand::Exit => break,
                InteractiveCommand::ShowHelp => {
                    show_help();
                }
                InteractiveCommand::ClearScreen => {
                    print!("\x1B[2J\x1B[1;1H");
                    io::stdout().flush()?;
                }
                InteractiveCommand::NewSession => {
                    println!("  ⚠  /new is not yet supported in CLI mode. Use exit and restart.");
                }
                InteractiveCommand::ShowStatus => {
                    let style = CliStyle::detect();
                    println!(
                        "  {} {}",
                        style.bold_cyan(style.bullet()),
                        style.bold("Session Status")
                    );
                    println!("    Agent:     {}", runtime.resolved_agent_name);
                    println!("    Model:     {}", runtime.resolved_model_label);
                    println!("    Directory: {}", current_dir.display());
                    if let Some(ref profile) = runtime.resolved_scheduler_profile_name {
                        println!("    Scheduler: {}", profile);
                    }
                }
                InteractiveCommand::ListModels => {
                    let style = CliStyle::detect();
                    println!(
                        "\n  {} {}\n",
                        style.bold_cyan(style.bullet()),
                        style.bold("Available Models")
                    );
                    for p in provider_registry.list() {
                        for m in p.models() {
                            println!("    {}:{}", p.id(), m.id);
                        }
                    }
                    println!();
                }
                InteractiveCommand::SelectModel(model_id) => {
                    let (provider, model) = parse_model_and_provider(Some(model_id.clone()));
                    if model.is_none() {
                        println!("  ⚠  Invalid model selector: {}", model_id);
                        continue;
                    }
                    let prior_conversation = Some(runtime.executor.conversation().clone());
                    let prior_subsessions = Some(runtime.executor.export_subsessions().await);
                    selection.provider = provider;
                    selection.model = model;
                    runtime = build_cli_execution_runtime(
                        &config,
                        &current_dir,
                        provider_registry.clone(),
                        tool_registry.clone(),
                        agent_registry_arc.clone(),
                        &selection,
                        prior_conversation,
                        prior_subsessions,
                    )
                    .await?;
                    cli_switch_message("model", &runtime.resolved_model_label);
                }
                InteractiveCommand::ListProviders => {
                    let style = CliStyle::detect();
                    println!(
                        "\n  {} {}\n",
                        style.bold_cyan(style.bullet()),
                        style.bold("Configured Providers")
                    );
                    for p in provider_registry.list() {
                        let model_count = p.models().len();
                        println!(
                            "    {} ({} model{})",
                            p.id(),
                            model_count,
                            if model_count != 1 { "s" } else { "" }
                        );
                    }
                    println!();
                }
                InteractiveCommand::ListThemes => {
                    println!("  ⚠  Theme switching is not yet supported in CLI mode.");
                }
                InteractiveCommand::ListPresets => {
                    cli_list_presets(&config, runtime.resolved_scheduler_profile_name.as_deref());
                }
                InteractiveCommand::SelectPreset(name) => {
                    if !cli_has_preset(&config, &name) {
                        println!("  ⚠  Unknown preset: {}", name);
                        cli_list_presets(
                            &config,
                            runtime.resolved_scheduler_profile_name.as_deref(),
                        );
                        continue;
                    }
                    let prior_conversation = Some(runtime.executor.conversation().clone());
                    let prior_subsessions = Some(runtime.executor.export_subsessions().await);
                    selection.requested_scheduler_profile = Some(name.clone());
                    selection.requested_agent = None;
                    selection.model = None;
                    selection.provider = None;
                    runtime = build_cli_execution_runtime(
                        &config,
                        &current_dir,
                        provider_registry.clone(),
                        tool_registry.clone(),
                        agent_registry_arc.clone(),
                        &selection,
                        prior_conversation,
                        prior_subsessions,
                    )
                    .await?;
                    cli_switch_message(
                        "preset",
                        runtime
                            .resolved_scheduler_profile_name
                            .as_deref()
                            .unwrap_or(name.as_str()),
                    );
                }
                InteractiveCommand::ListSessions => {
                    cli_list_sessions().await;
                }
                InteractiveCommand::ListAgents => {
                    let style = CliStyle::detect();
                    println!(
                        "\n  {} {}\n",
                        style.bold_cyan(style.bullet()),
                        style.bold("Available Agents")
                    );
                    for info in agent_registry_arc.list() {
                        let active = if info.name == runtime.resolved_agent_name {
                            " ← active"
                        } else {
                            ""
                        };
                        let model_info = info
                            .model
                            .as_ref()
                            .map(|m| format!(" ({}/{})", m.provider_id, m.model_id))
                            .unwrap_or_default();
                        println!("    {}{}{}", info.name, model_info, active);
                    }
                    println!();
                }
                InteractiveCommand::SelectAgent(name) => {
                    if agent_registry_arc.get(&name).is_none() {
                        println!("  ⚠  Unknown agent: {}", name);
                        continue;
                    }
                    let prior_conversation = Some(runtime.executor.conversation().clone());
                    let prior_subsessions = Some(runtime.executor.export_subsessions().await);
                    selection.requested_agent = Some(name.clone());
                    selection.requested_scheduler_profile = None;
                    runtime = build_cli_execution_runtime(
                        &config,
                        &current_dir,
                        provider_registry.clone(),
                        tool_registry.clone(),
                        agent_registry_arc.clone(),
                        &selection,
                        prior_conversation,
                        prior_subsessions,
                    )
                    .await?;
                    cli_switch_message("agent", &runtime.resolved_agent_name);
                }
                InteractiveCommand::Compact => {
                    println!("  ⚠  /compact is not yet supported in CLI mode.");
                }
                InteractiveCommand::Copy => {
                    println!("  ⚠  /copy is not yet supported in CLI mode.");
                }
                InteractiveCommand::ListTasks => {
                    cli_list_tasks();
                }
                InteractiveCommand::ShowTask(id) => {
                    cli_show_task(&id);
                }
                InteractiveCommand::KillTask(id) => {
                    cli_kill_task(&id);
                }
                InteractiveCommand::Unknown(name) => {
                    println!(
                        "  Unknown command: /{}. Type /help for available commands.",
                        name
                    );
                }
            }
            continue;
        }

        prompt_history.push(&trimmed);
        process_message(&mut runtime.executor, &trimmed).await?;
    }

    Ok(())
}

async fn process_message(executor: &mut AgentExecutor, input: &str) -> anyhow::Result<()> {
    let style = CliStyle::detect();

    print_block(
        OutputBlock::Status(StatusBlock::title(format!(
            "Prompt: {}",
            truncate_text(input, 72)
        ))),
        &style,
    )?;

    let spinner = Spinner::start("Assistant is generating…", &style);

    let mut md_streamer = MarkdownStreamer::new(&style);
    let stats = stream_prompt_to_blocks(executor, input, |block| {
        // Intercept message deltas for markdown rendering
        match &block {
            OutputBlock::Message(msg) if msg.phase == MessagePhase::Delta => {
                let rendered = md_streamer.push(&msg.text);
                if !rendered.is_empty() {
                    print!("{}", rendered);
                    io::stdout().flush()?;
                }
                Ok(())
            }
            OutputBlock::Message(msg) if msg.phase == MessagePhase::End => {
                // Flush remaining markdown buffer
                let remaining = md_streamer.finish();
                if !remaining.is_empty() {
                    print!("{}", remaining);
                }
                print_block(block, &style)
            }
            _ => print_block(block, &style),
        }
    })
    .await;

    spinner.stop().await;

    let (prompt_tokens, completion_tokens, stream_failed) = match stats {
        Ok(stats) => (stats.prompt_tokens, stats.completion_tokens, false),
        Err(error) => {
            print_block(
                OutputBlock::Status(StatusBlock::error(error.to_string())),
                &style,
            )?;
            (0, 0, true)
        }
    };

    if !stream_failed {
        if prompt_tokens > 0 || completion_tokens > 0 {
            print_block(
                OutputBlock::Status(StatusBlock::success(format!(
                    "Done. tokens: prompt={} completion={}",
                    prompt_tokens, completion_tokens
                ))),
                &style,
            )?;
        } else {
            print_block(OutputBlock::Status(StatusBlock::success("Done.")), &style)?;
        }
    }
    println!();
    Ok(())
}

fn print_block(block: OutputBlock, style: &CliStyle) -> anyhow::Result<()> {
    print!("{}", render_cli_block_rich(&block, style));
    io::stdout().flush()?;
    Ok(())
}

// ── CLI interactive question handler ─────────────────────────────────

async fn cli_ask_question(
    questions: Vec<rocode_tool::QuestionDef>,
) -> Result<Vec<Vec<String>>, rocode_tool::ToolError> {
    let style = CliStyle::detect();
    let mut all_answers = Vec::with_capacity(questions.len());

    for q in &questions {
        let options: Vec<SelectOption> = q
            .options
            .iter()
            .map(|opt| SelectOption {
                label: opt.label.clone(),
                description: opt.description.clone(),
            })
            .collect();

        let result = if options.is_empty() {
            // No options — free text input
            prompt_free_text(&q.question, q.header.as_deref(), &style)
        } else if q.multiple {
            interactive_multi_select(&q.question, q.header.as_deref(), &options, &style)
        } else {
            interactive_select(&q.question, q.header.as_deref(), &options, &style)
        };

        match result {
            Ok(SelectResult::Selected(choices)) => {
                all_answers.push(choices);
            }
            Ok(SelectResult::Other(text)) => {
                all_answers.push(vec![text]);
            }
            Ok(SelectResult::Cancelled) => {
                return Err(rocode_tool::ToolError::ExecutionError(
                    "User cancelled the question".to_string(),
                ));
            }
            Err(e) => {
                return Err(rocode_tool::ToolError::ExecutionError(format!(
                    "Interactive prompt error: {}",
                    e
                )));
            }
        }
    }

    Ok(all_answers)
}

fn prompt_free_text(
    question: &str,
    header: Option<&str>,
    style: &CliStyle,
) -> io::Result<SelectResult> {
    println!();
    if let Some(h) = header {
        println!("  {} {}", style.bold_cyan(style.bullet()), style.bold(h));
    }
    println!("  {}", question);
    eprint!("  {} ", style.bold_cyan("›"));
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let answer = input.trim().to_string();

    if answer.is_empty() {
        Ok(SelectResult::Cancelled)
    } else {
        Ok(SelectResult::Other(answer))
    }
}

// ── CLI agent task handlers ──────────────────────────────────────────

fn cli_list_tasks() {
    let tasks = global_task_registry().list();
    if tasks.is_empty() {
        println!("No agent tasks.");
        return;
    }
    let now = chrono::Utc::now().timestamp();
    for task in &tasks {
        let (icon, status_str) = match &task.status {
            AgentTaskStatus::Pending => ("◯", "pending".to_string()),
            AgentTaskStatus::Running { step } => {
                let steps = task
                    .max_steps
                    .map(|m| format!("{}/{}", step, m))
                    .unwrap_or(format!("{}/?", step));
                ("◐", format!("running  {}", steps))
            }
            AgentTaskStatus::Completed { steps } => ("●", format!("done     {}", steps)),
            AgentTaskStatus::Cancelled => ("✗", "cancelled".to_string()),
            AgentTaskStatus::Failed { .. } => ("✗", "failed".to_string()),
        };
        let elapsed = now - task.started_at;
        let elapsed_str = if elapsed < 60 {
            format!("{}s ago", elapsed)
        } else {
            format!("{}m ago", elapsed / 60)
        };
        println!(
            "  {}  {}  {:<20} {:<16} {}",
            icon, task.id, task.agent_name, status_str, elapsed_str
        );
    }
    let running = tasks
        .iter()
        .filter(|t| matches!(t.status, AgentTaskStatus::Running { .. }))
        .count();
    let done = tasks.iter().filter(|t| t.status.is_terminal()).count();
    println!("{} running, {} finished", running, done);
}

fn cli_show_task(id: &str) {
    match global_task_registry().get(id) {
        Some(task) => {
            let (status_label, step_info) = match &task.status {
                AgentTaskStatus::Pending => ("pending".to_string(), String::new()),
                AgentTaskStatus::Running { step } => {
                    let steps = task
                        .max_steps
                        .map(|m| format!(" (step {}/{})", step, m))
                        .unwrap_or(format!(" (step {}/?)", step));
                    ("running".to_string(), steps)
                }
                AgentTaskStatus::Completed { steps } => {
                    ("completed".to_string(), format!(" ({} steps)", steps))
                }
                AgentTaskStatus::Cancelled => ("cancelled".to_string(), String::new()),
                AgentTaskStatus::Failed { error } => (format!("failed: {}", error), String::new()),
            };
            let now = chrono::Utc::now().timestamp();
            let elapsed = now - task.started_at;
            let elapsed_str = if elapsed < 60 {
                format!("{}s ago", elapsed)
            } else {
                format!("{}m ago", elapsed / 60)
            };
            println!("Task {} — {}", task.id, task.agent_name);
            println!("Status: {}{}", status_label, step_info);
            println!("Started: {}", elapsed_str);
            println!("Prompt: {}", task.prompt);
            if !task.output_tail.is_empty() {
                println!("Recent output:");
                for line in &task.output_tail {
                    println!("  {}", line);
                }
            }
        }
        None => {
            println!("Task \"{}\" not found", id);
        }
    }
}

fn cli_kill_task(id: &str) {
    match rocode_orchestrator::global_lifecycle().cancel_task(id) {
        Ok(()) => println!("✓ Task {} cancelled", id),
        Err(err) => eprintln!("{}", err),
    }
}

// ── CLI session listing ─────────────────────────────────────────────

async fn cli_list_sessions() {
    let style = CliStyle::detect();

    let db = match rocode_storage::Database::new().await {
        Ok(db) => db,
        Err(e) => {
            println!(
                "  {} Failed to open session database: {}",
                style.bold_red("✗"),
                e
            );
            return;
        }
    };

    let session_repo = rocode_storage::SessionRepository::new(db.pool().clone());

    let sessions = match session_repo.list(None, 20).await {
        Ok(sessions) => sessions,
        Err(e) => {
            println!("  {} Failed to list sessions: {}", style.bold_red("✗"), e);
            return;
        }
    };

    if sessions.is_empty() {
        println!(
            "\n  {} {}\n",
            style.dim("○"),
            style.dim("No sessions found.")
        );
        return;
    }

    println!(
        "\n  {} {}\n",
        style.bold_cyan(style.bullet()),
        style.bold("Recent Sessions")
    );

    for session in &sessions {
        let title = if session.title.is_empty() {
            "(untitled)"
        } else {
            &session.title
        };
        let id_short = if session.id.len() > 8 {
            &session.id[..8]
        } else {
            &session.id
        };
        let time_str = format_session_time(session.time.updated);

        println!(
            "    {} {} {}",
            style.dim(id_short),
            title,
            style.dim(&time_str),
        );
    }
    println!();
    println!(
        "  {} Use {} to continue a previous session at startup.",
        style.dim("tip:"),
        style.bold("--continue"),
    );
    println!();
}

fn format_session_time(timestamp: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let elapsed = now - timestamp;
    if elapsed < 0 {
        return "just now".to_string();
    }
    if elapsed < 60 {
        format!("{}s ago", elapsed)
    } else if elapsed < 3600 {
        format!("{}m ago", elapsed / 60)
    } else if elapsed < 86400 {
        format!("{}h ago", elapsed / 3600)
    } else {
        format!("{}d ago", elapsed / 86400)
    }
}
