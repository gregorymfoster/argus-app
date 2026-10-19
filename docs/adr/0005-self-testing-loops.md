# 5. Every layer is drivable and checkable by Claude without a human

Status: accepted (2026-10-17)

## Context

Greg's standing rule for this build is that the app must have very fast, easy self-testing loops, so Claude can change something, check it, and iterate without waiting on a person. He's fine with us building our own harnesses to make that happen. He also wants the codebase to follow Matt Pocock's architecture patterns (deep modules, small interfaces, testing at the seam).

## Decision

The tool layer from ADR 0003 is the one external seam, and every test and harness goes through it.

1. **A headless CLI over the Engine.** Every tool (import, score, set_verdict, write_recipe, render, export) is a subcommand that takes JSON in and gives JSON out, and renders write PNGs to disk. Claude can run a scenario, look at the output image and iterate, with no app window involved.
2. **Golden images.** Fixed fixture Photos (real Sony ARWs from Greg's camera plus a few JPEGs, stored with Git LFS) are rendered through known Recipes and compared against reference outputs within a tolerance. A failure writes the actual output and a diff image so the problem is visible, not just a number.
3. **UI in a plain browser.** In dev mode the React UI talks to the Engine over a local HTTP bridge as well as Tauri IPC, so Playwright can load it in headless Chromium, click through flows and screenshot every screen for Claude to look at. A smaller Tauri smoke test confirms the real shell.
4. **The MCP server doubles as the harness.** Claude Code can connect to a running Argus over MCP and use it exactly as a user's Claude would.
5. **One command.** `just check` runs format, lint, Engine tests, golden images and UI smoke tests, and should finish in about a minute. `just shot <screen>` screenshots one screen.

## Why

Agents iterate fastest when they can see the result of their own change. Rendering to PNG and screenshotting the UI makes visual work checkable, which plain unit tests can't do. Testing only through the tool layer follows the deep-module rule (the interface is the test surface), keeps tests stable through refactors, and means a passing test reflects behavior the UI, the Agent and MCP all share.

## Consequences

The Engine must stay usable without the Tauri shell, which also keeps it honest as a deep module. Fixture RAWs add a few hundred MB of LFS storage. Golden references need re-blessing when the pipeline changes on purpose, through an explicit `just bless` step that always shows the before and after.
