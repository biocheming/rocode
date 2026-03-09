//! A generic `ModelCaller` implementation that wraps any `rocode_provider::Provider`.
//!
//! This eliminates the need for each consumer (session, server, compaction) to
//! implement its own near-identical ModelCaller. Per Constitution Article 1,
//! the execution kernel's adapter types should be written once.

use std::sync::Arc;

use crate::runtime::events::{LoopError as RuntimeLoopError, LoopRequest};
use crate::runtime::traits::ModelCaller;
use rocode_provider::{ChatRequest, Provider, StreamResult};

/// Configuration for building `ChatRequest` from `LoopRequest`.
#[derive(Clone)]
pub struct SimpleModelCallerConfig {
    pub model_id: String,
    pub max_tokens: u64,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub variant: Option<String>,
}

/// A reusable `ModelCaller` that translates `LoopRequest` → `ChatRequest` using
/// a `Provider` and `SimpleModelCallerConfig`. Covers the common case shared by
/// session, server, and compaction callers.
pub struct SimpleModelCaller {
    pub provider: Arc<dyn Provider>,
    pub config: SimpleModelCallerConfig,
}

#[async_trait::async_trait]
impl ModelCaller for SimpleModelCaller {
    async fn call_stream(
        &self,
        req: LoopRequest,
    ) -> std::result::Result<StreamResult, RuntimeLoopError> {
        let request = ChatRequest {
            model: self.config.model_id.clone(),
            messages: req.messages,
            max_tokens: Some(self.config.max_tokens),
            temperature: self.config.temperature,
            top_p: self.config.top_p,
            system: None,
            tools: if req.tools.is_empty() {
                None
            } else {
                Some(req.tools)
            },
            stream: Some(true),
            variant: self.config.variant.clone(),
            provider_options: None,
        };
        self.provider
            .chat_stream(request)
            .await
            .map_err(|error| RuntimeLoopError::ModelError(error.to_string()))
    }
}
