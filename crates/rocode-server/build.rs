use std::path::Path;
use std::process::Command;
use std::process::Stdio;

fn find_npm() -> Option<&'static str> {
    let candidates: &[&str] = if cfg!(windows) {
        &["npm.cmd", "npm"]
    } else {
        &["npm", "npm.cmd"]
    };

    candidates.iter().copied().find(|candidate| {
        Command::new(candidate)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    })
}

fn main() {
    let web_ui_dir = Path::new("web-ui");
    let src_dir = web_ui_dir.join("src");

    // Re-run if any source file changes
    println!("cargo:rerun-if-changed=web-ui/src");
    println!("cargo:rerun-if-changed=web-ui/index.html");
    println!("cargo:rerun-if-changed=web-ui/vite.config.ts");
    println!("cargo:rerun-if-changed=web-ui/package.json");

    // Skip npm build if dist already exists and we're in a CI/offline environment
    // where node_modules might not be available
    let dist_dir = web_ui_dir.join("dist");
    let node_modules = web_ui_dir.join("node_modules");

    if !node_modules.exists() {
        if dist_dir.exists()
            && dist_dir.join("index.html").exists()
            && dist_dir.join("app.js").exists()
            && dist_dir.join("app.css").exists()
        {
            // Pre-built dist exists, skip npm build
            println!("cargo:warning=web-ui: using pre-built dist/ (node_modules not found)");
            return;
        }
        panic!(
            "web-ui/node_modules not found and no pre-built dist/. Run `npm install` in web-ui/ first."
        );
    }

    if !src_dir.exists() {
        panic!("web-ui/src directory not found");
    }

    let Some(npm) = find_npm() else {
        if dist_dir.exists()
            && dist_dir.join("index.html").exists()
            && dist_dir.join("app.js").exists()
            && dist_dir.join("app.css").exists()
        {
            println!("cargo:warning=web-ui: using pre-built dist/ (npm executable not found)");
            return;
        }
        panic!("failed to find npm executable (`npm` or `npm.cmd`) for web-ui build");
    };

    let status = Command::new(npm)
        .arg("run")
        .arg("build")
        .current_dir(web_ui_dir)
        .status()
        .expect("failed to run `npm run build` in web-ui/");

    if !status.success() {
        panic!("web-ui build failed with status: {}", status);
    }
}
