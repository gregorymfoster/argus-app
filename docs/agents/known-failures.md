# Known failures

Tests that also fail on `main`, so a thread that sees them knows they aren't its own breakage and doesn't re-bisect them. One line each: date, commit, test, what fails, which thread owns the fix. Delete the line when it passes again.

- 2026-10-18, main at 71720ff: `app-e2e` "Esc closes Import" (Esc leaves the Import dialog open under WebDriver). Owner: the Argus desktop app shell thread.
- 2026-10-21, main at fa33229: `mac-e2e` live.mjs flaked once at "Stacks view (N)": `.burst-cell` never appeared after the cull (30 s timeout, screenshot showed the grid). The rerun a few minutes later passed, so it is a flake, not a break. Rerun once before digging. Owner: the Mac thread.
