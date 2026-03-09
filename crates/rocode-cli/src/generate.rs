use std::collections::HashMap;
use std::time::Duration;

use rocode_config::loader::load_config;
use rocode_provider::ModelsRegistry;

use crate::providers::setup_providers;
use crate::util::format_tokens;

pub(crate) async fn handle_generate_command() -> anyhow::Result<()> {
    let mut paths: HashMap<String, serde_json::Map<String, serde_json::Value>> = HashMap::new();
    let operations: &[(&str, &str, &str)] = &[
        ("/health", "get", "health"),
        ("/event", "get", "eventSubscribe"),
        ("/path", "get", "pathsGet"),
        ("/vcs", "get", "vcsGet"),
        ("/command", "get", "commandList"),
        ("/agent", "get", "agentList"),
        ("/skill", "get", "skillList"),
        ("/lsp", "get", "lspStatus"),
        ("/formatter", "get", "formatterStatus"),
        ("/auth/{id}", "put", "authSet"),
        ("/auth/{id}", "delete", "authDelete"),
        ("/session", "get", "sessionList"),
        ("/session", "post", "sessionCreate"),
        ("/session/status", "get", "sessionStatus"),
        ("/session/{id}", "get", "sessionGet"),
        ("/session/{id}", "patch", "sessionUpdate"),
        ("/session/{id}", "delete", "sessionDelete"),
        ("/session/{id}/children", "get", "sessionChildren"),
        ("/session/{id}/todo", "get", "sessionTodo"),
        ("/session/{id}/fork", "post", "sessionFork"),
        ("/session/{id}/abort", "post", "sessionAbort"),
        ("/session/{id}/share", "post", "sessionShare"),
        ("/session/{id}/share", "delete", "sessionUnshare"),
        ("/session/{id}/archive", "post", "sessionArchive"),
        ("/session/{id}/title", "patch", "sessionSetTitle"),
        ("/session/{id}/permission", "patch", "sessionSetPermission"),
        ("/session/{id}/summary", "get", "sessionSummaryGet"),
        ("/session/{id}/summary", "patch", "sessionSummarySet"),
        ("/session/{id}/revert", "post", "sessionRevert"),
        ("/session/{id}/revert", "delete", "sessionRevertClear"),
        ("/session/{id}/unrevert", "post", "sessionUnrevert"),
        ("/session/{id}/compaction", "post", "sessionCompaction"),
        ("/session/{id}/summarize", "post", "sessionSummarize"),
        ("/session/{id}/init", "post", "sessionInit"),
        ("/session/{id}/command", "post", "sessionCommand"),
        ("/session/{id}/shell", "post", "sessionShell"),
        ("/session/{id}/message", "get", "sessionMessageList"),
        ("/session/{id}/message", "post", "sessionMessageCreate"),
        ("/session/{id}/message/{msgID}", "get", "sessionMessageGet"),
        (
            "/session/{id}/message/{msgID}",
            "delete",
            "sessionMessageDelete",
        ),
        (
            "/session/{id}/message/{msgID}/part",
            "post",
            "sessionPartAdd",
        ),
        (
            "/session/{id}/message/{msgID}/part/{partID}",
            "patch",
            "sessionPartUpdate",
        ),
        (
            "/session/{id}/message/{msgID}/part/{partID}",
            "delete",
            "sessionPartDelete",
        ),
        ("/session/{id}/stream", "post", "sessionStream"),
        ("/session/{id}/prompt", "post", "sessionPrompt"),
        ("/session/{id}/prompt/abort", "post", "sessionPromptAbort"),
        ("/session/{id}/prompt_async", "post", "sessionPromptAsync"),
        ("/session/{id}/diff", "get", "sessionDiff"),
        ("/provider/", "get", "providerList"),
        ("/provider/auth", "get", "providerAuth"),
        (
            "/provider/{id}/oauth/authorize",
            "post",
            "providerOAuthAuthorize",
        ),
        (
            "/provider/{id}/oauth/callback",
            "post",
            "providerOAuthCallback",
        ),
        ("/config/", "get", "configGet"),
        ("/config/", "patch", "configPatch"),
        ("/config/providers", "get", "configProviderGet"),
        ("/mcp", "get", "mcpList"),
        ("/mcp", "post", "mcpAdd"),
        ("/mcp/{name}/connect", "post", "mcpConnect"),
        ("/mcp/{name}/disconnect", "post", "mcpDisconnect"),
        ("/mcp/{name}/auth", "post", "mcpAuthStart"),
        ("/mcp/{name}/auth", "delete", "mcpAuthDelete"),
        ("/mcp/{name}/auth/callback", "post", "mcpAuthCallback"),
        (
            "/mcp/{name}/auth/authenticate",
            "post",
            "mcpAuthAuthenticate",
        ),
        ("/file/read", "post", "fileRead"),
        ("/file/write", "post", "fileWrite"),
        ("/file/status", "get", "fileStatus"),
        ("/find", "post", "find"),
        ("/permission", "get", "permissionList"),
        ("/permission", "post", "permissionReply"),
        ("/project", "get", "projectList"),
        ("/project/current", "get", "projectCurrent"),
        ("/project/current", "patch", "projectCurrentPatch"),
        ("/pty", "post", "ptyCreate"),
        ("/pty/{id}", "get", "ptyRead"),
        ("/pty/{id}", "delete", "ptyDelete"),
        ("/question", "get", "questionList"),
        ("/question", "post", "questionReply"),
        ("/tui/session", "get", "tuiSessionList"),
        ("/tui/session", "post", "tuiSessionCreate"),
        ("/global/event", "get", "globalEventSubscribe"),
    ];

    for (path, method, operation_id) in operations {
        let entry = paths.entry((*path).to_string()).or_default();
        entry.insert(
            (*method).to_string(),
            serde_json::json!({
                "operationId": operation_id,
                "responses": { "200": { "description": "OK" } },
                "x-codeSamples": [
                    {
                        "lang": "js",
                        "source": format!(
                            "import {{ createOpencodeClient }} from \"@opencode-ai/sdk\"\n\nconst client = createOpencodeClient()\nawait client.{}({{\n  ...\n}})",
                            operation_id
                        )
                    }
                ]
            }),
        );
    }

    let spec = serde_json::json!({
        "openapi": "3.1.0",
        "info": {
            "title": "ROCODE API",
            "version": env!("CARGO_PKG_VERSION")
        },
        "paths": paths
    });
    println!("{}", serde_json::to_string_pretty(&spec)?);
    Ok(())
}

pub(crate) async fn list_models(
    provider_filter: Option<String>,
    refresh: bool,
    verbose: bool,
) -> anyhow::Result<()> {
    if refresh {
        let registry = ModelsRegistry::default();
        match tokio::time::timeout(Duration::from_secs(15), registry.refresh()).await {
            Ok(_) => eprintln!("Refreshed models.dev cache."),
            Err(_) => eprintln!("Warning: timed out refreshing models.dev cache."),
        }
    }

    let current_dir = std::env::current_dir()?;
    let config = load_config(&current_dir)?;
    let registry = setup_providers(&config).await?;

    println!("\n╔══════════════════════════════════════════╗");
    println!("║         Available Models                  ║");
    println!("╚══════════════════════════════════════════╝\n");

    let providers = registry.list();

    if providers.is_empty() {
        println!("No providers configured. Set API keys to enable providers:");
        println!("  - ANTHROPIC_API_KEY");
        println!("  - OPENAI_API_KEY");
        println!("  - OPENROUTER_API_KEY");
        println!("  - GOOGLE_API_KEY");
        println!("  - MISTRAL_API_KEY");
        println!("  - GROQ_API_KEY");
        println!("  - XAI_API_KEY");
        println!("  - DEEPSEEK_API_KEY");
        println!("  - COHERE_API_KEY");
        println!("  - TOGETHER_API_KEY");
        println!("  - PERPLEXITY_API_KEY");
        println!("  - CEREBRAS_API_KEY");
        println!("  - GOOGLE_VERTEX_API_KEY + GOOGLE_VERTEX_PROJECT_ID + GOOGLE_VERTEX_LOCATION");
        println!("  - AZURE_OPENAI_API_KEY + AZURE_OPENAI_ENDPOINT");
        println!("  - AWS_ACCESS_KEY_ID + AWS_SECRET_ACCESS_KEY + AWS_REGION");
        return Ok(());
    }

    for provider in providers {
        if let Some(ref filter) = provider_filter {
            if !provider.id().contains(filter.to_lowercase().as_str()) {
                continue;
            }
        }

        println!("Provider: {} ({})", provider.name(), provider.id());
        println!("{}", "─".repeat(50));

        let models = provider.models();
        for model in models {
            println!("  {}", model.id);
            println!(
                "    Context: {} tokens | Output: {} tokens",
                format_tokens(model.context_window),
                format_tokens(model.max_output_tokens)
            );
            if model.supports_vision || model.supports_tools {
                let mut caps = Vec::new();
                if model.supports_vision {
                    caps.push("vision");
                }
                if model.supports_tools {
                    caps.push("tools");
                }
                println!("    Capabilities: {}", caps.join(", "));
            }
            if verbose {
                println!(
                    "    Details: name={} vision={} tools={}",
                    model.name, model.supports_vision, model.supports_tools
                );
            }
            println!();
        }
    }

    Ok(())
}
