use async_trait::async_trait;
use bytes::{Bytes, BytesMut};

use crate::parsers::BackendStreamParser;
use crate::types::{OpenAIResponse, TokenUsage};

/// Parser for OpenAI-compatible SSE (Server-Sent Events) format
pub struct OpenAIParser {
    buffer: BytesMut,
    token_usage: TokenUsage,
}

impl OpenAIParser {
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::new(),
            token_usage: TokenUsage::default(),
        }
    }

    /// Process SSE events from the buffer
    fn process_events(&mut self) {
        // SSE format uses "data: " prefix and "\n\n" as delimiter

        // Look for complete SSE messages (delimited by \n\n)
        loop {
            let buffer_str = String::from_utf8_lossy(&self.buffer);
            let pos = match buffer_str.find("\n\n") {
                Some(p) => p,
                None => break,
            };

            let event_block = self.buffer.split_to(pos + 2);
            let event_str = String::from_utf8_lossy(&event_block);

            // Process each line in the event block
            for line in event_str.lines() {
                let line = line.trim();

                // Skip empty lines and comments
                if line.is_empty() || line.starts_with(':') {
                    continue;
                }

                // Check for [DONE] marker
                if line == "data: [DONE]" {
                    tracing::debug!("Received [DONE] marker from OpenAI stream");
                    continue;
                }

                // Parse data: prefix
                if let Some(data) = line.strip_prefix("data: ") {
                    // Try to parse as JSON
                    if let Ok(response) = serde_json::from_str::<OpenAIResponse>(data) {
                        if let Some(usage) = response.usage {
                            tracing::debug!(
                                "Parsed OpenAI usage: prompt_tokens={}, completion_tokens={}",
                                usage.prompt_tokens,
                                usage.completion_tokens
                            );

                            self.token_usage.prompt_tokens = Some(usage.prompt_tokens);
                            self.token_usage.completion_tokens = Some(usage.completion_tokens);
                        }
                    } else {
                        // This is a normal delta chunk without usage info
                        tracing::trace!("Parsed OpenAI delta chunk (no usage info)");
                    }
                }
            }
        }
    }
}

#[async_trait]
impl BackendStreamParser for OpenAIParser {
    async fn feed_chunk(&mut self, chunk: &Bytes) {
        // Append chunk to buffer
        self.buffer.extend_from_slice(chunk);

        // Process any complete events
        self.process_events();
    }

    async fn finalize(mut self: Box<Self>) -> TokenUsage {
        // Process any remaining data in the buffer
        self.process_events();

        self.token_usage
    }
}
