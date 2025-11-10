use async_trait::async_trait;
use bytes::{Bytes, BytesMut};

use crate::parsers::BackendStreamParser;
use crate::types::{OllamaStreamResponse, TokenUsage};

/// Parser for Ollama's NDJSON streaming format
pub struct OllamaParser {
    buffer: BytesMut,
    token_usage: TokenUsage,
}

impl OllamaParser {
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::new(),
            token_usage: TokenUsage::default(),
        }
    }

    /// Process complete lines from the buffer
    fn process_lines(&mut self) {
        while let Some(newline_pos) = self.buffer.iter().position(|&b| b == b'\n') {
            // Extract the line
            let line = self.buffer.split_to(newline_pos + 1);

            // Skip empty lines
            if line.trim_ascii().is_empty() {
                continue;
            }

            // Try to parse as JSON
            if let Ok(response) = serde_json::from_slice::<OllamaStreamResponse>(&line) {
                tracing::debug!("Parsed Ollama response: done={}, prompt_eval_count={:?}, eval_count={:?}",
                    response.done,
                    response.prompt_eval_count,
                    response.eval_count
                );

                // If this is the final response with the "done" flag, extract token counts
                if response.done {
                    if response.prompt_eval_count.is_some() {
                        self.token_usage.prompt_tokens = response.prompt_eval_count;
                    }
                    if response.eval_count.is_some() {
                        self.token_usage.completion_tokens = response.eval_count;
                    }
                }
            } else {
                tracing::debug!("Failed to parse Ollama JSON line: {:?}", String::from_utf8_lossy(&line));
            }
        }
    }
}

#[async_trait]
impl BackendStreamParser for OllamaParser {
    async fn feed_chunk(&mut self, chunk: &Bytes) {
        // Append chunk to buffer
        self.buffer.extend_from_slice(chunk);

        // Process any complete lines
        self.process_lines();
    }

    async fn finalize(mut self: Box<Self>) -> TokenUsage {
        // Process any remaining data in the buffer
        if !self.buffer.is_empty() {
            // Try to parse the remaining buffer as a final JSON object
            if let Ok(response) = serde_json::from_slice::<OllamaStreamResponse>(&self.buffer) {
                if response.done {
                    if response.prompt_eval_count.is_some() {
                        self.token_usage.prompt_tokens = response.prompt_eval_count;
                    }
                    if response.eval_count.is_some() {
                        self.token_usage.completion_tokens = response.eval_count;
                    }
                }
            }
        }

        self.token_usage
    }
}
