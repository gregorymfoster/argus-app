# Argus over MCP

`argus mcp --library <dir>` serves a Library's tools to any MCP client over stdio. Claude Desktop and Claude Code use it to cull and edit from a conversation, on your own subscription, because the chat app does the thinking (ADR 0003). The in-app Agent calls the very same tools through `argus_engine::tools::Tools`, so nothing is MCP-only or app-only.

Everything stays on the Mac. The server talks only to the client that launched it, and nothing listens on the network.

## Add Argus to Claude Desktop

Build the CLI once, from the repo:

```sh
cargo build --release -p argus-cli
```

Then open Claude Desktop's Settings, go to Developer, and press Edit Config. That opens `~/Library/Application Support/Claude/claude_desktop_config.json`. Add an `argus` entry under `mcpServers` (keep any servers already there), with absolute paths:

```json
{
  "mcpServers": {
    "argus": {
      "command": "/path/to/argus/target/release/argus",
      "args": ["mcp", "--library", "/Users/you/Pictures/Argus/Argus.library"]
    }
  }
}
```

The Library folder is created on first use if it doesn't exist. Point it at the same Library the app or `argus serve` uses and changes show up there too, since they share one `library.db`.

Quit Claude Desktop completely (Cmd-Q) and reopen it. Argus then appears in the tools menu under the chat box. Try "Import ~/Pictures/2026-10-12_tahoe into Argus and show me the contact sheet", then "reject the blurry ones".

If it doesn't show up, look at `~/Library/Logs/Claude/mcp-server-argus.log`. The server writes one line to stderr when it starts, and errors land there too.

## Add Argus to Claude Code

```sh
claude mcp add argus -- /path/to/argus/target/release/argus mcp --library /Users/you/Pictures/Argus/Argus.library
```

## The tools

| Tool | What it does |
| --- | --- |
| `list_shoots` | Shoots in the Library, with Photo counts |
| `rename_shoot` | Rename a Shoot by the naming rules; its originals folder follows |
| `list_photos` | Photos in capture order with Verdict, reason and Stack; filter by Shoot or Verdict |
| `find_cards` | Memory cards mounted on this Mac (volumes with a `DCIM` folder) |
| `scan_folder` | The photos in a folder or on a Card without importing them, each marked if the Library already has it (same name, size and capture time); a RAW+JPEG frame is one Photo, the RAW |
| `import_folder` | Import a folder, or just the chosen `files` in it, as one new Shoot. A RAW+JPEG frame is one Photo (the RAW, with its JPEG kept beside it, or skipped with `raw_only`) (dedupes, never writes to originals) |
| `estimate_import` | How much disk an Import would take before running it: the originals' size, whether they'd be copied into the Library, the estimated previews, the total and the free space. Reads file sizes only |
| `library_storage` | Where the Library folder is and what each part takes on disk (originals inside and outside it, previews, cache, database, Backups), how many originals are missing, and the free space |
| `library_settings` | Read or change the Library's settings: `preview_long_edge` and `preview_quality` for the previews Import makes |
| `rebuild_previews` | Re-make every Photo's preview at the current `library_settings` (about a second per RAW) |
| `clear_cache` | Delete what the Library can rebuild (its cache and unused previews); returns the bytes freed |
| `find_missing_originals` / `relink_originals` | List Photos whose original can't be found, or find them under a folder (name, size, then content hash) and point the Library at them, changing nothing in that folder |
| `get_preview` | One Photo's details plus a JPEG (1024 px by default, 1536 at most) |
| `get_contact_sheet` | A numbered grid of up to 30 thumbnails per page plus a legend from number to Photo id |
| `set_verdict` | One Verdict on many Photos. Recorded as the AI's by default, and the person's own flags are kept |
| `set_rating` | One star Rating (0-5) on many Photos. Recorded as the AI's by default, and the person's own stars are kept |
| `read_tags` | Each Photo's caption and Keywords (hierarchical, under a Facet), each marked as the person's or an AI suggestion not yet kept |
| `set_keywords` | Add (kept at once), remove (an AI suggestion is remembered as dismissed), `accept` or `accept_all` Keywords on many Photos |
| `set_caption` | Set the caption (IPTC Description) of many Photos; empty removes it |
| `list_keywords` | Keywords in use across the Library, most used first, with a `prefix` for autocomplete |
| `search_photos` | Find Photos by words across the Library: every word must match a Keyword (kept first, then AI suggestions), caption, file name or Shoot name. With the SigLIP model in the models folder (`engine/src/meaning.rs`) it also finds Photos that look like the words, marked `looks like` in `matched` |
| `meaning_model` | Whether search by meaning is installed (407 MB model in the models folder); `download: true` queues the one-time download (job `download_model`: resumes, checks each file's sha256, then analyses the Library) |
| `list_albums` / `save_album` / `edit_album` / `delete_album` | Albums: saved searches in the sidebar that find their Photos again each time |
| `list_people` / `get_person` | The People view: named people with their Photos, groups of unnamed faces that look like one person (with the named person each looks most like), and one person's Photos plus faces that might be them. Grouping runs locally (SFace) and never names anyone |
| `name_faces` / `not_person` / `hide_faces` / `rename_person` | Name faces only with a name the person gave (each Photo keeps `People|<name>`), say a face isn't someone, hide strangers, rename or merge people |
| `read_faces` | Queue the face models for Photos whose faces aren't read yet |
| `tag_photos` | The AI suggests a caption and Keywords per Photo (one model call each, never People); background job unless `wait` |
| `best_photos` | A Shoot's Best Photos from the last cull, best first, minus any the person rejected |
| `list_stacks` / `set_stacks` | Read or replace a Shoot's Stacks, best frame first |
| `stack_bursts` | Join a Shoot's bursts (alike frames seconds apart, from the embedding Fact) into its Stacks |
| `cull_shoot` | Run Argus's own Provider over a Shoot (Ollama by default, Gentle compute): Verdicts, stars, Stacks and Best Photos in one pass |
| `rerate_shoot` | Learn as you cull: refresh a culled Shoot's AI stars from the person's Picks and Rejects so far (not an undo step), and return the Keepers first `order` (every Photo but the Rejects, likeliest keeper first) |
| `read_recipe` / `write_recipe` | Read a Photo's Recipe, or save one as a new Version on many Photos in one batch |
| `edit_recipe` | Change a few settings (exposure, temp, tint, contrast, highlights, shadows, whites, blacks, vibrance, saturation, `hsl.<colour>.<hue\|sat\|lum>`, and the retouch amounts `skin`, `eyes`, `teeth` and `correction.face`) on many Photos as one batch, absolute or relative |
| `auto_edit` | Lightroom's Auto on `photo_ids` or every Photo of `shoot_id`, as one batch: exposure, tone, white balance and vibrance measured from small renders of each Photo (well under a second each, no model); the rest of the Recipe is kept |
| `learn_edit_head` | Learn this Library's own Edit head from every RAW the person has exported, on top of the one Argus ships; Auto uses it when the Edit head is on |
| `white_balance` | Lightroom's white balance menu (`as_shot`, `auto`, `daylight` and the rest) and eyedropper (`at`, a neutral point) on many Photos as one batch |
| `list_presets` / `apply_preset` | The Presets panel: the person's own (group Yours) and the built-in groups; apply one to many Photos at an Amount (0-200%) as one batch, or `remove` it |
| `save_preset` / `delete_preset` | Lightroom's Create Preset from one Photo's settings groups, rename one, or delete one of the person's own |
| `rank_presets` | The AI's suggested Preset order for a Shoot, cached; queued in the background unless `wait` |
| `analyze_photo` | What the local Mask models see: faces (numbered left to right, with boxes) and how much of the frame is people, skin, hair and sky |
| `add_mask` | Add a local adjustment (slider deltas inside a Mask: `subject`, `background`, `faces` with optional `face`, `skin`, `hair`, `sky`, `brush`, `linear`, `radial`) to many Photos as one batch |
| `edit_mask` / `remove_mask` | Change or remove one local adjustment by its id |
| `add_spot` / `edit_spot` / `remove_spot` | Lightroom's Heal and Clone: cover a small circle (blemish, stray hair, dust) with pixels from elsewhere in the Photo, on many Photos as one batch; Argus picks the source unless `from` is given. Move, resize, re-source or remove one by id |
| `pick_spot_source` | Where Argus would copy from for a Spot, without changing anything (the Edit screen's Heal tool) |
| `view_mask` | See where a Mask lands, tinted over the render, with the share of the frame it covers |
| `view_depth` | Lens Blur's depth map, the focal range in use, a depth histogram, and the range around a point |
| `render_photo` | See a Photo with its edits, rendered from the original (or try a draft Recipe without saving it) |
| `list_versions` | A Photo's Recipe history: every Version with who made it and its batch label |
| `undo_batch` | Undo one batch (an Agent request, a cull, a Look change): Recipes, Verdicts and Looks go back as they were. Defaults to the newest AI batch |
| `list_looks` / `set_look` | Read a Shoot's Looks, or create or tune one (sliders as in `edit_recipe`) and put Photos under it; a new Look goes on the Picks |
| `sync_to_look` | Lightroom's Sync: fold one Photo's own sliders into its Shoot's Look |
| `paste_settings` | Lightroom's Copy/Paste and Sync settings: groups of one Photo's settings onto others, one batch |
| `remove_look` | Take a Look off every Photo under it (the Look is kept, so undo puts it back) |
| `export_photos` | Write edited JPEGs to a folder, never overwriting |
| `crop.*` settings | `edit_recipe` also straightens (`crop.angle`, degrees) and crops (`crop.x/y/w/h`, fractions of the straightened frame, never empty corners) |
| `suggest_edits` | Queue Suggestions (2-3 whole edits each) for one Photo, a list, or a Shoot (rejects skipped, Picks first) |
| `match_to_photo` | "Apply this look to the rest": the Hero Photo's style becomes a Look, the others get 2 matched Suggestions under it (on a RAW the first is the Shoot match, worked out from the Shoot's hand edits without a model call) |
| `pre_edit_shoot` | Edit as you cull: a Shoot match Suggestion for each Pick and likely keeper (4★ and up) the person hasn't edited, from the Shoot's hand edits, in the background with no model call. Runs by itself after each hand edit of a RAW and as the stars refresh |
| `list_suggestions` / `get_suggestion` | Open Suggestions for a Photo or Shoot (title, why, whole Recipe), or one with its preview JPEG |
| `accept_suggestion` / `dismiss_suggestion` | The person's call: accepting saves a new Version; `suggestion_ids` accepts one take on each of several Photos in one batch |
| `enqueue_job` / `list_jobs` / `cancel_job` / `reorder_jobs` | The Queue directly: kinds `suggest`, `suggest_shoot`, `match_hero`, `cull_shoot` (alias `rate_shoot`), `check_shoot`, `tag_photos`, `analyze`, `rank_presets`, `learn_edits`, `denoise`, `render_previews` (`jobs::KINDS`); asking twice returns the queued job; `reorder_jobs` sets the waiting order (the open Photo's work stays first) |
| `suggestions_progress` | A Photo's takes on their way: the queued or running `suggest` job, its stage (Queued, Model thinking, Review round k of n, Saving takes), how many takes, what runs before it and `eta_s` until they land; the Edit strip's New takes bar. A cancelled job stops at its next step and saves nothing |
| `queue_status` / `set_queue` / `retry_failed` | What's running, queued, done today and failed, a rough ETA, and the Queue's Compute level, pause and idle boost. The UI also calls it `progress`. `retry_failed` re-queues failed jobs |
| `usage_log` | The usage log kept in this Library (ADR 0014): counts, one Photo's events with `photo_id`, `recording` on or off, `clear` |
| `export_usage_bench` | Write the usage log as bench cases for `python3 bench/score.py --bench <folder>` |
| `style_profile` | How the person usually corrects the AI's takes before exporting, learned from the usage log; fresh Suggestions add these moves |

A client that can see images (Claude can) usually culls by itself with `get_contact_sheet`, `get_preview`, `set_verdict` and `set_stacks`, which is quick and uses the subscription. `cull_shoot` hands the job to the local model instead, which can take many minutes and works the Mac.

Tool failures (an unknown Photo, a bad argument) come back as tool errors with a plain message, so the model can correct itself.

## The in-app Agent

The Agent (`engine/src/agent.rs`, the `agent` command, `argus agent "<request>" --library <dir> [--photo N]`) is a tool-use loop over a subset of this tool list (`agent::TOOLS`: no import, export, cull, Presets, Library storage or Queue control). It starts every request with the Shoot's Contact sheet and the Photo in focus rendered with its Recipe, records every change of one request in one batch (so `undo_batch` reverts the whole request), and talks to every Provider in one JSON step protocol (ADR 0008). A new tool reaches the Agent by adding its name to `agent::TOOLS`. The Agent can queue Suggestions and "apply to the rest" but never accepts a Suggestion: that is the person's call.

## The Queue

Background AI work (`engine/src/jobs.rs`, ADR 0016) lives in the Library's `jobs` table, so a quit loses nothing: the next start puts a job left running back in the queue. The app and `argus serve` each run one worker (`ARGUS_QUEUE=off` turns it off); `argus work --library <dir> [--once] [--compute C]` runs one from the shell with the same holds as the app (battery, heat, idle and overnight waits, pause); `--drain` runs every queued job now, for benches and scripts. Highest priority first: the Photo open in Loupe or Edit 100 (run at Full), the next few in its filmstrip 70, previews after an Import 60, whole-Shoot fan-outs and culls 50, "apply to the rest" 40, Picks 20, others 10. The UI's `focus_photo` command {photo_id, next_ids} moves those up. An Agent request runs at Full and background work makes way while it does (`compute::Interactive`: no new jobs, and a Gentle job's Ollama stops being paused). `argus mcp` runs a worker too. A Library move waits until no job is running. The worker rests between jobs by Compute level (Gentle 5 s, Balanced 2 s) and goes to Full by itself after the Mac has been idle `idle_boost_min` minutes (3 by default). On battery only the open Photo's jobs run, and no boost to Full, unless `run_on_battery` or `run_anyway` (until next plugged in) is set; `ARGUS_POWER=battery|ac` fakes the power source. `retry_failed` re-queues failed jobs. Suggestions are one model request per Photo (`engine/src/suggest.rs`), sent to Ollama with `think: false` (`ARGUS_OLLAMA_THINK=1` lets Qwen think first; every request logs its prompt and output tokens and times to stderr as `argus ollama: ...`), with previews saved under `cache/suggestions/`; other threads enqueue with `enqueue_job` (`import_folder` with `cull` on queues `check_shoot` (the rules cull; `cull_shoot` when the model checks), then `suggest_shoot` and `tag_photos`, and the Analyzers queue `analyze`).

## Adding a tool

Add its spec and its match arm to the area file in `engine/src/tools/` it belongs to (or a new area listed in `AREAS` in `tools/mod.rs`), calling a Library or engine function rather than putting logic there, and add a case to `engine/tests/tools.rs`. The MCP server picks it up with no change. `cli/tests/mcp.rs` checks the stdio round trip with a real MCP client.

## Not yet

ChatGPT needs the server over HTTPS (spec, Headless). That means a streamable-HTTP mode for `argus mcp` plus a tunnel, and it needs its own spike. The Recipe's slider units are documented in `engine/src/recipe.rs`.
