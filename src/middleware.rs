use axum::{
    body::Body,
    extract::Request,
    middleware::Next,
    response::Response,
};
use http_body_util::{BodyExt, Limited};

use crate::types::{content_to_text, GenericRequest, RequestData};

/// Default max request body size (10 MiB). Oversized bodies return 413.
pub const MAX_REQUEST_BODY_BYTES: usize = 10 * 1024 * 1024;

/// Extracts model and prompt from the request body, then reconstructs the body
pub async fn extract_request_data(mut req: Request, next: Next) -> Response {
    let body = std::mem::replace(req.body_mut(), Body::empty());
    let limited = Limited::new(body, MAX_REQUEST_BODY_BYTES);

    let collected = match limited.collect().await {
        Ok(c) => c,
        Err(e) => {
            let msg = e.to_string();
            // http_body_util::LengthLimitError surfaces when the cap is hit
            if msg.contains("length limit") || msg.contains("LengthLimit") {
                tracing::warn!("Request body exceeded {} bytes", MAX_REQUEST_BODY_BYTES);
                return Response::builder()
                    .status(413)
                    .body(Body::from("Request body too large"))
                    .unwrap();
            }
            tracing::error!("Failed to read request body: {}", e);
            return Response::builder()
                .status(400)
                .body(Body::from("Failed to read request body"))
                .unwrap();
        }
    };

    let body_bytes = collected.to_bytes();

    let (model, prompt) = parse_model_and_prompt(&body_bytes);

    req.extensions_mut().insert(RequestData { model, prompt });

    *req.body_mut() = Body::from(body_bytes);
    next.run(req).await
}

/// Parse model independently of messages so array/null content cannot wipe it.
pub fn parse_model_and_prompt(body_bytes: &[u8]) -> (String, String) {
    // Prefer a top-level model even when messages fail to deserialize.
    let model = serde_json::from_slice::<serde_json::Value>(body_bytes)
        .ok()
        .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    if let Ok(parsed) = serde_json::from_slice::<GenericRequest>(body_bytes) {
        let prompt = extract_prompt(&parsed);
        let model = parsed.model.unwrap_or(model);
        return (model, prompt);
    }

    tracing::warn!("Failed to parse full request body; model may still be known");
    (model, "unparseable".to_string())
}

fn extract_prompt(request: &GenericRequest) -> String {
    if let Some(prompt) = &request.prompt {
        prompt.clone()
    } else if let Some(messages) = &request.messages {
        messages
            .iter()
            .map(|m| format!("{}: {}", m.role, content_to_text(&m.content)))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        "no prompt found".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn array_and_null_content_keeps_model_and_text() {
        let body = serde_json::to_vec(&json!({
            "model": "llama3.2",
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "hello"},
                    {"type": "image_url", "image_url": {"url": "x"}}
                ]},
                {"role": "assistant", "content": null}
            ]
        })).unwrap();
        let (model, prompt) = parse_model_and_prompt(&body);
        assert_eq!(model, "llama3.2");
        assert!(prompt.contains("hello"), "{prompt}");
        assert!(prompt.contains("assistant:"), "{prompt}");
    }

    #[test]
    fn model_survives_when_only_top_level_parses() {
        // Intentionally weird messages that still leave model extractable
        let body = br#"{"model":"mistral","messages":"not-an-array"}"#;
        let (model, prompt) = parse_model_and_prompt(body);
        assert_eq!(model, "mistral");
        assert_eq!(prompt, "unparseable");
    }
}
