# 3. Pluggable model providers, plus an MCP server for headless use

Status: accepted (2026-10-17). The subscription question is settled in ADR 0006.

## Context

Greg wants Argus to run popular open-source models locally, to use his Claude or ChatGPT account, and to be operable headlessly from inside Claude and ChatGPT.

## Decision

Everything the AI can do in Argus is a single set of engine tools (list photos, read a preview, read and write a Recipe, score, cull, batch apply, export). Three kinds of caller use that same set:

1. **The in-app Agent**, backed by a pluggable Provider: a local open-source model run through Ollama (Greg, 2026-10-17: use Ollama rather than embedding a runtime; Argus can pull models through Ollama's API), or a cloud model (Claude or OpenAI) on the user's own API key.
2. **A local MCP server**, so Claude Desktop, Claude Code or ChatGPT can drive Argus headlessly. That route uses the person's existing Claude or ChatGPT subscription, since the chat app is doing the thinking.
3. **The UI itself**. Buttons call the same tools, so anything a person can do, the AI can do too.

## Why

One tool layer means every new capability reaches all three callers at once. Claude and ChatGPT subscriptions can't be plugged into third-party apps as API credentials as far as we know (inferred, to verify), so MCP is how a subscription gets used, and API keys cover the in-app cloud route.

## Consequences

The in-app Agent needs a vision-capable model to judge photos. Local vision models (for example the Qwen-VL or Gemma families) must be small enough to run on an M-series Mac. ChatGPT reaches a local MCP server only through a tunnel or a ChatGPT desktop connector, so headless ChatGPT support needs its own spike.
