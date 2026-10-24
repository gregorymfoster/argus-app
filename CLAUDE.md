# Argus

AI-native batch photo editing for the Mac. Read `docs/spec.md` for what we're building, `GLOSSARY.md` for the words we use, and `docs/adr/` for the decisions behind it.

## How we work

- Use the glossary's terms exactly in code, docs and conversation (Library, Photo, Shoot, Card, Recipe, Verdict, Stack, Contact sheet, Look, Correction, Mask). If a new concept appears, add it to the glossary before naming code after it.
- Follow Matt Pocock's skills (installed as the `mattpocock-skills` plugin). Use `grill-with-docs` before any non-trivial change, `codebase-design` vocabulary for module design (deep modules, small interfaces, tested at the seam), and `tdd` for implementation.
- pstack (Lauren Tan's poteto-mode, `pstack@pstack-claude` pinned in `.claude/settings.json`) is opt-in (Greg, 2026-10-21): ignore its session-start mandate (`scripts/pstack-quiet.sh` turns it off) and call `/pstack:poteto-mode` only when Greg or a brief asks, or for a bug of unknown cause or a perf issue. Inside it, Matt Pocock's skills and this file still win, and its PR, worktree and shipping steps become `just land` to main. Worth reaching for: `bug-fix`/`perf-issue` playbooks, `blast-radius`, `benchmark-checklist`, `correct`, `unslop`; `thermos` once before a release; `arena`/`swarm`/`interrogate` only for big design calls (token-heavy).
- Record a decision as an ADR in `docs/adr/` only when it's hard to reverse, surprising without context, and the result of a real trade-off.
- Every change must be checkable by Claude without a human (ADR 0005); the `verify` skill maps each feature to its narrowest check. Prefer testing through the tool layer. Look at rendered PNGs and UI screenshots rather than assuming a visual change worked.
- When in doubt, follow modern Lightroom's UX flows (Greg). Simplify where there's a reason, deviate only with a strong opinion. Never show a bare spinner when progress is knowable: use a bar with an ETA (see `docs/design.md`).
- UI is designed in Claude Design first, following `docs/design.md` (calm like Claude, keyboard-dense like Cursor, precise like Linear). Don't build a screen that hasn't been mocked up and agreed.
- `docs/design.md` is the single source of truth for the design language (Greg, 2026-10-20). Every UI change follows the `design` skill's checklist: shared primitives (`Button`, `Seg quiet`, `Kbd quiet`, `ProgressBar`), tokens not literals, and `npm run design-lint` (in `just check`), a ratchet that fails on new stray colors, radii, sizes, keycaps and spinners. A new design preference from Greg goes into `docs/design.md` in the same change.
- UX direction (Greg, 2026-10-18): a dense, keyboard-first prosumer power tool like Cursor, never bubbly with big text; when in doubt follow modern Lightroom flows and only deviate with a strong opinion. Details in `docs/design.md`.
- Never write to original photo files.
- Open source (Greg, 2026-10-19): this repo stays private (it holds his photos in `fixtures/bench`, `fixtures/golden` and their history). The public repo is gregorymfoster/argus-app, fed only by `scripts/public-export DIR` snapshots of main; never push this repo's history there, and add any new private path to that script's PRIVATE list.
- Cloud sessions run golden tests and the edit bench without touching Greg's Mac: `scripts/cloud-setup.sh` (SessionStart hook) copies the sample RAWs from `/mnt/project-files/fixtures/raw` (A7R II) and `raw-a7rv` (A7R V, his current camera) into `fixtures/`, installs `just` and pre-fetches crates. New sample RAWs go in both his checkout and that shared folder. Public sample-RAW sites are blocked from the cloud container.
- Greg works on the Mac that runs local models. Run heavy jobs on it (culls, model comparisons, benchmarks, release builds) only overnight or when he says go. Otherwise use `--compute gentle` and build at gentle priority, `taskpolicy -c utility nice -n 10 cargo ...` (performance cores, yields to his apps; Greg 2026-10-20), as `just mac-e2e` and releases do.
- Driving work on Greg's Mac (Remote Control) must never stall silently (Greg, 2026-10-18, after two stalls). Wrap every command that can hang in a time limit (macOS has no `timeout`: use `scripts/timeout SECONDS cmd...`, exit 124 on timeout). During any step longer than 5 minutes, the Mac session sends a progress note at least every 5 minutes: what it's doing, how far along, and the ETA. The driving session arms a check-in before each long step and acts if no note has arrived. Never queue a quick task behind a long one: interrupt, or tell Greg the order and the wait. Keep the status checklist current with the step and its ETA.
- Check Argus on Greg's Mac without screen control (Greg, 2026-10-18: no more permission prompts to test the app): `just mac-e2e` runs the real engine behind the app's UI headless in WebKit, at background priority, with screenshots in `prototype/out/shots/mac-*.png`. Ask for screen control only for checks that exist only natively (menu bar, macOS folder picker, first-launch Gatekeeper), batched into one request that says why.
- Paths never contain spaces: join words with hyphens and a date prefix with an underscore (`~/Pictures/Argus/2026-10-12_tahoe`). This covers default folders, export paths and file names Argus creates.
- Names follow pro DAM conventions (Greg, 2026-10-18): Shoots `2026-10-18_Boat-Trip`, originals keep camera names, exports `2026-10-18_Boat-Trip_DSC08005.jpg`, hierarchical Keywords like `Places|Portugal|Lisbon`. Build every file, folder, Shoot or Keyword name with `engine::naming`, never by hand; the rules are in `docs/naming.md` (ADR 0010).
- Keep code malleable: the UX will change quickly, so screens stay thin over the tool layer and are cheap to throw away.
- Use existing component and UI libraries (shadcn/ui, Radix, Tailwind, cmdk, TanStack Virtual) rather than reinventing widgets.
- No pull requests. This is Greg's solo private repo: commit straight to `main` and push, without asking. Land with `just land` (rebase on origin/main, `just check`, push `HEAD:main` with retries), from whatever branch the session started on.

## Working alongside other threads

Several Claude threads push to `main` at once. These rules keep them from colliding and from burning tokens:

- Take ADR and schema-step numbers last: write the ADR as `docs/adr/NNNN-slug.md` and the migration step at the end, and pick the next free number only after `git fetch` right before landing. `just land` stops on a clash.
- Put thread-specific notes in `docs/` or a skill, not here. This file is read by every session, so keep it short and edit it in small hunks.
- Tests and builds run on `--profile fast`. Never `--release` to test; release builds are for shipping and the MCP binary (`fast` renders just as quickly, so `just render`, `live`, `bench` and `speed` use it too).
- Iterate on the narrowest check (one test, one e2e step, one render) and run `just check` once, through `just land`. Run the full `app-e2e` once before landing a UI change, not in the edit loop.
- A test that also fails on `main` goes in `docs/agents/known-failures.md` with the date and commit, so the next thread doesn't re-bisect it. Delete the line once it passes.
- Long Mac steps run as one background job that prints progress; check on it at most every 5 minutes, never in a tight poll loop. Each poll re-reads the whole conversation.
- Anything that can run over a minute (builds, tests, bench, e2e, releases, model runs) goes through `scripts/progress NAME -- cmd` (Greg, 2026-10-18): heartbeat every 60 s, ETA, a stall watchdog (exit 124) and a one-line status in `out/logs/`. Check runs with `just status` (one cheap call), never by re-reading logs. Long scripts print `::progress DONE/TOTAL label` when they know their size. Speed ideas go in `docs/agents/build-speed.md`.
- Greg keeps running notes for a future blog post about building Argus (`docs/blog/`). When work gives a notable result, good or bad, append a dated entry to `docs/blog/results-log.md` in the same change; a decision, dead end, surprise or quotable Greg moment goes in `story-log.md`. Follow each file's template; the blog thread renders the page.
- When a thread's work has landed, write what a successor needs (state, open items) into project memory in a few lines, so a follow-up can start a fresh thread instead of reusing a huge one. Send one reply per result; no second summary for the coordinator.

## Keeping this file current

These instructions maintain themselves. Whenever Greg states a preference, a rule or a decision, or you learn something a future session would otherwise have to rediscover, write it down in the same change, without being asked:

- a working rule or preference goes in this file, under "How we work" or in `docs/agents/preferences.md` for longer notes
- a domain term goes in `GLOSSARY.md`
- a hard-to-reverse decision goes in an ADR
- a repeatable procedure (release, bless goldens, add a tool) becomes a skill under `.claude/skills/`

Keep this file short and true. Delete rules that no longer hold instead of adding exceptions. `AGENTS.md` points here so non-Claude agents read the same rules.

## Layout

- `engine/`: Rust crate for decode, pipeline, Library, ML and the tool layer
- `cli/`: the `argus` CLI, a thin wrapper over the engine (MCP server, HTTP bridge, `speed`, `fit-look`)
- `app/`: the Tauri 2 desktop app, a thin shell over the engine. Its UI is `prototype/` (no copy): in the app, the UI talks to the engine through `engine/src/commands.rs` (every tool plus the person's own flags and edits), the same commands `argus serve` exposes. See `app/README.md`
- `fixtures/`: golden renders and `bench/` (Greg's Lightroom picks and edits); sample RAWs in `fixtures/raw/` stay out of git
- `prototype/`: the React UI. With dummy data in a browser it is the design prototype (Variants, Playwright screenshot loop); inside the app or with `?library=live` it runs on the real Library. `src/data/backend.ts` is its only seam to the engine. See `prototype/README.md` and the `prototype` skill
- `docs/`: spec and ADRs

## Commands

One line per command family here; the full reference (every flag, env var and file) is `docs/commands.md`. Read its section before using a family you haven't used this session.

- `just check` is the CI (no GitHub CI: Greg avoids Actions billing, 2026-10-18): fmt, clippy, all tests including goldens, and `design-lint`, on the `fast` profile, about 40 s after a one-line engine edit on the 4-core cloud box (test binaries run side by side; in the cloud it first fetches missing sample RAWs and turns on sccache). `scripts/cached` skips unchanged Rust (`ARGUS_NO_CACHE=1` forces it). Run it before every push (`just land` does). It warns when `fixtures/raw` is empty, because goldens then skip. One engine test: `cargo test --profile fast --test engine <module>::` from the repo root (never `-p argus-engine`: it rebuilds ~90 crates) (all of `engine/tests/` is one binary). `just bless` re-accepts goldens after a deliberate pipeline change (`bless-goldens` skill)
- Look at an edit: `just render <raw> [--set exposure=0.3]` writes `out/<stem>.jpg`; `argus tool <name> '<json>' --library <dir> [--out x.jpg]` calls any tool-layer tool, e.g. `render_photo` or `view_mask`
- Drive a Library headlessly (JSON out): `argus import|list|flag|rate|edit|export|previews|backup|restore ... --library <dir>`. A Library is one portable folder (ADR 0011). Schema changes are a step appended to `engine/src/library/migrate.rs` plus the same change in `schema.sql` (ADR 0009)
- AI: `argus cull`, `argus agent "<request>"` and `argus work` (the background Queue). Cull and Agent take `--provider ollama|claude-code|claude-api` (`claude-code` works in the cloud, no Mac needed); all take `--compute gentle|balanced|full`, Gentle by default (ADR 0007). A model cull or big batch on the Mac waits for overnight or Greg's go
- Tool layer: `argus mcp` (stdio, `docs/mcp.md`) and `argus serve` (HTTP `/api/<command>`, the prototype's `?library=live`) serve `engine/src/tools/` and `engine/src/commands.rs`. New Agent capabilities go in as tools, never as MCP-only or UI-only code
- App and UI checks: `just app` (dev), `just app-e2e` and `app-e2e-quick` (cloud, Xvfb; `just app-e2e-setup` once per session), `just mac-e2e` (Greg's Mac, headless WebKit), `just layout` and `just import-check` after touching a screen's bars or Import, `just perf` (cloud) for what an open app costs
- Benches: `just bench` and `python3 bench/score.py` gut-check culls and edits against Greg's Lightroom work in `fixtures/bench/` (`bench-report` skill). `just speed` after touching decode, develop, import or render (fails on a >30% slowdown). Edit sliders re-render from a cache of demosaiced RAWs, so keep Recipe effects in stages 4+ or the cache key must grow
- Logs: the Usage log lives in each Library (ADR 0014); the Debug log is `~/Library/Logs/Argus/debug.jsonl`, summed up by `scripts/debug-log`
- Releases: always local on Greg's Mac (no GitHub Actions); his one "ship it" covers `just ship X.Y.Z` (`release` skill, `docs/release.md`). Never regenerate the update-signing key in `~/.tauri`: installed copies only trust the original

`just` may not be installed; each recipe is plain cargo underneath, see `justfile`.
