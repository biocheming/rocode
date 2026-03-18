use strum_macros::EnumString;

/// Shared attachment payload + metadata contracts.
///
/// Attachments are surfaced across:
/// - tool metadata (`attachment` / `attachments`)
/// - message parts (tool call inputs may include attachments)
/// - UI layers (CLI/TUI/Web) for rich previews
///
/// Keep these keys stable — they form a cross-crate contract.
pub mod keys {
    /// Tool/message metadata key for a single attachment payload.
    pub const ATTACHMENT: &str = "attachment";
    /// Tool/message metadata key for a list of attachment payloads.
    pub const ATTACHMENTS: &str = "attachments";

    /// Attachment object field: type discriminator.
    pub const TYPE: &str = "type";
    /// Attachment object field: stable identifier (optional).
    pub const ID: &str = "id";

    /// Attachment object field: media type (e.g. `"image/png"`).
    pub const MIME: &str = "mime";
    /// Attachment object field: URL or data URL (e.g. `"file:///..."`, `"data:..."`).
    pub const URL: &str = "url";
    /// Attachment object field: filename hint.
    pub const FILENAME: &str = "filename";

    /// Attachment object field: local path (used when output is persisted to disk).
    pub const PATH: &str = "path";

    /// Attachment object field: original byte length (for demoted large outputs).
    pub const ORIGINAL_BYTES: &str = "original_bytes";
    /// Attachment object field: original line count (for demoted large outputs).
    pub const ORIGINAL_LINES: &str = "original_lines";
}

/// Canonical attachment type discriminator strings.
///
/// Wire format: lowercase strings (`"file"`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum AttachmentTypeWire {
    File,
}

impl std::fmt::Display for AttachmentTypeWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AttachmentTypeWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        value.trim().parse().ok()
    }
}

