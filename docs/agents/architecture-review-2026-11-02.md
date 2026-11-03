# Architecture review, 2026-11-02

Greg asked for a pass with Matt Pocock's `improve-codebase-architecture` skill to keep Argus easy to change. Two explorers walked the engine and the prototype on main at 6574fc6, weighted toward the files changed most since 26 Oct. The visual report with before and after diagrams is https://claude.ai/artifact/1qQ2xS3atTMY4vUwHCUw56. The previous audit (2026-10-22) is `architecture-audit-2026-10-22.md`; its tools/ split and providers/ move are done.

The vocabulary is Matt Pocock's `codebase-design` skill: module, interface, depth, seam, adapter, leverage, locality. Each item says whether it changes behaviour, because refactors here land alone through `just land` while other threads push to main.

## Engine

E1. Give the Look rule one home (Strong, pure move). Domain modules imported `tools::current_recipe`, `with_look` and `find_photo` 24 times, so the seam pointed up into the tool layer, and `jobs::match_hero` hand-wrote the Look-under-Recipe rule again. Done: they are `Library::current_recipe`, `Library::with_look` and `Library::find_photo`, and match_hero calls them. Still open: Look ids are made three ways (`set_look` appends `-2` again and again, so a third copy is `x-2-2`; `match_hero` counts `-2`, `-3` with its own inline slug; `synced` hardcodes `shoot-look`). Unifying them changes new ids, so it waits for a yes.

E2. Split `jobs.rs` (1,556 lines) by concern (done): `jobs/mod.rs` holds the kinds list and priorities, `kinds.rs` how each kind is queued and run, `run.rs` claiming the next job, `policy.rs` the settings, boosts, holds and `gate`, `fanout.rs` the whole-Shoot jobs, `focus.rs` the open Photo jumping the queue, `worker.rs` the round and the thread. Same `pub` paths. Still open: collapse the four telescoping `run_next*` wrappers into one call that takes a `Claim`. High collision risk while the Queue thread is active.

E3. One Queue tick shared by the app's Worker and `argus work` (done: `jobs::Rounds::tick`; its test also caught ending Run now bringing back a cleared battery override). The CLI copy has drifted: it settles Prepare on `!first.is_empty()` rather than `cfg.preparing`, never clears the battery override or go-ahead, never queues due backups and never holds the Mac awake. The loop has no tests.

E4. The Library enforces person-wins (done). "The AI never overrides a person's Verdict or stars" was re-derived in `tools/library.rs` (twice) and `cull.rs` (twice). Now `Library::set_verdict` and `set_rating` skip AI writes over a person's and return the Photos they kept, so `argus flag --ai` and `argus rate --ai` keep a person's too. The person's own `flag` and `rate` commands use `restore_verdict`/`restore_rating`, because the app's Undo writes an AI Verdict back over theirs. Rejected half: making the person's `edit` command refuse unknown Recipe fields the way the `write_recipe` tool does. `Recipe::other` deliberately carries fields the UI owns (such as `presets`, which Preset undo reads), so a person's edit must keep them. Don't re-suggest it.

E5. Small plumbing (Worth exploring): five JPEG encoders, six hand-rolled write-temp-then-rename sites, two `expand_home`, two `newest_shoot`, Recipe path addressing (`suggest::apply_setting`) that belongs on `Recipe`, the after-Import pipeline living in the `import_folder` match arm (so `argus import` skips it), and `Library::photo` loading every Photo to find one, which makes a 100+ Photo export quadratic.

## Prototype

U1. One "change many Photos" verb (done for the Apply popover): `store/changePhotos.ts` takes the engine tool and its arguments plus the demo's Recipes, picks the adapter and records the one ⌘Z step. Apply look, Crop & straighten and Paste use it. Still open: Edit's and the Look screen's Sync to Look, Auto (`data/auto.ts` builds an Action instead) and the `look/*` cases in the store; and `String(e).replace(/^Error: /, '')` is still copied about 27 times where `aiError` would do.

U2. Move the Export domain out of `ui/Export.tsx` (done): settings, presets, file names, size and time estimates, learned JPEG rates and the `export_photos` arguments live in `data/export.ts`. The run store and `startExport` stay with the dialog because they drive its toast.

U3. One harness for the live scripts (started): `prototype/scripts/harness.mjs` has `benchLibrary`, `startBridge` (private model lock, killed on exit, waits until the engine answers), `startVite` and `launchBrowser`. `framing-check` and `apply-edit-check` use it (apply-edit-check never killed its engine before). 14 more scripts still spawn `argus serve` by hand; move each when you next touch it.

U4. Put the Tauri shell calls behind `data/shell.ts` (done): window drag, zoom, menu events and update commands, no-ops in a browser. `just check` fails if anything but `data/backend.ts` and `data/shell.ts` imports @tauri-apps.

U5. Smaller (Worth exploring): CullMode hardcodes its flag and star keys instead of the shared `VERDICT_KEYS`/`STAR_KEYS`; four take-once localStorage handoffs after a reload each have their own effect in App.tsx; ETA, byte and seconds-left formatters are scattered (co-locate unchanged; merging changes copy, so it's a design call).

## Checked and fine

`Tools::call` routing by area, `commands.rs` as the person's own seam, `library/places.rs`, `export.rs`, shoot_look, shoot_match and pre_edit, decode and develop in `lib.rs`, `backend.ts`, `router.ts`, `ProgressBar`, `layout-lib.mjs`, Import's separate demo and live dialogs.
