#[path = "app.rs"]
mod app_impl;
mod state;
pub(crate) mod terminal;

pub use app_impl::{App, RunOutcome};
pub use state::AppState;
