use async_trait::async_trait;
use bytes::Bytes;

use crate::parsers::BackendStreamParser;
use crate::types::TokenUsage;

/// Passthrough parser that doesn't extract any metrics
/// Used for unknown backend types
pub struct PassthroughParser;

#[async_trait]
impl BackendStreamParser for PassthroughParser {
    async fn feed_chunk(&mut self, _chunk: &Bytes) {
        // Do nothing - just pass through
    }

    async fn finalize(self: Box<Self>) -> TokenUsage {
        TokenUsage::default()
    }
}
