mod anthropic;
mod json;
mod ollama;
mod openai;
mod passthrough;
pub mod sse;
pub mod usage;

pub use anthropic::AnthropicParser;
pub use json::JsonResponseParser;
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

/// Detected response format
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackendType {
    /// `application/x-ndjson` (Ollama streaming)
    Ollama,
    /// `text/event-stream` from an OpenAI-compatible server
    OpenAI,
    /// `text/event-stream` from the Anthropic Messages API
    Anthropic,
    /// Non-streaming `application/json` (OpenAI, Anthropic or Ollama)
    Json,
    Unknown,
}

/// Detect the response format from the content type alone.
pub fn detect_backend_type(content_type: &str) -> BackendType {
    detect_backend(content_type, "")
}

/// Detect the response format from the content type plus the request path
/// (`/v1/messages` is Anthropic's streaming endpoint).
pub fn detect_backend(content_type: &str, request_path: &str) -> BackendType {
    let ct = content_type.to_ascii_lowercase();
    if ct.contains("application/x-ndjson") {
        BackendType::Ollama
    } else if ct.contains("text/event-stream") {
        if request_path.trim_end_matches('/').ends_with("/v1/messages") {
            BackendType::Anthropic
        } else {
            BackendType::OpenAI
        }
    } else if ct.contains("application/json") {
        BackendType::Json
    } else {
        BackendType::Unknown
    }
}

/// Build the parser for a detected format.
pub fn parser_for(backend: BackendType) -> Box<dyn BackendStreamParser> {
    match backend {
        BackendType::Ollama => Box::new(OllamaParser::new()),
        BackendType::OpenAI => Box::new(OpenAIParser::new()),
        BackendType::Anthropic => Box::new(AnthropicParser::new()),
        BackendType::Json => Box::new(JsonResponseParser::new()),
        BackendType::Unknown => Box::new(PassthroughParser),
    }
}
