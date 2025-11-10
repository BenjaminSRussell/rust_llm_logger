// tests/parsers.rs

use bytes::Bytes;
use rust_llm_logger::parsers::{BackendStreamParser, OllamaParser};
use rust_llm_logger::types::TokenUsage;

#[tokio::test]
async fn test_ollama_parser_missing_prompt_tokens() {
    // This test simulates a scenario where the final Ollama chunk is missing
    // the `prompt_eval_count` field, which was causing a bug where
    // `completion_tokens` were incorrectly reported as 0.

    let mut parser: Box<dyn BackendStreamParser> = Box::new(OllamaParser::new());

    // Simulate a stream with a final chunk missing `prompt_eval_count`
    let chunk1 = Bytes::from_static(br#"{"model":"llama2","created_at":"2025-11-09T12:34:56.789Z","response":"hello","done":false}
"#);
    let chunk2 = Bytes::from_static(br#"{"model":"llama2","created_at":"2025-11-09T12:34:57.789Z","response":" world","done":false}
"#);
    // The final chunk has `eval_count` (completion tokens) but is missing `prompt_eval_count`
    let final_chunk = Bytes::from_static(br#"{"model":"llama2","created_at":"2025-11-09T12:34:58.789Z","response":"","done":true,"eval_count":42}
"#);

    // Feed chunks to the parser
    parser.feed_chunk(&chunk1).await;
    parser.feed_chunk(&chunk2).await;
    parser.feed_chunk(&final_chunk).await;

    // Finalize and get the results
    let usage = parser.finalize().await;

    // Before the fix, `completion_tokens` would be `None` because `prompt_tokens` was `None`.
    // The fix ensures that `completion_tokens` is correctly parsed and returned.
    assert_eq!(
        usage,
        TokenUsage {
            prompt_tokens: None,
            completion_tokens: Some(42),
        },
        "Parser should correctly extract completion_tokens even when prompt_tokens is missing"
    );
}
