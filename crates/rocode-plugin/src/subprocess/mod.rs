//! TypeScript plugin subprocess support.
//!
//! This module provides the ability to load TS/JS plugins by spawning a
//! `plugin-host.ts` child process and communicating over Content-Length
//! framed JSON-RPC 2.0 (stdin/stdout).

pub mod auth;
pub mod client;
pub mod loader;
pub mod protocol;
pub mod runtime;

// Re-exports for convenience
pub use auth::{
    PluginAuthBridge, PluginAuthError, PluginFetchRequest, PluginFetchResponse,
    PluginFetchStreamResponse,
};
pub use client::{PluginContext, PluginSubprocess, PluginSubprocessError};
pub use loader::{
    get_tool_call_tracking, remove_tool_call_tracking, track_tool_call, PluginLoader,
    PluginLoaderError, PluginToolCallRef,
};
pub use runtime::{detect_runtime, JsRuntime};

pub use crate::hook_names::hook_name_to_event;
