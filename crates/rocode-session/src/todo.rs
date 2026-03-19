use rocode_core::bus::{Bus, BusEventDef};
use rocode_core::contracts::events::BusEventName;
use rocode_core::contracts::todo::keys as todo_keys;
use rocode_core::contracts::wire::keys as wire_keys;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TodoInfo {
    pub content: String,
    pub status: String,
    pub priority: String,
}

pub struct TodoManager {
    state: Arc<RwLock<HashMap<String, Vec<TodoInfo>>>>,
    bus: Option<Arc<Bus>>,
}

pub static TODO_UPDATED_EVENT: BusEventDef = BusEventDef::new(BusEventName::TodoUpdated.as_str());

impl TodoManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(HashMap::new())),
            bus: None,
        }
    }

    pub fn with_bus(bus: Arc<Bus>) -> Self {
        Self {
            state: Arc::new(RwLock::new(HashMap::new())),
            bus: Some(bus),
        }
    }

    pub async fn update(&self, session_id: &str, todos: Vec<TodoInfo>) {
        let todos_payload = todos.clone();

        let mut state = self.state.write().await;
        if todos.is_empty() {
            state.remove(session_id);
        } else {
            state.insert(session_id.to_string(), todos);
        }

        if let Some(ref bus) = self.bus {
            bus.publish(
                &TODO_UPDATED_EVENT,
                serde_json::json!({
                    wire_keys::SESSION_ID: session_id,
                    todo_keys::TODOS: todos_payload,
                }),
            )
            .await;
        }
    }

    pub async fn get(&self, session_id: &str) -> Vec<TodoInfo> {
        let state = self.state.read().await;
        state.get(session_id).cloned().unwrap_or_default()
    }

    pub async fn clear(&self, session_id: &str) {
        let mut state = self.state.write().await;
        state.remove(session_id);
    }

    pub async fn set_status(&self, session_id: &str, index: usize, status: &str) -> bool {
        let mut state = self.state.write().await;
        if let Some(todos) = state.get_mut(session_id) {
            if index < todos.len() {
                todos[index].status = status.to_string();

                return true;
            }
        }
        false
    }

    pub async fn add(&self, session_id: &str, todo: TodoInfo) {
        let mut state = self.state.write().await;
        state
            .entry(session_id.to_string())
            .or_insert_with(Vec::new)
            .push(todo);
    }

    pub async fn remove(&self, session_id: &str, index: usize) -> bool {
        let mut state = self.state.write().await;
        if let Some(todos) = state.get_mut(session_id) {
            if index < todos.len() {
                todos.remove(index);

                return true;
            }
        }
        false
    }
}

impl Default for TodoManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn todo_updated_event_is_published() {
        let bus = Arc::new(Bus::new());
        let mut rx = bus.subscribe_channel();
        let manager = TodoManager::with_bus(bus.clone());

        manager
            .update(
                "session-1",
                vec![TodoInfo {
                    content: "write tests".to_string(),
                    status: TodoStatus::Pending.as_str().to_string(),
                    priority: TodoPriority::High.as_str().to_string(),
                }],
            )
            .await;

        let event = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("event timeout")
            .expect("event channel closed");
        assert_eq!(event.event_type, TODO_UPDATED_EVENT.event_type);
        assert_eq!(event.properties[wire_keys::SESSION_ID], "session-1");
        assert_eq!(
            event.properties[todo_keys::TODOS][0][todo_keys::CONTENT],
            "write tests"
        );
    }
}

pub use rocode_core::contracts::todo::{TodoPriority, TodoStatus};

pub fn parse_status(status: &str) -> TodoStatus {
    TodoStatus::parse(status).unwrap_or(TodoStatus::Pending)
}

pub fn parse_priority(priority: &str) -> TodoPriority {
    TodoPriority::parse(priority).unwrap_or(TodoPriority::Medium)
}
