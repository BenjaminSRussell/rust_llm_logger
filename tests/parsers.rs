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

// ---- #12 / #8: JSON, SSE framing, Anthropic ----

use rust_llm_logger::parsers::{
    detect_backend, parser_for, AnthropicParser, BackendType, JsonResponseParser, OpenAIParser,
};

async fn run(mut p: Box<dyn BackendStreamParser>, chunks: &[&[u8]]) -> TokenUsage {
    for c in chunks {
        p.feed_chunk(&Bytes::copy_from_slice(c)).await;
    }
    p.finalize().await
}

#[test]
fn application_json_uses_json_parser() {
    assert_eq!(
        detect_backend("application/json; charset=utf-8", "/v1/chat/completions"),
        BackendType::Json
    );
    assert_eq!(
        detect_backend("application/x-ndjson", "/api/chat"),
        BackendType::Ollama
    );
    assert_eq!(
        detect_backend("text/event-stream", "/v1/messages"),
        BackendType::Anthropic
    );
    assert_eq!(
        detect_backend("text/event-stream", "/v1/chat/completions"),
        BackendType::OpenAI
    );
}

#[tokio::test]
async fn non_streaming_openai_json_yields_usage() {
    let body = br#"{"id":"x","choices":[{"message":{"content":"hi"}}],"usage":{"prompt_tokens":12,"completion_tokens":7,"total_tokens":19}}"#;
    let (a, b) = body.split_at(20);
    let usage = run(Box::new(JsonResponseParser::new()), &[a, b]).await;
    assert_eq!(usage, TokenUsage::new(Some(12), Some(7)));
}

#[tokio::test]
async fn non_streaming_ollama_json_yields_usage() {
    let body = br#"{"model":"llama3","message":{"content":"hi"},"done":true,"prompt_eval_count":5,"eval_count":9}"#;
    let usage = run(parser_for(BackendType::Json), &[body]).await;
    assert_eq!(usage, TokenUsage::new(Some(5), Some(9)));
}

#[tokio::test]
async fn non_streaming_anthropic_json_yields_usage() {
    let body = br#"{"type":"message","content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":21,"output_tokens":4}}"#;
    let usage = run(parser_for(BackendType::Json), &[body]).await;
    assert_eq!(usage, TokenUsage::new(Some(21), Some(4)));
}

#[tokio::test]
async fn sse_usage_split_inside_multibyte_char() {
    let stream = "data: {\"choices\":[{\"delta\":{\"content\":\"caf\u{e9}\"}}]}\n\n\
                  data: {\"choices\":[],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2},\"note\":\"\u{e9}\u{e9}\"}\n\n\
                  data: [DONE]\n\n";
    let bytes = stream.as_bytes();
    // Split right after the first byte of the 2-byte 'é' inside the usage event.
    let usage_start = stream.find("\"note\":\"").unwrap() + "\"note\":\"".len();
    let (a, b) = bytes.split_at(usage_start + 1);
    let usage = run(Box::new(OpenAIParser::new()), &[a, b]).await;
    assert_eq!(usage, TokenUsage::new(Some(3), Some(2)));
}

#[tokio::test]
async fn sse_crlf_and_no_space_data() {
    let stream = b"data:{\"choices\":[]}\r\n\r\ndata:{\"usage\":{\"prompt_tokens\":8,\"completion_tokens\":1}}\r\n\r\ndata:[DONE]\r\n\r\n";
    let usage = run(Box::new(OpenAIParser::new()), &[&stream[..]]).await;
    assert_eq!(usage, TokenUsage::new(Some(8), Some(1)));
}

#[tokio::test]
async fn sse_unterminated_final_event_is_flushed() {
    let stream = b"data: {\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":6}}";
    let usage = run(Box::new(OpenAIParser::new()), &[&stream[..]]).await;
    assert_eq!(usage, TokenUsage::new(Some(4), Some(6)));
}

#[tokio::test]
async fn anthropic_stream_usage() {
    let stream = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"usage\":{\"input_tokens\":25,\"output_tokens\":1}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":15}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let bytes = stream.as_bytes();
    let mid = bytes.len() / 2;
    let usage = run(
        Box::new(AnthropicParser::new()),
        &[&bytes[..7], &bytes[7..mid], &bytes[mid..]],
    )
    .await;
    assert_eq!(usage, TokenUsage::new(Some(25), Some(15)));
}
