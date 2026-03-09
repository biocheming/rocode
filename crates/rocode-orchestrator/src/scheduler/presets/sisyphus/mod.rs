use serde_json::json;

pub fn sisyphus_workflow_todos_payload() -> serde_json::Value {
    json!({
        "todos": [
            { "id": "sisyphus-1", "content": "Classify intent and choose the execution path", "status": "pending", "priority": "high" },
            { "id": "sisyphus-2", "content": "Assess the codebase shape before following patterns", "status": "pending", "priority": "high" },
            { "id": "sisyphus-3", "content": "Explore and research in parallel before committing", "status": "pending", "priority": "high" },
            { "id": "sisyphus-4", "content": "Execute or delegate with explicit task tracking", "status": "pending", "priority": "high" },
            { "id": "sisyphus-5", "content": "Verify evidence and report concrete outcomes", "status": "pending", "priority": "high" }
        ]
    })
}

mod api;
mod definition;
mod input;
mod output;
mod prompt;
#[cfg(test)]
mod tests;

pub use api::*;
pub use definition::*;
pub use input::*;
pub use output::*;
pub use prompt::*;
