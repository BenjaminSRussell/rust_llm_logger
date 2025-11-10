mod parsers;
mod proxy;
mod middleware;
mod types;

use axum::{routing::any, Router};
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rust_llm_logger=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Create shared HTTP client for proxying
    let client = Arc::new(create_http_client());

    // Build the application router
    let app = Router::new()
        .route("/proxy/:backend_port/*path", any(proxy::proxy_handler))
        .layer(axum::middleware::from_fn(middleware::extract_request_data))
        .layer(TraceLayer::new_for_http())
        .with_state(client);

    // Start the server
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("Failed to bind to port 3000");

    tracing::info!("LLM Logging Proxy listening on {}", listener.local_addr().unwrap());

    axum::serve(listener, app)
        .await
        .expect("Server failed");
}

fn create_http_client() -> hyper_util::client::legacy::Client<
    hyper_util::client::legacy::connect::HttpConnector,
    axum::body::Body,
> {
    hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http()
}
