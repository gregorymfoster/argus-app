//! The tool layer as data: every tool the Agent and the MCP server call, each with a JSON
//! Schema, behind one entry point ([`Tools::call`]). Transports (MCP over stdio, the in-app
//! Agent's loop, later the UI) stay thin: they list [`Tools::specs`] and forward calls.
//!
//! Each call opens the Library afresh, so long calls (a cull) never block short ones and
//! changes land in the same `library.db` the app and `argus serve` read (WAL mode).
//!
//! Each area of tools has its own file (library, keywords, people, culling, edit, presets,
//! looks, suggestions, queue, usage, exports, masks) with its tools' specs, their match arms
//! and the helpers only they use, so a new tool goes into one area file. `AREAS` lists the
//! areas in spec order; helpers several areas share stay here.

mod culling;
mod edit;
mod exports;
mod keywords;
mod library;
mod looks;
mod masks;
mod people;
mod presets;
mod queue;
mod suggestions;
mod usage;

use crate::agent::Chat;
use crate::compute::Compute;
use crate::cull::{self, Provider};
use crate::jobs;
use crate::library::{
    is_on_card, Author, BatchId, ImportOptions, Library, Look, Photo, PhotoFilter, PhotoId,
    ShootId, Verdict,
};
use crate::masks::{Canvas, Mask, Shape};
use crate::providers;
use crate::recipe::{Local, Recipe};
use crate::{export, heal, render, Error, Result};
use rayon::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Largest preview a caller can ask for. Cloud models never need more (spec, Agent).
pub const MAX_PREVIEW_EDGE: u32 = 1536;
/// One cell of a Contact sheet page, in pixels.
pub const SHEET_CELL: u32 = 256;
const SHEET_COLUMNS: u32 = 6;
const SHEET_PER_PAGE: usize = 30;

/// A tool as a caller sees it.
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON Schema (an object schema) for the arguments.
    pub input_schema: Value,
    /// True when the tool only reads the Library.
    pub read_only: bool,
}

/// One piece of a tool's result, in order.
#[derive(Debug)]
pub enum Part {
    Json(Value),
    Jpeg(Vec<u8>),
}

type ProviderFactory =
    dyn Fn(Option<&str>, Option<Compute>) -> Result<Box<dyn Provider>> + Send + Sync;

type ChatFactory =
    dyn Fn(Option<&str>, Option<Compute>, Value) -> Result<Box<dyn Chat>> + Send + Sync;

pub struct Tools {
    library: PathBuf,
    providers: Box<ProviderFactory>,
    chats: Box<ChatFactory>,
    /// The harness variant file Suggestions run with (crate::variant), from ARGUS_VARIANT.
    variant: Option<PathBuf>,
}

/// Library setting naming the Ollama model chosen in Settings.
pub const OLLAMA_MODEL_KEY: &str = "ollama_model";

impl Tools {
    /// Tools over the Library folder at `library`, culling with [`providers::provider`].
    pub fn new(library: impl Into<PathBuf>) -> Self {
        let library = library.into();
        // The Ollama model this Library was set to use (Settings › AI and Compute).
        if library.join("library.db").exists() {
            if let Ok(lib) = Library::open(&library) {
                let m = lib.setting(OLLAMA_MODEL_KEY).ok().flatten();
                providers::choose_ollama_model(m.and_then(|v| v.as_str().map(str::to_owned)));
            }
        }
        Self {
            library,
            providers: Box::new(providers::provider),
            chats: Box::new(crate::agent::chat_with_format),
            variant: std::env::var_os("ARGUS_VARIANT")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
        }
    }

    /// Swap how background jobs (Suggestions) pick their model (tests use a script).
    pub fn with_chats(
        mut self,
        f: impl Fn(Option<&str>, Option<Compute>, Value) -> Result<Box<dyn Chat>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        self.chats = Box::new(f);
        self
    }

    /// Run Suggestions with this harness variant file instead of ARGUS_VARIANT's.
    pub fn with_variant(mut self, path: impl Into<PathBuf>) -> Self {
        self.variant = Some(path.into());
        self
    }

    /// The harness variant (crate::variant), or the built-in one when none is set.
    pub fn variant(&self) -> Result<crate::variant::Variant> {
        match &self.variant {
            Some(p) => crate::variant::load(&p.to_string_lossy()),
            None => Ok(Default::default()),
        }
    }

    /// A model by Provider name whose replies follow the JSON Schema `format`.
    pub fn chat(
        &self,
        provider: Option<&str>,
        compute: Option<Compute>,
        format: Value,
    ) -> Result<Box<dyn Chat>> {
        (self.chats)(provider, compute, format)
    }

    /// Run `cull_shoot`'s Provider, as `cull_shoot` would pick it.
    pub fn provider(
        &self,
        name: Option<&str>,
        compute: Option<Compute>,
    ) -> Result<Box<dyn Provider>> {
        (self.providers)(name, compute)
    }

    /// Swap how `cull_shoot` picks its Provider (tests use a fake).
    pub fn with_providers(
        mut self,
        f: impl Fn(Option<&str>, Option<Compute>) -> Result<Box<dyn Provider>> + Send + Sync + 'static,
    ) -> Self {
        self.providers = Box::new(f);
        self
    }

    pub fn library(&self) -> &Path {
        &self.library
    }

    /// What Auto would make of `own` (a Recipe for `photo_id`), without saving anything:
    /// the deterministic anchor the Agent and Suggestions start from.
    pub fn auto_recipe(&self, photo_id: PhotoId, own: &Recipe) -> Result<Recipe> {
        let lib = Library::open(&self.library)?;
        let p = find_photo(&lib, photo_id)?;
        let base = with_look(&lib, &p, &crate::auto::cleared(own))?;
        let masks = render::masks_for(&lib, &p, &base);
        let head = crate::auto::head_input(&lib, &p);
        let auto = crate::auto::auto_adjust_for(
            |s| {
                let r = Recipe {
                    global: base.global.plus(s),
                    ..base.clone()
                };
                render::render_photo(&p, &r, Some(crate::auto::MEASURE_EDGE), masks.clone())
            },
            p.kind == "raw",
            head.as_ref(),
        )?;
        Ok(crate::auto::applied(own, &auto))
    }

    pub fn specs() -> Vec<ToolSpec> {
        specs()
    }

    /// Run one tool. Bad arguments and engine failures come back as `Err` with a message
    /// meant for the caller (a model), so it can correct itself.
    /// `render_photo` for the app's zoomed Loupe and Edit: the same render, up to the
    /// original's full size (the tool caps it so MCP replies stay small).
    pub fn render_detail(&self, args: Value) -> Result<Vec<Part>> {
        render_parts(&Library::open(&self.library)?, parse(args)?, u32::MAX)
    }

    pub fn call(&self, name: &str, args: Value) -> Result<Vec<Part>> {
        self.call_in(name, args, None)
    }

    /// [`Tools::call`], with every change recorded in `batch` instead of a batch of its
    /// own, so a whole Agent turn undoes in one step.
    pub fn call_in(&self, name: &str, args: Value, batch: Option<BatchId>) -> Result<Vec<Part>> {
        let args = if args.is_null() { json!({}) } else { args };
        let lib = Library::open(&self.library)?;
        match route(name) {
            Some(call) => call(self, name, args, lib, batch),
            None => Err(unknown_tool(name)),
        }
    }
}

/// An area's tool specs, in order.
type Specs = fn() -> Vec<ToolSpec>;
/// The `impl Tools` method that runs an area's tools.
type Call = fn(&Tools, &str, Value, Library, Option<BatchId>) -> Result<Vec<Part>>;

/// The tools by area, one file each. specs() lists them in this order, which is the order
/// MCP clients and the Agent see.
const AREAS: &[(Specs, Call)] = &[
    (library::specs, Tools::call_library),
    (keywords::specs, Tools::call_keywords),
    (people::specs, Tools::call_people),
    (culling::specs, Tools::call_culling),
    (edit::specs, Tools::call_edit),
    (presets::specs, Tools::call_presets),
    (looks::specs, Tools::call_looks),
    (suggestions::specs, Tools::call_suggestions),
    (queue::specs, Tools::call_queue),
    (usage::specs, Tools::call_usage),
    (exports::specs, Tools::call_exports),
    (masks::specs, Tools::call_masks),
];

/// The area method that answers `name`, from the areas' specs.
fn route(name: &str) -> Option<Call> {
    static ROUTES: std::sync::OnceLock<std::collections::HashMap<&'static str, Call>> =
        std::sync::OnceLock::new();
    ROUTES
        .get_or_init(|| {
            AREAS
                .iter()
                .flat_map(|&(specs, call)| specs().into_iter().map(move |s| (s.name, call)))
                .collect()
        })
        .get(name)
        .copied()
}

fn unknown_tool(other: &str) -> Error {
    Error(format!(
        "unknown tool {other:?}; known tools: {}",
        specs()
            .iter()
            .map(|s| s.name)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn specs() -> Vec<ToolSpec> {
    AREAS.iter().flat_map(|(specs, _)| specs()).collect()
}

/// Start a batch for a tool's changes unless the caller already has one open.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AutoEdit {
    photo_ids: Option<Vec<PhotoId>>,
    shoot_id: Option<ShootId>,
    compute: Option<Compute>,
}

/// Lightroom's Auto ([`crate::auto`]) on many Photos: measured in parallel at the Compute
/// level's share of the CPU, then saved as one batch.
fn auto_edit(lib: &mut Library, a: AutoEdit, batch: Option<BatchId>) -> Result<Vec<Part>> {
    let photos: Vec<Photo> = match (&a.photo_ids, a.shoot_id) {
        (Some(ids), _) => ids
            .iter()
            .map(|&id| find_photo(lib, id))
            .collect::<Result<_>>()?,
        (None, Some(shoot)) => lib.list_photos(&PhotoFilter {
            shoot: Some(shoot),
            ..Default::default()
        })?,
        (None, None) => return Err(Error("pass photo_ids or shoot_id".into())),
    };
    if photos.is_empty() {
        return Err(Error("no Photos to Auto".into()));
    }
    // What each Photo looks like apart from Auto's sliders: its Recipe over its Look.
    let jobs: Vec<(Photo, Recipe, Recipe, _, _)> = photos
        .into_iter()
        .map(|p| {
            let own = current_recipe(lib, p.id)?;
            let base = with_look(lib, &p, &crate::auto::cleared(&own))?;
            let masks = render::masks_for(lib, &p, &base);
            let head = crate::auto::head_input(lib, &p);
            Ok((p, own, base, masks, head))
        })
        .collect::<Result<_>>()?;
    let task =
        crate::progress::Task::start("auto", format!("Auto on {} Photos", jobs.len()), jobs.len());
    let pool = a.compute.unwrap_or_else(Compute::from_env).cpu_pool()?;
    // Cancel (`cancel_task`) stops between Photos; the ones done are saved as usual.
    let stop = task.stop_flag();
    let results: Vec<(PhotoId, Result<Recipe>)> = pool.install(|| {
        jobs.par_iter()
            .map(|(p, own, base, masks, head)| {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    return (p.id, Err(Error(crate::progress::STOPPED.into())));
                }
                let auto = crate::auto::auto_adjust_for(
                    |s| {
                        // The Look's sliders sit under Auto's, as they do under the person's.
                        let r = Recipe {
                            global: base.global.plus(s),
                            ..base.clone()
                        };
                        render::render_photo(p, &r, Some(crate::auto::MEASURE_EDGE), masks.clone())
                    },
                    p.kind == "raw",
                    head.as_ref(),
                );
                task.tick();
                (p.id, auto.map(|s| crate::auto::applied(own, &s)))
            })
            .collect()
    });
    let batch = batch_or(lib, batch, "Auto")?;
    let mut out = BTreeMap::new();
    let mut failed = BTreeMap::new();
    let mut stopped = Vec::new();
    for (id, r) in results {
        match r {
            Ok(r) => {
                let version = lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
                out.insert(id, json!({ "version": version, "global": r.global }));
            }
            Err(e) if e.0 == crate::progress::STOPPED => stopped.push(id),
            Err(e) => {
                failed.insert(id, e.to_string());
            }
        }
    }
    one(json!({ "batch_id": batch, "photos": out, "failed": failed, "stopped": stopped }))
}

fn batch_or(lib: &mut Library, batch: Option<BatchId>, label: &str) -> Result<BatchId> {
    match batch {
        Some(b) => Ok(b),
        None => lib.begin_batch(Author::Ai, label),
    }
}

/// What the renderer sees: the Recipe with its Look's sliders underneath its own. A Look
/// this Shoot doesn't have (one from the prototype's demo set) is ignored.
pub(crate) fn with_look(lib: &Library, photo: &Photo, recipe: &Recipe) -> Result<Recipe> {
    let mut r = recipe.clone();
    if let Some(id) = &recipe.look {
        if let Some(look) = lib.look(photo.shoot_id, id)? {
            r.global = look.adjust.plus(&recipe.global);
        }
    }
    Ok(r)
}

/// The Photo's current Recipe, or an empty one if it was never edited.
pub(crate) fn current_recipe(lib: &Library, id: PhotoId) -> Result<Recipe> {
    match lib.read_recipe(id)? {
        Some(v) => Recipe::from_value(&v.recipe),
        None => Ok(Recipe::default()),
    }
}

/// The Photos an Album's saved query finds now, best first.
pub(crate) fn album_photos(lib: &Library, query: &Value) -> Result<Vec<PhotoId>> {
    let text = query["text"].as_str().unwrap_or("");
    Ok(crate::meaning::search(lib, text, None)?
        .into_iter()
        .map(|h| h.photo_id)
        .collect())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShootArg {
    shoot_id: ShootId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PhotoArg {
    photo_id: PhotoId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenderPhoto {
    photo_id: PhotoId,
    long_edge: Option<u32>,
    recipe: Option<Value>,
    /// Try a Preset on top (not saved), at `amount` percent (default 100).
    preset: Option<String>,
    amount: Option<f64>,
}

/// A Photo as tools show it: no Library-internal preview path.
#[derive(Serialize)]
struct PhotoView<'a> {
    id: PhotoId,
    shoot_id: ShootId,
    file_name: &'a str,
    path: &'a str,
    captured_at: &'a str,
    width: u32,
    height: u32,
    camera: &'a str,
    verdict: Verdict,
    verdict_by: Option<Author>,
    reason: Option<&'a str>,
    stack: Option<i64>,
    stack_rank: Option<i64>,
    stars: u8,
    stars_by: Option<Author>,
    best: Option<i64>,
}

impl<'a> From<&'a Photo> for PhotoView<'a> {
    fn from(p: &'a Photo) -> Self {
        Self {
            id: p.id,
            shoot_id: p.shoot_id,
            file_name: &p.file_name,
            path: &p.path,
            captured_at: &p.captured_at,
            width: p.width,
            height: p.height,
            camera: &p.camera,
            verdict: p.verdict,
            verdict_by: p.verdict_by,
            reason: p.reason.as_deref(),
            stack: p.stack,
            stack_rank: p.stack_rank,
            stars: p.stars,
            stars_by: p.stars_by,
            best: p.best,
        }
    }
}

fn parse<T: DeserializeOwned>(args: Value) -> Result<T> {
    serde_json::from_value(args).map_err(|e| Error(format!("bad arguments: {e}")))
}

fn to_json(v: impl Serialize) -> Result<Value> {
    serde_json::to_value(v).map_err(|e| Error(format!("json: {e}")))
}

fn one(v: impl Serialize) -> Result<Vec<Part>> {
    Ok(vec![Part::Json(to_json(v)?)])
}

/// `render_photo` at most `max_edge` on the long edge.
fn render_parts(lib: &Library, a: RenderPhoto, max_edge: u32) -> Result<Vec<Part>> {
    let photo = find_photo(lib, a.photo_id)?;
    let mut recipe = match &a.recipe {
        Some(v) => Recipe::from_value(v)?,
        None => current_recipe(lib, a.photo_id)?,
    };
    if let Some(id) = &a.preset {
        let amount = a.amount.unwrap_or(100.0) / 100.0;
        let yours = crate::presets::yours(lib)?;
        let p = crate::presets::find_in(lib, id)?;
        recipe = crate::presets::apply_among(&recipe, &p, amount, &yours)?;
    }
    let edge = a.long_edge.unwrap_or(1024).clamp(64, max_edge);
    let img = render::render_in(
        lib,
        &photo,
        &with_look(lib, &photo, &recipe)?,
        Some(edge as usize),
    )?;
    let light = crate::signals::light(&img);
    Ok(vec![
        Part::Json(json!({ "photo_id": photo.id, "width": img.width,
            "height": img.height, "recipe": recipe.to_value(),
            "light": light.line(), "light_stats": light })),
        Part::Jpeg(render::encode_jpeg(&img, 85)?),
    ])
}

/// Where edited previews live, under the previews folder so the app can serve them. They
/// are a cache: no Photo row points at them, so `clear_cache` removes them as orphans.
const EDITED_DIR: &str = "previews/edited";

/// Each Photo rendered with its current Recipe (and Look) at `edge` px, as a JPEG path
/// relative to the Library, or None for a Photo that couldn't render (original offline).
/// The file name carries a hash of what it shows, so a render is reused until the Recipe,
/// the Look or Argus itself changes, across launches; the grid then fills at file speed
/// instead of a RAW decode (~0.25 s) per tile. Misses render `in_flight` at a time.
pub fn edited_previews(
    lib: &Library,
    ids: &[PhotoId],
    edge: u32,
) -> Result<BTreeMap<PhotoId, Option<String>>> {
    let edge = edge.clamp(64, MAX_PREVIEW_EDGE);
    let photos: BTreeMap<PhotoId, Photo> = lib
        .list_photos(&PhotoFilter::default())?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let mut out = BTreeMap::new();
    let mut todo = Vec::new();
    for id in ids {
        let photo = photos
            .get(id)
            .ok_or_else(|| Error(format!("no Photo with id {id}")))?;
        let recipe = with_look(lib, photo, &current_recipe(lib, *id)?)?;
        let mut h = blake3::Hasher::new();
        h.update(recipe.to_value().to_string().as_bytes());
        h.update(env!("CARGO_PKG_VERSION").as_bytes());
        // A Denoise Amount shows once its base exists, so the base is part of what it shows.
        if crate::denoise::amount(&recipe) > 0.0 && crate::denoise::ready(photo).is_some() {
            h.update(b"denoised");
        }
        let key = &h.finalize().to_hex()[..16];
        let rel = format!("{EDITED_DIR}/{:02x}/{id}-{edge}-{key}.jpg", id % 256);
        if lib.root().join(&rel).is_file() {
            out.insert(*id, Some(rel));
        } else {
            let masks = render::masks_for(lib, photo, &recipe);
            todo.push((photo, recipe, masks, rel));
        }
    }
    let root = lib.root();
    let next = std::sync::atomic::AtomicUsize::new(0);
    // Two RAWs in flight, as previews do: each holds a 61 MP mosaic while it decodes.
    let in_flight = 2.min(todo.len());
    let done: Vec<(PhotoId, Option<String>)> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..in_flight)
            .map(|_| {
                s.spawn(|| {
                    let mut done = Vec::new();
                    while let Some((photo, recipe, masks, rel)) =
                        todo.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                    {
                        let path = root.join(rel);
                        let written =
                            render::render_photo(photo, recipe, Some(edge as usize), masks.clone())
                                .and_then(|img| render::encode_jpeg(&img, 85))
                                .and_then(|jpeg| write_edited(&path, &jpeg));
                        if let Err(e) = &written {
                            eprintln!("edited preview of Photo {}: {e}", photo.id);
                        }
                        done.push((photo.id, written.ok().map(|_| rel.clone())));
                    }
                    done
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("edited preview worker"))
            .collect()
    });
    out.extend(done);
    Ok(out)
}

/// Write an edited preview (via a temporary file, so a reader never sees half of one) and
/// drop the Photo's older ones at the same size.
fn write_edited(path: &Path, jpeg: &[u8]) -> Result<()> {
    let dir = path.parent().expect("edited preview has a folder");
    std::fs::create_dir_all(dir).map_err(|e| Error(format!("{}: {e}", dir.display())))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, jpeg)
        .and_then(|_| std::fs::rename(&tmp, path))
        .map_err(|e| Error(format!("{}: {e}", path.display())))?;
    let name = path.file_name().unwrap().to_string_lossy();
    let prefix = &name[..name.rfind('-').expect("id-edge-key.jpg") + 1];
    for old in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let n = old.file_name().to_string_lossy().into_owned();
        if n.starts_with(prefix) && n != name && n.ends_with(".jpg") {
            let _ = std::fs::remove_file(old.path());
        }
    }
    Ok(())
}

pub(crate) fn find_photo(lib: &Library, id: PhotoId) -> Result<Photo> {
    lib.list_photos(&PhotoFilter::default())?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| Error(format!("no Photo with id {id}")))
}

fn newest_shoot(lib: &Library) -> Result<ShootId> {
    lib.shoots()?
        .last()
        .map(|s| s.id)
        .ok_or_else(|| Error("the Library has no Shoots yet; import a folder first".into()))
}

fn shoot_photos(lib: &Library, shoot: ShootId) -> Result<Vec<Photo>> {
    if !lib.shoots()?.iter().any(|s| s.id == shoot) {
        return Err(Error(format!("no Shoot with id {shoot}")));
    }
    lib.list_photos(&PhotoFilter {
        shoot: Some(shoot),
        verdict: None,
    })
}

pub(crate) fn open_preview(p: &Photo) -> Result<image::DynamicImage> {
    image::open(&p.preview).map_err(|e| Error(format!("preview {}: {e}", p.preview.display())))
}

pub(crate) fn encode_jpeg(img: &image::RgbImage) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, 82)
        .encode(
            img.as_raw(),
            img.width() as u16,
            img.height() as u16,
            jpeg_encoder::ColorType::Rgb,
        )
        .map_err(|e| Error(format!("jpeg encode: {e}")))?;
    Ok(out)
}

/// 3x5 bitmap digits, one row per entry, high bit on the left.
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];

/// Draw `n` as white digits on a black tag, so a model can read cell numbers.
pub(crate) fn draw_number(img: &mut image::RgbImage, x: u32, y: u32, n: usize) {
    const SCALE: u32 = 4;
    let digits: Vec<usize> = n.to_string().bytes().map(|b| (b - b'0') as usize).collect();
    let (w, h) = (digits.len() as u32 * 4 * SCALE + SCALE, 7 * SCALE);
    for py in y..(y + h).min(img.height()) {
        for px in x..(x + w).min(img.width()) {
            img.put_pixel(px, py, image::Rgb([0, 0, 0]));
        }
    }
    for (i, &d) in digits.iter().enumerate() {
        for (row, bits) in DIGITS[d].iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) == 0 {
                    continue;
                }
                let gx = x + SCALE + (i as u32 * 4 + col) * SCALE;
                let gy = y + SCALE + row as u32 * SCALE;
                for py in gy..(gy + SCALE).min(img.height()) {
                    for px in gx..(gx + SCALE).min(img.width()) {
                        img.put_pixel(px, py, image::Rgb([255, 255, 255]));
                    }
                }
            }
        }
    }
}
