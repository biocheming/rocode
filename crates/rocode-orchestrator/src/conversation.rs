use crate::tool_runner::ToolCallInput;

#[derive(Debug, Clone, Default)]
pub struct OrchestratorConversation {
    messages: Vec<rocode_provider::Message>,
}

impl OrchestratorConversation {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    pub fn with_system_prompt(prompt: &str) -> Self {
        Self {
            messages: vec![rocode_provider::Message::system(prompt.to_string())],
        }
    }

    pub fn from_messages(messages: Vec<rocode_provider::Message>) -> Self {
        Self { messages }
    }

    pub fn load_messages(&mut self, messages: Vec<rocode_provider::Message>) {
        self.messages = messages;
    }

    pub fn add_user_message(&mut self, content: &str) {
        self.messages
            .push(rocode_provider::Message::user(content.to_string()));
    }

    pub fn add_assistant_message(&mut self, content: &str) {
        self.messages
            .push(rocode_provider::Message::assistant(content.to_string()));
    }

    pub fn add_assistant_with_tools(&mut self, content: &str, tool_calls: Vec<ToolCallInput>) {
        let mut parts = Vec::new();
        if !content.is_empty() {
            parts.push(rocode_provider::ContentPart {
                content_type: "text".to_string(),
                text: Some(content.to_string()),
                image_url: None,
                tool_use: None,
                tool_result: None,
                cache_control: None,
                filename: None,
                media_type: None,
                provider_options: None,
            });
        }

        for call in tool_calls {
            parts.push(rocode_provider::ContentPart {
                content_type: "tool_use".to_string(),
                text: None,
                image_url: None,
                tool_use: Some(rocode_provider::ToolUse {
                    id: call.id,
                    name: call.name,
                    input: call.arguments,
                }),
                tool_result: None,
                cache_control: None,
                filename: None,
                media_type: None,
                provider_options: None,
            });
        }

        self.messages.push(rocode_provider::Message {
            role: rocode_provider::Role::Assistant,
            content: rocode_provider::Content::Parts(parts),
            cache_control: None,
            provider_options: None,
        });
    }

    pub fn add_tool_result(&mut self, call_id: &str, _name: &str, content: String, is_error: bool) {
        self.messages.push(rocode_provider::Message {
            role: rocode_provider::Role::Tool,
            content: rocode_provider::Content::Parts(vec![rocode_provider::ContentPart {
                content_type: "tool_result".to_string(),
                text: None,
                image_url: None,
                tool_use: None,
                tool_result: Some(rocode_provider::ToolResult {
                    tool_use_id: call_id.to_string(),
                    content,
                    is_error: Some(is_error),
                }),
                cache_control: None,
                filename: None,
                media_type: None,
                provider_options: None,
            }]),
            cache_control: None,
            provider_options: None,
        });
    }

    pub fn messages(&self) -> &[rocode_provider::Message] {
        &self.messages
    }

    pub fn extend_messages(&mut self, messages: Vec<rocode_provider::Message>) {
        self.messages.extend(messages);
    }

    pub fn into_messages(self) -> Vec<rocode_provider::Message> {
        self.messages
    }
}
