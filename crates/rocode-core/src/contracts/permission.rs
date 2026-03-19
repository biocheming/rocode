/// Common metadata keys used by permission requests across tools and UI layers.
///
/// Canonical permission models live in `rocode-permission`.
pub mod keys {
    /// Human-readable permission prompt description.
    pub const DESCRIPTION: &str = "description";
    /// Alternate prompt key used by some tools.
    pub const QUESTION: &str = "question";
    /// Command string that triggered the permission request.
    pub const COMMAND: &str = "command";

    /// Permission request input JSON field: permission name.
    pub const REQUEST_PERMISSION: &str = "permission";
    /// Permission request input JSON field: patterns array.
    pub const REQUEST_PATTERNS: &str = "patterns";
    /// Permission request input JSON field: metadata object.
    pub const REQUEST_METADATA: &str = "metadata";
    /// Permission request input JSON field: always allow flag.
    pub const REQUEST_ALWAYS: &str = "always";
}
