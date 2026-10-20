# Performance: what an open Argus costs

Greg (2026-10-20): outside AI jobs and rendering, the app should be as light as it can be. This page holds the measurements, the rules that came out of them, and the ideas not done yet with what they'd buy.

## Measuring

`just perf` (cloud session only, it reads `/proc`) builds the UI and the CLI, imports the sample RAWs, copies their rows up to 1,000 Photos with their own preview files, and drives the built UI against `argus serve` in Chromium. It reports:

- startup: page load to the first thumbnail, and the JS, CSS and fonts shipped
- idle: engine calls, KB, React commits and CPU (bridge and page) per minute with the grid open, then the same with the window hidden
- scroll: the whole grid a screen at a time, each screen's thumbnails loaded before the next, then page CPU, image bytes, page memory and JS heap

`--profile` adds the page's top functions while scrolling, `--diff a.json b.json` compares two runs. Headless Chromium paints in software, so absolute CPU numbers run high next to WebKit on the Mac's GPU; compare runs with each other, not with the Mac. For the engine alone, run `argus serve` on a Library and read its CPU time and voluntary context switches (wakeups) from `/proc/<pid>/task/*/stat` and `status` a minute apart.

## Results, 2026-10-20

Before is main at b398224, after is the commit that added this page. 1,000 Photos, grid open, 60 s each.

| | before | after |
|---|---|---|
| Idle engine calls / min | 90 | 53 |
| Idle JSON / min | 133 KB (up to 350 KB as the meter's history filled) | 19 KB |
| Idle React commits / min | 85 | 6 |
| Idle page CPU / min | 1,930 ms | 420 ms |
| Hidden window: calls, page CPU / min | 90, 1,550 ms | 0, 10 ms |
| `argus serve` alone, idle CPU / wakeups per min | 90 ms / 300 | 20 ms / 30 |
| Scroll 1,000 Photos: page CPU | 21.9 s (32 s with every tile loading in turn) | 10.8 s |
| Scroll: image bytes fetched | 195 MB | 25 MB |
| Scroll: page memory after | 1,074 MB | 709 MB |

What made the difference, biggest first:

1. Grid thumbnails. Tiles loaded the 1,600 to 2,048 px previews; at the default tile size the screen shows about 260 px. `previews/thumbs/<preview>` is the preview at 480 px, made by the engine on first request (`library_file`, so `argus://` and `argus serve` both get it) and remade when the preview changes. It's cache: Clear cache removes it. Tiles bigger than 480 device pixels still load the preview.
2. No `backdrop-filter` on grid tiles. Every tile had three hidden flag buttons and a verdict chip with a blur, each its own compositing layer.
3. Real preview URLs set at once instead of one `requestIdleCallback` (and one React commit, layout and paint) per tile.
4. Shared polls (`prototype/src/data/poll.ts`): one request per command and arguments however many components show it, none while the window is hidden, and a re-render only when the answer changed. `queue_status` folded into `progress`.
5. The Resource meter's sidebar line asks every 10 s for 90 points without marking itself as watched, so the engine samples every 10 s instead of 2 s; the panel asks every 2 s only while open.
6. The Queue worker sleeps parked instead of waking every 200 ms, and on an empty queue checks one `EXISTS` query instead of opening the Library three times and reading power and heat.
7. `eye_flags` reads one query for the Shoot instead of one per Photo (10 ms to under 1 ms at 1,000 Photos).
8. Memoised grid tiles. A P or X re-rendered every tile on screen with its context menu and tooltips; now only the tiles whose flag, stars or selection changed do. Flagging 30 Photos in a row: 1,480 ms of script down to 540 ms (about 50 to 18 ms a keypress in the cloud). Tile handlers go through the screen's latest state (`act` in Cull.tsx) and the context menu is built when it opens, so a tile that skipped a render never acts on an old selection.

## Rules

- Poll through `usePoll` or `whileShown`, never a bare `setInterval`: they pause while the window is hidden and skip unchanged answers.
- Grid-sized images use `size="tile"` on `PhotoView` (the 480 px thumbnail). Large views keep `thumb` or `full`.
- No `backdrop-filter` or CSS `filter` on anything repeated per tile. An identity filter is left off the `<img>`.
- A command a screen calls for a whole Shoot does one query, not one per Photo.
- Grid tiles stay memoised: pass data props that keep their identity (no fresh objects or arrays per render) and route handlers through `act`.

## Not done yet, with what they'd buy

- Push instead of poll. The engine could bump a change counter on every write and send a Tauri event (or one cheap `changes` call); `list_suggestions`, `list_jobs` and `eye_flags` would then load only when something changed. Saves the remaining ~50 idle calls a minute (about 40 ms of engine CPU and most of the page's 420 ms).
- The rest of a flag's cost is the Cull screen itself recomputing counts, star filters and the visible list over every Photo of the Shoot on each change (about 18 ms a keypress at 1,000 Photos in the cloud). Moving those into the store, updated per change, would make a flag cost the same at 100 or 5,000 Photos.
- One Library connection per process. Each command opens SQLite, sets pragmas and checks the schema (0.47 ms, about 70% of a cheap call like `list_jobs`). A pooled connection would halve idle engine time; it needs care around `migrate` and long writers (ADR 0009).
- Code splitting. The UI ships 886 KB of JS in one chunk; Edit, Look, Settings and the Agent panel could load on first use. Startup is already under a second from local files, so this mostly saves parse time and memory.
- Smaller `photos` rows. 546 bytes per Photo (1,000 Photos: 546 KB, 11 ms); dropping fields the grid doesn't read would trim All photos on big Libraries.
