use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;

use rocode_storage::{Database, MessageRepository, SessionRepository};

use crate::cli::{DbCommands, DbOutputFormat};

fn local_database_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rocode")
        .join("rocode.db")
}

pub(crate) async fn handle_db_command(
    action: Option<DbCommands>,
    query: Option<String>,
    format: DbOutputFormat,
) -> anyhow::Result<()> {
    if matches!(action, Some(DbCommands::Path)) {
        println!("{}", local_database_path().display());
        return Ok(());
    }

    let db_path = local_database_path();
    if let Some(query) = query {
        let mut args = vec![db_path.display().to_string()];
        match format {
            DbOutputFormat::Json => args.push("-json".to_string()),
            DbOutputFormat::Tsv => args.push("-tabs".to_string()),
        }
        args.push(query);

        let output = ProcessCommand::new("sqlite3")
            .args(&args)
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to run sqlite3: {}", e))?;
        if output.status.success() {
            print!("{}", String::from_utf8_lossy(&output.stdout));
            return Ok(());
        }
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr));
    }

    let status = ProcessCommand::new("sqlite3")
        .arg(db_path)
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run sqlite3 interactive shell: {}", e))?;
    if !status.success() {
        anyhow::bail!("sqlite3 exited with status {}", status);
    }
    Ok(())
}

pub(crate) async fn handle_stats_command(
    days: Option<i64>,
    tools_limit: Option<usize>,
    models_limit: Option<usize>,
    project: Option<String>,
) -> anyhow::Result<()> {
    let db = Database::new().await?;
    let session_repo = SessionRepository::new(db.pool().clone());
    let message_repo = MessageRepository::new(db.pool().clone());

    let mut sessions = session_repo.list(None, 50_000).await?;
    if let Some(project) = project {
        if project.is_empty() {
            let cwd = std::env::current_dir()?.display().to_string();
            sessions.retain(|s| s.directory == cwd);
        } else {
            sessions.retain(|s| s.project_id == project);
        }
    }

    if let Some(days) = days {
        let now = chrono::Utc::now().timestamp_millis();
        let cutoff = if days == 0 {
            let dt = chrono::Utc::now()
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap();
            chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc)
                .timestamp_millis()
        } else {
            now - (days * 24 * 60 * 60 * 1000)
        };
        sessions.retain(|s| s.time.updated >= cutoff);
    }

    let mut total_messages = 0usize;
    let mut total_cost = 0.0f64;
    let mut total_input = 0u64;
    let mut total_output = 0u64;
    let mut total_reasoning = 0u64;
    let mut total_cache_read = 0u64;
    let mut total_cache_write = 0u64;
    let mut tool_usage: BTreeMap<String, usize> = BTreeMap::new();
    let mut model_usage: BTreeMap<String, usize> = BTreeMap::new();

    for session in &sessions {
        if let Some(usage) = &session.usage {
            total_cost += usage.total_cost;
            total_input += usage.input_tokens;
            total_output += usage.output_tokens;
            total_reasoning += usage.reasoning_tokens;
            total_cache_read += usage.cache_read_tokens;
            total_cache_write += usage.cache_write_tokens;
        }

        let messages = message_repo.list_for_session(&session.id).await?;
        total_messages += messages.len();

        for message in messages {
            if let Some(provider) = message.metadata.get("provider_id").and_then(|v| v.as_str()) {
                let model = message
                    .metadata
                    .get("model_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                *model_usage
                    .entry(format!("{}/{}", provider, model))
                    .or_insert(0) += 1;
            }
            for part in message.parts {
                if let rocode_types::PartType::ToolCall { name, .. } = part.part_type {
                    *tool_usage.entry(name).or_insert(0) += 1;
                }
            }
        }
    }

    println!("Sessions: {}", sessions.len());
    println!("Messages: {}", total_messages);
    println!("Total Cost: ${:.4}", total_cost);
    println!(
        "Tokens: input={} output={} reasoning={} cache_read={} cache_write={}",
        total_input, total_output, total_reasoning, total_cache_read, total_cache_write
    );

    if !model_usage.is_empty() {
        println!("\nModel usage:");
        let mut rows: Vec<_> = model_usage.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1));
        if let Some(limit) = models_limit {
            rows.truncate(limit);
        }
        for (model, count) in rows {
            println!("  {:<40} {}", model, count);
        }
    }

    if !tool_usage.is_empty() {
        println!("\nTool usage:");
        let mut rows: Vec<_> = tool_usage.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1));
        if let Some(limit) = tools_limit {
            rows.truncate(limit);
        }
        for (tool, count) in rows {
            println!("  {:<30} {}", tool, count);
        }
    }

    Ok(())
}
