//! Specialist models (ADR 0013): every small model or measurement that looks at a Photo is
//! an [`Analyzer`], and what it finds is a [`Fact`] kept in the Library's `facts` table.
//!
//! Callers ask for Facts and never name a model, a file or a runtime:
//!
//! - [`facts`] / [`facts_for`] return Facts for Photos, computing missing or stale ones
//!   inline (in parallel across Photos);
//! - [`enqueue`] queues an `analyze` job so the Queue computes them in the background;
//! - [`REGISTRY`] lists every Analyzer, in the order they run.
//!
//! A Fact is stale when its Analyzer's version changed or the preview it was made from
//! did, and is then recomputed on the next read. Adding a specialist is one file here, one
//! [`REGISTRY`] line, one `engine/models/manifest.json` entry and one test in
//! `engine/tests/analyzers.rs`.

pub mod depth;
pub mod embedding;
pub mod eyes;
pub mod faces;
pub mod identity;
pub mod runtime;
pub mod segment;
pub mod signals;
pub mod sky;

use crate::compute::Compute;
use crate::library::{FactRow, Library, Photo};
use crate::{Error, Result};
use rayon::prelude::*;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// How much an Analyzer costs, which decides when the Queue runs it (ADR 0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Cost {
    /// Arithmetic on the preview, milliseconds.
    Cheap,
    /// A small model, up to about a second a Photo on the CPU.
    Model,
    /// A large model: runs only at Full, or when the Mac is idle.
    Heavy,
}

impl Cost {
    /// Whether the Queue runs this cost at `compute`.
    pub fn runs_at(self, compute: Compute) -> bool {
        self != Cost::Heavy || compute == Compute::Full
    }
}

/// What an Analyzer found: small JSON, plus a blob for planes or vectors.
#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    pub value: Value,
    pub blob: Option<Vec<u8>>,
}

impl Fact {
    pub fn json(value: Value) -> Self {
        Self { value, blob: None }
    }
}

/// Every Fact known about one Photo, by Analyzer id.
pub type Facts = BTreeMap<&'static str, Fact>;

/// What an Analyzer gets: the Photo, its upright preview, and the Facts it `needs`.
pub struct Input<'a> {
    pub photo: &'a Photo,
    pub preview: &'a image::RgbImage,
    pub facts: &'a Facts,
    small: OnceLock<crate::RgbImage>,
}

/// Long edge of [`Input::small`], the size the mask models were tuned on.
pub const SMALL_EDGE: usize = 768;

impl<'a> Input<'a> {
    pub fn new(photo: &'a Photo, preview: &'a image::RgbImage, facts: &'a Facts) -> Self {
        Self {
            photo,
            preview,
            facts,
            small: OnceLock::new(),
        }
    }

    /// The preview at [`SMALL_EDGE`] (area-averaged), made once and shared.
    pub fn small(&self) -> &crate::RgbImage {
        self.small.get_or_init(|| {
            let full = crate::RgbImage {
                width: self.preview.width() as usize,
                height: self.preview.height() as usize,
                data: self.preview.as_raw().clone(),
            };
            crate::masks::shrink(&full, SMALL_EDGE)
        })
    }

    /// A Fact this Analyzer declared in `needs`.
    pub fn fact(&self, id: &str) -> Result<&Fact> {
        self.facts
            .get(id)
            .ok_or_else(|| Error(format!("Analyzer input is missing the {id} Fact")))
    }
}

/// One specialist. Implement it in its own file and add it to [`REGISTRY`].
pub trait Analyzer: Sync {
    /// Stable name, also the Fact's key: "faces", "eyes".
    fn id(&self) -> &'static str;
    /// Bump whenever the output would change, so stored Facts are recomputed.
    fn version(&self) -> u32;
    fn cost(&self) -> Cost;
    /// Facts it reads, from Analyzers earlier in [`REGISTRY`] (one level deep).
    fn needs(&self) -> &'static [&'static str] {
        &[]
    }
    /// Whether its model is installed; a large one lives in the models folder and can be
    /// missing ([`crate::meaning`]).
    fn available(&self) -> bool {
        true
    }
    /// Only computed when something asks for it by name, never by the Queue or
    /// `analyze_photo`: a Fact that is big and that few Photos need (the depth map).
    fn on_demand(&self) -> bool {
        false
    }
    fn run(&self, input: &Input) -> Result<Fact>;
    /// A few words for the LLM's Contact sheet caption, if this Fact has something to say.
    fn caption(&self, _fact: &Fact) -> Option<String> {
        None
    }
}

/// Every Analyzer, in the order they run (an Analyzer comes after the ones it needs).
pub static REGISTRY: &[&dyn Analyzer] = &[
    &signals::Signals,
    &faces::Faces,
    &segment::Segment,
    &sky::Sky,
    &eyes::Eyes,
    &identity::Identity,
    &embedding::Embedding,
    &depth::DepthMap,
    &crate::meaning::Meaning,
];

/// The current version of an Analyzer's Facts.
pub fn version_of(id: &str) -> Result<u32> {
    Ok(analyzer(id)?.version())
}

fn analyzer(id: &str) -> Result<&'static dyn Analyzer> {
    REGISTRY
        .iter()
        .copied()
        .find(|a| a.id() == id)
        .ok_or_else(|| Error(format!("no Analyzer called {id:?}")))
}

/// `ids` plus what they need, in registry order.
fn with_needs(ids: &[&str]) -> Result<Vec<&'static dyn Analyzer>> {
    let mut want: Vec<&str> = Vec::new();
    for id in ids {
        let a = analyzer(id)?;
        want.extend(a.needs());
        want.push(a.id());
    }
    Ok(REGISTRY
        .iter()
        .copied()
        .filter(|a| want.contains(&a.id()))
        .collect())
}

/// The Facts `ids` for one Photo, computing what's missing or stale.
pub fn facts(lib: &Library, photo: &Photo, ids: &[&str]) -> Result<Facts> {
    Ok(facts_for(lib, std::slice::from_ref(photo), ids)?.remove(0))
}

/// The Facts `ids` for each Photo, in order. Missing or stale ones are computed in
/// parallel on the current rayon pool and stored.
pub fn facts_for(lib: &Library, photos: &[Photo], ids: &[&str]) -> Result<Vec<Facts>> {
    facts_with(lib, photos, ids, None, &|_, _| {})
}

/// [`facts_for`] on `compute`'s share of the CPU (the current pool if None), calling
/// `progress(done, total)` as each Photo's Facts are stored.
pub fn facts_with(
    lib: &Library,
    photos: &[Photo],
    ids: &[&str],
    compute: Option<Compute>,
    progress: &dyn Fn(usize, usize),
) -> Result<Vec<Facts>> {
    let pool = compute.map(|c| c.cpu_pool()).transpose()?;
    let analyzers = with_needs(ids)?;
    // What's stored and still fresh, and the preview hash each Photo is checked against.
    let mut known: Vec<(Facts, String)> = Vec::with_capacity(photos.len());
    for p in photos {
        let hash = preview_hash(&p.preview)?;
        let mut have = Facts::new();
        for a in &analyzers {
            if let Some(row) = lib.fact(p.id, a.id())? {
                if row.version == a.version() && row.input_hash == hash {
                    have.insert(
                        a.id(),
                        Fact {
                            value: row.value,
                            blob: row.blob,
                        },
                    );
                }
            }
        }
        known.push((have, hash));
    }
    let todo: Vec<usize> = (0..photos.len())
        .filter(|&i| analyzers.iter().any(|a| !known[i].0.contains_key(a.id())))
        .collect();
    let total = todo.len();
    let mut out: Vec<Facts> = Vec::with_capacity(photos.len());
    let mut hashes = Vec::with_capacity(photos.len());
    for (f, h) in known {
        out.push(f);
        hashes.push(h);
    }
    // Compute a pool's worth at a time in parallel, then store them (the Library isn't
    // shared across threads), so a quit loses at most one chunk.
    let threads = pool
        .as_ref()
        .map_or_else(rayon::current_num_threads, |p| p.current_num_threads());
    let mut done = 0;
    for part in todo.chunks((threads * 2).max(1)) {
        let run = || -> Vec<(usize, Result<Made>)> {
            part.par_iter()
                .map(|&i| (i, run_missing(&photos[i], out[i].clone(), &analyzers)))
                .collect()
        };
        let made = match &pool {
            Some(p) => p.install(run),
            None => run(),
        };
        for (i, r) in made {
            let (all, new) = r?;
            for id in new {
                let a = analyzer(id)?;
                let f = &all[id];
                lib.put_fact(
                    photos[i].id,
                    id,
                    &FactRow {
                        version: a.version(),
                        input_hash: hashes[i].clone(),
                        value: f.value.clone(),
                        blob: f.blob.clone(),
                    },
                )?;
            }
            out[i] = all;
            done += 1;
            progress(done, total);
        }
    }
    let wanted: Vec<&str> = ids.to_vec();
    for f in &mut out {
        f.retain(|k, _| wanted.contains(k));
    }
    Ok(out)
}

/// All of a Photo's Facts, and the ids just made.
type Made = (Facts, Vec<&'static str>);

/// Run the Analyzers a Photo is missing.
fn run_missing(
    photo: &Photo,
    mut have: Facts,
    analyzers: &[&'static dyn Analyzer],
) -> Result<Made> {
    let preview = image::open(&photo.preview)
        .map_err(|e| Error(format!("preview of Photo {}: {e}", photo.id)))?
        .to_rgb8();
    let mut made = Vec::new();
    for a in analyzers {
        if have.contains_key(a.id()) {
            continue;
        }
        let fact = {
            let input = Input::new(photo, &preview, &have);
            a.run(&input)
                .map_err(|e| Error(format!("{} on Photo {}: {}", a.id(), photo.id, e.0)))?
        };
        // As stored: JSON numbers made from f32 read back as the same f64 after this.
        let fact = Fact {
            value: serde_json::from_str(&fact.value.to_string()).unwrap_or(fact.value),
            ..fact
        };
        have.insert(a.id(), fact);
        made.push(a.id());
    }
    Ok((have, made))
}

/// The caption every Analyzer adds for these Facts, joined ("2 faces, eyes open").
pub fn caption(facts: &Facts) -> String {
    REGISTRY
        .iter()
        .filter_map(|a| facts.get(a.id()).and_then(|f| a.caption(f)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every Analyzer the Queue runs at `compute`.
pub fn background_ids(compute: Compute) -> Vec<&'static str> {
    REGISTRY
        .iter()
        .filter(|a| a.cost().runs_at(compute) && a.available() && !a.on_demand())
        .map(|a| a.id())
        .collect()
}

/// Queue an `analyze` job for a Shoot's Photos (or `photo_ids`), so their Facts are ready
/// before the cull or the person asks.
pub fn enqueue(
    lib: &mut Library,
    shoot: Option<crate::library::ShootId>,
    photo_ids: Option<&[crate::library::PhotoId]>,
) -> Result<crate::library::JobId> {
    let mut args = serde_json::Map::new();
    if let Some(s) = shoot {
        args.insert("shoot_id".into(), s.into());
    }
    if let Some(p) = photo_ids {
        args.insert("photo_ids".into(), p.into());
    }
    Ok(crate::jobs::enqueue(lib, "analyze", &Value::Object(args), None, None)?.0)
}

/// blake3 of a preview's bytes, remembered while the file's size and mtime hold.
fn preview_hash(path: &std::path::Path) -> Result<String> {
    type Seen = HashMap<PathBuf, (u64, std::time::SystemTime, String)>;
    static SEEN: OnceLock<Mutex<Seen>> = OnceLock::new();
    let meta =
        std::fs::metadata(path).map_err(|e| Error(format!("preview {}: {e}", path.display())))?;
    let stamp = (meta.len(), meta.modified().unwrap_or(std::time::UNIX_EPOCH));
    let seen = SEEN.get_or_init(Default::default);
    if let Some((len, at, h)) = seen.lock().unwrap().get(path) {
        if (*len, *at) == stamp {
            return Ok(h.clone());
        }
    }
    let bytes =
        std::fs::read(path).map_err(|e| Error(format!("preview {}: {e}", path.display())))?;
    let h = blake3::hash(&bytes).to_hex()[..32].to_string();
    seen.lock()
        .unwrap()
        .insert(path.to_path_buf(), (stamp.0, stamp.1, h.clone()));
    Ok(h)
}

/// Quantise 0..1 floats to bytes, for planes stored as blobs.
pub fn to_bytes(plane: &[f32]) -> Vec<u8> {
    plane
        .iter()
        .map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
        .collect()
}

/// The inverse of [`to_bytes`].
pub fn from_bytes(b: &[u8]) -> Vec<f32> {
    b.iter().map(|&v| v as f32 / 255.0).collect()
}
