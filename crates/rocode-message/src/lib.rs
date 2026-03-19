#![forbid(unsafe_code)]

//! Canonical message + part model shared across rocode crates.
//!
//! This crate intentionally focuses on:
//! - clear conversation message semantics (`SessionMessage` + `MessagePart`)
//! - activity/tool part variants in one place (`PartType`)
//! - stable wire-facing tags (`snake_case`) with legacy aliases for reads

mod finish;
mod id;
mod message;
mod part;
mod role;
mod status;
mod usage;

pub use finish::{normalize_finish_reason, FinishReason};
pub use message::{Message, SessionMessage};
pub use part::{CompletedTime, ErrorTime, MessagePart, PartKind, PartType, RunningTime, ToolState};
pub use role::MessageRole;
pub use status::ToolCallStatus;
pub use usage::MessageUsage;
