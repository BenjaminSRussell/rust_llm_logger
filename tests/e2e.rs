//! End-to-end: real upstream (Ollama-style NDJSON) -> proxy -> SQLite / Prometheus / dashboard.
//! Covers #3 (row written), #4 (counters), #6 (dashboard populated), #7 (cost), #9 (no secrets), #11 (trace id).

use axum::{body::Body, http::Request, routing::post, Router};
use http_body_util::BodyExt;
use rust_llm_logger::{app, pricing::Pricing, store::Store, telemetry::Telemetry};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

async fn spawn(router: Router) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    port
}

async fn ollama_generate() -> axum::response::Response {
    let body = concat!(
        "{\"model\":\"acme-local\",\"response\":\"Hel\",\"done\":false}\n",
        "{\"model\":\"acme-local\",\"response\":\"lo\",\"done\":false}\n",
        "{\"model\":\"acme-local\",\"response\":\"\",\"done\":true,\"prompt_eval_count\":11,\"eval_count\":22}\n",
    );
    axum::response::Response::builder()
        .header("content-type", "application/x-ndjson")
        .body(Body::from(body))
        .unwrap()
}

async fn get(client: &app::HttpClient, url: String) -> String {
    let resp = client
        .request(Request::get(url).body(Body::empty()).unwrap())
        .await
        .unwrap();
    String::from_utf8(
        resp.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

#[tokio::test]
async fn proxied_call_is_persisted_counted_and_shown() {
    let upstream = spawn(Router::new().route("/api/generate", post(ollama_generate))).await;

    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("calls.db")).unwrap());
    let mut pricing = Pricing::empty();
    pricing
        .merge_json(r#"{"acme-local":{"prompt_per_1k":1.0,"completion_per_1k":2.0}}"#)
        .unwrap();
    let telemetry = Arc::new(Telemetry::new(pricing, Some(Arc::clone(&store))));
    let allow = Arc::new(HashSet::from([upstream]));
    let proxy = spawn(app::build_app(allow, Arc::clone(&telemetry))).await;

    let client = app::create_http_client();

    // Dashboard empty state before traffic.
    let html = get(&client, format!("http://127.0.0.1:{proxy}/")).await;
    assert!(html.contains("No calls recorded yet"), "{html}");

    let req = Request::post(format!(
        "http://127.0.0.1:{proxy}/proxy/{upstream}/api/generate"
    ))
    .header("content-type", "application/json")
    .header("authorization", "Bearer sk-live-SECRETSECRETSECRET")
    .header("x-api-key", "super-secret")
    .body(Body::from(
        r#"{"model":"acme-local","prompt":"my key is sk-proj-ABCDEFGHIJKLMNOPQRSTUV ok"}"#,
    ))
    .unwrap();
    let resp = client.request(req).await.unwrap();
    assert_eq!(resp.status(), 200);
    let streamed = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&streamed).contains("\"done\":true"));

    // Recording happens after the stream ends; poll briefly.
    let mut n = 0;
    for _ in 0..100 {
        n = store.count().unwrap();
        if n == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(n, 1, "one proxied call -> one row");

    let row = &store.recent(1, None).unwrap()[0];
    assert_eq!(row.model, "acme-local");
    assert_eq!(row.prompt_tokens, Some(11));
    assert_eq!(row.completion_tokens, Some(22));
    assert_eq!(row.estimated_cost_usd, Some(0.055)); // 11/1000*1 + 22/1000*2
    assert_eq!(row.trace_id.as_deref().map(str::len), Some(32));
    assert!(
        !row.prompt.contains("sk-proj"),
        "prompt redacted: {}",
        row.prompt
    );
    let headers = store.latest_headers().unwrap().unwrap();
    assert!(
        !headers.to_lowercase().contains("authorization"),
        "{headers}"
    );
    assert!(
        !headers.contains("SECRET") && !headers.contains("super-secret"),
        "{headers}"
    );

    let metrics = get(&client, format!("http://127.0.0.1:{proxy}/metrics")).await;
    assert!(
        metrics.contains(r#"llm_requests_total{model="acme-local",outcome="ok"} 1"#),
        "{metrics}"
    );
    assert!(metrics.contains(r#"llm_completion_tokens_total{model="acme-local"} 22"#));
    assert!(metrics.contains("llm_request_latency_ms_count 1"));

    let html = get(&client, format!("http://127.0.0.1:{proxy}/")).await;
    assert!(
        html.contains("acme-local") && html.contains("<table>"),
        "{html}"
    );
    assert!(!html.contains("No calls recorded yet"));
}

#[tokio::test]
async fn incoming_traceparent_is_recorded() {
    let upstream = spawn(Router::new().route("/api/generate", post(ollama_generate))).await;
    let telemetry = Arc::new(Telemetry::new(Pricing::empty(), None));
    let proxy = spawn(app::build_app(
        Arc::new(HashSet::from([upstream])),
        Arc::clone(&telemetry),
    ))
    .await;
    let client = app::create_http_client();
    let req = Request::post(format!(
        "http://127.0.0.1:{proxy}/proxy/{upstream}/api/generate"
    ))
    .header(
        "traceparent",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
    )
    .body(Body::from(r#"{"model":"m","prompt":"p"}"#))
    .unwrap();
    let resp = client.request(req).await.unwrap();
    let _ = resp.into_body().collect().await.unwrap();
    for _ in 0..100 {
        if let Some(r) = telemetry.recent(1, None).first() {
            assert_eq!(
                r.trace_id.as_deref(),
                Some("4bf92f3577b34da6a3ce929d0e0e4736")
            );
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("call was not recorded");
}
