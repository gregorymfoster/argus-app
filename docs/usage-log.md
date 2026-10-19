# Usage log

Argus records what happens to each Photo in the Library's `usage_events` table, so benches and a future "learn my edits" can run on Greg's real use (ADR 0014). It stays in the Library and is never uploaded. Settings › General turns it off, shows counts, exports it and clears it. The same is the `usage_log` tool (`{"recording": false}`, `{"clear": true}`, `{"photo_id": 7}` for one Photo's events and History).

## Events

Each row is `(id, photo_id, kind, data, at)`, oldest first. `data` by kind:

| kind | data |
|---|---|
| `start` | `recipe` (null = never edited, the default develop), `version`. Written once, before the Photo's first other event |
| `recipe` | `version`, `author` (person/ai), `batch_id`, `label` (the batch: "Auto", "Auto on import", an Agent request, "Suggestion: Warm", "Undo: …"), `recipe` |
| `take` | a Suggestion: `source` (suggestion, or match for "apply to the rest"), `suggestion_id`, `title`, `note`, `recipe`, `reference_id`, `job_id`, `job_kind`, `job_args`, `provider`, `model`, `argus` (version), `seconds` |
| `take_accepted` | `suggestion_id`, `version`, `batch_id` |
| `take_dismissed` | `suggestion_id` |
| `agent` | `prompt`, `answer`, `batch_id`, `provider`, `model`, `argus`, `seconds`. Logged after the Recipes it wrote; they share `batch_id` |
| `verdict` | `verdict`, `author`, `reason`, `batch_id` |
| `stars` | `stars`, `author`, `batch_id` |
| `export` | `recipe` (Look folded in), `path`, `format`, `long_edge`, `quality`, `naming`, `verdict`, `stars` |

Auto has no event of its own: it is a `recipe` event by `ai` whose label starts with "Auto".

## History

`engine::usage::history` folds one Photo's events into: `start`, `takes` (Suggestions, matches, Agent edits and Autos, each with `status` open, dismissed, accepted or applied), `kept` (index of the applied take nearest the exported Recipe, if nearer than the start), `final_recipe`, `export`, `moved` (each slider the person changed from the kept take, or the start, to the final Recipe, by dotted path) and `moved_total` (summed absolute change, mixed units, a rough size), `ai_verdict`/`ai_stars` (the AI's first) and `person_verdict`/`person_stars` (the person's last).

## Bench export

`argus tool export_usage_bench '{"folder":"out/usage-bench"}' --library <dir>` (or Export… in Settings) writes `fixtures/bench`'s layout:

- `edit/cases.json`: one case per exported Photo still in the Library. The usual fields (`id` is `<Shoot>_<stem>`, `raw` is the original's absolute path, `greg` is the exported Recipe rendered at 640 px, `camera` is the starting Recipe rendered the same way) plus `start_recipe`, `final_recipe`, `takes`, `kept`, `moved`, `moved_total` and `export`.
- `cull/<Shoot>/labels.json` and `photos/`: every Shoot where the person flagged or starred something, all its Photos (exported at 640 px with their edits). `flag` is pick or null as in the Lightroom bench, plus `verdict` (real rejects here), `verdict_by`, `rating`, `ai_verdict` and `ai_stars`.

Then `python3 bench/score.py --bench out/usage-bench` runs the usual benches on it, and `--only edit --logged` scores the logged takes themselves: the first, the best and the kept take against what Greg exported. `"images": false` writes only the JSON.
