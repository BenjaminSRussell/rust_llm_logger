use rust_llm_logger::{middleware, proxy};

use axum::{
    extract::Request,
    middleware::{from_fn, from_fn_with_state, Next},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use clap::Parser;
use std::collections::HashSet;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// LLM logging reverse proxy.
#[derive(Debug, Parser)]
#[command(name = "rust_llm_logger", about = "LLM request logging proxy")]
struct Args {
    /// Bind address (env: LLM_LOGGER_BIND)
    #[arg(long, env = "LLM_LOGGER_BIND", default_value = "127.0.0.1:3000")]
    bind: String,

    /// SQLite path for persisted metrics (env: METRICS_DB); empty disables
    #[arg(long, env = "METRICS_DB", default_value = "")]
    metrics_db: String,

    /// Comma-separated backend ports allowed for /proxy/{port}/...
    /// (env: ALLOWED_BACKEND_PORTS). Default: common local LLM ports.
    #[arg(
        long,
        env = "ALLOWED_BACKEND_PORTS",
        default_value = "11434,8080,8000,5000"
    )]
    allowed_backend_ports: String,

    /// Add `stream_options.include_usage=true` to streamed OpenAI-compatible requests
    /// that omit it, so token usage is reported (env: LLM_LOGGER_INJECT_STREAM_USAGE)
    #[arg(long, env = "LLM_LOGGER_INJECT_STREAM_USAGE", default_value_t = false)]
    inject_stream_usage: bool,

    /// Log filter when RUST_LOG is unset
    #[arg(
        long,
        env = "LLM_LOGGER_LOG",
        default_value = "rust_llm_logger=info,tower_http=info"
    )]
    log_filter: String,
}

fn parse_allow_list(raw: &str) -> Result<HashSet<u16>, String> {
    let mut set = HashSet::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let port: u16 = part
            .parse()
            .map_err(|_| format!("invalid port in allow-list: {part}"))?;
        if port == 0 {
            return Err("port 0 is not allowed".into());
        }
        set.insert(port);
    }
    if set.is_empty() {
        return Err("ALLOWED_BACKEND_PORTS allow-list is empty".into());
    }
    Ok(set)
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let allow = match parse_allow_list(&args.allowed_backend_ports) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("config error: {e}");
            std::process::exit(2);
        }
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .or_else(|_| tracing_subscriber::EnvFilter::try_new(&args.log_filter))
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    if !args.metrics_db.is_empty() {
        tracing::info!(path = %args.metrics_db, "METRICS_DB configured");
    }

    middleware::set_inject_stream_usage(args.inject_stream_usage);

    let client = Arc::new(create_http_client());

    let app = Router::new()
        .route("/proxy/:backend_port/*path", any(proxy::proxy_handler))
        .layer(from_fn(middleware::extract_request_data))
        .layer(from_fn_with_state(
            Arc::clone(&allow),
            allow_port_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(client);

    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind {}: {e}", args.bind));

    tracing::info!(
        "LLM Logging Proxy listening on {} (allowed ports: {:?})",
        listener.local_addr().unwrap(),
        allow
    );

    axum::serve(listener, app).await.expect("Server failed");
}

async fn allow_port_middleware(
    axum::extract::State(allow): axum::extract::State<Arc<HashSet<u16>>>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    if let Some(rest) = path.strip_prefix("/proxy/") {
        let port_str = rest.split('/').next().unwrap_or("");
        if let Ok(port) = port_str.parse::<u16>() {
            if !allow.contains(&port) {
                return (
                    axum::http::StatusCode::FORBIDDEN,
                    format!("backend port {port} not in ALLOWED_BACKEND_PORTS"),
                )
                    .into_response();
            }
        } else if !port_str.is_empty() {
            return (axum::http::StatusCode::BAD_REQUEST, "invalid backend port").into_response();
        }
    }
    next.run(req).await
}

fn create_http_client() -> hyper_util::client::legacy::Client<
    hyper_util::client::legacy::connect::HttpConnector,
    axum::body::Body,
> {
    hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new()).build_http()
}

#[cfg(test)]
mod tests {
    use super::parse_allow_list;

    #[test]
    fn parses_ports() {
        let s = parse_allow_list("11434, 8080").unwrap();
        assert!(s.contains(&11434) && s.contains(&8080));
    }

    #[test]
    fn rejects_empty() {
        assert!(parse_allow_list("").is_err());
        assert!(parse_allow_list(",,").is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_allow_list("abc").is_err());
    }
}
