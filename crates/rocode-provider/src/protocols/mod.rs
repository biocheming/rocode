mod anthropic;
mod bedrock;
mod copilot;
mod gitlab;
mod google;
mod openai;
mod vertex;

use std::sync::Arc;

pub use anthropic::AnthropicProtocol;
pub use bedrock::BedrockProtocol;
pub use copilot::CopilotProtocol;
pub use gitlab::GitLabProtocol;
pub use google::GoogleProtocol;
pub use openai::OpenAIProtocol;
pub use vertex::VertexProtocol;

use crate::{Protocol, ProtocolImpl};

pub fn create_protocol_impl(protocol: Protocol) -> Arc<dyn ProtocolImpl> {
    match protocol {
        Protocol::OpenAI => Arc::new(OpenAIProtocol::new()),
        Protocol::Anthropic => Arc::new(AnthropicProtocol::new()),
        Protocol::Google => Arc::new(GoogleProtocol::new()),
        Protocol::Bedrock => Arc::new(BedrockProtocol::new()),
        Protocol::Vertex => Arc::new(VertexProtocol::new()),
        Protocol::GitHubCopilot => Arc::new(CopilotProtocol::new()),
        Protocol::GitLab => Arc::new(GitLabProtocol::new()),
    }
}
