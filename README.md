# Rust LLM Logger

A high-performance, non-buffering reverse proxy for LLM servers built with Rust, Axum, and the Tower ecosystem. This proxy intercepts and logs LLM requests and responses with near-zero latency overhead (≤ 2-3ms) by streaming data through an in-memory channel while parsing metrics concurrently.

## Features

- **Zero-Copy Stream Interception**: Uses a stream-tee architecture to forward response chunks to clients while simultaneously parsing them for metrics
- **Multi-Backend Support**:
  - Ollama (NDJSON format)
  - OpenAI-compatible APIs (SSE/Server-Sent Events) - vLLM, llama.cpp, etc.
- **Dynamic Routing**: Route to different backend ports on the fly
- **Non-Blocking**: Client receives streaming responses without waiting for parsing or logging
- **Comprehensive Metrics**: Captures model name, prompt, token counts (input/output), and end-to-end latency

## Architecture

### Phase 1: High-Performance Proxy Foundation

The proxy is built on:
- **axum**: Web framework with Tower ecosystem integration
- **hyper**: High-performance HTTP client/server
- **tokio**: Async runtime
- **tower-http**: Composable middleware

### Phase 2: Non-Buffering Stream Interception

The core innovation is the stream-tee architecture implemented in `src/proxy.rs:handle_stream_tee`:

1. Incoming request is processed by middleware to extract model/prompt
2. Request is forwarded to upstream LLM server
3. Response body stream is split into two channels:
   - **Client channel**: Immediate forwarding via `mpsc::channel`
   - **Parser channel**: Concurrent parsing in separate tokio task
4. Metrics are aggregated and logged when stream completes

### Parsers

#### Ollama Parser (`src/parsers/ollama.rs`)
- Parses NDJSON (Newline Delimited JSON)
- Extracts `prompt_eval_count` and `eval_count` from final object with `"done": true`

#### OpenAI Parser (`src/parsers/openai.rs`)
- Parses SSE (Server-Sent Events) format
- Looks for final `usage` object containing `prompt_tokens` and `completion_tokens`
- Ignores intermediate delta chunks

## Quick Start

### Build

```bash
cargo build --release
```

### Run

```bash
cargo run --release
```

The proxy will start on `http://127.0.0.1:3000`.

### Usage

Route requests through the proxy using the pattern:

```
http://127.0.0.1:3000/proxy/<backend_port>/<endpoint>
```

#### Example 1: Ollama

If you have Ollama running on `localhost:11434`:

```bash
curl http://127.0.0.1:3000/proxy/11434/api/generate \
  -H "Content-Type: application/json" \
  -d '{
    "model": "llama2",
    "prompt": "Why is the sky blue?",
    "stream": true
  }'
```

#### Example 2: vLLM (OpenAI-compatible)

If you have vLLM running on `localhost:8080`:

```bash
curl http://127.0.0.1:3000/proxy/8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "meta-llama/Llama-2-7b-chat-hf",
    "messages": [
      {"role": "user", "content": "Explain quantum computing"}
    ],
    "stream": true
  }'
```

## Metrics Output

Metrics are logged to stdout in JSON format:

```json
{
  "model": "llama2",
  "prompt": "Why is the sky blue?",
  "prompt_tokens": 8,
  "completion_tokens": 150,
  "latency_ms": 1243,
  "timestamp": "2025-11-09T12:34:56.789Z"
}
```

## Configuration

### Logging Level

Control logging verbosity via the `RUST_LOG` environment variable:

```bash
# Debug mode (verbose)
RUST_LOG=rust_llm_logger=debug cargo run

# Info mode (default)
RUST_LOG=rust_llm_logger=info cargo run

# Quiet mode
RUST_LOG=rust_llm_logger=warn cargo run
```

### Server Port

Edit `src/main.rs` to change the listening port (default: 3000):

```rust
let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
```

## Project Structure

```
src/
├── main.rs              # Server initialization and routing
├── proxy.rs             # Core proxy handler and stream-tee logic
├── middleware.rs        # Request body extraction middleware
├── types.rs             # Data structures and serialization types
└── parsers/
    ├── mod.rs           # Parser trait and backend detection
    ├── ollama.rs        # NDJSON parser for Ollama
    ├── openai.rs        # SSE parser for OpenAI-compatible APIs
    └── passthrough.rs   # Null parser for unknown formats
```

## Performance Characteristics

- **Added Latency**: ≤ 2-3ms overhead from stream-tee channel
- **Memory**: Minimal buffering - only stores incomplete JSON objects
- **Concurrency**: Fully async, handles thousands of concurrent connections
- **Streaming**: Client receives first byte immediately, no waiting for parsing

## Future Enhancements

- [ ] Database persistence (PostgreSQL, ClickHouse)
- [ ] Prometheus metrics export
- [ ] Authentication/API key management
- [ ] Request/response filtering and transformation
- [ ] Rate limiting per model/user
- [ ] Cost estimation based on token counts

## License

MIT

## Contributing

Contributions welcome! Please open an issue or PR.
