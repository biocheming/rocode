use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use rocode_storage::{Database, SessionRepository};

use crate::server::{start_mdns_publisher_if_needed, wait_for_server_ready, MdnsPublisher};

pub(crate) async fn run_tui(
    project: Option<PathBuf>,
    model: Option<String>,
    continue_last: bool,
    session: Option<String>,
    fork: bool,
    agent_name: Option<String>,
    initial_prompt: Option<String>,
    port: u16,
    hostname: String,
    mdns: bool,
    mdns_domain: String,
    cors: Vec<String>,
    attach_url: Option<String>,
    _password: Option<String>,
) -> anyhow::Result<()> {
    if let Some(project) = project {
        std::env::set_current_dir(&project).map_err(|e| {
            anyhow::anyhow!("Failed to change directory to {}: {}", project.display(), e)
        })?;
    }

    if fork && !continue_last && session.is_none() {
        anyhow::bail!("--fork requires --continue or --session");
    }

    let mut server_handle = None;
    let mut mdns_publisher: Option<MdnsPublisher> = None;
    let base_url = if let Some(url) = attach_url {
        url
    } else {
        let bind_host = if mdns && hostname == "127.0.0.1" {
            "0.0.0.0".to_string()
        } else {
            hostname.clone()
        };
        let client_host = if bind_host == "0.0.0.0" {
            "127.0.0.1".to_string()
        } else {
            bind_host.clone()
        };
        let bind_port = if port == 0 { 3000 } else { port };
        let addr: SocketAddr = format!("{}:{}", bind_host, bind_port).parse()?;
        let server_url = format!("http://{}:{}", client_host, bind_port);
        eprintln!("Starting local server for TUI at {}", server_url);
        rocode_server::set_cors_whitelist(cors.clone());
        let mut handle = tokio::spawn(async move { rocode_server::run_server(addr).await });
        wait_for_server_ready(&server_url, Duration::from_secs(90), Some(&mut handle)).await?;
        server_handle = Some(handle);
        mdns_publisher = start_mdns_publisher_if_needed(mdns, &bind_host, bind_port, &mdns_domain);
        server_url
    };

    let selected_session = resolve_requested_session(continue_last, session, fork).await?;
    std::env::set_var("ROCODE_TUI_BASE_URL", &base_url);
    if let Some(model) = model {
        std::env::set_var("ROCODE_TUI_MODEL", model);
    }
    if let Some(prompt) = initial_prompt {
        std::env::set_var("ROCODE_TUI_PROMPT", prompt);
    }
    if let Some(agent_name) = agent_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        std::env::set_var("ROCODE_TUI_AGENT", agent_name);
    }
    if let Some(session_id) = selected_session {
        std::env::set_var("ROCODE_TUI_SESSION", session_id);
    }

    let run_result = tokio::task::spawn_blocking(|| rocode_tui::run_tui())
        .await
        .map_err(|e| anyhow::anyhow!("TUI task panicked: {}", e))?;

    std::env::remove_var("ROCODE_TUI_BASE_URL");
    std::env::remove_var("ROCODE_TUI_MODEL");
    std::env::remove_var("ROCODE_TUI_PROMPT");
    std::env::remove_var("ROCODE_TUI_AGENT");
    std::env::remove_var("ROCODE_TUI_SESSION");

    drop(mdns_publisher);
    if let Some(handle) = server_handle {
        handle.abort();
    }

    run_result
}

async fn resolve_requested_session(
    continue_last: bool,
    session: Option<String>,
    fork: bool,
) -> anyhow::Result<Option<String>> {
    let selected = if let Some(session_id) = session {
        Some(session_id)
    } else if continue_last {
        let db = Database::new().await?;
        let session_repo = SessionRepository::new(db.pool().clone());
        session_repo
            .list(None, 100)
            .await?
            .into_iter()
            .find(|s| s.parent_id.is_none())
            .map(|s| s.id)
    } else {
        None
    };

    if fork && selected.is_some() {
        eprintln!(
            "Note: --fork for TUI session attach is not fully wired yet; using base session."
        );
    }

    Ok(selected)
}
