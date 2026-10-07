use async_trait::async_trait;
use bytes::Bytes;

use crate::parsers::sse::SseFramer;
use crate::parsers::usage::{anthropic_usage, merge};
use crate::parsers::BackendStreamParser;
use crate::types::TokenUsage;

/// Parser for Anthropic Messages SSE streams (`POST /v1/messages`, `stream: true`).
///
/// `message_start` carries `message.usage.input_tokens`; the closing `message_delta`
/// carries the cumulative `usage.output_tokens`.
#[derive(Default)]
pub struct AnthropicParser {
    framer: SseFramer,
    token_usage: TokenUsage,
}

impl AnthropicParser {
    pub fn new() -> Self {
        Self::default()
    }

    fn handle(&mut self, events: Vec<String>) {
        for data in events {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(u) = anthropic_usage(&v) {
                    merge(&mut self.token_usage, u);
                }
            }
        }
    }
}

#[async_trait]
impl BackendStreamParser for AnthropicParser {
    async fn feed_chunk(&mut self, chunk: &Bytes) {
        let events = self.framer.push(chunk);
        self.handle(events);
    }

    async fn finalize(mut self: Box<Self>) -> TokenUsage {
        let events = self.framer.finish();
        self.handle(events);
        self.token_usage
    }
}
