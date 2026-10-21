# Known failures

Tests that also fail on `main`, so a thread that sees them knows they aren't its own breakage and doesn't re-bisect them. One line each: date, commit, test, what fails, which thread owns the fix. Delete the line when it passes again.

- 2026-10-18, main at 71720ff: `app-e2e` "Esc closes Import" (Esc leaves the Import dialog open under WebDriver). Owner: the Argus desktop app shell thread.
