# 0013. Specialist models sit behind one Analyzer seam and write Facts to the Library

Status: accepted (2026-10-18). Build spec for the "Specialist models in the engine" thread.

## Context

Argus will keep adding small offline models next to the LLM: face landmarks for eyes open, an image embedding for Stacks and search, quality scores, depth, later SAM (see the "Offline AI patterns for Argus" doc). Greg wants that set to grow without the app getting complicated (2026-10-18: "as long as it's clean and nice"). Today each model is wired by hand: the three mask models live in `masks::models`, their output is a bespoke binary cache (`<preview>.mask`, `ANALYSIS_VERSION`), and cull Signals are recomputed from previews on every cull. A tenth model done that way means ten caches, ten version constants and ten places the cull, the Agent and the UI have to know about.

## Decision

One deep module, `engine/src/analyzers/`, owns every specialist. Callers ask for Facts about a Photo and never name a model, a file or a runtime.

**The seam.** An Analyzer is one file implementing a small trait:

```rust
pub trait Analyzer: Sync {
    fn id(&self) -> &'static str;          // "faces", "eyes", "embedding"
    fn version(&self) -> u32;              // bump when output would change
    fn cost(&self) -> Cost;                // Cheap (no model) | Model | Heavy
    fn needs(&self) -> &'static [&'static str]; // Facts it reads, one level deep
    fn run(&self, input: &Input) -> Result<Fact>; // preview + EXIF + needed Facts
    fn caption(&self, _fact: &Fact) -> Option<String> { None } // a few words for the LLM
}
```

A `Fact` is small JSON (`serde_json::Value`) plus an optional binary blob for planes or vectors. The module's public interface is three functions: `facts(lib, photo, ids)` returns Facts, computing missing or stale ones inline; `enqueue(lib, photos)` queues the rest; `REGISTRY` lists the Analyzers in one array. Adding a specialist is one file, one registry line, one model manifest entry and one test.

**One store.** Facts live in `library.db`, table `facts(photo_id, analyzer, version, input_hash, value, blob, computed_at)`, primary key `(photo_id, analyzer)`. A Fact is stale when its `version` differs from the registry's or `input_hash` (the preview's content hash) changed, and stale Facts are recomputed lazily on the next read. Facts are a cache: `clear_cache` may drop them, and nothing a person did lives there. Large planes that already work as files (mask probabilities) may keep a file in `previews/` with the row pointing at it.

**One way to run.** Background work is one Queue job kind, `analyze`, per batch of Photos (ADR 0016), so it survives quits, yields to the Photo Greg has open, and obeys Compute levels: Gentle runs Cheap and Model Analyzers at background priority, Heavy waits for Full or idle (ADR 0007). Import enqueues it after previews. The open Photo computes inline through `facts()`.

**One runtime, one manifest.** Models run through tract on the CPU, as the mask models do (ADR 0012), behind `analyzers::runtime`. `engine/models/manifest.json` lists each model's id, sha256, size, source URL and SPDX licence. Models up to about 30 MB stay embedded with `include_bytes!`. Larger ones download on first use, hash-verified with a progress bar, into the app support folder (`~/Library/Application Support/Argus/models/`), shared by all Libraries so a Library stays portable and small. Cloud sessions read them from `/mnt/project-files/fixtures/models/`, as with sample RAWs. A test fails if any manifest licence is outside an allow-list (Apache-2.0, MIT, BSD), which keeps non-commercial weights out.

**One way for the LLM to see them.** The existing `analyze_photo` tool returns every Fact for a Photo, and Contact sheet captions join each Analyzer's `caption()`. No new Agent tools are needed per model.

**Migration.** The current mask models become the first Analyzers (`segment`, `sky`, `faces`) and the cull Signals become a Cheap `signals` Analyzer, with no change in behaviour. `<preview>.mask` and `ANALYSIS_VERSION` go away. The `facts` table is a schema step appended in `migrate.rs`.

**Proof.** Each Analyzer has a test in `engine/tests/analyzers.rs` on fixture photos (eyes closed are closed, two near-duplicates share a Stack). Ones that change a judgment also get a bench number before the cull or Agent relies on them.

```
 Import ─▶ previews ─▶ Queue job "analyze" ─┐
                                            ▼
 analyze_photo / cull / Agent ─▶ facts() ─▶ REGISTRY ─▶ runtime (tract) ─▶ manifest models
                     ▲                │
                     └── facts table ◀┘   (stale = new version or new preview)
```

## What we deliberately don't build

- No plugin system or dynamic loading. Analyzers are compiled in; the registry is an array.
- No dependency scheduler. `needs` is one level deep and the registry is ordered.
- No second runtime. Candle, ONNX Runtime or Core ML come only through a later ADR, when a model is measurably too slow on tract, and only `analyzers::runtime` changes.
- No Python, model server or separate process at runtime. Training happens offline; only weights ship.
- No per-model settings or model-manager screen. Settings › Library shows the models' disk use, nothing more.
- No migrations of Fact formats. A changed Analyzer bumps its version and recomputes.
- Proposers (style model, Auto, Presets) stay where Suggestions are made, reading Facts. They get their own registry only when there are three of them.

## Consequences

- The cull, the Agent and the UI read one shape of data, and a new model doesn't touch them.
- Every model's licence, size and source is reviewable in one file.
- The first read of a Photo with a new Analyzer pays its cost inline; the Queue usually gets there first.
- Facts make `library.db` larger (an embedding is a few KB per Photo, about 1 MB per 300 Photos).
