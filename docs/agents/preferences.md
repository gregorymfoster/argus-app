# Greg's working preferences

Notes for any agent working on Argus. Update this file the moment a new preference comes up.

- **Autonomy.** Keep driving forward until done or genuinely stuck. Make reversible calls yourself and say which you made.
- **Questions.** Ask decisions as one tappable question each, with a recommendation marked, not as long numbered lists.
- **Iteration loops.** Every change should be verifiable by an agent without a human: render it, screenshot it, test it. Build harnesses when one is missing.
- **Architecture.** Follow Matt Pocock's skills: deep modules, small interfaces, test at the seam, grill before building, TDD.
- **Design first.** UI is mocked up in Claude Design and agreed before it's built.
- **Malleable code.** The UX will change fast once Greg uses the app. Keep screens thin over the tool layer, keep layout choices behind small switches, and prefer code that's cheap to delete over clever abstractions.
- **Don't reinvent.** Use established component and UI libraries (see docs/design.md, Components) instead of building widgets from scratch.
- **Steal from Lightroom.** Where Lightroom has a simple, familiar pattern (pick/reject flags, P/X/U keys), copy it.
- **Shoot-wide AI.** The Agent judges and styles every Photo with the whole Shoot in view, never one frame in isolation.
- **AI providers.** Local Ollama is the default and slow is fine (Greg, 2026-10-18). Cloud use should run on his Claude or ChatGPT subscription rather than API keys, where the provider's terms allow it (ADR 0006).
- **Share the Mac.** Local models must not make Greg's Mac unusable while he works (2026-10-18). Gentle compute is the default. Heavy runs happen overnight or when he says go, and the compute level is configurable (ADR 0007).
- **Performance and beauty.** When trading off, favor maximum performance first, then the most beautiful UI AI can produce.
- **Self-maintenance.** Keep CLAUDE.md, this file, the glossary, ADRs and skills current as you go, unprompted.
- **Naming.** Two hard syllables, clear in a loud room, no spelling help needed. Latin, Roman or Greek in flavor.
- **Writing for humans.** Varied sentences, no buzzwords, no em-dashes, plain paragraphs over bullet piles.
- **Git.** Never open PRs. Commit to main and push without asking (solo private project).
- **Originals.** Never modify Greg's original photo files.
- **Colour reference.** On 2026-10-17 Greg chose not to provide Lightroom reference exports, so Argus's default colour and tone are matched to the camera's embedded JPEG (`argus fit-look`), not to Lightroom.
