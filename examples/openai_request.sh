#!/bin/bash

# Example script to test the proxy with OpenAI-compatible API (vLLM, llama.cpp, etc.)
# Prerequisites: OpenAI-compatible server running on localhost:8080

echo "Testing LLM Logger Proxy with OpenAI-compatible API..."
echo "---------------------------------------"

curl -N http://127.0.0.1:3000/proxy/8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "meta-llama/Llama-2-7b-chat-hf",
    "messages": [
      {
        "role": "system",
        "content": "You are a helpful assistant."
      },
      {
        "role": "user",
        "content": "What are the key differences between Rust and C++?"
      }
    ],
    "stream": true,
    "max_tokens": 500
  }'

echo ""
echo "---------------------------------------"
echo "Check the proxy logs to see the metrics!"
