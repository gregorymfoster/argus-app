# 8. The Agent speaks one JSON step protocol to every Provider

Status: accepted (2026-10-18). Builds on ADR 0003 (tools as the one API) and ADR 0006 (Providers).

## Context

The in-app Agent has to turn "warmer, less contrast" or "make the whole shoot consistent" into calls on the tool layer. Each Provider has its own way to call tools. The Messages API has native tool use. Ollama has a `tools` field, but whether it works depends on the model and its template, and Greg is still choosing between Gemma 4 and Qwen 3.8. Claude Code on Greg's plan (`claude -p`) can only reach our tools through an MCP server, which would mean shipping the `argus` CLI inside the app or hosting an MCP endpoint from it.

## Decision

The engine runs the loop itself (`engine/src/agent.rs`). Each model turn returns one JSON object, `{"say": "...", "calls": [{"tool": "...", "args": {...}}]}`. The engine runs the calls through `Tools::call_in`, hands the results back (JSON as text, rendered images as images), and stops when `calls` is empty. Ollama gets the step schema as its structured-output `format`, so even a small local model returns parseable JSON. Claude Code resumes one session per request (`--resume`) and reads images from a temp folder, as the cull does. Tests drive the loop with a scripted model.

## Consequences

One loop serves every Provider, the batch-per-request undo lives in one place, and switching models needs no tool-calling support from them. We give up the native tool-use features of each API (parallel-call hints, server-side schema validation), and Claude Code pays a process start (a few seconds) per turn. If native tool calling proves clearly better for the chosen local model, it can sit behind the same `Chat` seam.
