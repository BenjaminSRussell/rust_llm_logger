use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Data extracted from the request body
#[derive(Clone, Debug)]
pub struct RequestData {
    pub model: String,
    pub prompt: String,
}

/// Token usage information
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TokenUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
}

impl TokenUsage {
    pub fn new(prompt_tokens: Option<u32>, completion_tokens: Option<u32>) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
        }
    }
}

/// How a proxied call finished
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallOutcome {
    Ok,
    UpstreamError,
    ClientAborted,
    UpstreamStreamError,
}

/// Complete metrics for a single LLM request
#[derive(Clone, Debug, Serialize)]
pub struct LLMMetrics {
    pub model: String,
    pub prompt: String,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub latency_ms: u64,
    pub ttft_ms: Option<u64>,
    pub status: u16,
    pub outcome: CallOutcome,
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
    #[serde(default)]
    pub role: String,
    /// OpenAI allows string, array of parts, or null (tool-call assistants).
    #[serde(default)]
    pub content: Value,
}

/// Pull human-readable text out of a message content Value.
pub fn content_to_text(content: &Value) -> String {
    match content {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                if let Some(s) = p.as_str() {
                    return Some(s.to_string());
                }
                let obj = p.as_object()?;
                if obj.get("type").and_then(|t| t.as_str()) == Some("text") {
                    return obj.get("text").and_then(|t| t.as_str()).map(|s| s.to_string());
                }
                None
            })
            .collect::<Vec<_>>()
            .join(" "),
        other => other.to_string(),
    }
}
