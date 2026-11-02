# Concurrency and stalls

Written 2026-11-02, after the Apply look popover hung in 0.1.17 with nothing on screen: it had drawn 84 previews, then the next batch never answered and Apply never reached the engine. The cause (fixed in 4bee11c): `Lens::corrected` held its Mutex while `correct()` ran on rayon, a waiting worker stole another render of the same image, and blocked on that lock for good. A hang that shows nothing is the worst kind of bug Argus can have, because Greg can't tell it from slowness. These are the rules that keep it from happening again.

## In the engine

Never call into rayon (`par_iter`, `into_par_iter`, `par_chunks_mut`, `pool.install`) while holding a lock guard (`Mutex`, `RwLock`, a `static` cache) or from inside a one-time initialiser (`OnceLock::get_or_init`, our `Lazy::get` for models). A rayon thread that waits on that call steals other queued work, and if that work wants the same lock or the same initialiser, every thread ends up waiting on another. Take what you need out of the lock, drop the guard, then go parallel. Load a model before the parallel loop (`Backend::load` in `denoise.rs` does this), not inside it.

`scripts/lock-lint.py` (in `just check`, under 2 s) enforces this by reading the source: a guard bound with `let g = x.lock()` (or `.write()`, `.read()`), a temporary guard, or a `get_or_init` closure, that reaches rayon or a function that transitively does before the guard drops. It catches the 0.1.17 lens bug on the old code. It matches functions by name, so a shared name can be a false alarm: move the work out of the lock, or put `// lock-lint: ok <why>` on the lines just above the guard. For a value made once whose making goes parallel, use `argus_engine::get_or_compute(&cell, || ...)` instead of `get_or_init` (a deadlock there needs the initialising worker to steal an asker of the same cell, which a test couldn't force reliably, so this one rests on the lint).

Test parallel code that shares a lock, a cache or a one-time init with `must_finish(secs, what, || ...)` from `engine/tests/main.rs`: it fails the test on a deadlock instead of hanging `just check`. Make the test hostile, as `lens::parallel_renders_of_one_image_never_deadlock_on_its_lens` does: many rayon tasks on one shared value, with `rayon::join` around each call so waiting workers steal.

The first audit (2026-11-02) found, besides the lens bug: `Libraries::dispatch` held the Libraries read lock for the whole of every command, so an `open_library` waiting for a long Apply look would have blocked every other command behind it (now copied out and dropped first); and two `get_or_init`s whose making went parallel (`Analysis::small`, `local.rs`'s small image), now `get_or_compute`.

Don't nest thread pools without a reason. A command that renders many Photos already runs them side by side, so the work inside each one (denoise tiles, masks) should not start a second pool of its own on top.

Every command that can take more than a second writes a start and an end to the Debug log, so a hang reads as a start with no end instead of silence. Tests for a path that handles many Photos run it at a real Shoot's size (100 or more, or a count past any batch or page boundary), because the 0.1.17 hangs and layout bugs all needed more Photos than the tests used.

## In the UI

`call()` in `prototype/src/data/backend.ts` gives every engine call a time budget (10 s, or 30 s for the known long ones in `LONG`). Past it, a quiet toast says what is still working and for how long (`ui/SlowCalls.tsx`); past the stall budget (90 s, or 10 min for long ones) it turns into a warning and a `[stall]` console error. Every scripted check fails on console errors, so a hang now fails a test before it reaches Greg. `smoke-stalled-call-is-loud` in `shots.mjs` proves the chain with a call that never answers.

A screen that waits on something that isn't a single engine call (several batches, a poll, a queue job) shows the time so far next to its progress bar and turns into an error with a reason when the wait has no end. A spinner or a frozen label with no clock is a bug.

Anything that should take longer than about a minute belongs in the Queue (`argus work`), where it has progress, a cancel and a place in the Status bar.
