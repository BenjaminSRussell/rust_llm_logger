use async_trait::async_trait;
use bytes::{Bytes, BytesMut};

use crate::parsers::usage::{anthropic_usage, ollama_usage, openai_usage};
use crate::parsers::BackendStreamParser;
use crate::types::TokenUsage;

/// Cap on buffered non-streaming bodies (matches the proxy's request cap).
const MAX_JSON_BODY: usize = 10 * 1024 * 1024;

/// Parser for non-streaming `application/json` responses.
///
/// Buffers the whole body and tries every known usage shape: OpenAI `usage.*_tokens`,
/// Anthropic `usage.input/output_tokens`, Ollama `done` + `*eval_count`.
#[derive(Default)]
pub struct JsonResponseParser {
    buffer: BytesMut,
    overflow: bool,
}

impl JsonResponseParser {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl BackendStreamParser for JsonResponseParser {
    async fn feed_chunk(&mut self, chunk: &Bytes) {
        if self.buffer.len() + chunk.len() > MAX_JSON_BODY {
            self.overflow = true;
            return;
        }
        self.buffer.extend_from_slice(chunk);
    }

    async fn finalize(self: Box<Self>) -> TokenUsage {
        if self.overflow {
            return TokenUsage::default();
        }
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&self.buffer) else {
            return TokenUsage::default();
        };
        openai_usage(&v)
            .or_else(|| anthropic_usage(&v))
            .or_else(|| ollama_usage(&v))
            .unwrap_or_default()
    }
}
