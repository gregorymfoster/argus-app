//! Turning the usage log (library::usage) into bench cases: one edit case per exported
//! Photo (where it started, every AI take, what the person kept and how far they moved
//! from it, the final Recipe) and one cull case per Shoot the person flagged or starred
//! (the AI's Verdicts and stars against theirs). The layout is fixtures/bench's, so
//! `python3 bench/score.py --bench <folder>` scores Argus against real use. Format and
//! fields: docs/usage-log.md.

use crate::library::{Library, Photo, PhotoFilter, PhotoId, UsageEvent};
use crate::tools::{Part, Tools};
use crate::{Error, Result};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The Provider name and model an AI take came from, resolved the way the Provider
/// itself resolves them (argument, then ARGUS_PROVIDER, then Ollama).
pub fn provider_model(provider: Option<&str>) -> (String, String) {
    let env = std::env::var("ARGUS_PROVIDER").ok();
    let name = provider.or(env.as_deref()).unwrap_or("ollama").to_string();
    let model = match name.as_str() {
        "ollama" => crate::cull::Ollama::from_env().model,
        "claude-code" => crate::cull::ClaudeCode::from_env().model,
        _ => std::env::var("ARGUS_CLAUDE_MODEL").unwrap_or_else(|_| "default".into()),
    };
    (name, model)
}

/// One Photo's story, read from its events.
#[derive(Debug, Clone, Default, Serialize)]
pub struct History {
    /// The Recipe before anything changed (null: the default develop).
    pub start: Value,
    /// Every AI take: Suggestions, matches, Agent edits and Auto, oldest first.
    pub takes: Vec<Value>,
    /// Index into `takes` of the applied take nearest the exported Recipe, if any is
    /// nearer than the start: the one the person built on.
    pub kept: Option<usize>,
    /// The Recipe at the last export (Look folded in), and the export itself.
    pub final_recipe: Option<Value>,
    pub export: Option<Value>,
    /// The person's slider changes from the kept take (or the start) to the final Recipe.
    pub moved: Map<String, Value>,
    pub moved_total: f64,
    /// The first AI Verdict and stars, and the person's last ones.
    pub ai_verdict: Option<String>,
    pub ai_stars: Option<u64>,
    pub person_verdict: Option<String>,
    pub person_stars: Option<u64>,
}

/// Fold one Photo's events (oldest first) into its History.
pub fn history(events: &[UsageEvent]) -> History {
    let mut h = History::default();
    // The take that a recipe event of an AI batch came from, by batch id.
    let mut by_batch: BTreeMap<i64, usize> = BTreeMap::new();
    let mut by_suggestion: BTreeMap<i64, usize> = BTreeMap::new();
    // The Agent logs its request after the Recipes it wrote, so find its batches first.
    let agent: BTreeMap<i64, &Value> = events
        .iter()
        .filter(|e| e.kind == "agent")
        .filter_map(|e| Some((e.data.get("batch_id")?.as_i64()?, &e.data)))
        .collect();
    for e in events {
        let d = &e.data;
        let batch = d.get("batch_id").and_then(Value::as_i64);
        match e.kind.as_str() {
            "start" => h.start = d["recipe"].clone(),
            "take" => {
                let mut t = d.clone();
                t["status"] = json!("open");
                if let Some(id) = d["suggestion_id"].as_i64() {
                    by_suggestion.insert(id, h.takes.len());
                }
                h.takes.push(t);
            }
            "take_dismissed" => {
                if let Some(&i) = d["suggestion_id"]
                    .as_i64()
                    .and_then(|s| by_suggestion.get(&s))
                {
                    h.takes[i]["status"] = json!("dismissed");
                }
            }
            "take_accepted" => {
                if let Some(&i) = d["suggestion_id"]
                    .as_i64()
                    .and_then(|s| by_suggestion.get(&s))
                {
                    h.takes[i]["status"] = json!("accepted");
                    if let Some(b) = batch {
                        by_batch.insert(b, i);
                    }
                }
            }
            "recipe" => {
                let label = d["label"].as_str().unwrap_or("");
                if let Some((b, a)) = batch.and_then(|b| Some((b, *agent.get(&b)?))) {
                    if let std::collections::btree_map::Entry::Vacant(e) = by_batch.entry(b) {
                        let mut t = a.clone();
                        t["source"] = json!("agent");
                        t["status"] = json!("applied");
                        e.insert(h.takes.len());
                        h.takes.push(t);
                    }
                }
                let from = batch.and_then(|b| by_batch.get(&b).copied());
                if let Some(i) = from {
                    // The Agent's take is whatever its batch wrote last.
                    if h.takes[i]["source"] == "agent" {
                        h.takes[i]["recipe"] = d["recipe"].clone();
                    }
                } else if d["author"] == "ai" && label.starts_with("Auto") {
                    h.takes.push(json!({ "source": "auto", "status": "applied",
                                         "label": label, "recipe": d["recipe"] }));
                }
            }
            "verdict" | "stars" => {
                let person_side = d["author"] == "person";
                match (e.kind.as_str(), person_side) {
                    ("verdict", false) if h.ai_verdict.is_none() => {
                        h.ai_verdict = d["verdict"].as_str().map(str::to_owned)
                    }
                    ("verdict", true) => {
                        h.person_verdict = d["verdict"].as_str().map(str::to_owned)
                    }
                    ("stars", false) if h.ai_stars.is_none() => h.ai_stars = d["stars"].as_u64(),
                    ("stars", true) => h.person_stars = d["stars"].as_u64(),
                    _ => {}
                }
            }
            "export" => {
                h.final_recipe = Some(d["recipe"].clone());
                h.export = Some(d.clone());
            }
            _ => {}
        }
    }
    if let Some(fin) = &h.final_recipe {
        // Kept: of the takes that were applied, the one nearest the exported Recipe, if
        // it is nearer than where the Photo started. Robust to Undo and re-accepts.
        let dist = |r: &Value| slider_moves(r, fin).1;
        let mut best = dist(&h.start);
        for (i, t) in h.takes.iter().enumerate() {
            if t["status"] == "accepted" || t["status"] == "applied" {
                let d = dist(&t["recipe"]);
                if d < best {
                    (best, h.kept) = (d, Some(i));
                }
            }
        }
        let from = h.kept.map_or(&h.start, |i| &h.takes[i]["recipe"]);
        let (moved, total) = slider_moves(from, fin);
        h.moved = moved;
        h.moved_total = total;
    }
    h
}

/// Every numeric slider that differs between two Recipes (null = the default develop),
/// keyed by its dotted path ("hsl.blue.sat"), with the change, plus the summed size.
pub fn slider_moves(from: &Value, to: &Value) -> (Map<String, Value>, f64) {
    // Through Recipe, so a take's few sliders and a saved whole Recipe compare alike.
    let whole = |v: &Value| match v {
        Value::Null => crate::recipe::Recipe::default().to_value(),
        v => crate::recipe::Recipe::from_value(v).map_or_else(|_| v.clone(), |r| r.to_value()),
    };
    let (mut a, mut b) = (BTreeMap::new(), BTreeMap::new());
    leaves(&whole(from), "", &mut a);
    leaves(&whole(to), "", &mut b);
    let mut moved = Map::new();
    let mut total = 0.0;
    let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    for k in keys {
        let d = b.get(k).unwrap_or(&0.0) - a.get(k).unwrap_or(&0.0);
        if d.abs() > 1e-9 {
            total += d.abs();
            moved.insert(k.clone(), json!((d * 1000.0).round() / 1000.0));
        }
    }
    (moved, (total * 1000.0).round() / 1000.0)
}

fn leaves(v: &Value, prefix: &str, out: &mut BTreeMap<String, f64>) {
    match v {
        Value::Number(n) => {
            out.insert(prefix.to_string(), n.as_f64().unwrap_or(0.0));
        }
        Value::Object(m) => {
            for (k, v) in m.iter().filter(|(k, _)| k.as_str() != "schema") {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                leaves(v, &key, out);
            }
        }
        _ => {}
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BenchReport {
    pub folder: PathBuf,
    pub edit_cases: usize,
    pub cull_shoots: usize,
    pub cull_photos: usize,
    pub skipped: Vec<Value>,
}

/// Write the usage log as bench cases under `folder` (edit/ and cull/<Shoot>/, as in
/// fixtures/bench). With `images`, renders each edit case's start and final Recipe at
/// `edge` pixels and exports each cull Shoot's Photos; without, writes only the JSON.
pub fn export_bench(tools: &Tools, folder: &Path, images: bool, edge: u32) -> Result<BenchReport> {
    let lib = Library::open(tools.library())?;
    let mut events: BTreeMap<PhotoId, Vec<UsageEvent>> = BTreeMap::new();
    for e in lib.usage_events(None)? {
        events.entry(e.photo_id).or_default().push(e);
    }
    let photos: BTreeMap<PhotoId, Photo> = lib
        .list_photos(&PhotoFilter::default())?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let shoots: BTreeMap<i64, String> = lib.shoots()?.into_iter().map(|s| (s.id, s.name)).collect();
    let mut report = BenchReport {
        folder: folder.to_path_buf(),
        ..Default::default()
    };
    let histories: BTreeMap<PhotoId, History> =
        events.iter().map(|(&id, ev)| (id, history(ev))).collect();

    // Edit cases: every exported Photo still in the Library.
    let edit = folder.join("edit");
    let mut cases = Vec::new();
    for (id, h) in &histories {
        let (Some(fin), Some(photo)) = (&h.final_recipe, photos.get(id)) else {
            continue;
        };
        let stem = Path::new(&photo.file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("photo");
        let shoot = shoots.get(&photo.shoot_id).map_or("", String::as_str);
        let case_id = crate::naming::safe_file_name(&format!("{shoot}_{stem}"));
        if images {
            for (dir, recipe) in [("greg", fin), ("camera", &h.start)] {
                let recipe = if recipe.is_null() {
                    json!({})
                } else {
                    recipe.clone()
                };
                if let Err(e) = render_to(
                    tools,
                    *id,
                    &recipe,
                    edge,
                    &edit.join(dir).join(format!("{case_id}.jpg")),
                ) {
                    report
                        .skipped
                        .push(json!({ "photo_id": id, "error": e.to_string() }));
                }
            }
        }
        cases.push(json!({
            "id": case_id,
            "photo_id": id,
            "file": photo.file_name,
            "captured": photo.captured_at,
            "camera_model": photo.camera,
            "raw": lib.resolve(&photo.path),
            "develop": {},
            "greg": format!("greg/{case_id}.jpg"),
            "camera": format!("camera/{case_id}.jpg"),
            "start_recipe": h.start,
            "final_recipe": fin,
            "takes": h.takes,
            "kept": h.kept,
            "moved": h.moved,
            "moved_total": h.moved_total,
            "export": h.export,
        }));
    }
    report.edit_cases = cases.len();
    write_json(
        &edit.join("cases.json"),
        &json!({
            "note": "From Argus's usage log (docs/usage-log.md). greg/ is the Recipe the person exported, camera/ the Recipe the Photo started with.",
            "cases": cases,
        }),
    )?;

    // Cull cases: every Shoot where the person flagged or starred something.
    let mut by_shoot: BTreeMap<i64, Vec<&Photo>> = BTreeMap::new();
    for p in photos.values() {
        by_shoot.entry(p.shoot_id).or_default().push(p);
    }
    for (shoot_id, mut list) in by_shoot {
        let judged = list.iter().any(|p| {
            histories
                .get(&p.id)
                .is_some_and(|h| h.person_verdict.is_some() || h.person_stars.is_some())
        });
        if !judged {
            continue;
        }
        list.sort_by(|a, b| a.captured_at.cmp(&b.captured_at));
        let name = shoots
            .get(&shoot_id)
            .cloned()
            .unwrap_or_else(|| shoot_id.to_string());
        let dir = folder
            .join("cull")
            .join(crate::naming::safe_file_name(&name));
        let mut files: BTreeMap<PhotoId, String> = BTreeMap::new();
        if images {
            let ids: Vec<PhotoId> = list.iter().map(|p| p.id).collect();
            let out = tools.call(
                "export_photos",
                json!({
                    "photo_ids": ids, "folder": dir.join("photos"), "shoot_folder": false,
                    "format": "jpeg", "long_edge": edge, "quality": 85, "naming": "original",
                    "record": false,
                }),
            )?;
            if let Some(Part::Json(v)) = out.first() {
                for x in v["exported"].as_array().into_iter().flatten() {
                    if let (Some(id), Some(path)) = (x["photo_id"].as_i64(), x["path"].as_str()) {
                        let file = Path::new(path)
                            .file_name()
                            .and_then(|f| f.to_str())
                            .unwrap_or("");
                        files.insert(id, file.to_string());
                    }
                }
            }
        }
        let labels: Vec<Value> = list.iter().map(|p| {
            let h = histories.get(&p.id).cloned().unwrap_or_default();
            let stem = Path::new(&p.file_name).file_stem().and_then(|s| s.to_str()).unwrap_or("");
            json!({
                "image": format!("photos/{}", files.get(&p.id).cloned().unwrap_or_else(|| format!("{stem}.jpg"))),
                "photo_id": p.id,
                "flag": (p.verdict.as_str() == "pick").then_some("pick"),
                "verdict": p.verdict.as_str(),
                "verdict_by": p.verdict_by.map(|a| a.as_str()),
                "rating": p.stars,
                "ai_verdict": h.ai_verdict,
                "ai_stars": h.ai_stars,
                "aesthetics": null,
            })
        }).collect();
        let picks = labels.iter().filter(|l| l["flag"] == "pick").count();
        write_json(
            &dir.join("labels.json"),
            &json!({
                "shoot": name, "photos": labels.len(), "picks": picks,
                "note": "From Argus's usage log: the person's final Verdicts and stars, with the AI cull's first ones. Rejects are real rejects here.",
                "labels": labels,
            }),
        )?;
        report.cull_shoots += 1;
        report.cull_photos += labels.len();
    }
    Ok(report)
}

fn render_to(tools: &Tools, photo: PhotoId, recipe: &Value, edge: u32, out: &Path) -> Result<()> {
    let parts = tools.call(
        "render_photo",
        json!({ "photo_id": photo, "long_edge": edge, "recipe": recipe }),
    )?;
    let jpeg = parts.into_iter().find_map(|p| match p {
        Part::Jpeg(b) => Some(b),
        _ => None,
    });
    let jpeg = jpeg.ok_or_else(|| Error("render_photo returned no image".into()))?;
    std::fs::create_dir_all(out.parent().unwrap_or(Path::new(".")))
        .and_then(|_| std::fs::write(out, jpeg))
        .map_err(|e| Error(format!("writing {}: {e}", out.display())))
}

fn write_json(path: &Path, v: &Value) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))
        .and_then(|_| std::fs::write(path, serde_json::to_string_pretty(v).unwrap_or_default()))
        .map_err(|e| Error(format!("writing {}: {e}", path.display())))
}
