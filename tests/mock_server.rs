use axum::{
    body::Body,
    extract::Json,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration};
use tokio_stream::wrappers::ReceiverStream;

#[derive(Deserialize)]
struct OllamaRequest {
    model: String,
    #[allow(dead_code)]
    prompt: String,
    #[serde(default)]
    stream: bool,
}

#[derive(Serialize)]
struct OllamaStreamChunk {
    model: String,
    created_at: String,
    response: String,
    done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_eval_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    eval_count: Option<u32>,
}

#[derive(Deserialize)]
struct OpenAIRequest {
    model: String,
    #[allow(dead_code)]
    messages: Vec<Message>,
    #[serde(default)]
    stream: bool,
}

#[derive(Deserialize, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OpenAIStreamChunk {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<Choice>,
}

#[derive(Serialize)]
struct Choice {
    index: u32,
    delta: Delta,
    finish_reason: Option<String>,
}

#[derive(Serialize)]
struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

#[derive(Serialize)]
struct OpenAIFinalChunk {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<FinalChoice>,
    usage: Usage,
}

#[derive(Serialize)]
struct FinalChoice {
    index: u32,
    message: Message,
    finish_reason: String,
}

#[derive(Serialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

async fn ollama_generate(Json(req): Json<OllamaRequest>) -> Response {
    println!("Mock Ollama: Received request for model: {}", req.model);

    if !req.stream {
        return (StatusCode::BAD_REQUEST, "Non-streaming not implemented").into_response();
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(32);

    tokio::spawn(async move {
        let response_text = "The sky appears blue due to a phenomenon called Rayleigh scattering. \
                           When sunlight enters Earth's atmosphere, it collides with gas molecules. \
                           Blue light has a shorter wavelength and gets scattered more than other colors, \
                           making the sky look blue to our eyes.";

        let words: Vec<&str> = response_text.split_whitespace().collect();

        // Send chunks
        for word in &words {
            sleep(Duration::from_millis(10)).await;

            let chunk = OllamaStreamChunk {
                model: req.model.clone(),
                created_at: chrono::Utc::now().to_rfc3339(),
                response: format!("{} ", word),
                done: false,
                prompt_eval_count: None,
                eval_count: None,
            };

            let json = serde_json::to_string(&chunk).unwrap();
            let _ = tx.send(Ok(format!("{}\n", json))).await;
        }

        // Send final chunk with token counts
        sleep(Duration::from_millis(10)).await;
        let final_chunk = OllamaStreamChunk {
            model: req.model.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
            response: "".to_string(),
            done: true,
            prompt_eval_count: Some(5),  // Simulated prompt tokens
            eval_count: Some(words.len() as u32),  // Simulated completion tokens
        };

        let json = serde_json::to_string(&final_chunk).unwrap();
        let _ = tx.send(Ok(format!("{}\n", json))).await;
    });

    let stream = ReceiverStream::new(rx);
    let body = Body::from_stream(stream);

    Response::builder()
        .status(200)
        .header("content-type", "application/x-ndjson")
        .body(body)
        .unwrap()
}

async fn openai_chat_completions(Json(req): Json<OpenAIRequest>) -> Response {
    println!("Mock OpenAI: Received request for model: {}", req.model);

    if !req.stream {
        return (StatusCode::BAD_REQUEST, "Non-streaming not implemented").into_response();
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(32);

    tokio::spawn(async move {
        let response_text = "Rust and C++ are both systems programming languages, but they differ in key ways. \
                           Rust provides memory safety without garbage collection through its ownership system. \
                           C++ offers more manual control but requires careful memory management.";

        let words: Vec<&str> = response_text.split_whitespace().collect();

        // Send delta chunks
        for word in &words {
            sleep(Duration::from_millis(10)).await;

            let chunk = OpenAIStreamChunk {
                id: "chatcmpl-mock123".to_string(),
                object: "chat.completion.chunk".to_string(),
                created: chrono::Utc::now().timestamp() as u64,
                model: req.model.clone(),
                choices: vec![Choice {
                    index: 0,
                    delta: Delta {
                        content: Some(format!("{} ", word)),
                    },
                    finish_reason: None,
                }],
            };

            let json = serde_json::to_string(&chunk).unwrap();
            let _ = tx.send(Ok(format!("data: {}\n\n", json))).await;
        }

        // Send final chunk with usage stats
        sleep(Duration::from_millis(10)).await;
        let final_response = OpenAIFinalChunk {
            id: "chatcmpl-mock123".to_string(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp() as u64,
            model: req.model.clone(),
            choices: vec![FinalChoice {
                index: 0,
                message: Message {
                    role: "assistant".to_string(),
                    content: response_text.to_string(),
                },
                finish_reason: "stop".to_string(),
            }],
            usage: Usage {
                prompt_tokens: 12,  // Simulated
                completion_tokens: words.len() as u32,
                total_tokens: 12 + words.len() as u32,
            },
        };

        let json = serde_json::to_string(&final_response).unwrap();
        let _ = tx.send(Ok(format!("data: {}\n\n", json))).await;

        // Send [DONE] marker
        let _ = tx.send(Ok("data: [DONE]\n\n".to_string())).await;
    });

    let stream = ReceiverStream::new(rx);
    let body = Body::from_stream(stream);

    Response::builder()
        .status(200)
        .header("content-type", "text/event-stream")
        .body(body)
        .unwrap()
}

#[tokio::main]
async fn main() {
    // Ollama mock server on port 11434
    let ollama_app = Router::new()
        .route("/api/generate", post(ollama_generate));

    tokio::spawn(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:11434")
            .await
            .expect("Failed to bind Ollama mock on 11434");
        println!("Mock Ollama server listening on 127.0.0.1:11434");
        axum::serve(listener, ollama_app).await.unwrap();
    });

    // OpenAI-compatible mock server on port 8080
    let openai_app = Router::new()
        .route("/v1/chat/completions", post(openai_chat_completions));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .expect("Failed to bind OpenAI mock on 8080");
    println!("Mock OpenAI server listening on 127.0.0.1:8080");
    println!("\nMock servers ready! Run the proxy and test scripts.\n");

    axum::serve(listener, openai_app).await.unwrap();
}
