use strum_macros::EnumString;

/// Shared file-system / file-watcher wire contracts.
///
/// These are used across:
/// - tools that edit files (`edit`, `write`, `apply_patch`, etc.)
/// - server/runtime components that react to file edits
///
/// Keep them stable — they are part of the cross-crate contract.

/// Bus event payload keys for file-related events.
pub mod keys {
    /// Path string field used in file bus events.
    pub const FILE: &str = "file";
    /// Event kind field used in file-watcher update events.
    pub const EVENT: &str = "event";
}

/// File watcher event kinds surfaced in `file_watcher.updated` bus events.
///
/// Wire format: lowercase strings (`"add"`, `"change"`, `"unlink"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum FileWatcherEventKind {
    Add,
    Change,
    Unlink,
}

impl std::fmt::Display for FileWatcherEventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FileWatcherEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Change => "change",
            Self::Unlink => "unlink",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_watcher_event_kind_round_trips() {
        let values: &[FileWatcherEventKind] = &[
            FileWatcherEventKind::Add,
            FileWatcherEventKind::Change,
            FileWatcherEventKind::Unlink,
        ];
        for value in values {
            assert_eq!(
                FileWatcherEventKind::parse(value.as_str()),
                Some(*value)
            );
            assert_eq!(value.to_string(), value.as_str());
        }
    }
}

