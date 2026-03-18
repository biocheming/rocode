use std::fs;
use std::path::{Path, PathBuf};

use rocode_config::Config;
use rocode_plugin::{HookContext, HookEvent};
use serde::Deserialize;

fn deserialize_opt_bool_lossy<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::Bool(value)) => Some(value),
        Some(serde_json::Value::Number(value)) => value.as_i64().map(|value| value != 0),
        Some(serde_json::Value::String(value)) => {
            match value.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" => Some(false),
                _ => None,
            }
        }
        _ => None,
    })
}

#[derive(Debug, Default, Deserialize)]
struct NativeHookPayloadWire {
    #[serde(default, deserialize_with = "deserialize_opt_bool_lossy")]
    native_demo_loaded: Option<bool>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let config_path = args
        .next()
        .map(PathBuf::from)
        .ok_or("usage: verify_native_dylib <path-to-rocode.json>")?;

    let config_raw = fs::read_to_string(&config_path)?;
    let config: Config = serde_json::from_str(&config_raw)?;
    let base_dir = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let native_paths: Vec<PathBuf> = config
        .plugin
        .iter()
        .filter_map(|(_name, cfg)| {
            if !cfg.is_native() {
                return None;
            }
            let raw = cfg.dylib_path()?;
            let path = PathBuf::from(raw);
            if path.is_absolute() {
                Some(path)
            } else {
                Some(base_dir.join(path))
            }
        })
        .collect();

    if native_paths.is_empty() {
        return Err("no type=dylib plugins found in config".into());
    }

    let hook_system = rocode_plugin::global();
    let errors = rocode_plugin::load_native_plugins(&native_paths, hook_system.clone()).await;
    if !errors.is_empty() {
        return Err(format!("failed to load native plugins: {}", errors[0]).into());
    }

    let results = hook_system
        .trigger(
            HookContext::new(HookEvent::SessionStart)
                .with_data("agent", serde_json::json!("native-dylib-e2e")),
        )
        .await;

    let matched = results.into_iter().any(|result| match result {
        Ok(output) => output.payload.is_some_and(|payload| {
            serde_json::from_value::<NativeHookPayloadWire>(payload)
                .ok()
                .and_then(|parsed| parsed.native_demo_loaded)
                == Some(true)
        }),
        Err(_) => false,
    });

    if !matched {
        return Err("native plugin loaded but did not emit expected hook payload".into());
    }

    println!("native dylib plugin loaded and hook executed successfully");
    Ok(())
}
