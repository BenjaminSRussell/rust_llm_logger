use axum::{
    body::Body,
    extract::{Path, Request, State},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use http_body_util::{BodyExt, StreamBody};
use hyper::StatusCode;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::parsers::{detect_backend_type, BackendStreamParser, BackendType};
use crate::types::{LLMMetrics, RequestData};

type HttpClient = hyper_util::client::legacy::Client<
    hyper_util::client::legacy::connect::HttpConnector,
    Body,
>;

/// Main proxy handler that routes to different backends
pub async fn proxy_handler(
    State(client): State<Arc<HttpClient>>,
    Path((backend_port, path)): Path<(u16, String)>,
    req: Request,
) -> Response {
    // Start latency timer
    let start_time = tokio::time::Instant::now();

    // Extract request data from extensions (added by middleware)
    let request_data = req.extensions().get::<RequestData>().cloned();

    // Construct the upstream URI
    let upstream_uri = format!("http://127.0.0.1:{}/{}", backend_port, path.trim_start_matches('/'));

    // Add query string if present
    let upstream_uri = if let Some(query) = req.uri().query() {
        format!("{}?{}", upstream_uri, query)
    } else {
        upstream_uri
    };

    tracing::debug!("Proxying request to: {}", upstream_uri);

    // Parse the URI
    let uri = match upstream_uri.parse::<hyper::Uri>() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("Failed to parse upstream URI: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Invalid upstream URI").into_response();
        }
    };

    // Build the upstream request
    let (mut parts, body) = req.into_parts();
    parts.uri = uri;

    // Remove host header to avoid conflicts
    parts.headers.remove("host");

    let upstream_request = hyper::Request::from_parts(parts, body);

    // Send request to upstream
    let upstream_response = match client.request(upstream_request).await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("Failed to proxy request: {}", e);
            return (StatusCode::BAD_GATEWAY, format!("Upstream error: {}", e)).into_response();
        }
    };

    // Extract response parts
    let (parts, body) = upstream_response.into_parts();
    let content_type = parts
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Detect backend type from content-type
    let backend_type = detect_backend_type(content_type);

    tracing::debug!("Detected backend type: {:?}, content-type: {}", backend_type, content_type);

    // Create the stream-tee architecture
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(32);

    // Spawn task to handle stream inspection
    let request_data_clone = request_data.clone();
    tokio::spawn(async move {
        handle_stream_tee(
            body,
            tx,
            backend_type,
            request_data_clone,
            start_time,
        )
        .await;
    });

    // Create the response body from the receiver
    let stream = ReceiverStream::new(rx);
    let body = StreamBody::new(stream.map(|result| {
        result.map(hyper::body::Frame::data)
    }));

    // Reconstruct the response
    Response::from_parts(parts, Body::new(body))
}

/// Handles the stream-tee: forwards chunks to client and parser simultaneously
async fn handle_stream_tee(
    mut upstream_body: hyper::body::Incoming,
    client_tx: mpsc::Sender<Result<Bytes, std::io::Error>>,
    backend_type: BackendType,
    request_data: Option<RequestData>,
    start_time: tokio::time::Instant,
) {
    // Create the appropriate parser
    let mut parser: Box<dyn BackendStreamParser> = match backend_type {
        BackendType::Ollama => Box::new(crate::parsers::OllamaParser::new()),
        BackendType::OpenAI => Box::new(crate::parsers::OpenAIParser::new()),
        BackendType::Unknown => Box::new(crate::parsers::PassthroughParser),
    };

    // Process the stream
    loop {
        match upstream_body.frame().await {
            Some(Ok(frame)) => {
                if let Ok(data) = frame.into_data() {
                    // Feed chunk to parser (non-blocking)
                    parser.feed_chunk(&data).await;

                    // Forward chunk to client
                    if client_tx.send(Ok(data)).await.is_err() {
                        tracing::debug!("Client disconnected");
                        break;
                    }
                }
            }
            Some(Err(e)) => {
                tracing::error!("Error reading upstream body: {}", e);
                let _ = client_tx.send(Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))).await;
                break;
            }
            None => {
                // Stream ended normally
                break;
            }
        }
    }

    // Finalize parser and get token usage
    let token_usage = parser.finalize().await;

    // Calculate final latency
    let latency = start_time.elapsed();

    // Log the metrics
    if let Some(req_data) = request_data {
        let metrics = LLMMetrics {
            model: req_data.model,
            prompt: req_data.prompt,
            prompt_tokens: token_usage.prompt_tokens,
            completion_tokens: token_usage.completion_tokens,
            latency_ms: latency.as_millis() as u64,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        tracing::info!(
            "LLM Request Complete: model={}, prompt_tokens={:?}, completion_tokens={:?}, latency_ms={}",
            metrics.model,
            metrics.prompt_tokens,
            metrics.completion_tokens,
            metrics.latency_ms
        );

        // Here you could write to a database, file, or other logging backend
        if let Ok(json) = serde_json::to_string_pretty(&metrics) {
            tracing::info!("Metrics: {}", json);
        }
    }
}
