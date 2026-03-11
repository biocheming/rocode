use std::process::Command;

fn main() {
    // Capture rustc version at compile time.
    let rustc_version = Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string());
    println!(
        "cargo:rustc-env=ROCODE_RUSTC_VERSION={}",
        rustc_version.trim()
    );

    // Capture the target triple.
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=ROCODE_TARGET={}", target);

    // Capture the build profile (debug or release).
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=ROCODE_PROFILE={}", profile);

    // Capture the host triple (what we're compiling on).
    let host = std::env::var("HOST").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=ROCODE_HOST={}", host);

    // Build timestamp (ISO 8601).
    let now = chrono_lite_utc_now();
    println!("cargo:rustc-env=ROCODE_BUILD_TIME={}", now);
}

/// Minimal UTC timestamp without pulling in chrono at build time.
fn chrono_lite_utc_now() -> String {
    Command::new("date")
        .arg("-u")
        .arg("+%Y-%m-%dT%H:%M:%SZ")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
