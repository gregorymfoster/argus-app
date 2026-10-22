//! `argus speed`: how fast Argus is on real camera files, per Photo, and what that means for
//! a card of a few hundred 61 MP RAWs. Compares against `bench/speed-baseline.json` (one
//! entry per machine) so a slowdown shows up; `--save` records the current numbers.

use argus_engine::compute::Compute;
use argus_engine::library::{ImportOptions, Library, PhotoFilter};
use argus_engine::tools::Tools;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::ms;

type BoxError = Box<dyn std::error::Error>;

/// Greg's test card from his A7R V (2026-10-18): 266 RAWs.
const CARD_PHOTOS: f64 = 266.0;
/// A metric this much slower than its baseline is a regression.
const TOLERANCE: f64 = 1.3;

fn round(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// The machine a baseline belongs to: timings only compare on the same kind of box.
fn machine() -> String {
    let cores = std::thread::available_parallelism().map_or(0, |n| n.get());
    format!(
        "{}-{}-{cores}cores",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

pub fn run(folders: &[PathBuf], baseline: &Path, save: bool) -> Result<bool, BoxError> {
    let mut raws = Vec::new();
    for f in folders {
        for e in std::fs::read_dir(f)?.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("arw")) {
                raws.push(p.canonicalize()?);
            }
        }
    }
    raws.sort();
    if raws.is_empty() {
        return Err(
            "no .ARW files in the given folders (see CLAUDE.md for fetching samples)".into(),
        );
    }
    let n = raws.len() as f64;
    let mut m = Map::new();

    // Decode alone, per camera.
    let mut decode = Map::new();
    for p in &raws {
        let t = Instant::now();
        let raw = argus_engine::decode(p)?;
        let e = decode.entry(raw.model.clone()).or_insert(json!([]));
        e.as_array_mut().unwrap().push(json!(ms(t)));
    }
    for (model, v) in &decode {
        let v: Vec<f64> = v
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_f64)
            .collect();
        m.insert(
            format!("decode_ms[{model}]"),
            json!(round(v.iter().sum::<f64>() / v.len() as f64)),
        );
    }

    // Import (hash, metadata and the camera's preview, as the grid first sees them), then
    // Argus's own previews at Full and at Gentle (what runs while Greg works). Each level
    // gets a fresh Library so both start from camera previews.
    let card = tempfile::tempdir()?;
    for p in &raws {
        std::os::unix::fs::symlink(p, card.path().join(p.file_name().unwrap()))?;
    }
    let mut lib_dir = None;
    for (name, compute) in [("gentle", Compute::Gentle), ("full", Compute::Full)] {
        let dir = tempfile::tempdir()?;
        let mut lib = Library::open(dir.path())?;
        let t = Instant::now();
        let report = lib.import(card.path(), &ImportOptions::default())?;
        let shoot = report.shoot_id.ok_or("nothing imported")?;
        if name == "full" {
            m.insert("import_ms_per_photo".into(), json!(round(ms(t) / n)));
        }
        let t = Instant::now();
        let r = lib.render_previews(Some(shoot), compute, &|_, _| {})?;
        if r.rendered != raws.len() {
            return Err(format!("rendered {} of {} previews", r.rendered, raws.len()).into());
        }
        m.insert(
            format!("previews_ms_per_photo[{name}]"),
            json!(round(ms(t) / n)),
        );
        lib_dir = Some(dir);
    }
    let lib_dir = lib_dir.unwrap();

    // Edit: opening a Photo renders it from the RAW; each slider change after re-renders.
    let tools = Tools::new(lib_dir.path());
    let photos = Library::open(lib_dir.path())?.list_photos(&PhotoFilter::default())?;
    let (mut open, mut slide) = (Vec::new(), Vec::new());
    for p in &photos {
        let call = |exposure: f64| -> Result<f64, BoxError> {
            let t = Instant::now();
            tools.call(
                "render_photo",
                json!({ "photo_id": p.id, "long_edge": 1536,
                        "recipe": { "global": { "exposure": exposure } } }),
            )?;
            Ok(ms(t))
        };
        open.push(call(0.0)?);
        for e in [0.2, 0.4, 0.6] {
            slide.push(call(e)?);
        }
    }
    let mean = |v: &[f64]| round(v.iter().sum::<f64>() / v.len() as f64);
    m.insert("edit_open_ms".into(), json!(mean(&open)));
    m.insert("edit_slider_ms".into(), json!(mean(&slide)));

    // Grid: what each tile's preview costs to load.
    let bytes: u64 = photos
        .iter()
        .filter_map(|p| std::fs::metadata(&p.preview).ok())
        .map(|md| md.len())
        .sum();
    m.insert(
        "preview_kb".into(),
        json!(round(bytes as f64 / 1024.0 / photos.len() as f64)),
    );

    let get = |k: &str| m[k].as_f64().unwrap_or(0.0);
    let card = json!({
        "photos": CARD_PHOTOS,
        "import_s": round(get("import_ms_per_photo") * CARD_PHOTOS / 1000.0),
        "previews_full_s": round(get("previews_ms_per_photo[full]") * CARD_PHOTOS / 1000.0),
        "previews_gentle_s": round(get("previews_ms_per_photo[gentle]") * CARD_PHOTOS / 1000.0),
    });

    let machine = machine();
    let mut all: Map<String, Value> = std::fs::read_to_string(baseline)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mut regressions = Vec::new();
    if let Some(Value::Object(base)) = all.get(&machine) {
        for (k, v) in &m {
            // Only timings regress; sizes are informational.
            let (Some(now), Some(was)) = (v.as_f64(), base.get(k).and_then(Value::as_f64)) else {
                continue;
            };
            let timing = k.ends_with("_ms") || k.contains("_ms[") || k.contains("_ms_");
            if timing && now > was * TOLERANCE && now - was > 5.0 {
                regressions.push(json!({ "metric": k, "baseline": was, "now": now }));
            }
        }
    }
    let out = json!({
        "machine": machine,
        "samples": raws.len(),
        "metrics": m,
        "card_estimate": card,
        "baseline": all.contains_key(&machine),
        "regressions": regressions,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    if save {
        all.insert(machine, Value::Object(m));
        std::fs::write(baseline, serde_json::to_string_pretty(&all)? + "\n")?;
    }
    Ok(regressions.is_empty())
}
