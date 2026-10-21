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
| `get_preview` | One Photo's details plus a JPEG (1024 px by default, 1536 at most) |
| `get_contact_sheet` | A numbered grid of up to 30 thumbnails per page plus a legend from number to Photo id |
| `set_verdict` | One Verdict on many Photos. Recorded as the AI's by default, and the person's own flags are kept |
| `set_rating` | One star Rating (0-5) on many Photos. Recorded as the AI's by default, and the person's own stars are kept |
| `best_photos` | A Shoot's Best Photos from the last cull, best first, minus any the person rejected |
| `list_stacks` / `set_stacks` | Read or replace a Shoot's Stacks, best frame first |
| `stack_bursts` | Join a Shoot's bursts (alike frames seconds apart, from the embedding Fact) into its Stacks |
| `cull_shoot` | Run Argus's own Provider over a Shoot (Ollama by default, Gentle compute): Verdicts, stars, Stacks and Best Photos in one pass |
| `read_recipe` / `write_recipe` | Read a Photo's Recipe, or save one as a new Version on many Photos in one batch |
| `edit_recipe` | Change a few settings (exposure, temp, tint, contrast, highlights, shadows, whites, blacks, vibrance, saturation, `hsl.<colour>.<hue\|sat\|lum>`, and the retouch amounts `skin`, `eyes`, `teeth` and `correction.face`) on many Photos as one batch, absolute or relative |
| `auto_edit` | Lightroom's Auto on `photo_ids` or every Photo of `shoot_id`, as one batch: exposure, tone, white balance and vibrance measured from small renders of each Photo (well under a second each, no model); the rest of the Recipe is kept |
| `analyze_photo` | What the local Mask models see: faces (numbered left to right, with boxes) and how much of the frame is people, skin, hair and sky |
| `add_mask` | Add a local adjustment (slider deltas inside a Mask: `subject`, `background`, `faces` with optional `face`, `skin`, `hair`, `sky`, `brush`, `linear`, `radial`) to many Photos as one batch |
| `edit_mask` / `remove_mask` | Change or remove one local adjustment by its id |
| `add_spot` / `edit_spot` / `remove_spot` | Lightroom's Heal and Clone: cover a small circle (blemish, stray hair, dust) with pixels from elsewhere in the Photo, on many Photos as one batch; Argus picks the source unless `from` is given. Move, resize, re-source or remove one by id |
| `pick_spot_source` | Where Argus would copy from for a Spot, without changing anything (the Edit screen's Heal tool) |
| `view_mask` | See where a Mask lands, tinted over the render, with the share of the frame it covers |
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
| `match_to_photo` | "Apply this look to the rest": the Hero Photo's style becomes a Look, the others get 2 matched Suggestions under it |
| `list_suggestions` / `get_suggestion` | Open Suggestions for a Photo or Shoot (title, why, whole Recipe), or one with its preview JPEG |
| `accept_suggestion` / `dismiss_suggestion` | The person's call: accepting saves a new Version; `suggestion_ids` accepts one take on each of several Photos in one batch |
| `enqueue_job` / `list_jobs` / `cancel_job` / `reorder_jobs` | The Queue directly: kinds `suggest`, `suggest_shoot`, `match_hero`, `cull_shoot` (alias `rate_shoot`), `render_previews`; asking twice returns the queued job; `reorder_jobs` sets the waiting order (the open Photo's work stays first) |
| `suggestions_progress` | A Photo's takes on their way: the queued or running `suggest` job, its stage (Queued, Model thinking, Review round k of n, Saving takes), how many takes, what runs before it and `eta_s` until they land; the Edit strip's New takes bar. A cancelled job stops at its next step and saves nothing |
| `queue_status` / `set_queue` | What's running, queued, done today and failed, a rough ETA, and the Queue's Compute level, pause and idle boost. The UI also calls it `progress` |

A client that can see images (Claude can) usually culls by itself with `get_contact_sheet`, `get_preview`, `set_verdict` and `set_stacks`, which is quick and uses the subscription. `cull_shoot` hands the job to the local model instead, which can take many minutes and works the Mac.

Tool failures (an unknown Photo, a bad argument) come back as tool errors with a plain message, so the model can correct itself.

## The in-app Agent

The Agent (`engine/src/agent.rs`, the `agent` command, `argus agent "<request>" --library <dir> [--photo N]`) is a tool-use loop over this same tool list, minus import, export and `cull_shoot`. It starts every request with the Shoot's Contact sheet and the Photo in focus rendered with its Recipe, records every change of one request in one batch (so `undo_batch` reverts the whole request), and talks to every Provider in one JSON step protocol (ADR 0008). A new tool reaches the Agent by adding its name to `agent::TOOLS`. The Agent can queue Suggestions and "apply to the rest" but never accepts a Suggestion: that is the person's call.

## The Queue

Background AI work (`engine/src/jobs.rs`, ADR 0010) lives in the Library's `jobs` table, so a quit loses nothing: the next start puts a job left running back in the queue. The app and `argus serve` each run one worker (`ARGUS_QUEUE=off` turns it off); `argus work --library <dir> [--once] [--compute C]` runs one from the shell with the same holds as the app (battery, heat, idle and overnight waits, pause); `--drain` runs every queued job now, for benches and scripts. Highest priority first: the Photo open in Loupe or Edit 100 (run at Full), the next few in its filmstrip 70, previews after an Import 60, whole-Shoot fan-outs and culls 50, "apply to the rest" 40, Picks 20, others 10. The UI's `focus_photo` command {photo_id, next_ids} moves those up. An Agent request runs at Full and background work makes way while it does (`compute::Interactive`: no new jobs, and a Gentle job's Ollama stops being paused). `argus mcp` runs a worker too. A Library move waits until no job is running. The worker rests between jobs by Compute level (Gentle 5 s, Balanced 2 s) and goes to Full by itself after the Mac has been idle `idle_boost_min` minutes (3 by default). On battery only the open Photo's jobs run, and no boost to Full, unless `run_on_battery` or `run_anyway` (until next plugged in) is set; `ARGUS_POWER=battery|ac` fakes the power source. `retry_failed` re-queues failed jobs. Suggestions are one model request per Photo (`engine/src/suggest.rs`), sent to Ollama with `think: false` (`ARGUS_OLLAMA_THINK=1` lets Qwen think first; every request logs its prompt and output tokens and times to stderr as `argus ollama: ...`), with previews saved under `cache/suggestions/`; other threads enqueue with `enqueue_job` (the post-import pipeline queues `cull_shoot`, then `suggest_shoot`).

## Adding a tool

Add it to `specs()` and `Tools::call` in `engine/src/tools.rs`, calling a Library or engine function rather than putting logic there, and add a case to `engine/tests/tools.rs`. The MCP server picks it up with no change. `cli/tests/mcp.rs` checks the stdio round trip with a real MCP client.

## Not yet

ChatGPT needs the server over HTTPS (spec, Headless). That means a streamable-HTTP mode for `argus mcp` plus a tunnel, and it needs its own spike. Masks become tools as the engine grows them. The Recipe's slider units are documented in `engine/src/recipe.rs`.
