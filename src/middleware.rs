use axum::{
    body::Body,
    extract::Request,
    middleware::Next,
    response::Response,
};
use http_body_util::BodyExt;

use crate::types::{GenericRequest, RequestData};

/// Extracts model and prompt from the request body, then reconstructs the body
pub async fn extract_request_data(mut req: Request, next: Next) -> Response {
    // Read the entire body
    let body = req.body_mut();
    let collected = match body.collect().await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to read request body: {}", e);
            return Response::builder()
                .status(400)
                .body(Body::from("Failed to read request body"))
                .unwrap();
        }
    };

    let body_bytes = collected.to_bytes();

    // Try to parse the request body
    if let Ok(parsed) = serde_json::from_slice::<GenericRequest>(&body_bytes) {
        let prompt = extract_prompt(&parsed);
        let model = parsed.model.unwrap_or_else(|| "unknown".to_string());

        // Store the extracted data in request extensions
        req.extensions_mut().insert(RequestData {
            model,
            prompt,
            raw_body: body_bytes.clone(),
        });
    } else {
        tracing::warn!("Failed to parse request body as JSON, storing raw body");
        req.extensions_mut().insert(RequestData {
            model: "unknown".to_string(),
            prompt: "unparseable".to_string(),
            raw_body: body_bytes.clone(),
        });
    }

    // Reconstruct the body so the proxy handler can forward it
    *req.body_mut() = Body::from(body_bytes);

    next.run(req).await
}

/// Extracts the prompt from either the prompt field or messages field
fn extract_prompt(request: &GenericRequest) -> String {
    if let Some(prompt) = &request.prompt {
        prompt.clone()
    } else if let Some(messages) = &request.messages {
        // Concatenate all message contents
        messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        "no prompt found".to_string()
    }
}
