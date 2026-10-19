# Argus

Argus is a photo editor for the Mac that does the boring half of Lightroom for you. Import a card, and a model looks at the whole Shoot at once: it flags the keepers, rejects the blurry and blinking frames, groups bursts into Stacks and picks the best of each. Then it offers two or three finished edits per Photo (Natural, Punchy, Moody) that you accept, dial down or ignore. You can also just ask it things, like "fix the lighting on this one" or "make the whole shoot consistent", and undo anything it did in one step.

It runs on your own Mac with a local model, so your photos never leave the machine unless you choose otherwise. There are no API keys and no accounts. Your originals are never written to.

It's early. Expect rough edges, and please tell Greg when you hit one.

## What you need

A Mac with Apple Silicon. The default local model wants about 32 GB of memory to run comfortably; with less, use a smaller model or your Claude plan (both covered below). Argus is Mac only for now.

## Install

Download the latest `.dmg` from [argus-releases](https://github.com/gregorymfoster/argus-releases/releases/latest), open it and drag Argus into Applications.

Argus isn't notarized by Apple (there's no paid developer account behind it, see [ADR 0004](docs/adr/0004-unsigned-direct-download-with-auto-update.md)), so macOS blocks it the first time. Open it once and dismiss the warning. Then go to System Settings > Privacy & Security, scroll down to the message about Argus and click Open Anyway. On macOS 15 and later the old right-click > Open trick no longer skips this step.

If you'd rather use the terminal, this clears the quarantine flag and Argus opens normally:

```sh
xattr -dr com.apple.quarantine /Applications/Argus.app
```

You only do this once. Argus checks for a new version each time it launches, downloads it in the background and runs it the next time you open the app.

On first launch Argus asks where to keep your Library. A Library is one folder holding your imported photos, previews and edits, and you can move it to an external drive later.

## Set up the AI

Argus needs a model to cull and edit. Pick one of these two.

**Ollama (the default, free and private).** Install [Ollama](https://ollama.com/download), open it once so it runs in the menu bar, then pull the default model in Terminal:

```sh
ollama pull qwen3.8:27b
```

That's a large download. On a Mac with less memory, pull a smaller vision model instead and choose it in Argus under Settings (⌘,) > AI. Argus runs the model gently by default so it doesn't take over your Mac, and saves big jobs like culling a whole card for when the Mac is idle and plugged in, or overnight. "Run now" on a job skips the wait.

**Your Claude plan (faster, but photos go to Anthropic).** If you pay for Claude Pro or Max, install [Claude Code](https://docs.claude.com/en/docs/claude-code/setup), sign in once with `claude` in Terminal, then pick "Your Claude plan" in Settings > AI. Argus talks to Claude only through the official `claude` command, so it uses your plan and needs no key. Small previews of your photos are sent to Claude when you do this.

(There's also a Claude API option for people who already have an Anthropic API key. You don't need it.)

## Build from source

You need Xcode's command line tools (`xcode-select --install`), [Rust](https://rustup.rs), Node 20 or later and, optionally, [just](https://github.com/casey/just) (`brew install just`).

```sh
git clone https://github.com/gregorymfoster/argus-app.git
cd argus-app
just check            # format, lint and tests (cargo underneath; see the justfile without just)
just app              # the desktop app in dev mode
cd app && npm install && npm run build   # Argus.app and a .dmg in app/target/release/bundle/
```

A build of your own has no auto-update. The command line tool `argus` (in `cli/`) drives a Library without the app: `cargo run -p argus-cli -- --help`. It also serves Argus's tools to Claude Desktop or Claude Code over MCP, see [docs/mcp.md](docs/mcp.md).

## For contributors

[docs/spec.md](docs/spec.md) says what Argus is for, [GLOSSARY.md](GLOSSARY.md) has the words we use (Shoot, Verdict, Stack, Recipe and the rest) and [docs/adr/](docs/adr/) has the decisions behind it. [CLAUDE.md](CLAUDE.md) is the working guide for anyone building Argus, human or agent. Some tests and the edit bench use Greg's own photos, which aren't in this repo, so those tests skip.

Releasing a new version is in [docs/release.md](docs/release.md).

## License

MIT, see [LICENSE](LICENSE). Argus bundles small open models and links open-source libraries under their own licences; they're listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
