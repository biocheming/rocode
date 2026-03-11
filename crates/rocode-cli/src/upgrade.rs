use std::io::{self, Write};
use std::process::Command as ProcessCommand;

use crate::util::parse_http_json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstallMethod {
    Curl,
    Npm,
    Pnpm,
    Bun,
    Brew,
    Choco,
    Scoop,
    Unknown,
}

impl InstallMethod {
    pub(crate) fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "curl" => Self::Curl,
            "npm" => Self::Npm,
            "pnpm" => Self::Pnpm,
            "bun" => Self::Bun,
            "brew" => Self::Brew,
            "choco" => Self::Choco,
            "scoop" => Self::Scoop,
            _ => Self::Unknown,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Curl => "curl",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Bun => "bun",
            Self::Brew => "brew",
            Self::Choco => "choco",
            Self::Scoop => "scoop",
            Self::Unknown => "unknown",
        }
    }
}

fn command_text(program: &str, args: &[&str]) -> Option<String> {
    let output = ProcessCommand::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

fn detect_install_method() -> InstallMethod {
    let exec_path = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if exec_path.contains(".opencode/bin") || exec_path.contains(".local/bin") {
        return InstallMethod::Curl;
    }

    let checks: &[(InstallMethod, &str, &[&str], &str)] = &[
        (
            InstallMethod::Npm,
            "npm",
            &["list", "-g", "--depth=0"],
            "rocode-ai",
        ),
        (
            InstallMethod::Pnpm,
            "pnpm",
            &["list", "-g", "--depth=0"],
            "rocode-ai",
        ),
        (InstallMethod::Bun, "bun", &["pm", "ls", "-g"], "rocode-ai"),
        (
            InstallMethod::Brew,
            "brew",
            &["list", "--formula", "rocode"],
            "rocode",
        ),
        (
            InstallMethod::Choco,
            "choco",
            &["list", "--limit-output", "rocode"],
            "rocode",
        ),
        (InstallMethod::Scoop, "scoop", &["list", "rocode"], "rocode"),
    ];

    for (method, program, args, marker) in checks {
        if let Some(text) = command_text(program, args) {
            if text.to_ascii_lowercase().contains(marker) {
                return *method;
            }
        }
    }

    InstallMethod::Unknown
}

async fn latest_version(method: InstallMethod) -> anyhow::Result<String> {
    let client = reqwest::Client::new();

    match method {
        InstallMethod::Brew => {
            let response = client
                .get("https://formulae.brew.sh/api/formula/rocode.json")
                .send()
                .await?;
            let json: serde_json::Value = parse_http_json(response).await?;
            let version = json
                .get("versions")
                .and_then(|v| v.get("stable"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Unable to parse brew stable version"))?;
            Ok(version.to_string())
        }
        InstallMethod::Npm | InstallMethod::Pnpm | InstallMethod::Bun => {
            let response = client
                .get("https://registry.npmjs.org/rocode-ai/latest")
                .send()
                .await?;
            let json: serde_json::Value = parse_http_json(response).await?;
            let version = json
                .get("version")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Unable to parse npm latest version"))?;
            Ok(version.to_string())
        }
        InstallMethod::Scoop => {
            let response = client
                .get("https://raw.githubusercontent.com/ScoopInstaller/Main/master/bucket/rocode.json")
                .send()
                .await?;
            let json: serde_json::Value = parse_http_json(response).await?;
            let version = json
                .get("version")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Unable to parse scoop version"))?;
            Ok(version.to_string())
        }
        _ => {
            let response = client
                .get("https://api.github.com/repos/anomalyco/rocode/releases/latest")
                .header("User-Agent", "rocode-cli-rust")
                .send()
                .await?;
            let json: serde_json::Value = parse_http_json(response).await?;
            let tag = json
                .get("tag_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Unable to parse latest GitHub release"))?;
            Ok(tag.trim_start_matches('v').to_string())
        }
    }
}

fn run_upgrade_process(method: InstallMethod, target: &str) -> anyhow::Result<()> {
    let status = match method {
        InstallMethod::Curl => ProcessCommand::new("sh")
            .arg("-c")
            .arg("curl -fsSL https://rocode.dev/install | bash")
            .env("VERSION", target)
            .status(),
        InstallMethod::Npm => ProcessCommand::new("npm")
            .args(["install", "-g", &format!("rocode-ai@{}", target)])
            .status(),
        InstallMethod::Pnpm => ProcessCommand::new("pnpm")
            .args(["install", "-g", &format!("rocode-ai@{}", target)])
            .status(),
        InstallMethod::Bun => ProcessCommand::new("bun")
            .args(["install", "-g", &format!("rocode-ai@{}", target)])
            .status(),
        InstallMethod::Brew => ProcessCommand::new("brew")
            .args(["upgrade", "rocode"])
            .status(),
        InstallMethod::Choco => ProcessCommand::new("choco")
            .args(["upgrade", "rocode", "--version", target, "-y"])
            .status(),
        InstallMethod::Scoop => ProcessCommand::new("scoop")
            .args(["install", &format!("rocode@{}", target)])
            .status(),
        InstallMethod::Unknown => {
            anyhow::bail!("Unknown install method; pass --method to specify one explicitly.")
        }
    }
    .map_err(|e| anyhow::anyhow!("Failed to execute upgrade command: {}", e))?;

    if !status.success() {
        anyhow::bail!("Upgrade command exited with status {}", status);
    }
    Ok(())
}

fn prompt_yes_no(question: &str) -> anyhow::Result<bool> {
    print!("{} [y/N]: ", question);
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

pub(crate) async fn handle_upgrade_command(
    target: Option<String>,
    method: Option<String>,
) -> anyhow::Result<()> {
    let detected = detect_install_method();
    let method = method
        .as_deref()
        .map(InstallMethod::parse)
        .unwrap_or(detected);

    println!("Using method: {}", method.as_str());

    if method == InstallMethod::Unknown
        && !prompt_yes_no("Installation method is unknown. Continue anyway?")?
    {
        println!("Cancelled.");
        return Ok(());
    }

    let target = if let Some(target) = target {
        target.trim_start_matches('v').to_string()
    } else {
        latest_version(method).await?
    };

    let current = env!("CARGO_PKG_VERSION").trim_start_matches('v');
    if current == target {
        println!("rocode upgrade skipped: {} is already installed", target);
        return Ok(());
    }

    println!("From {} -> {}", current, target);
    run_upgrade_process(method, &target)?;
    println!("Upgrade complete.");
    Ok(())
}

pub(crate) async fn handle_uninstall_command(
    keep_config: bool,
    keep_data: bool,
    dry_run: bool,
    force: bool,
) -> anyhow::Result<()> {
    let mut targets = vec![
        ("data", dirs::data_local_dir().map(|p| p.join("opencode"))),
        ("cache", dirs::cache_dir().map(|p| p.join("opencode"))),
        ("config", dirs::config_dir().map(|p| p.join("opencode"))),
        ("state", dirs::state_dir().map(|p| p.join("opencode"))),
    ];

    println!("Uninstall targets:");
    for (label, path) in &targets {
        if let Some(path) = path {
            println!("  {:<8} {}", label, path.display());
        }
    }

    if dry_run {
        println!("Dry run mode, no files removed.");
        return Ok(());
    }

    if !force {
        println!("Use --force to perform removal.");
        return Ok(());
    }

    for (label, path) in targets.drain(..) {
        let Some(path) = path else {
            continue;
        };
        if (label == "config" && keep_config) || (label == "data" && keep_data) {
            println!("Skipping {} ({})", label, path.display());
            continue;
        }
        if path.exists() {
            std::fs::remove_dir_all(&path)
                .map_err(|e| anyhow::anyhow!("Failed removing {}: {}", path.display(), e))?;
            println!("Removed {}", path.display());
        }
    }
    Ok(())
}
