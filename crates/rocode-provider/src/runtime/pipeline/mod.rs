pub mod decode;
pub mod event_map;

use bytes::Bytes;
use futures::{stream, Stream, StreamExt};
use std::pin::Pin;

use crate::driver::StreamingEvent;
use crate::protocol_loader::ProtocolManifest;
use crate::runtime::pipeline::decode::SseDecoder;
use crate::runtime::pipeline::event_map::PathEventMapper;
use crate::ProviderError;

pub struct Pipeline {
    decoder: SseDecoder,
    mapper: PathEventMapper,
}

impl Pipeline {
    pub fn from_manifest(manifest: &ProtocolManifest) -> Result<Self, ProviderError> {
        let decoder = manifest
            .streaming
            .as_ref()
            .map(|s| SseDecoder::from_config(&s.decoder))
            .unwrap_or_else(SseDecoder::default_sse);
        let mapper = PathEventMapper::from_manifest(manifest);
        Ok(Self { decoder, mapper })
    }

    pub fn openai_default() -> Self {
        Self {
            decoder: SseDecoder::default_sse(),
            mapper: PathEventMapper::openai_defaults(),
        }
    }

    pub fn anthropic_default() -> Self {
        Self {
            decoder: SseDecoder::default_sse(),
            mapper: PathEventMapper::anthropic_defaults(),
        }
    }

    pub fn google_default() -> Self {
        Self {
            decoder: SseDecoder::default_sse(),
            mapper: PathEventMapper::google_defaults(),
        }
    }

    pub fn vertex_default() -> Self {
        Self {
            decoder: SseDecoder::default_sse(),
            mapper: PathEventMapper::vertex_defaults(),
        }
    }

    pub fn for_provider(provider_id: &str) -> Self {
        let id = provider_id.to_ascii_lowercase();
        if id.contains("anthropic") {
            Self::anthropic_default()
        } else if id.contains("google-vertex") || id.contains("vertex") {
            Self::vertex_default()
        } else if id.contains("google") || id.contains("gemini") {
            Self::google_default()
        } else {
            Self::openai_default()
        }
    }

    pub fn process_stream(
        &self,
        input: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamingEvent, ProviderError>> + Send>> {
        let decoded = self.decoder.decode_stream(input);
        let mapper = self.mapper.clone();

        let mapped = decoded.flat_map(move |frame_result| match frame_result {
            Ok(frame) => {
                let events: Vec<Result<StreamingEvent, ProviderError>> =
                    mapper.map_frame(&frame).into_iter().map(Ok).collect();
                stream::iter(events)
            }
            Err(err) => stream::iter(vec![Err(err)]),
        });

        Box::pin(mapped)
    }
}
