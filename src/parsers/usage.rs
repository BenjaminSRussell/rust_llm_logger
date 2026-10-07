//! Usage extraction for the JSON shapes we know about (OpenAI, Anthropic, Ollama).

use serde_json::Value;

use crate::types::TokenUsage;

fn as_u32(v: Option<&Value>) -> Option<u32> {
    v.and_then(|x| x.as_u64())
        .and_then(|n| u32::try_from(n).ok())
}

/// OpenAI-compatible: `{"usage": {"prompt_tokens": n, "completion_tokens": m}}`.
pub fn openai_usage(v: &Value) -> Option<TokenUsage> {
    let u = v.get("usage")?;
    let p = as_u32(u.get("prompt_tokens"));
    let c = as_u32(u.get("completion_tokens"));
    (p.is_some() || c.is_some()).then(|| TokenUsage::new(p, c))
}

/// Anthropic Messages: `usage.input_tokens` / `usage.output_tokens`, either at the
/// top level (non-streaming, `message_delta`) or under `message` (`message_start`).
pub fn anthropic_usage(v: &Value) -> Option<TokenUsage> {
    let u = v
        .get("usage")
        .or_else(|| v.get("message").and_then(|m| m.get("usage")))?;
    let p = as_u32(u.get("input_tokens"));
    let c = as_u32(u.get("output_tokens"));
    (p.is_some() || c.is_some()).then(|| TokenUsage::new(p, c))
}

/// Ollama final object: `done: true` with `prompt_eval_count` / `eval_count`.
pub fn ollama_usage(v: &Value) -> Option<TokenUsage> {
    if v.get("done").and_then(|d| d.as_bool()) != Some(true) {
        return None;
    }
    let p = as_u32(v.get("prompt_eval_count"));
    let c = as_u32(v.get("eval_count"));
    (p.is_some() || c.is_some()).then(|| TokenUsage::new(p, c))
}

/// Merge `new` into `acc`, keeping previously seen values when `new` lacks them.
pub fn merge(acc: &mut TokenUsage, new: TokenUsage) {
    if new.prompt_tokens.is_some() {
        acc.prompt_tokens = new.prompt_tokens;
    }
    if new.completion_tokens.is_some() {
        acc.completion_tokens = new.completion_tokens;
    }
}
