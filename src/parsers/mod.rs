mod ollama;
mod openai;
mod passthrough;

pub use ollama::OllamaParser;
pub use openai::OpenAIParser;
pub use passthrough::PassthroughParser;

use async_trait::async_trait;
use bytes::Bytes;

use crate::types::TokenUsage;

/// Trait for parsing backend-specific streaming responses
#[async_trait]
pub trait BackendStreamParser: Send {
    /// Feed a chunk of data to the parser
    async fn feed_chunk(&mut self, chunk: &Bytes);

    /// Finalize parsing and return token usage
    async fn finalize(self: Box<Self>) -> TokenUsage;
}

/// Detected backend type based on content-type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackendType {
    Ollama,  // application/x-ndjson
    OpenAI,  // text/event-stream
    Unknown,
}

/// Detect backend type from content-type header
pub fn detect_backend_type(content_type: &str) -> BackendType {
    if content_type.contains("application/x-ndjson") || content_type.contains("application/json") {
        BackendType::Ollama
    } else if content_type.contains("text/event-stream") {
        BackendType::OpenAI
    } else {
        BackendType::Unknown
    }
}
