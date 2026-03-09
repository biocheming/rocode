use std::path::{Path, PathBuf};

use crate::loader::resolve_configured_path;
use std::sync::Arc;

use arc_swap::ArcSwap;
use tokio::sync::RwLock;

use crate::schema::Config;

/// Single source of truth for application configuration.
///
/// Read path is lock-free (`ArcSwap`). Write path (`patch`) swaps the Arc
/// and invalidates derived caches. Consumers hold `Arc<Config>` snapshots.
pub struct ConfigStore {
    base: ArcSwap<Config>,
    plugin_applied: RwLock<Option<Arc<Config>>>,
    project_dir: RwLock<Option<PathBuf>>,
}

impl ConfigStore {
    /// Create a ConfigStore with an initial config value.
    pub fn new(config: Config) -> Self {
        Self {
            base: ArcSwap::from_pointee(config),
            plugin_applied: RwLock::new(None),
            project_dir: RwLock::new(None),
        }
    }

    /// Create a ConfigStore by loading config from disk.
    pub fn from_project_dir(project_dir: &Path) -> anyhow::Result<Self> {
        let config = crate::load_config(project_dir)?;
        let store = Self::new(config);
        let dir = project_dir.to_path_buf();
        // Use try_write since we're the only owner at construction time
        if let Ok(mut guard) = store.project_dir.try_write() {
            *guard = Some(dir);
        }
        Ok(store)
    }

    /// Read current base config. Lock-free, returns Arc snapshot.
    pub fn config(&self) -> Arc<Config> {
        self.base.load_full()
    }

    /// Merge a JSON patch into the base config, invalidate derived caches.
    pub fn patch(&self, patch: serde_json::Value) -> anyhow::Result<Arc<Config>> {
        let current = self.config();
        let mut updated = (*current).clone();

        let patch_config: Config = serde_json::from_value(patch)?;
        updated.merge(patch_config);

        let new_arc = Arc::new(updated);
        self.base.store(new_arc.clone());

        // Invalidate plugin cache synchronously (best-effort)
        if let Ok(mut guard) = self.plugin_applied.try_write() {
            *guard = None;
        }

        Ok(new_arc)
    }

    /// Get the cached plugin-applied config (if any).
    pub async fn plugin_applied(&self) -> Option<Arc<Config>> {
        self.plugin_applied.read().await.clone()
    }

    /// Store plugin-applied config after hooks have been executed.
    pub async fn set_plugin_applied(&self, config: Config) {
        *self.plugin_applied.write().await = Some(Arc::new(config));
    }

    /// Invalidate the plugin-applied cache. Next consumer must re-run hooks.
    pub async fn invalidate_plugin_cache(&self) {
        *self.plugin_applied.write().await = None;
    }

    /// Reload base config from disk (if project_dir is known).
    pub async fn resolved_scheduler_path(&self) -> Option<PathBuf> {
        let config = self.config();
        let raw = config
            .scheduler_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())?;

        let path = PathBuf::from(raw);
        if path.is_absolute() {
            return Some(path);
        }

        let project_dir = self.project_dir.read().await.clone();
        project_dir.map(|dir| resolve_configured_path(&dir, raw))
    }

    pub async fn resolved_task_category_path(&self) -> Option<PathBuf> {
        let config = self.config();
        let raw = config
            .task_category_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())?;

        let path = PathBuf::from(raw);
        if path.is_absolute() {
            return Some(path);
        }

        let project_dir = self.project_dir.read().await.clone();
        project_dir.map(|dir| resolve_configured_path(&dir, raw))
    }

    pub async fn reload(&self) -> anyhow::Result<Arc<Config>> {
        let dir = self.project_dir.read().await;
        let project_dir = dir
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no project directory set for config reload"))?;
        let config = crate::load_config(project_dir)?;
        let new_arc = Arc::new(config);
        self.base.store(new_arc.clone());
        self.invalidate_plugin_cache().await;
        Ok(new_arc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn config_returns_arc_without_clone() {
        let store = ConfigStore::new(Config::default());
        let a = store.config();
        let b = store.config();
        // Same Arc, not cloned data
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[tokio::test]
    async fn patch_replaces_config() {
        let store = ConfigStore::new(Config::default());
        let before = store.config();

        let patch = serde_json::json!({ "model": "test-model" });
        store.patch(patch).unwrap();

        let after = store.config();
        assert!(!Arc::ptr_eq(&before, &after));
    }

    #[tokio::test]
    async fn patch_invalidates_plugin_cache() {
        let store = ConfigStore::new(Config::default());

        store.set_plugin_applied(Config::default()).await;
        assert!(store.plugin_applied().await.is_some());

        store.patch(serde_json::json!({})).unwrap();
        assert!(store.plugin_applied().await.is_none());
    }

    #[tokio::test]
    async fn resolved_scheduler_path_uses_project_dir_for_relative_paths() {
        let store = ConfigStore::new(Config {
            scheduler_path: Some(".rocode/scheduler/sisyphus.jsonc".to_string()),
            ..Default::default()
        });

        *store.project_dir.write().await = Some(PathBuf::from("/tmp/rocode-project"));

        assert_eq!(
            store.resolved_scheduler_path().await,
            Some(PathBuf::from(
                "/tmp/rocode-project/.rocode/scheduler/sisyphus.jsonc"
            ))
        );
    }
}
