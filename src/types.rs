use serde::{Deserialize, Serialize};

/// Data extracted from the request body
#[derive(Clone, Debug)]
pub struct RequestData {
    pub model: String,
    pub prompt: String,
    #[allow(dead_code)]
    pub raw_body: bytes::Bytes,
}

/// Token usage information
#[derive(Clone, Debug, Default)]
pub struct TokenUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
}

/// Complete metrics for a single LLM request
#[derive(Clone, Debug, Serialize)]
pub struct LLMMetrics {
    pub model: String,
    pub prompt: String,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub latency_ms: u64,
    pub timestamp: String,
}

/// Ollama streaming response format
#[derive(Debug, Deserialize)]
pub struct OllamaStreamResponse {
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub prompt_eval_count: Option<u32>,
    #[serde(default)]
    pub eval_count: Option<u32>,
}

/// OpenAI-compatible usage format
#[derive(Debug, Deserialize)]
pub struct OpenAIUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// OpenAI-compatible response format
#[derive(Debug, Deserialize)]
pub struct OpenAIResponse {
    pub usage: Option<OpenAIUsage>,
}

/// Generic request body for extracting model and prompt
#[derive(Debug, Deserialize)]
pub struct GenericRequest {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub messages: Option<Vec<Message>>,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}
