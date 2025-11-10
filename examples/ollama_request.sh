#!/bin/bash

# Example script to test the proxy with Ollama
# Prerequisites: Ollama running on localhost:11434

echo "Testing LLM Logger Proxy with Ollama..."
echo "---------------------------------------"

curl -N http://127.0.0.1:3000/proxy/11434/api/generate \
  -H "Content-Type: application/json" \
  -d '{
    "model": "llama2",
    "prompt": "Explain the concept of recursion in programming",
    "stream": true
  }'

echo ""
echo "---------------------------------------"
echo "Check the proxy logs to see the metrics!"
