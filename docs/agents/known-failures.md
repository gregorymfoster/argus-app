# Known failures

Tests that also fail on `main`, so a thread that sees them knows they aren't its own breakage and doesn't re-bisect them. One line each: date, commit, test, what fails, which thread owns the fix. Delete the line when it passes again.

- 2026-10-21, main at fa33229: `mac-e2e` live.mjs flaked once at "Stacks view (N)": `.burst-cell` never appeared after the cull (30 s timeout, screenshot showed the grid). The rerun a few minutes later passed, so it is a flake, not a break. Rerun once before digging. Owner: the Mac thread.
- 2026-10-22, main at cc0db61: `live.mjs --only cull` on a Library imported from the nine A7R II sample RAWs fails "AI Verdicts and reasons reached the Library, and the cautious cull left picking to the person" and "the burst was stacked", then times out; same result with the UX sweep's changes stashed, so it is the test's expectations against that card (no burst in it, rules cull), not a UI break. Every other live step passes. Owner: "AI cull quality" thread.
