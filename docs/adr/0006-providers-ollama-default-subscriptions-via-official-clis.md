# 6. Ollama by default; subscriptions only through the providers' own CLIs

Status: accepted (2026-10-18). Refines ADR 0003, which left the subscription question open.

## Context

Greg doesn't want to set up an API key. He asked for local models through Ollama for now, and otherwise wants cloud models to run on his Claude or ChatGPT subscription. He said speed doesn't matter as long as it works. His Mac is an M4 Max with 64 GB of memory, and Ollama 0.35 is already installed and running.

We checked what each provider allows, as of 2026-10-18.

Anthropic's support article on using the Agent SDK with a Claude plan (updated 2026-10-07) says the Agent SDK, `claude -p` and third-party apps can still be used and draw on the plan's limits, and that Max and Team plans now include monthly API credits for them. Its February 2026 legal-compliance page said subscription OAuth tokens are only for Claude Code and Claude.ai, and that taking those tokens into another product breaks the Consumer Terms. The newer article is the current word, and both agree on one thing: running the official Claude Code CLI itself on your own machine is a first-party use.

OpenAI's Codex docs offer "Sign in with ChatGPT" for subscription access but tell you to use an API key for programmatic CLI workflows. Shelling out to `codex exec` on a ChatGPT login is technically possible, but OpenAI doesn't describe it as a supported way to power another app. Greg's Codex install is also broken right now (its native binary is missing).

## Decision

Argus picks a Provider per call through one seam (`engine::cull::Provider`, chosen by `--provider`, the bridge, or `ARGUS_PROVIDER`):

1. **`ollama`, the default.** A local vision model through Ollama's HTTP API. The default model is `qwen3.8:27b` (about 17 GB at Q4). It replaced `gemma4:31b` on 2026-10-18 after a Full-compute cull of the four 100-Photo bench Shoots: star AUC 0.68/0.84/0.56/0.52 against Gemma's 0.53/0.72/0.55/0.48, and 23 of 33 Best Photos among Greg's picks against 17 of 30. Gemma was then removed from the Mac. `ARGUS_OLLAMA_MODEL` swaps it, for example to `qwen3-vl:8b`, which Greg already has. Nothing leaves the Mac.
2. **`claude-code`, Greg's Claude plan.** Argus runs the official `claude -p` CLI that Greg signs into himself, hands it the Contact sheet as image files, and reads its JSON reply. Argus never reads, copies or stores Claude's OAuth tokens. Usage counts against his plan limits.
3. **`claude-api`, an Anthropic API key**, from `ANTHROPIC_API_KEY` or the Keychain item `argus-anthropic`. It stays available for anyone who has a key.

There is no ChatGPT-subscription Provider. ChatGPT is still reachable the supported way, by driving Argus over MCP from ChatGPT (ADR 0003).

## Why

Ollama keeps photos private, costs nothing, and needs no account, and Greg already runs it. The `claude-code` route follows the line both Anthropic pages draw: the official CLI is used as itself, not mined for credentials. We hold back on Codex until OpenAI documents subscription-backed scripting, since a Provider that quietly breaks the terms would be worse than not having one.

Greg wants the most popular simple local runtime (2026-10-18). That is Ollama: about 179k GitHub stars, the backend most other tools integrate with first, and it has run Apple's MLX engine since March 2026. LM Studio is the main alternative, but it's closed source and fewer tools build on it. The Ollama Provider uses Ollama's native `/api/chat`, not the OpenAI-compatible `/v1` endpoint, because only the native API lets it set `num_ctx`, which a whole Contact sheet needs. Adding an LM Studio Provider later is one more `Provider` impl. Greg confirmed Ollama and asked for the latest, fastest version: keep it current, and default to a model's `-mlx` build once a timed cull on his photos shows MLX is faster.

## Consequences

Local culls are slow (minutes for a 50-photo Shoot) and weaker than Claude at picking the best of similar frames, so the cull prompt and the tolerant result handling (skipping made-up photo numbers, leaving missing Photos untouched) matter more. Ollama's default context is too small for a Shoot's worth of images, so Argus asks for 64K (`ARGUS_OLLAMA_CTX`). The `claude-code` Provider needs `claude auth login` once, and it depends on Anthropic keeping its current policy. If that changes, this ADR gets revisited. A Codex Provider can be added behind the same seam if OpenAI's terms allow it.
