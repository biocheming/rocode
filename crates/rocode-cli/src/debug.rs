use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::Arc;
use std::time::Duration;

use rocode_agent::AgentRegistry;
use rocode_config::loader::load_config;
use rocode_config::{LspConfig, LspServerConfig as ConfigLspServerConfig};
use rocode_grep::{FileSearchOptions, Ripgrep};
use rocode_lsp::{LspClient, LspServerConfig};
use rocode_session::snapshot::Snapshot;
use rocode_storage::{Database, SessionRepository};
use rocode_tool::skill::list_available_skills;
use rocode_tool::{registry::create_default_registry, ToolContext};

use crate::cli::*;

fn resolve_document_input_to_path(input: &str) -> anyhow::Result<PathBuf> {
    if input.starts_with("file://") {
        let url = url::Url::parse(input)?;
        return url
            .to_file_path()
            .map_err(|_| anyhow::anyhow!("Invalid file URI: {}", input));
    }
    let path = PathBuf::from(input);
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(std::env::current_dir()?.join(path))
}

fn select_lsp_server(
    config: &rocode_config::Config,
    file_hint: Option<&Path>,
) -> anyhow::Result<(String, ConfigLspServerConfig)> {
    let Some(lsp_config) = &config.lsp else {
        anyhow::bail!("No `lsp` configuration found in rocode.json(c).");
    };

    let servers = match lsp_config {
        LspConfig::Disabled(false) => {
            anyhow::bail!("LSP is disabled by config (`\"lsp\": false`).");
        }
        LspConfig::Disabled(true) => {
            anyhow::bail!("Invalid `lsp: true` config. Use an object mapping LSP servers.");
        }
        LspConfig::Enabled(map) => map,
    };

    let ext = file_hint
        .and_then(|p| p.extension().and_then(|x| x.to_str()))
        .map(|x| format!(".{}", x.to_ascii_lowercase()));

    let mut fallback: Option<(String, ConfigLspServerConfig)> = None;
    for (id, server) in servers {
        if server.disabled.unwrap_or(false) || server.command.is_empty() {
            continue;
        }
        if fallback.is_none() {
            fallback = Some((id.clone(), server.clone()));
        }

        if let Some(ref ext) = ext {
            if !server.extensions.is_empty()
                && !server
                    .extensions
                    .iter()
                    .any(|value| value.eq_ignore_ascii_case(ext.as_str()))
            {
                continue;
            }
        }
        return Ok((id.clone(), server.clone()));
    }

    fallback
        .ok_or_else(|| anyhow::anyhow!("No enabled LSP server with an executable command found."))
}

async fn create_lsp_client(file_hint: Option<&Path>) -> anyhow::Result<LspClient> {
    let cwd = std::env::current_dir()?;
    let config = load_config(&cwd)?;
    let (id, server) = select_lsp_server(&config, file_hint)?;
    let command = server.command[0].clone();
    let args = server.command.iter().skip(1).cloned().collect::<Vec<_>>();
    let initialization_options = server
        .initialization
        .map(serde_json::to_value)
        .transpose()?;

    LspClient::start(
        LspServerConfig {
            id,
            command,
            args,
            initialization_options,
        },
        cwd,
    )
    .await
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}

fn infer_language_id(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "kt" => "kotlin",
        "swift" => "swift",
        "cpp" | "cc" | "cxx" | "c" | "h" | "hpp" => "cpp",
        "json" => "json",
        "md" => "markdown",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "sh" | "bash" | "zsh" => "shellscript",
        _ => "plaintext",
    }
}

fn resolve_debug_path(path: PathBuf) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn resolve_context_docs_registry_path_from_config() -> anyhow::Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let config = load_config(&cwd)?;
    let runtime_config = rocode_tool::ToolRuntimeConfig::from_config(&config);
    let configured = runtime_config
        .context_docs_registry_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "context_docs registry path is not configured; set docs.contextDocsRegistryPath in rocode.json or rocode.jsonc"
            )
        })?;
    let path = PathBuf::from(configured);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(cwd.join(path))
    }
}

pub(crate) async fn handle_debug_command(action: DebugCommands) -> anyhow::Result<()> {
    match action {
        DebugCommands::Paths => {
            println!("Global paths:");
            println!("  {:<12} {}", "cwd", std::env::current_dir()?.display());
            println!(
                "  {:<12} {}",
                "home",
                dirs::home_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<none>".to_string())
            );
            println!(
                "  {:<12} {}",
                "config",
                dirs::config_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<none>".to_string())
            );
            println!(
                "  {:<12} {}",
                "data",
                dirs::data_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<none>".to_string())
            );

            println!(
                "  {:<12} {}",
                "cache",
                dirs::cache_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<none>".to_string())
            );
        }
        DebugCommands::Config => {
            let cwd = std::env::current_dir()?;
            let config = load_config(&cwd)?;
            println!("{}", serde_json::to_string_pretty(&config)?);
        }
        DebugCommands::Skill => {
            let skills = list_available_skills();
            let list: Vec<_> = skills
                .into_iter()
                .map(|(name, description)| {
                    serde_json::json!({
                        "name": name,
                        "description": description
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&list)?);
        }
        DebugCommands::Scrap => {
            let db = Database::new().await?;
            let session_repo = SessionRepository::new(db.pool().clone());
            let sessions = session_repo.list(None, 10_000).await?;
            let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for session in sessions {
                grouped
                    .entry(session.project_id)
                    .or_default()
                    .push(session.directory);
            }
            println!("{}", serde_json::to_string_pretty(&grouped)?);
        }
        DebugCommands::Wait => loop {
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        },
        DebugCommands::Snapshot { action } => {
            let cwd = std::env::current_dir()?;
            match action {
                DebugSnapshotCommands::Track => {
                    println!("{}", Snapshot::track(&cwd)?);
                }
                DebugSnapshotCommands::Patch { hash } => {
                    let output = ProcessCommand::new("git")
                        .args(["show", "--no-color", &hash])
                        .output()
                        .map_err(|e| anyhow::anyhow!("Failed to run git show: {}", e))?;

                    if output.status.success() {
                        print!("{}", String::from_utf8_lossy(&output.stdout));
                    } else {
                        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr));
                    }
                }
                DebugSnapshotCommands::Diff { hash } => {
                    let diffs = Snapshot::diff(&cwd, &hash)?;
                    println!("{}", serde_json::to_string_pretty(&diffs)?);
                }
            }
        }
        DebugCommands::File { action } => match action {
            DebugFileCommands::Search { query } => {
                let files = Ripgrep::files(".", FileSearchOptions::default())?;
                let matches: Vec<String> = files
                    .into_iter()
                    .filter_map(|p| {
                        let p = p.to_string_lossy().to_string();
                        p.contains(&query).then_some(p)
                    })
                    .collect();
                for line in matches {
                    println!("{}", line);
                }
            }
            DebugFileCommands::Read { path } => {
                let content = fs::read_to_string(&path)
                    .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path, e))?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "path": path,
                        "content": content
                    }))?
                );
            }
            DebugFileCommands::Status => {
                let output = ProcessCommand::new("git")
                    .args(["status", "--porcelain"])
                    .output()
                    .map_err(|e| anyhow::anyhow!("Failed to run git status: {}", e))?;
                let status = String::from_utf8_lossy(&output.stdout);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "cwd": std::env::current_dir()?.display().to_string(),
                        "git_status_porcelain": status.lines().collect::<Vec<_>>()
                    }))?
                );
            }

            DebugFileCommands::List { path } => {
                let mut entries = Vec::new();
                for entry in fs::read_dir(&path)? {
                    let entry = entry?;
                    let meta = entry.metadata()?;
                    entries.push(serde_json::json!({
                        "name": entry.file_name().to_string_lossy().to_string(),
                        "path": entry.path().display().to_string(),
                        "is_dir": meta.is_dir(),
                        "is_file": meta.is_file(),
                        "len": meta.len(),
                    }));
                }
                println!("{}", serde_json::to_string_pretty(&entries)?);
            }
            DebugFileCommands::Tree { dir } => {
                let base = dir.unwrap_or_else(|| PathBuf::from("."));
                let tree = Ripgrep::tree(base, Some(200))?;
                println!("{}", tree);
            }
        },
        DebugCommands::Rg { action } => match action {
            DebugRgCommands::Tree { limit } => {
                let tree = Ripgrep::tree(".", limit)?;
                println!("{}", tree);
            }
            DebugRgCommands::Files { query, glob, limit } => {
                let mut options = FileSearchOptions::default();
                if let Some(glob) = glob {
                    options.glob = vec![glob];
                }
                let mut files = Ripgrep::files(".", options)?;
                if let Some(query) = query {
                    files.retain(|p| p.to_string_lossy().contains(&query));
                }
                if let Some(limit) = limit {
                    files.truncate(limit);
                }
                for file in files {
                    println!("{}", file.display());
                }
            }
            DebugRgCommands::Search {
                pattern,
                glob,
                limit,
            } => {
                let mut matches = Ripgrep::search_with_limit(".", &pattern, limit.unwrap_or(200))
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                if !glob.is_empty() {
                    matches.retain(|m| glob.iter().any(|g| m.path.contains(g)));
                }
                println!("{}", serde_json::to_string_pretty(&matches)?);
            }
        },

        DebugCommands::Lsp { action } => match action {
            DebugLspCommands::Diagnostics { file } => {
                let path = resolve_document_input_to_path(&file)?;
                let content = fs::read_to_string(&path)
                    .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;
                let client = create_lsp_client(Some(&path)).await?;
                client
                    .open_document(&path, &content, infer_language_id(&path))
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;

                let mut rx = client.subscribe();
                let _ = tokio::time::timeout(Duration::from_millis(1200), rx.recv()).await;
                let diagnostics = client.get_diagnostics(&path).await;
                println!("{}", serde_json::to_string_pretty(&diagnostics)?);
            }
            DebugLspCommands::Symbols { query } => {
                let client = create_lsp_client(None).await?;
                let symbols = client
                    .workspace_symbol(&query)
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                println!("{}", serde_json::to_string_pretty(&symbols)?);
            }
            DebugLspCommands::DocumentSymbols { uri } => {
                let path = resolve_document_input_to_path(&uri)?;
                let content = fs::read_to_string(&path)
                    .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;
                let client = create_lsp_client(Some(&path)).await?;
                client
                    .open_document(&path, &content, infer_language_id(&path))
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                let symbols = client
                    .document_symbol(&path)
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                println!("{}", serde_json::to_string_pretty(&symbols)?);
            }
        },

        DebugCommands::Docs { action } => match action {
            DebugDocsCommands::Validate { registry, index } => {
                let output = if let Some(index_path) = index {
                    let index_path = resolve_debug_path(index_path)?;
                    serde_json::to_value(rocode_tool::context_docs::validate_docs_index_file(
                        &index_path,
                    )?)?
                } else {
                    let registry_path = if let Some(registry_path) = registry {
                        resolve_debug_path(registry_path)?
                    } else {
                        resolve_context_docs_registry_path_from_config()?
                    };
                    serde_json::to_value(rocode_tool::context_docs::validate_registry_file(
                        &registry_path,
                    )?)?
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
        },

        DebugCommands::Agent { name, tool, params } => {
            let cwd = std::env::current_dir()?;
            let config = load_config(&cwd)?;
            let registry = AgentRegistry::from_config(&config);
            let Some(agent) = registry.get(&name) else {
                anyhow::bail!("Agent not found: {}", name);
            };
            if let Some(tool_name) = tool {
                let args = if let Some(raw) = params {
                    serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| {
                        anyhow::anyhow!("Invalid --params JSON for tool `{}`: {}", tool_name, e)
                    })?
                } else {
                    serde_json::json!({})
                };
                let cwd = std::env::current_dir()?;
                let tool_registry = Arc::new(create_default_registry().await);
                let ctx = ToolContext::new(
                    format!("debug-{}", name),
                    "debug-message".to_string(),
                    cwd.display().to_string(),
                )
                .with_agent(name.clone())
                .with_tool_runtime_config(rocode_tool::ToolRuntimeConfig::from_config(&config))
                .with_registry(tool_registry.clone());
                let output = tool_registry
                    .execute(&tool_name, args, ctx)
                    .await
                    .map_err(|e| anyhow::anyhow!("Tool execution failed: {}", e))?;
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "agent": agent
                    }))?
                );
            }
        }
    }
    Ok(())
}
