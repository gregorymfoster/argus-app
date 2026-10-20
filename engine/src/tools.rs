//! The tool layer as data: every tool the Agent and the MCP server call, each with a JSON
//! Schema, behind one entry point ([`Tools::call`]). Transports (MCP over stdio, the in-app
//! Agent's loop, later the UI) stay thin: they list [`Tools::specs`] and forward calls.
//!
//! Each call opens the Library afresh, so long calls (a cull) never block short ones and
//! changes land in the same `library.db` the app and `argus serve` read (WAL mode).

use crate::agent::Chat;
use crate::compute::Compute;
use crate::cull::{self, Provider};
use crate::jobs;
use crate::library::{
    is_on_card, Author, BatchId, ImportOptions, Library, Look, Photo, PhotoFilter, PhotoId,
    ShootId, Verdict,
};
use crate::masks::{Canvas, Mask, Shape};
use crate::recipe::{Local, Recipe};
use crate::{export, render, Error, Result};
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
    /// Tools over the Library folder at `library`, culling with [`cull::provider`].
    pub fn new(library: impl Into<PathBuf>) -> Self {
        let library = library.into();
        // The Ollama model this Library was set to use (Settings › AI and Compute).
        if library.join("library.db").exists() {
            if let Ok(lib) = Library::open(&library) {
                let m = lib.setting(OLLAMA_MODEL_KEY).ok().flatten();
                cull::choose_ollama_model(m.and_then(|v| v.as_str().map(str::to_owned)));
            }
        }
        Self {
            library,
            providers: Box::new(cull::provider),
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
        let auto = crate::auto::auto_adjust(
            |s| {
                let r = Recipe {
                    global: base.global.plus(s),
                    ..base.clone()
                };
                render::render_photo(&p, &r, Some(crate::auto::MEASURE_EDGE), masks.clone())
            },
            p.kind == "raw",
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
        let mut lib = Library::open(&self.library)?;
        match name {
            "list_shoots" => one(lib.shoots()?),
            "rename_shoot" => {
                let a: RenameShoot = parse(args)?;
                one(lib.rename_shoot(a.shoot_id, &a.name)?)
            }
            "list_photos" => {
                let a: ListPhotos = parse(args)?;
                let photos = lib.list_photos(&PhotoFilter {
                    shoot: a.shoot_id,
                    verdict: a.verdict,
                })?;
                let min = a.min_stars.unwrap_or(0);
                one(photos
                    .iter()
                    .filter(|p| p.stars >= min)
                    .map(PhotoView::from)
                    .collect::<Vec<_>>())
            }
            "import_folder" => {
                let a: ImportFolder = parse(args)?;
                let folder = Path::new(&a.folder);
                let report = lib.import(
                    folder,
                    &ImportOptions {
                        shoot_name: a.shoot_name,
                        files: a.files.map(|f| f.into_iter().map(PathBuf::from).collect()),
                        raw_only: a.raw_only,
                        copy: a.copy.unwrap_or_else(|| is_on_card(folder)),
                        ..Default::default()
                    },
                )?;
                // RAWs come in showing the camera's preview; Argus's own render follows on
                // the background Queue at the Compute level.
                if let (Some(shoot), 1..) = (report.shoot_id, report.camera_previews) {
                    jobs::enqueue(
                        &mut lib,
                        "render_previews",
                        &json!({ "shoot_id": shoot }),
                        None,
                        None,
                    )?;
                }
                // The specialist models' Facts, after the previews they're read from.
                if let Some(shoot) = report.shoot_id {
                    crate::analyzers::enqueue(&mut lib, Some(shoot), None)?;
                }
                // Auto first, so the cull and Suggestions see the toned Photos. A failed
                // Auto leaves the Import as it is.
                if let (Some(shoot), true) = (report.shoot_id, a.auto) {
                    let auto = lib
                        .begin_batch(Author::Person, "Auto on import")
                        .and_then(|b| {
                            let a = AutoEdit {
                                photo_ids: None,
                                shoot_id: Some(shoot),
                                compute: None,
                            };
                            auto_edit(&mut lib, a, Some(b))
                        });
                    if let Err(e) = auto {
                        eprintln!("argus import: Auto failed: {e}");
                    }
                }
                // Queued here rather than by the caller, so the app can leave the dialog
                // (and reload into the new Shoot) while the Import still runs.
                if let Some(shoot) = report.shoot_id {
                    let kinds = [
                        ("cull_shoot", a.cull),
                        ("suggest_shoot", a.suggest.unwrap_or(a.cull)),
                    ];
                    let mut args = json!({ "shoot_id": shoot });
                    if let Some(p) = &a.provider {
                        args["provider"] = json!(p);
                    }
                    for (kind, _) in kinds.iter().filter(|(_, on)| *on) {
                        jobs::enqueue(&mut lib, kind, &args, None, None)?;
                    }
                }
                one(report)
            }
            "estimate_import" => {
                let a: EstimateImport = parse(args)?;
                let folder = Path::new(&a.folder);
                let files: Option<Vec<PathBuf>> =
                    a.files.map(|f| f.into_iter().map(PathBuf::from).collect());
                let copy = a.copy.unwrap_or_else(|| is_on_card(folder));
                one(lib.estimate_import(folder, files.as_deref(), copy, a.raw_only)?)
            }
            "library_storage" => one(lib.storage()?),
            "clear_cache" => one(lib.clear_cache()?),
            "usage_log" => {
                let a: UsageLogArgs = parse(args)?;
                if let Some(on) = a.recording {
                    lib.set_recording_usage(on)?;
                }
                let cleared = if a.clear {
                    Some(lib.clear_usage()?)
                } else {
                    None
                };
                let mut out = to_json(lib.usage_summary()?)?;
                out["cleared"] = json!(cleared);
                if let Some(id) = a.photo_id {
                    let events = lib.usage_events(Some(id))?;
                    out["history"] = to_json(crate::usage::history(&events))?;
                    out["photo_events"] = to_json(events)?;
                }
                one(out)
            }
            "style_profile" => {
                let a: StyleArgs = parse(args)?;
                let p = match &a.corrections {
                    Some(c) => crate::style::Profile::learn(&crate::style::corrections_from(c)?),
                    None => crate::style::Profile::of_library(&lib)?,
                };
                let mut out = to_json(&p)?;
                out["line"] = json!(crate::style::Profile::line(&p.moves));
                out["min_photos"] = json!(crate::style::MIN_PHOTOS);
                one(out)
            }
            "export_usage_bench" => {
                let a: ExportUsageBench = parse(args)?;
                let folder = export::expand_home(&a.folder);
                one(crate::usage::export_bench(
                    self,
                    &folder,
                    a.images.unwrap_or(true),
                    a.long_edge.unwrap_or(640),
                )?)
            }
            "library_settings" => {
                let a: SettingsArgs = parse(args)?;
                let mut s = lib.settings()?;
                if let Some(v) = a.preview_long_edge {
                    s.preview_long_edge = v;
                }
                if let Some(v) = a.preview_quality {
                    s.preview_quality = v;
                }
                if a.preview_long_edge.is_some() || a.preview_quality.is_some() {
                    lib.set_settings(&s)?;
                }
                one(s)
            }
            "rebuild_previews" => one(lib.rebuild_previews(4)?),
            "find_missing_originals" => one(lib.missing_originals()?),
            "relink_originals" => {
                let a: ScanFolder = parse(args)?;
                one(lib.relink(Path::new(&a.folder))?)
            }
            "find_cards" => one(crate::library::find_cards()),
            "scan_folder" => {
                let a: ScanFolder = parse(args)?;
                one(lib.scan(Path::new(&a.folder))?)
            }
            "get_preview" => {
                let a: GetPreview = parse(args)?;
                let photo = find_photo(&lib, a.photo_id)?;
                let edge = a.long_edge.unwrap_or(1024).clamp(64, MAX_PREVIEW_EDGE);
                let img = open_preview(&photo)?.thumbnail(edge, edge).to_rgb8();
                Ok(vec![
                    Part::Json(to_json(PhotoView::from(&photo))?),
                    Part::Jpeg(encode_jpeg(&img)?),
                ])
            }
            "get_contact_sheet" => {
                let a: GetContactSheet = parse(args)?;
                contact_sheet(&lib, a.shoot_id, a.page.unwrap_or(1))
            }
            "set_verdict" => {
                let a: SetVerdict = parse(args)?;
                set_verdict(&mut lib, a, batch)
            }
            "set_rating" => {
                let a: SetRating = parse(args)?;
                set_rating(&mut lib, a, batch)
            }
            "best_photos" => {
                let a: BestPhotos = parse(args)?;
                let shoot = match a.shoot_id {
                    Some(s) => s,
                    None => newest_shoot(&lib)?,
                };
                let photos = shoot_photos(&lib, shoot)?;
                let best: Vec<PhotoView> = lib
                    .best_photos(shoot)?
                    .iter()
                    .filter_map(|b| photos.iter().find(|p| p.id == b.photo_id))
                    .take(a.limit.unwrap_or(usize::MAX))
                    .map(PhotoView::from)
                    .collect();
                one(best)
            }
            "set_stacks" => {
                let a: SetStacks = parse(args)?;
                lib.replace_stacks(a.shoot_id, &a.stacks)?;
                one(stacks(&lib, a.shoot_id)?)
            }
            "stack_bursts" => {
                let a: ShootArg = parse(args)?;
                crate::bursts::stack_shoot(&mut lib, a.shoot_id, true)?;
                one(stacks(&lib, a.shoot_id)?)
            }
            "list_stacks" => {
                let a: ShootArg = parse(args)?;
                one(stacks(&lib, a.shoot_id)?)
            }
            "cull_shoot" => {
                let a: CullShoot = parse(args)?;
                let shoot = match a.shoot_id {
                    Some(s) => s,
                    None => newest_shoot(&lib)?,
                };
                // Without a level it runs as the queue would now: Full while the Mac is idle
                // or overnight, else the queue's own level.
                let compute = match a.compute {
                    Some(c) => c,
                    None => jobs::effective_compute(&jobs::config(&lib)?),
                };
                let provider = (self.providers)(a.provider.as_deref(), Some(compute))?;
                // The app's cull bar reads this task (`progress`).
                let task = crate::progress::Task::start("cull", "Model looking at the photos", 0);
                let _who = crate::model_lock::as_who(format!(
                    "the AI cull in {}",
                    crate::model_lock::who()
                ));
                one(cull::cull_shoot_with(
                    &mut lib,
                    shoot,
                    provider.as_ref(),
                    cull::max_per_request(),
                    &|p| {
                        task.set(p.photos, p.photos_total);
                        task.relabel(p.stage());
                        task.set_waiting(p.waiting.clone());
                    },
                )?)
            }
            "read_recipe" => {
                let a: PhotoArg = parse(args)?;
                find_photo(&lib, a.photo_id)?;
                one(lib.read_recipe(a.photo_id)?)
            }
            "write_recipe" => {
                let a: WriteRecipe = parse(args)?;
                if !a.recipe.is_object() {
                    return Err(Error("recipe must be a JSON object".into()));
                }
                let mut recipe = Recipe::from_value(&a.recipe)?;
                if let Some(k) = recipe.other.keys().next() {
                    return Err(Error(format!(
                        "unknown Recipe field {k:?}: sliders go under \"global\" (e.g. {{\"global\": {{\"exposure\": 0.3}}}}); top-level fields are schema, global, look, correction and local"
                    )));
                }
                recipe.global = recipe.global.clamped();
                for &id in &a.photo_ids {
                    find_photo(&lib, id)?;
                }
                let batch = batch_or(&mut lib, batch, "write_recipe")?;
                let mut versions = BTreeMap::new();
                for &id in &a.photo_ids {
                    versions.insert(
                        id,
                        lib.write_recipe(id, &recipe.to_value(), Author::Ai, Some(batch))?,
                    );
                }
                one(json!({ "batch_id": batch, "versions": versions }))
            }
            "edit_recipe" => {
                let a: EditRecipe = parse(args)?;
                if a.set.is_empty() {
                    return Err(Error("set at least one setting".into()));
                }
                let mut next = Vec::new();
                for &id in &a.photo_ids {
                    find_photo(&lib, id)?;
                    let mut r = current_recipe(&lib, id)?;
                    for (path, v) in &a.set {
                        r.set_value(path, v, a.relative)?;
                    }
                    next.push((id, r));
                }
                let batch = batch_or(&mut lib, batch, "edit_recipe")?;
                let mut out = BTreeMap::new();
                for (id, r) in next {
                    let version = lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
                    out.insert(id, json!({ "version": version, "global": r.global }));
                }
                one(json!({ "batch_id": batch, "photos": out }))
            }
            "render_photo" => render_parts(&lib, parse(args)?, MAX_PREVIEW_EDGE),
            "auto_edit" => auto_edit(&mut lib, parse(args)?, batch),
            "white_balance" => white_balance(&mut lib, parse(args)?, batch),
            "export_photos" => {
                let a: ExportPhotos = parse(args)?;
                export_photos(&lib, a)
            }
            "list_versions" => {
                let a: PhotoArg = parse(args)?;
                find_photo(&lib, a.photo_id)?;
                one(lib.versions(a.photo_id)?)
            }
            "undo_batch" => {
                let a: UndoBatch = parse(args)?;
                let id = match a.batch_id {
                    Some(b) => b,
                    None => {
                        lib.last_batch(Some(Author::Ai))?
                            .ok_or_else(|| Error("there is no AI change to undo".into()))?
                            .0
                    }
                };
                one(lib.undo_batch(id, Author::Person)?)
            }
            "list_looks" => {
                let a: ShootArg = parse(args)?;
                shoot_photos(&lib, a.shoot_id)?;
                one(lib.looks(a.shoot_id)?)
            }
            "set_look" => {
                let a: SetLook = parse(args)?;
                set_look(&mut lib, a, batch)
            }
            "sync_to_look" => {
                let a: SyncToLook = parse(args)?;
                sync_to_look(&mut lib, a, batch)
            }
            "remove_look" => {
                let a: RemoveLook = parse(args)?;
                let under = photos_under(&lib, a.shoot_id, &a.look_id)?;
                let batch = batch_or(&mut lib, batch, "Remove Look")?;
                for &id in &under {
                    let mut r = current_recipe(&lib, id)?;
                    r.look = None;
                    lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
                }
                one(json!({ "batch_id": batch, "removed_from": under }))
            }
            "analyze_photo" => {
                let a: PhotoArg = parse(args)?;
                analyze_photo(&lib, &find_photo(&lib, a.photo_id)?)
            }
            "add_mask" => add_mask(&mut lib, parse(args)?, batch),
            "edit_mask" => edit_mask(&mut lib, parse(args)?, batch),
            "remove_mask" => {
                let a: LocalArg = parse(args)?;
                find_photo(&lib, a.photo_id)?;
                let mut r = current_recipe(&lib, a.photo_id)?;
                r.local_mut(&a.local_id)?;
                r.local.retain(|l| l.id != a.local_id);
                let batch = batch_or(&mut lib, batch, "remove_mask")?;
                let version =
                    lib.write_recipe(a.photo_id, &r.to_value(), Author::Ai, Some(batch))?;
                one(json!({ "batch_id": batch, "version": version, "local": r.local }))
            }
            "view_mask" => view_mask(&lib, parse(args)?),
            "enqueue_job" => {
                let a: EnqueueJob = parse(args)?;
                let (job_id, created) = jobs::enqueue(
                    &mut lib,
                    &a.kind,
                    &a.args,
                    a.priority,
                    a.dedupe_key.as_deref(),
                )?;
                one(json!({ "job_id": job_id, "created": created }))
            }
            "suggest_edits" => {
                let a: SuggestEdits = parse(args)?;
                let mut args = json!({});
                if let Some(c) = a.count {
                    args["count"] = json!(c);
                }
                if let Some(p) = &a.provider {
                    args["provider"] = json!(p);
                }
                let (kind, args) = match (a.photo_ids, a.shoot_id) {
                    (Some(ids), _) if ids.len() == 1 => {
                        args["photo_id"] = json!(ids[0]);
                        ("suggest", args)
                    }
                    (Some(ids), _) => {
                        args["photo_ids"] = json!(ids);
                        ("suggest_shoot", args)
                    }
                    (None, shoot) => {
                        args["shoot_id"] = json!(match shoot {
                            Some(s) => s,
                            None => newest_shoot(&lib)?,
                        });
                        ("suggest_shoot", args)
                    }
                };
                let (job_id, created) = jobs::enqueue(&mut lib, kind, &args, None, None)?;
                one(json!({ "job_id": job_id, "created": created }))
            }
            "match_to_photo" => {
                let a: MatchToPhoto = parse(args)?;
                find_photo(&lib, a.photo_id)?;
                let mut args = json!({ "photo_id": a.photo_id });
                if let Some(ids) = a.photo_ids {
                    args["photo_ids"] = json!(ids);
                }
                if let Some(p) = &a.provider {
                    args["provider"] = json!(p);
                }
                let (job_id, created) = jobs::enqueue(&mut lib, "match_hero", &args, None, None)?;
                one(json!({ "job_id": job_id, "created": created }))
            }
            "list_jobs" => {
                let a: ListJobs = parse(args)?;
                let status = a.status.as_deref().map(str::parse).transpose()?;
                one(lib.jobs(
                    status,
                    a.kind.as_deref(),
                    a.limit.unwrap_or(50).clamp(1, 1000),
                )?)
            }
            "cancel_job" => {
                let a: JobArg = parse(args)?;
                one(json!({ "cancelled": lib.cancel_job(a.job_id)? }))
            }
            "reorder_jobs" => {
                let a: ReorderJobs = parse(args)?;
                one(json!({ "moved": lib.reorder_jobs(&a.job_ids)? }))
            }
            "queue_status" => one(queue_status(&lib)?),
            "suggestions_progress" => {
                let a: PhotoArg = parse(args)?;
                one(jobs::suggestions_progress(&lib, a.photo_id)?)
            }
            "retry_failed" => {
                let n = lib.retry_failed_jobs()?;
                let mut v = queue_status(&lib)?;
                v["retried"] = json!(n);
                one(v)
            }
            "set_queue" => {
                let a: SetQueue = parse(args)?;
                let mut c = jobs::config(&lib)?;
                if let Some(v) = a.compute {
                    c.compute = v;
                }
                if let Some(v) = a.paused {
                    c.paused = v;
                }
                if let Some(v) = a.idle_boost_min {
                    c.idle_boost_min = v;
                }
                if let Some(v) = a.overnight {
                    c.overnight = v;
                }
                if let Some(v) = a.focus_first {
                    c.focus_first = v;
                }
                if let Some(v) = a.takes {
                    c.takes = v.clamp(1, 5);
                }
                if let Some(v) = a.run_on_battery {
                    c.run_on_battery = v;
                }
                if let Some(v) = a.run_anyway {
                    c.battery_override = v;
                }
                if let Some(v) = a.idle_only {
                    c.idle_only = v;
                }
                if let Some(v) = a.run_now {
                    c.go_ahead = v;
                }
                jobs::set_config(&lib, &c)?;
                one(queue_status(&lib)?)
            }
            "list_suggestions" => {
                let a: ListSuggestions = parse(args)?;
                one(lib.suggestions(a.photo_id, a.shoot_id, a.all.unwrap_or(false))?)
            }
            "get_suggestion" => {
                let a: SuggestionArg = parse(args)?;
                let sug = lib.suggestion(a.suggestion_id)?;
                let edge = a.long_edge.unwrap_or(1024).clamp(64, MAX_PREVIEW_EDGE);
                // At another Amount the saved preview doesn't show it: render.
                let amount = a.amount.filter(|&v| v != 1.0);
                let saved = sug
                    .preview
                    .as_ref()
                    .map(|p| lib.root().join(p))
                    .filter(|p| p.exists() && amount.is_none());
                let jpeg = match saved {
                    Some(p) => {
                        let img = image::open(&p)
                            .map_err(|e| Error(format!("{}: {e}", p.display())))?
                            .thumbnail(edge, edge)
                            .to_rgb8();
                        encode_jpeg(&img)?
                    }
                    None => {
                        let photo = find_photo(&lib, sug.photo_id)?;
                        let recipe = Recipe::from_value(&match amount {
                            Some(v) => lib.suggestion_at(&sug, v)?,
                            None => sug.recipe.clone(),
                        })?;
                        let img = render::render_in(
                            &lib,
                            &photo,
                            &with_look(&lib, &photo, &recipe)?,
                            Some(edge as usize),
                        )?;
                        render::encode_jpeg(&img, 85)?
                    }
                };
                Ok(vec![Part::Json(to_json(&sug)?), Part::Jpeg(jpeg)])
            }
            "accept_suggestion" => {
                let a: AcceptSuggestion = parse(args)?;
                let ids: Vec<i64> = a
                    .suggestion_id
                    .into_iter()
                    .chain(a.suggestion_ids.unwrap_or_default())
                    .collect();
                if ids.is_empty() {
                    return Err(Error("give suggestion_id or suggestion_ids".into()));
                }
                let (batch_id, done) = lib.accept_suggestions(&ids, Author::Person, a.amount)?;
                let photos: Vec<Value> = done
                    .iter()
                    .map(|(version, s)| json!({ "photo_id": s.photo_id, "version": version, "title": s.title }))
                    .collect();
                let mut v = json!({ "batch_id": batch_id, "photos": photos });
                if let [(version, s)] = done.as_slice() {
                    v["photo_id"] = json!(s.photo_id);
                    v["version"] = json!(version);
                    v["title"] = json!(s.title);
                }
                one(v)
            }
            "dismiss_suggestion" => {
                let a: SuggestionArg = parse(args)?;
                lib.dismiss_suggestion(a.suggestion_id)?;
                one(json!({ "dismissed": a.suggestion_id }))
            }
            other => Err(Error(format!(
                "unknown tool {other:?}; known tools: {}",
                specs()
                    .iter()
                    .map(|s| s.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }
}

fn specs() -> Vec<ToolSpec> {
    let id = json!({ "type": "integer" });
    let ids = json!({ "type": "array", "items": { "type": "integer" }, "minItems": 1 });
    let verdict = json!({ "type": "string", "enum": ["pick", "reject", "unflagged"] });
    vec![
        ToolSpec {
            name: "list_shoots",
            description: "List the Shoots in the Library (one per Import) with their Photo counts.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
        ToolSpec {
            name: "rename_shoot",
            description: "Rename a Shoot. The name is stored by the naming rules (date first, words joined by hyphens, no spaces: \"Boat tour\" becomes 2026-10-10_Boat-tour); the Shoot keeps its date unless the name starts with a new one. Its originals folder inside the Library is renamed to match.",
            input_schema: json!({ "type": "object", "required": ["shoot_id", "name"], "properties": {
                "shoot_id": id, "name": { "type": "string", "minLength": 1 } } }),
            read_only: false,
        },
        ToolSpec {
            name: "list_photos",
            description: "List Photos in capture order with their Verdict (pick, reject, unflagged), who set it and why, their star Rating (0-5, 0 = unrated) and who set it, their Stack, and `best`: their rank in the Shoot's Best Photos. Narrow by Shoot, Verdict and/or least stars.",
            input_schema: json!({ "type": "object", "properties": {
                "shoot_id": id, "verdict": verdict,
                "min_stars": { "type": "integer", "minimum": 0, "maximum": 5 } } }),
            read_only: true,
        },
        ToolSpec {
            name: "import_folder",
            description: "Import a folder on this Mac (recursively; RAW and JPEG) into the Library as one new Shoot, or only the `files` you name from it. Duplicates are skipped and originals are never changed; from a memory card they are copied into the Library (see `copy`). Takes about a second per RAW. Photos appear in the Shoot as they land, so `photos` and `progress` can follow a running Import.",
            input_schema: json!({ "type": "object", "required": ["folder"], "properties": {
                "folder": { "type": "string", "description": "Absolute path" },
                "shoot_name": { "type": "string", "description": "Defaults to the folder's name" },
                "files": { "type": "array", "items": { "type": "string" },
                    "description": "Absolute paths of the files to import, all inside folder (e.g. from scan_folder); default everything" },
                "raw_only": { "type": "boolean",
                    "description": "For RAW+JPEG frames, skip the camera's JPEG. By default it is kept with its RAW as one Photo" },
                "copy": { "type": "boolean",
                    "description": "Copy the originals into the Library's originals folder (verified) instead of reading them in place. Default: true from a memory card, false from a folder" },
                "cull": { "type": "boolean",
                    "description": "Then queue an AI cull and edit Suggestions for the new Shoot" },
                "auto": { "type": "boolean",
                    "description": "Then run Auto (as auto_edit) on every new Photo, as one undoable batch" },
                "provider": { "type": "string", "description": "ollama, claude-code or claude-api; default ARGUS_PROVIDER, else ollama" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "estimate_import",
            description: "How much disk an Import would take, before running it: the originals' size, whether they'd be copied into the Library (by default yes from a memory card, no from a folder), the estimated previews, the total needed and the free space on the Library's disk. Reads file sizes only.",
            input_schema: json!({ "type": "object", "required": ["folder"], "properties": {
                "folder": { "type": "string", "description": "Absolute path" },
                "files": { "type": "array", "items": { "type": "string" },
                    "description": "Only these files (e.g. the new ones from scan_folder); default everything" },
                "copy": { "type": "boolean", "description": "Copy into the Library; default true for a memory card, false otherwise" },
                "raw_only": { "type": "boolean", "description": "As import_folder: leave out the camera JPEGs of RAW+JPEG frames" } } }),
            read_only: true,
        },
        ToolSpec {
            name: "library_storage",
            description: "Where the Library folder is and how much disk each part takes: originals copied into it, originals read from elsewhere, previews, cache, database and backups, plus how many originals are missing (moved or on a disconnected disk) and the free space on its disk.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
        ToolSpec {
            name: "clear_cache",
            description: "Delete what the Library can rebuild (its cache folder and previews no Photo uses). Never touches originals, edits or the previews in use. Returns the bytes freed.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: false,
        },
        ToolSpec {
            name: "library_settings",
            description: "Read the Library's settings, or change them by passing values: preview_long_edge (512-4096 px, default 2048) and preview_quality (50-95, default 85) for the previews Import makes. Smaller previews save disk; run rebuild_previews to apply to existing Photos.",
            input_schema: json!({ "type": "object", "properties": {
                "preview_long_edge": { "type": "integer" },
                "preview_quality": { "type": "integer" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "rebuild_previews",
            description: "Re-make every Photo's preview at the current library_settings. Slow: about a second per RAW. Photos whose original is missing keep their old preview.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: false,
        },
        ToolSpec {
            name: "find_missing_originals",
            description: "List Photos whose original file can't be found where the Library expects it (moved, renamed folder, or a disk that isn't connected).",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
        ToolSpec {
            name: "relink_originals",
            description: "Look for missing originals under a folder (recursively), match them by name and size, confirm each by content hash, and point the Library at them. Nothing in the folder is changed.",
            input_schema: json!({ "type": "object", "required": ["folder"], "properties": {
                "folder": { "type": "string", "description": "Absolute path to search" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "find_cards",
            description: "List the memory cards mounted on this Mac (volumes with a camera DCIM folder), with their paths.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
        ToolSpec {
            name: "scan_folder",
            description: "List the photos in a folder or on a card without importing them: path, name, size, capture time, camera, and whether the Library already has each one (same name, size and capture time). A RAW+JPEG frame is one entry, the RAW, with its JPEG beside it, as Import makes it one Photo. Fast: reads only file headers.",
            input_schema: json!({ "type": "object", "required": ["folder"], "properties": {
                "folder": { "type": "string", "description": "Absolute path, e.g. a card from find_cards" } } }),
            read_only: true,
        },
        ToolSpec {
            name: "get_preview",
            description: "Look at one Photo: its details plus a JPEG preview (long edge 1024 by default, at most 1536).",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": {
                "photo_id": id,
                "long_edge": { "type": "integer", "minimum": 64, "maximum": MAX_PREVIEW_EDGE } } }),
            read_only: true,
        },
        ToolSpec {
            name: "get_contact_sheet",
            description: "See a whole Shoot at once, as you need to before judging any one Photo: a grid image of up to 30 numbered thumbnails per page, in capture order, plus a legend mapping each number to its Photo id, Verdict and Stack. Ask for further pages while `pages` is larger.",
            input_schema: json!({ "type": "object", "required": ["shoot_id"], "properties": {
                "shoot_id": id, "page": { "type": "integer", "minimum": 1 } } }),
            read_only: true,
        },
        ToolSpec {
            name: "set_verdict",
            description: "Set one Verdict on many Photos. By default it is recorded as the AI's suggestion with your reason, and Photos the person flagged themselves are left alone (reported as kept). Use as=person only when the person told you to flag those exact Photos.",
            input_schema: json!({ "type": "object", "required": ["photo_ids", "verdict"], "properties": {
                "photo_ids": ids, "verdict": verdict,
                "reason": { "type": "string", "description": "Short, for the person: why" },
                "as": { "type": "string", "enum": ["ai", "person"] } } }),
            read_only: false,
        },
        ToolSpec {
            name: "set_rating",
            description: "Set one star Rating (Lightroom stars: 0 = none, 1 = reject-worthy, 3 = solid keeper, 5 = the best of the Shoot) on many Photos. By default it is recorded as the AI's suggestion, and Photos the person rated themselves are left alone (reported as kept). Use as=person only when the person told you to rate those exact Photos.",
            input_schema: json!({ "type": "object", "required": ["photo_ids", "stars"], "properties": {
                "photo_ids": ids,
                "stars": { "type": "integer", "minimum": 0, "maximum": 5 },
                "as": { "type": "string", "enum": ["ai", "person"] } } }),
            read_only: false,
        },
        ToolSpec {
            name: "best_photos",
            description: "A Shoot's Best Photos, best first: the AI cull's short list of the strongest frames (about one in ten, at most one per Stack), leaving out any the person rejected. Empty until the Shoot is culled.",
            input_schema: json!({ "type": "object", "properties": {
                "shoot_id": { "type": "integer", "description": "Defaults to the newest Shoot" },
                "limit": { "type": "integer", "minimum": 1 } } }),
            read_only: true,
        },
        ToolSpec {
            name: "list_stacks",
            description: "List a Shoot's Stacks (bursts or repeated shots of one moment), each as Photo ids best first.",
            input_schema: json!({ "type": "object", "required": ["shoot_id"], "properties": { "shoot_id": id } }),
            read_only: true,
        },
        ToolSpec {
            name: "stack_bursts",
            description: "Find a Shoot's bursts (frames seconds apart that look alike) and join them into its Stacks, keeping the Stacks it has. Returns every Stack, best frame first. Import does this by itself once a Shoot's Photos are analyzed; a cull keeps the bursts too.",
            input_schema: json!({ "type": "object", "required": ["shoot_id"], "properties": { "shoot_id": id } }),
            read_only: false,
        },
        ToolSpec {
            name: "set_stacks",
            description: "Replace all of a Shoot's Stacks. Each inner list is one Stack of Photo ids, best frame first; lists of one are ignored.",
            input_schema: json!({ "type": "object", "required": ["shoot_id", "stacks"], "properties": {
                "shoot_id": id,
                "stacks": { "type": "array", "items": { "type": "array", "items": { "type": "integer" } } } } }),
            read_only: false,
        },
        ToolSpec {
            name: "cull_shoot",
            description: "Have Argus's own Provider cull a whole Shoot in one pass: Verdicts with reasons, star Ratings, Stacks and the Shoot's Best Photos, keeping the person's own flags and stars. Local models can take many minutes. If you can see images yourself, you can instead cull with get_contact_sheet, set_verdict, set_rating and set_stacks.",
            input_schema: json!({ "type": "object", "properties": {
                "shoot_id": { "type": "integer", "description": "Defaults to the newest Shoot" },
                "provider": { "type": "string", "enum": ["ollama", "claude-code", "claude-api"] },
                "compute": { "type": "string", "enum": ["gentle", "balanced", "full"],
                    "description": "How much of the Mac a local model may take; gentle unless the person said otherwise" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "read_recipe",
            description: "Read a Photo's current Recipe (its JSON edit settings) and Version, or null if it was never edited.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": { "photo_id": id } }),
            read_only: true,
        },
        ToolSpec {
            name: "write_recipe",
            description: "Save a whole Recipe as a new Version on one or many Photos, as one undoable batch: {\"schema\":1, \"global\":{sliders}, \"look\":null, \"correction\":{...}, \"local\":[]}, sliders as in edit_recipe. Prefer edit_recipe to change a few settings. Originals are never touched.",
            input_schema: json!({ "type": "object", "required": ["photo_ids", "recipe"], "properties": {
                "photo_ids": ids, "recipe": { "type": "object" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "edit_recipe",
            description: EDIT_RECIPE_DOC,
            input_schema: json!({ "type": "object", "required": ["photo_ids", "set"], "properties": {
                "photo_ids": ids,
                "set": { "type": "object", "additionalProperties": { "type": ["number", "array"] },
                    "description": "Setting path -> value, e.g. {\"exposure\": 0.3, \"temp\": -10, \"hsl.blue.sat\": -20, \"clarity\": 15}; a list only for curve.<rgb|red|green|blue> and pointColor" },
                "relative": { "type": "boolean", "description": "Add to the current values instead of replacing them (default false)" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "render_photo",
            description: "See a Photo with its edits: renders it from the original with its current Recipe (or the `recipe` you pass, without saving it) and returns a JPEG (long edge 1024 by default, at most 1536) and `light`, the render measured (luma percentiles, clipped and crushed percent, zones, mid-tone colour cast): trust those numbers over your eye for brightness and clipping. About a second for a RAW.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": {
                "photo_id": id,
                "long_edge": { "type": "integer", "minimum": 64, "maximum": MAX_PREVIEW_EDGE },
                "recipe": { "type": "object", "description": "Try a Recipe without saving it" } } }),
            read_only: true,
        },
        ToolSpec {
            name: "auto_edit",
            description: "Lightroom's Auto on one or many Photos (photo_ids, or every Photo of shoot_id), as one undoable batch: sets Exposure, Contrast, Highlights, Shadows, Whites, Blacks, Temp, Tint and Vibrance from the Photo's own light, deterministically and in well under a second per Photo (no model). Everything else in the Recipe (Look, HSL, Masks, crop) is kept. A good first step before finer edits.",
            input_schema: json!({ "type": "object", "properties": {
                "photo_ids": ids, "shoot_id": id,
                "compute": { "type": "string", "enum": ["gentle", "balanced", "full"] } } }),
            read_only: false,
        },
        ToolSpec {
            name: "white_balance",
            description: WHITE_BALANCE_DOC,
            input_schema: json!({ "type": "object", "required": ["photo_ids"], "properties": {
                "photo_ids": ids,
                "preset": { "type": "string", "enum": ["as_shot", "auto", "daylight", "cloudy", "shade", "tungsten", "fluorescent", "flash"] },
                "at": { "type": "object", "required": ["x", "y"], "description": "Eyedropper: a point that should be neutral grey, x and y 0..1 of the frame as render_photo shows it",
                    "properties": { "x": { "type": "number" }, "y": { "type": "number" } } },
                "dry_run": { "type": "boolean", "description": "Work out temp and tint without saving them" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "list_versions",
            description: "A Photo's Recipe history, oldest first: every saved Version with who made it (person or ai), its batch and label, and the Recipe.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": { "photo_id": id } }),
            read_only: true,
        },
        ToolSpec {
            name: "undo_batch",
            description: "Undo one batch of changes (an Agent turn, a cull, a Look change): every Recipe, Verdict, Rating and Look it touched goes back to how it was before. Without batch_id, undoes the newest AI batch. The undo is itself a new batch.",
            input_schema: json!({ "type": "object", "properties": { "batch_id": id } }),
            read_only: false,
        },
        ToolSpec {
            name: "list_looks",
            description: "List a Shoot's Looks (shared styles applied under each Photo's own sliders), each with its id, name and sliders.",
            input_schema: json!({ "type": "object", "required": ["shoot_id"], "properties": { "shoot_id": id } }),
            read_only: true,
        },
        ToolSpec {
            name: "set_look",
            description: SET_LOOK_DOC,
            input_schema: json!({ "type": "object", "required": ["shoot_id"], "properties": {
                "shoot_id": id,
                "look_id": { "type": "string", "description": "An existing Look to tune; omit to create one (or tune the Shoot's only Look)" },
                "name": { "type": "string", "description": "For a new Look, e.g. \"Warm film\"" },
                "set": { "type": "object", "additionalProperties": { "type": ["number", "array"] },
                    "description": "Setting path -> value, as in edit_recipe (no correction.*)" },
                "relative": { "type": "boolean", "description": "Add to the Look's current values (default false)" },
                "photo_ids": { "type": "array", "items": { "type": "integer" },
                    "description": "Photos to put under this Look; a new Look defaults to the Shoot's Picks" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "remove_look",
            description: "Take a Look off every Photo under it, leaving each Photo's own sliders. The Look itself is kept, so undo puts the Photos back under it.",
            input_schema: json!({ "type": "object", "required": ["shoot_id", "look_id"], "properties": {
                "shoot_id": id, "look_id": { "type": "string" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "sync_to_look",
            description: "Lightroom's Sync: fold one Photo's own sliders into its Shoot's Look so every Photo under that Look gets them, and clear them on that Photo. Creates a Look for the Shoot's Picks if there is none.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": { "photo_id": id } }),
            read_only: false,
        },
        ToolSpec {
            name: "suggest_edits",
            description: "Queue Suggestions: 2-3 complete edits (sliders, crop, straighten) the AI works out in the background for each Photo, for the person to tab through and accept. Nothing changes until they accept one. One Photo, a list, or a whole Shoot (rejects skipped, Picks first; Photos that already have Suggestions skipped).",
            input_schema: json!({ "type": "object", "properties": {
                "photo_ids": ids, "shoot_id": id,
                "count": { "type": "integer", "minimum": 1, "maximum": 5 },
                "provider": { "type": "string", "description": "ollama, claude-code or claude-api; default ARGUS_PROVIDER, else ollama" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "match_to_photo",
            description: "\"Apply this look to the rest\": the hero Photo's style becomes a new Look of its Shoot, and every other Photo (or photo_ids) gets 2 matched Suggestions under it, with its own exposure, white balance and crop, worked out in the background. Nothing changes until the person accepts them.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": {
                "photo_id": id, "photo_ids": ids,
                "provider": { "type": "string", "description": "ollama, claude-code or claude-api; default ARGUS_PROVIDER, else ollama" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "list_suggestions",
            description: "A Photo's or a whole Shoot's Suggestions, open and accepted (dismissed ones too with all: true), best first: title, why, status, the whole Recipe, and for matched ones the reference Photo.",
            input_schema: json!({ "type": "object", "properties": {
                "photo_id": id, "shoot_id": id, "all": { "type": "boolean" } } }),
            read_only: true,
        },
        ToolSpec {
            name: "get_suggestion",
            description: "One Suggestion and a JPEG of the Photo with it applied, at amount (the take's Amount slider: 0 = the Photo as it is, 1 = the take, the default, up to 1.5).",
            input_schema: json!({ "type": "object", "required": ["suggestion_id"], "properties": {
                "suggestion_id": id, "long_edge": { "type": "integer", "minimum": 64 },
                "amount": { "type": "number", "minimum": 0, "maximum": 1.5 } } }),
            read_only: true,
        },
        ToolSpec {
            name: "accept_suggestion",
            description: "Accept a Suggestion, or several (one per Photo) with suggestion_ids: each becomes its Photo's new Recipe Version, all in one undoable batch, at amount (the Amount slider: 0..1.5, default 1). The person's call; the AI never accepts its own.",
            input_schema: json!({ "type": "object", "properties": { "suggestion_id": id, "suggestion_ids": ids,
                "amount": { "type": "number", "minimum": 0, "maximum": 1.5 } } }),
            read_only: false,
        },
        ToolSpec {
            name: "dismiss_suggestion",
            description: "Dismiss a Suggestion so it no longer shows.",
            input_schema: json!({ "type": "object", "required": ["suggestion_id"], "properties": { "suggestion_id": id } }),
            read_only: false,
        },
        ToolSpec {
            name: "enqueue_job",
            description: "Queue background AI work. kind: suggest {photo_id, count?, reference_id?, look_id?}, suggest_shoot {shoot_id?, photo_ids?, count?, redo?}, match_hero {photo_id, photo_ids?}, cull_shoot (or rate_shoot) {shoot_id?}, render_previews {shoot_id?}, analyze {shoot_id?, photo_ids?} (the specialist models' Facts). Asking twice for the same thing returns the queued job.",
            input_schema: json!({ "type": "object", "required": ["kind"], "properties": {
                "kind": { "type": "string", "enum": jobs::KINDS },
                "args": { "type": "object" },
                "priority": { "type": "integer", "description": "Higher runs first" },
                "dedupe_key": { "type": "string" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "list_jobs",
            description: "Background jobs, newest first: kind, label, status (queued, running, done, failed, cancelled), progress and result.",
            input_schema: json!({ "type": "object", "properties": {
                "status": { "type": "string", "enum": ["queued", "running", "done", "failed", "cancelled"] },
                "kind": { "type": "string" }, "limit": { "type": "integer", "minimum": 1 } } }),
            read_only: true,
        },
        ToolSpec {
            name: "cancel_job",
            description: "Cancel a queued background job (a running one stops at its next step; Suggestions save nothing).",
            input_schema: json!({ "type": "object", "required": ["job_id"], "properties": { "job_id": id } }),
            read_only: false,
        },
        ToolSpec {
            name: "suggestions_progress",
            description: "How a Photo's Suggestions are coming along: the queued or running job, its stage (Queued, Model thinking, Review round 1 of 1, ...), how many takes, what runs before it, and eta_s until its takes land. job is null when none is coming.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": { "photo_id": id } }),
            read_only: true,
        },
        ToolSpec {
            name: "reorder_jobs",
            description: "Put queued background jobs in this order, first runs first (the Background work panel's drag). Jobs not listed keep their place; running ones finish first.",
            input_schema: json!({ "type": "object", "required": ["job_ids"], "properties": { "job_ids": ids } }),
            read_only: false,
        },
        ToolSpec {
            name: "queue_status",
            description: "How the background queue stands: the running job and its progress, how many are queued, done today and failed, a rough ETA in seconds, its Compute level and settings, boost (\"idle\" or \"overnight\") when it is running at Full above that level, and on_battery / battery_paused when the Mac is unplugged and background work waits for the charger (the open Photo's work still runs).",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
        ToolSpec {
            name: "set_queue",
            description: "Set how hard the background queue works: compute gentle (default, leaves the Mac to the person), balanced or full; pause it; how many idle minutes before it goes to full on its own (0 = never); whether it goes to full overnight (1:00-7:00); whether the Photo open in Loupe or Edit jumps the queue; how many Suggestions each Photo gets (1-5); run_on_battery to always run background work on battery (off by default); run_anyway to run on battery until the Mac is next plugged in; idle_only (on by default) to hold model work (Suggestions, culls) until the Mac is idle and plugged in, or overnight; and run_now to run it anyway until the queue is empty.",
            input_schema: json!({ "type": "object", "properties": {
                "compute": { "type": "string", "enum": ["gentle", "balanced", "full"] },
                "paused": { "type": "boolean" },
                "idle_boost_min": { "type": "integer", "minimum": 0 },
                "overnight": { "type": "boolean" },
                "focus_first": { "type": "boolean" },
                "takes": { "type": "integer", "minimum": 1, "maximum": 5 },
                "run_on_battery": { "type": "boolean" },
                "run_anyway": { "type": "boolean" },
                "idle_only": { "type": "boolean" },
                "run_now": { "type": "boolean" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "retry_failed",
            description: "Queue failed background jobs again (say a model was missing and is back). Repeats of the same job run once; returns queue_status plus retried, how many were queued.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: false,
        },
        ToolSpec {
            name: "usage_log",
            description: "The usage log Argus keeps in this Library (never uploaded): each Photo's starting Recipe, every AI take with its Provider and model, what the person accepted, flagged and exported. Returns counts; recording turns logging on or off (Settings › General, on by default); clear deletes the whole log; photo_id adds that Photo's events and folded history (kept take, sliders moved from it).",
            input_schema: json!({ "type": "object", "properties": {
                "recording": { "type": "boolean" }, "clear": { "type": "boolean" }, "photo_id": id } }),
            read_only: false,
        },
        ToolSpec {
            name: "style_profile",
            description: "The person's style profile, learned from the usage log: how they usually correct the AI's takes before exporting (median slider changes on Photos where they kept a take, shrunk while there are few). Fresh Suggestions add these moves to every take; a take name with enough corrections of its own gets its own (by_take). Empty below min_photos corrections. corrections, a list of {take, moved: {slider: change}}, learns from that list instead of this Library (benches, tests).",
            input_schema: json!({ "type": "object", "properties": {
                "corrections": { "type": "array", "items": { "type": "object" } } } }),
            read_only: true,
        },
        ToolSpec {
            name: "export_usage_bench",
            description: "Write the usage log as bench cases in fixtures/bench's layout (edit/cases.json with start and exported renders, cull/<Shoot>/labels.json with the person's and the AI's Verdicts and stars), for `python3 bench/score.py --bench <folder>`. images: false writes only the JSON.",
            input_schema: json!({ "type": "object", "required": ["folder"], "properties": {
                "folder": { "type": "string" }, "images": { "type": "boolean" },
                "long_edge": { "type": "integer", "minimum": 64, "maximum": 2048, "description": "Default 640, as the bench fixtures" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "export_photos",
            description: "Export Photos into a folder on this Mac (created if missing), as Lightroom's Export does: JPEG or TIFF rendered with each Photo's edits and Look, or the untouched original plus an XMP sidecar Lightroom reads. Names never contain spaces and nothing is overwritten (an existing name gets -2). JPEGs keep capture metadata but never location. Photos that fail are listed under failed; the rest still export.",
            input_schema: json!({ "type": "object", "required": ["photo_ids", "folder"], "properties": {
                "photo_ids": ids,
                "folder": { "type": "string", "description": "Export location: an absolute path, or starting with ~/" },
                "format": { "enum": ["jpeg", "tiff", "original"], "description": "Default jpeg" },
                "long_edge": { "type": "integer", "minimum": 64, "description": "Pixels; full size if omitted" },
                "quality": { "type": "integer", "minimum": 50, "maximum": 100, "description": "JPEG quality, default 90" },
                "naming": { "enum": ["shoot", "sequence", "original"],
                    "description": "shoot (default): 2026-10-18_Boat-Trip_DSC08005; sequence: 2026-10-18_Boat-Trip_001; original: DSC08005" },
                "shoot_folder": { "type": "boolean", "description": "Put files in a folder named after their Shoot inside folder (default true)" },
                "sequence_start": { "type": "integer", "minimum": 1, "description": "Number of the first Photo, for sequence names (default 1)" },
                "sequence_total": { "type": "integer", "minimum": 1, "description": "Photos in the whole export, when it is split across calls (sets the digits)" },
                "record": { "type": "boolean", "description": "false keeps this export out of the usage log" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "analyze_photo",
            description: "What the local specialist models see in a Photo: its faces (numbered from 0, left to right, with boxes as 0..1 of the image, and per face whether the eyes are closed and how sharp they are: focus about 1 = as sharp as anything in the frame, under 0.25 = soft), how much of the frame is people, skin, hair and sky, and its sharpness, brightness and clipping. Use it before targeting one face, checking a Photo has sky, or judging focus and blinks. A second or two the first time, then cached.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": { "photo_id": id } }),
            read_only: true,
        },
        ToolSpec {
            name: "add_mask",
            description: ADD_MASK_DOC,
            input_schema: json!({ "type": "object", "required": ["photo_ids", "mask", "adjust"], "properties": {
                "photo_ids": ids,
                "mask": { "type": "object", "description": "e.g. {\"kind\": \"sky\"}, {\"kind\": \"faces\", \"face\": 0}, {\"kind\": \"radial\", \"x\": 0.5, \"y\": 0.4, \"rx\": 0.2, \"ry\": 0.3}" },
                "adjust": { "type": "object", "additionalProperties": { "type": "number" },
                    "description": "Slider deltas inside the Mask, same paths and units as edit_recipe, e.g. {\"exposure\": -0.3, \"highlights\": -20}" },
                "name": { "type": "string", "description": "Shown to the person, e.g. \"Sky\" or \"Her face\"" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "edit_mask",
            description: "Change one local adjustment: its sliders (set, same paths as edit_recipe; relative adds), its Mask, or its name. Ids are in read_recipe's local list.",
            input_schema: json!({ "type": "object", "required": ["photo_id", "local_id"], "properties": {
                "photo_id": id, "local_id": { "type": "string" },
                "set": { "type": "object", "additionalProperties": { "type": "number" } },
                "relative": { "type": "boolean" },
                "mask": { "type": "object", "description": "Replaces the Mask" },
                "name": { "type": "string" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "remove_mask",
            description: "Remove one local adjustment (and its Mask) from a Photo's Recipe, as a new undoable Version.",
            input_schema: json!({ "type": "object", "required": ["photo_id", "local_id"], "properties": {
                "photo_id": id, "local_id": { "type": "string" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "view_mask",
            description: "See where a Mask lands before or after using it: the Photo rendered with its Recipe and the Mask tinted red, plus the share of the frame it covers. Pass a saved local_id or a mask to try.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": {
                "photo_id": id,
                "local_id": { "type": "string" },
                "mask": { "type": "object" },
                "long_edge": { "type": "integer", "minimum": 64, "maximum": MAX_PREVIEW_EDGE },
                "as": { "type": "string", "enum": ["overlay", "weights"],
                    "description": "overlay (default): tinted over the render; weights: the Mask alone as greyscale, white = full" } } }),
            read_only: true,
        },
    ]
}

const MASK_DOC: &str = "Mask kinds (JSON, \"kind\" plus fields): \
subject (every person), background (all but the people), faces (face skin; \"face\": n picks one face, numbered from 0 left to right as analyze_photo lists them), \
skin (face and body skin), hair, sky; \
person {person?: n, parts: [face_skin|body_skin|eyes|teeth|hair|whole]} (Lightroom's Select People: those parts of person n, or of everyone); \
brush {dots: [{x, y, r}], erase?: [{x, y, r}]}, linear {from: {x, y}, to: {x, y}} (full at from, none at to), \
radial {x, y, rx, ry, feather? 0..1 (default 0.5)}. \
Add \"invert\": true to flip any Mask. x and y are 0..1 of the upright image's width and height; rx is a fraction of the width, ry of the height, a dot's r of the long edge. \
AI kinds come from local models on this Mac and follow the image's edges.";

const ADD_MASK_DOC: &str = "Add a local adjustment to one or many Photos as one undoable batch: slider deltas that apply only inside a Mask, \
added to the global sliders (e.g. sky {\"exposure\": -0.4, \"highlights\": -30, \"dehaze\": 20}; a face {\"exposure\": 0.3, \"skin\": 30, \"texture\": -15}). \
Light, colour, curve, grading and retouch settings work in a Mask, plus texture, clarity, dehaze, sharpen, noise, colorNoise and defringe. \
Returns each Photo's new local id for edit_mask and remove_mask. Check it with view_mask or render_photo.";

const EDIT_RECIPE_DOC: &str = "Change a few edit settings on one or many Photos as one undoable batch, keeping the rest of each Recipe. \
Settings follow Lightroom's Edit panel, its names and units; 0 means unchanged unless a default is given. \
LIGHT: exposure in stops (-5..5); contrast, highlights, shadows, whites, blacks -100..100. \
TONE CURVE: curve.<highlights|lights|darks|shadows> -100..100 bend one tonal region (lights +20 brightens upper mids, shadows -20 deepens the low end); \
curve.<splitShadows|splitMidtones|splitHighlights> 0..100 move the region borders (defaults 25, 50, 75); \
curve.rgb, curve.red, curve.green, curve.blue: point curves as a list [[in, out], ...] on 0..255 (e.g. a fade: [[0, 20], [255, 255]]; [] resets); \
curve.refineSat 0..100 (default 100) keeps that much of the curves' saturation change. \
COLOR: temp (+ warmer, - cooler) and tint (+ magenta, - green) -100..100, relative to as shot (white_balance sets them from a preset or a grey point); \
vibrance (muted colours, spares skin) and saturation -100..100 (saturation -100 is black and white). \
COLOR MIXER: hsl.<red|orange|yellow|green|aqua|blue|purple|magenta>.<hue|sat|lum> -100..100 (skin lives in orange; skies in blue and aqua). \
POINT COLOR: pointColor, a list of {hue 0..360, sat 0..100, lum 0..100 (the colour to target, as sampled), hueShift, satShift, lumShift -100..100, range 0..100 (default 50)} for one exact colour. \
COLOR GRADING: grading.<shadows|midtones|highlights|global>.hue 0..360 (0 red, 40 orange, 60 yellow, 120 green, 200 teal, 220 blue, 280 purple), .sat 0..100 (10-30 is subtle), .lum -100..100; \
grading.blending 0..100 (default 50, how much the ranges overlap), grading.balance -100..100 (+ gives highlights more of the range). Teal shadows + orange highlights is the classic film grade. \
EFFECTS: texture -100..100 (fine detail: + crisper, - smoother skin); clarity -100..100 (midtone local contrast: + punchy, - soft glow); dehaze -100..100 (+ cuts haze and adds depth, - adds atmosphere); \
vignette -100..100 (post-crop: - darker corners, + lighter) with vignetteMidpoint 0..100 (default 50, higher = smaller), vignetteRoundness -100..100, vignetteFeather 0..100 (default 50), vignetteHighlights 0..100 (protects bright corners); \
grain 0..100 with grainSize 0..100 (default 25) and grainRoughness 0..100 (default 50). \
DETAIL: sharpen 0..150 (Lightroom applies 40 to RAWs), sharpenRadius 0.5..3 px (default 1), sharpenDetail 0..100 (default 25), sharpenMasking 0..100 (higher = edges only); \
noise 0..100 luminance noise reduction with noiseDetail (default 50) and noiseContrast; colorNoise 0..100 with colorNoiseDetail and colorNoiseSmoothness (default 50). Detail shows at full size more than in a small render. \
OPTICS: removeCA 1 removes lateral chromatic aberration; defringePurple, defringeGreen 0..20 take colour out of fringes at strong edges; distortion -100..100 (+ straightens barrel); lensVignette -100..100 (+ brightens dark lens corners) with lensVignetteMidpoint (default 50). \
CALIBRATION: calibration.shadowsTint -100..100 (+ magenta); calibration.<red|green|blue>.<hue|sat> -100..100 shift the camera primaries (blue sat +30 is a popular richer-colour trick). \
RETOUCH: skin (natural smoothing of every person's skin), eyes (brighter, crisper eyes) and teeth (whiter teeth), 0..100, found by local models. \
correction.exposure (stops) and correction.wbK (Kelvin) for the per-Photo baseline under a Look, and correction.face (stops of light on faces). \
crop.angle straightens in degrees (-45..45, + turns clockwise); crop.x, crop.y, crop.w, crop.h crop as fractions 0..1 of the \
straightened frame (never shows empty corners). \
For changes to one region (the sky, a face, a drawn area) use add_mask. Check the result with render_photo.";

const SET_LOOK_DOC: &str = "Create or tune one of a Shoot's Looks: a shared style (colour grade, contrast, mood) \
under each Photo's own sliders, so a set reads as one shoot. Tuning a Look changes every Photo under it at once. \
Sliders as in edit_recipe; for consistency across frames shot in different light, pair it with per-Photo \
correction.exposure and correction.wbK through edit_recipe. One undoable batch.";

const WHITE_BALANCE_DOC: &str = "Lightroom's white balance menu and eyedropper on one or many Photos, \
as one undoable batch: sets temp and tint (relative to as shot). preset as_shot (the camera's), auto (from the Photo's own light), \
or a Kelvin preset for RAW Photos: daylight 5500 K, cloudy 6500 K, shade 7500 K, tungsten 2850 K, fluorescent 3800 K, flash 5500 K. \
Or `at` {x, y}: the eyedropper, making that spot neutral grey (pick something that should be white or grey, not a highlight). \
Returns each Photo's temp and tint.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WhiteBalance {
    photo_ids: Vec<PhotoId>,
    preset: Option<String>,
    at: Option<At>,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Deserialize)]
struct At {
    x: f64,
    y: f64,
}

/// White balance ([`crate::white_balance`]): global temp and tint for a preset or an
/// eyedropper point, net of the Photo's Correction, so the Photo ends up there.
fn white_balance(lib: &mut Library, a: WhiteBalance, batch: Option<BatchId>) -> Result<Vec<Part>> {
    use crate::white_balance as wb;
    if a.photo_ids.is_empty() {
        return Err(Error("pass photo_ids".into()));
    }
    let mut next = Vec::new();
    for &id in &a.photo_ids {
        let photo = find_photo(lib, id)?;
        let own = current_recipe(lib, id)?;
        // The Correction's own white balance shift, in Temp steps.
        let corr = own.correction.wb_k / 25.0;
        let with = |temp: f64, tint: f64| {
            let mut r = own.clone();
            r.global.temp = temp;
            r.global.tint = tint;
            r
        };
        let render = |temp: f64, tint: f64| -> Result<crate::RgbImage> {
            let r = with_look(lib, &photo, &with(temp, tint))?;
            render::render_in(lib, &photo, &r, Some(crate::auto::MEASURE_EDGE))
        };
        let (temp, tint) = match (&a.at, a.preset.as_deref()) {
            (Some(p), None) => {
                wb::neutral_at(p.x, p.y, (own.global.temp, own.global.tint), render)?
            }
            (None, Some("as_shot")) => (-corr, 0.0),
            (None, Some("auto")) => {
                let (t, n) = crate::auto::white_balance(&render(-corr, 0.0)?)
                    .ok_or_else(|| Error(format!("Photo {id} has nothing to balance on")))?;
                (t - corr, n)
            }
            (None, Some(name)) => {
                if photo.kind != "raw" {
                    return Err(Error(format!(
                        "Photo {id} is a JPEG: Kelvin presets need a RAW; use as_shot, auto or the eyedropper"
                    )));
                }
                let raw = crate::decode(std::path::Path::new(&photo.path))?;
                let k = wb::as_shot_kelvin(raw.wb, raw.xyz_to_cam).ok_or_else(|| {
                    Error(format!(
                        "can't read the camera's white balance for Photo {id}"
                    ))
                })?;
                let (t, n) = wb::preset(name, k)?;
                (t - corr, n)
            }
            _ => return Err(Error("pass either preset or at".into())),
        };
        next.push((
            id,
            with(temp.clamp(-100.0, 100.0), tint.clamp(-100.0, 100.0)),
        ));
    }
    let out: BTreeMap<PhotoId, Value> = next
        .iter()
        .map(|(id, r)| (*id, json!({ "temp": r.global.temp, "tint": r.global.tint })))
        .collect();
    if a.dry_run {
        return one(json!({ "photos": out }));
    }
    let batch = batch_or(lib, batch, "White balance")?;
    for (id, r) in next {
        lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
    }
    one(json!({ "batch_id": batch, "photos": out }))
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
    let jobs: Vec<(Photo, Recipe, Recipe, _)> = photos
        .into_iter()
        .map(|p| {
            let own = current_recipe(lib, p.id)?;
            let base = with_look(lib, &p, &crate::auto::cleared(&own))?;
            let masks = render::masks_for(lib, &p, &base);
            Ok((p, own, base, masks))
        })
        .collect::<Result<_>>()?;
    let task =
        crate::progress::Task::start("auto", format!("Auto on {} Photos", jobs.len()), jobs.len());
    let pool = a.compute.unwrap_or_else(Compute::from_env).cpu_pool()?;
    let results: Vec<(PhotoId, Result<Recipe>)> = pool.install(|| {
        jobs.par_iter()
            .map(|(p, own, base, masks)| {
                let auto = crate::auto::auto_adjust(
                    |s| {
                        // The Look's sliders sit under Auto's, as they do under the person's.
                        let r = Recipe {
                            global: base.global.plus(s),
                            ..base.clone()
                        };
                        render::render_photo(p, &r, Some(crate::auto::MEASURE_EDGE), masks.clone())
                    },
                    p.kind == "raw",
                );
                task.tick();
                (p.id, auto.map(|s| crate::auto::applied(own, &s)))
            })
            .collect()
    });
    let batch = batch_or(lib, batch, "Auto")?;
    let mut out = BTreeMap::new();
    let mut failed = BTreeMap::new();
    for (id, r) in results {
        match r {
            Ok(r) => {
                let version = lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
                out.insert(id, json!({ "version": version, "global": r.global }));
            }
            Err(e) => {
                failed.insert(id, e.to_string());
            }
        }
    }
    one(json!({ "batch_id": batch, "photos": out, "failed": failed }))
}

fn batch_or(lib: &mut Library, batch: Option<BatchId>, label: &str) -> Result<BatchId> {
    match batch {
        Some(b) => Ok(b),
        None => lib.begin_batch(Author::Ai, label),
    }
}

/// What the renderer sees: the Recipe with its Look's sliders underneath its own. A Look
/// this Shoot doesn't have (one from the prototype's demo set) is ignored.
fn with_look(lib: &Library, photo: &Photo, recipe: &Recipe) -> Result<Recipe> {
    let mut r = recipe.clone();
    if let Some(id) = &recipe.look {
        if let Some(look) = lib.look(photo.shoot_id, id)? {
            r.global = look.adjust.plus(&recipe.global);
        }
    }
    Ok(r)
}

fn slug(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() {
        "look".into()
    } else {
        s
    }
}

/// Put Photos under a Look by saving a Recipe Version that names it.
fn assign_look(lib: &mut Library, ids: &[PhotoId], look: &str, batch: BatchId) -> Result<()> {
    for &id in ids {
        let mut r = current_recipe(lib, id)?;
        if r.look.as_deref() != Some(look) {
            r.look = Some(look.to_string());
            lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
        }
    }
    Ok(())
}

fn picks_or_all(lib: &Library, shoot: ShootId) -> Result<Vec<PhotoId>> {
    let photos = shoot_photos(lib, shoot)?;
    let picks: Vec<PhotoId> = photos
        .iter()
        .filter(|p| p.verdict == Verdict::Pick)
        .map(|p| p.id)
        .collect();
    Ok(if picks.is_empty() {
        photos.iter().map(|p| p.id).collect()
    } else {
        picks
    })
}

fn set_look(lib: &mut Library, a: SetLook, batch: Option<BatchId>) -> Result<Vec<Part>> {
    let photos = shoot_photos(lib, a.shoot_id)?;
    let looks = lib.looks(a.shoot_id)?;
    let existing = match &a.look_id {
        Some(id) => Some(looks.iter().find(|l| &l.id == id).cloned().ok_or_else(|| {
            Error(format!(
                "Shoot {} has no Look {id:?}; omit look_id to create one",
                a.shoot_id
            ))
        })?),
        None if a.name.is_none() && looks.len() == 1 => looks.first().cloned(),
        None => None,
    };
    let created = existing.is_none();
    let mut look = existing.unwrap_or_else(|| {
        let name = a.name.clone().unwrap_or_else(|| "Shoot Look".into());
        let mut id = slug(&name);
        while looks.iter().any(|l| l.id == id) {
            id = format!("{id}-2");
        }
        Look {
            id,
            name,
            adjust: Default::default(),
        }
    });
    if let (false, Some(name)) = (created, &a.name) {
        look.name = name.clone();
    }
    let mut tmp = Recipe {
        global: look.adjust.clone(),
        ..Recipe::default()
    };
    for (path, v) in &a.set {
        if path.starts_with("correction.") {
            return Err(Error(format!(
                "{path}: Corrections are per Photo; set them with edit_recipe"
            )));
        }
        tmp.set_value(path, v, a.relative)?;
    }
    look.adjust = tmp.global;
    let targets = match a.photo_ids {
        Some(ids) => {
            for id in &ids {
                if !photos.iter().any(|p| p.id == *id) {
                    return Err(Error(format!("Photo {id} is not in Shoot {}", a.shoot_id)));
                }
            }
            ids
        }
        None if created => picks_or_all(lib, a.shoot_id)?,
        None => Vec::new(),
    };
    let batch = batch_or(lib, batch, &format!("Look: {}", look.name))?;
    lib.save_look(a.shoot_id, &look, Some(batch))?;
    assign_look(lib, &targets, &look.id, batch)?;
    let under = photos_under(lib, a.shoot_id, &look.id)?;
    one(
        json!({ "batch_id": batch, "look": look, "created": created, "applied_to": targets, "photos_under_look": under }),
    )
}

fn photos_under(lib: &Library, shoot: ShootId, look: &str) -> Result<Vec<PhotoId>> {
    let mut out = Vec::new();
    for p in shoot_photos(lib, shoot)? {
        if current_recipe(lib, p.id)?.look.as_deref() == Some(look) {
            out.push(p.id);
        }
    }
    Ok(out)
}

fn sync_to_look(lib: &mut Library, a: SyncToLook, batch: Option<BatchId>) -> Result<Vec<Part>> {
    let photo = find_photo(lib, a.photo_id)?;
    let mut r = current_recipe(lib, photo.id)?;
    let looks = lib.looks(photo.shoot_id)?;
    let current = r
        .look
        .as_ref()
        .and_then(|id| looks.iter().find(|l| &l.id == id))
        .or(looks.first())
        .cloned();
    let created = current.is_none();
    let mut look = current.unwrap_or_else(|| Look {
        id: "shoot-look".into(),
        name: "Shoot Look".into(),
        adjust: Default::default(),
    });
    look.adjust = look.adjust.plus(&r.global).clamped();
    let batch = batch_or(lib, batch, &format!("Sync to {}", look.name))?;
    lib.save_look(photo.shoot_id, &look, Some(batch))?;
    r.global = Default::default();
    r.look = Some(look.id.clone());
    lib.write_recipe(photo.id, &r.to_value(), Author::Person, Some(batch))?;
    if created {
        let others: Vec<PhotoId> = picks_or_all(lib, photo.shoot_id)?
            .into_iter()
            .filter(|&id| id != photo.id)
            .collect();
        assign_look(lib, &others, &look.id, batch)?;
    }
    let under = photos_under(lib, photo.shoot_id, &look.id)?;
    one(json!({ "batch_id": batch, "look": look, "created": created, "photos_under_look": under }))
}

/// The Photo's current Recipe, or an empty one if it was never edited.
fn current_recipe(lib: &Library, id: PhotoId) -> Result<Recipe> {
    match lib.read_recipe(id)? {
        Some(v) => Recipe::from_value(&v.recipe),
        None => Ok(Recipe::default()),
    }
}

fn export_photos(lib: &Library, a: ExportPhotos) -> Result<Vec<Part>> {
    let opts = export::Options {
        folder: export::expand_home(&a.folder),
        shoot_folder: a.shoot_folder.unwrap_or(true),
        format: a.format.unwrap_or_default(),
        long_edge: a.long_edge,
        quality: a.quality.unwrap_or(90),
        naming: a.naming.unwrap_or_default(),
        sequence_start: a.sequence_start.unwrap_or(1).max(1),
    };
    let shoots: BTreeMap<ShootId, String> =
        lib.shoots()?.into_iter().map(|s| (s.id, s.name)).collect();
    let photos: Vec<(Photo, Recipe)> = a
        .photo_ids
        .iter()
        .map(|&id| {
            let photo = find_photo(lib, id)?;
            let recipe = with_look(lib, &photo, &current_recipe(lib, id)?)?;
            Ok((photo, recipe))
        })
        .collect::<Result<_>>()?;
    let total = a
        .sequence_total
        .unwrap_or(opts.sequence_start + photos.len() - 1);
    let (mut exported, mut failed) = (Vec::new(), Vec::new());
    for (i, (photo, recipe)) in photos.iter().enumerate() {
        let item = export::Item {
            photo,
            recipe,
            verdict: photo.verdict,
            shoot_name: shoots.get(&photo.shoot_id).map_or("", |s| s.as_str()),
            masks: render::masks_for(lib, photo, recipe),
        };
        // One Photo failing (its drive offline, say) doesn't stop the rest, as in Lightroom.
        match export::export(&item, &opts, opts.sequence_start + i, total) {
            Ok(x) => {
                if a.record.unwrap_or(true) {
                    lib.log_usage(
                        photo.id,
                        "export",
                        json!({ "recipe": recipe.to_value(), "path": x.path, "format": opts.format,
                                "long_edge": opts.long_edge, "quality": opts.quality,
                                "naming": opts.naming, "verdict": photo.verdict.as_str(),
                                "stars": photo.stars }),
                    );
                }
                exported.push(x)
            }
            Err(e) => failed.push(json!({ "photo_id": photo.id, "error": e.to_string() })),
        }
    }
    one(json!({ "exported": exported, "failed": failed }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalArg {
    photo_id: PhotoId,
    local_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddMask {
    photo_ids: Vec<PhotoId>,
    mask: Value,
    adjust: BTreeMap<String, f64>,
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditMask {
    photo_id: PhotoId,
    local_id: String,
    #[serde(default)]
    set: BTreeMap<String, f64>,
    #[serde(default)]
    relative: bool,
    mask: Option<Value>,
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewMask {
    photo_id: PhotoId,
    local_id: Option<String>,
    mask: Option<Value>,
    long_edge: Option<u32>,
    #[serde(rename = "as", default)]
    view: MaskView,
}

#[derive(Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
enum MaskView {
    #[default]
    Overlay,
    Weights,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListPhotos {
    shoot_id: Option<ShootId>,
    verdict: Option<Verdict>,
    min_stars: Option<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetRating {
    photo_ids: Vec<PhotoId>,
    stars: u8,
    #[serde(rename = "as")]
    author: Option<Author>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BestPhotos {
    shoot_id: Option<ShootId>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameShoot {
    shoot_id: ShootId,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportFolder {
    folder: String,
    shoot_name: Option<String>,
    files: Option<Vec<String>>,
    #[serde(default)]
    raw_only: bool,
    copy: Option<bool>,
    /// Queue an AI cull once the Photos are in.
    #[serde(default)]
    cull: bool,
    /// Queue edit Suggestions once the Photos are in (default: as `cull`).
    suggest: Option<bool>,
    /// Lightroom's Auto on every new Photo, as one batch of the person's (⌘Z undoes it).
    #[serde(default)]
    auto: bool,
    /// The model the queued cull and Suggestions use (default: ARGUS_PROVIDER, else Ollama).
    provider: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EstimateImport {
    folder: String,
    files: Option<Vec<String>>,
    copy: Option<bool>,
    #[serde(default)]
    raw_only: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsArgs {
    preview_long_edge: Option<u32>,
    preview_quality: Option<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScanFolder {
    folder: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetPreview {
    photo_id: PhotoId,
    long_edge: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetContactSheet {
    shoot_id: ShootId,
    page: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetVerdict {
    photo_ids: Vec<PhotoId>,
    verdict: Verdict,
    reason: Option<String>,
    #[serde(rename = "as")]
    author: Option<Author>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetStacks {
    shoot_id: ShootId,
    stacks: Vec<Vec<PhotoId>>,
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
struct CullShoot {
    shoot_id: Option<ShootId>,
    provider: Option<String>,
    compute: Option<Compute>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditRecipe {
    photo_ids: Vec<PhotoId>,
    set: BTreeMap<String, Value>,
    #[serde(default)]
    relative: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenderPhoto {
    photo_id: PhotoId,
    long_edge: Option<u32>,
    recipe: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportPhotos {
    photo_ids: Vec<PhotoId>,
    folder: String,
    format: Option<export::Format>,
    long_edge: Option<usize>,
    quality: Option<u8>,
    naming: Option<export::Naming>,
    shoot_folder: Option<bool>,
    sequence_start: Option<usize>,
    sequence_total: Option<usize>,
    /// False keeps this export out of the usage log (bench exports of the log itself).
    record: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StyleArgs {
    corrections: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UsageLogArgs {
    recording: Option<bool>,
    #[serde(default)]
    clear: bool,
    photo_id: Option<PhotoId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportUsageBench {
    folder: String,
    images: Option<bool>,
    long_edge: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UndoBatch {
    batch_id: Option<BatchId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetLook {
    shoot_id: ShootId,
    look_id: Option<String>,
    name: Option<String>,
    #[serde(default)]
    set: BTreeMap<String, Value>,
    #[serde(default)]
    relative: bool,
    photo_ids: Option<Vec<PhotoId>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnqueueJob {
    kind: String,
    #[serde(default)]
    args: Value,
    priority: Option<i64>,
    dedupe_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuggestEdits {
    photo_ids: Option<Vec<PhotoId>>,
    shoot_id: Option<ShootId>,
    count: Option<usize>,
    provider: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatchToPhoto {
    photo_id: PhotoId,
    photo_ids: Option<Vec<PhotoId>>,
    provider: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReorderJobs {
    job_ids: Vec<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListJobs {
    status: Option<String>,
    kind: Option<String>,
    limit: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobArg {
    job_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetQueue {
    compute: Option<Compute>,
    paused: Option<bool>,
    idle_boost_min: Option<u32>,
    overnight: Option<bool>,
    focus_first: Option<bool>,
    takes: Option<u8>,
    run_on_battery: Option<bool>,
    run_anyway: Option<bool>,
    idle_only: Option<bool>,
    run_now: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListSuggestions {
    photo_id: Option<PhotoId>,
    shoot_id: Option<ShootId>,
    all: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptSuggestion {
    suggestion_id: Option<i64>,
    suggestion_ids: Option<Vec<i64>>,
    amount: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuggestionArg {
    suggestion_id: i64,
    long_edge: Option<u32>,
    amount: Option<f64>,
}

/// The queue for the app's progress line: [`crate::library::QueueSummary`] plus its settings.
fn queue_status(lib: &Library) -> Result<Value> {
    let mut v = to_json(lib.queue_summary()?)?;
    let c = jobs::config(lib)?;
    v["compute"] = json!(c.compute);
    v["paused"] = json!(c.paused);
    v["idle_boost_min"] = json!(c.idle_boost_min);
    v["overnight"] = json!(c.overnight);
    v["overnight_hours"] = json!(jobs::OVERNIGHT);
    v["focus_first"] = json!(c.focus_first);
    v["takes"] = json!(c.takes);
    let battery = crate::power::on_battery();
    v["on_battery"] = json!(battery);
    v["run_on_battery"] = json!(c.run_on_battery);
    v["run_anyway"] = json!(c.battery_override);
    // Background work waits for the charger; the open Photo's work still runs.
    v["battery_paused"] = json!(jobs::held_on_battery(&c, battery));
    // Why it runs at Full above its own level right now, if it does: "idle" or "overnight".
    let boost = jobs::boost(&c);
    v["boost"] = json!(boost);
    v["idle_only"] = json!(c.idle_only);
    v["run_now"] = json!(c.go_ahead);
    // Model work (Suggestions, culls) waits for the person to step away.
    let held = jobs::held_kinds(&c, boost, jobs::model_backlog(lib)?);
    v["waiting_for_idle"] = json!(!held.is_empty());
    // Idle, but a cull or a big batch still waits for overnight or Run now.
    v["waiting_for_overnight"] = json!(held.contains(&"cull_shoot") && boost == Some("idle"));
    v["thermal_paused"] = json!(crate::power::thermal_serious());
    // Another Argus process (`argus cull`, a bench run) using the model right now.
    v["model_busy"] = json!(crate::model_lock::busy_elsewhere());
    // What the running job is doing right now, when this process runs it.
    if let Some(id) = v["running"]["id"].as_i64() {
        if let Some(t) = crate::progress::snapshot()
            .into_iter()
            .find(|t| t.job_id == Some(id))
        {
            v["running"]["stage"] = json!(t.label);
            v["running"]["waiting"] = json!(t.waiting);
        }
    }
    Ok(v)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SyncToLook {
    photo_id: PhotoId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveLook {
    shoot_id: ShootId,
    look_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteRecipe {
    photo_ids: Vec<PhotoId>,
    recipe: Value,
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
    let recipe = match &a.recipe {
        Some(v) => Recipe::from_value(v)?,
        None => current_recipe(lib, a.photo_id)?,
    };
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

fn find_photo(lib: &Library, id: PhotoId) -> Result<Photo> {
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

fn stacks(lib: &Library, shoot: ShootId) -> Result<Vec<Vec<PhotoId>>> {
    let mut by_stack: BTreeMap<i64, Vec<(i64, PhotoId)>> = BTreeMap::new();
    for p in shoot_photos(lib, shoot)? {
        if let Some(s) = p.stack {
            by_stack
                .entry(s)
                .or_default()
                .push((p.stack_rank.unwrap_or(i64::MAX), p.id));
        }
    }
    Ok(by_stack
        .into_values()
        .map(|mut m| {
            m.sort();
            m.into_iter().map(|(_, id)| id).collect()
        })
        .collect())
}

fn set_verdict(lib: &mut Library, a: SetVerdict, batch: Option<BatchId>) -> Result<Vec<Part>> {
    let author = a.author.unwrap_or(Author::Ai);
    let all = lib.list_photos(&PhotoFilter::default())?;
    let mut changed = Vec::new();
    let mut kept = Vec::new();
    for &id in &a.photo_ids {
        let p = all
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error(format!("no Photo with id {id}")))?;
        if author == Author::Ai && p.verdict_by == Some(Author::Person) {
            kept.push(id);
        } else {
            changed.push(id);
        }
    }
    let batch = match batch {
        Some(b) => b,
        None => lib.begin_batch(author, "set_verdict")?,
    };
    lib.set_verdict(
        &changed,
        a.verdict,
        author,
        a.reason.as_deref(),
        Some(batch),
    )?;
    one(json!({ "verdict": a.verdict, "changed": changed, "kept_person_verdicts": kept }))
}

fn set_rating(lib: &mut Library, a: SetRating, batch: Option<BatchId>) -> Result<Vec<Part>> {
    if a.stars > 5 {
        return Err(Error(format!("stars must be 0 to 5, not {}", a.stars)));
    }
    let author = a.author.unwrap_or(Author::Ai);
    let all = lib.list_photos(&PhotoFilter::default())?;
    let mut changed = Vec::new();
    let mut kept = Vec::new();
    for &id in &a.photo_ids {
        let p = all
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error(format!("no Photo with id {id}")))?;
        if author == Author::Ai && p.stars_by == Some(Author::Person) {
            kept.push(id);
        } else {
            changed.push(id);
        }
    }
    let batch = match batch {
        Some(b) => b,
        None => lib.begin_batch(author, "set_rating")?,
    };
    lib.set_rating(&changed, a.stars, author, Some(batch))?;
    one(json!({ "stars": a.stars, "changed": changed, "kept_person_ratings": kept }))
}

fn open_preview(p: &Photo) -> Result<image::DynamicImage> {
    image::open(&p.preview).map_err(|e| Error(format!("preview {}: {e}", p.preview.display())))
}

fn encode_jpeg(img: &image::RgbImage) -> Result<Vec<u8>> {
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

/// One page of a Shoot's Contact sheet: a grid JPEG with each cell numbered (1-based
/// position in capture order across the whole Shoot) and a legend for those numbers.
fn contact_sheet(lib: &Library, shoot: ShootId, page: usize) -> Result<Vec<Part>> {
    let photos = shoot_photos(lib, shoot)?;
    let pages = photos.len().div_ceil(SHEET_PER_PAGE).max(1);
    if page == 0 || page > pages {
        return Err(Error(format!("page {page} is out of range 1..={pages}")));
    }
    let first = (page - 1) * SHEET_PER_PAGE;
    let on_page: Vec<(usize, &Photo)> = photos
        .iter()
        .enumerate()
        .skip(first)
        .take(SHEET_PER_PAGE)
        .map(|(i, p)| (i + 1, p))
        .collect();

    let legend: Vec<Value> = on_page
        .iter()
        .map(|(n, p)| {
            json!({ "n": n, "photo_id": p.id, "file_name": p.file_name, "captured_at": p.captured_at,
                    "verdict": p.verdict, "verdict_by": p.verdict_by, "stars": p.stars, "stack": p.stack, "stack_rank": p.stack_rank, "best": p.best })
        })
        .collect();
    let info = json!({ "shoot_id": shoot, "page": page, "pages": pages,
                       "photos_in_shoot": photos.len(), "photos": legend });
    if on_page.is_empty() {
        return Ok(vec![Part::Json(info)]);
    }

    let thumbs = on_page
        .par_iter()
        .map(|(_, p)| {
            Ok(open_preview(p)?
                .thumbnail(SHEET_CELL - 8, SHEET_CELL - 8)
                .to_rgb8())
        })
        .collect::<Result<Vec<_>>>()?;
    let cols = SHEET_COLUMNS.min(on_page.len() as u32);
    let rows = (on_page.len() as u32).div_ceil(SHEET_COLUMNS);
    let mut sheet = image::RgbImage::from_pixel(
        cols * SHEET_CELL,
        rows * SHEET_CELL,
        image::Rgb([24, 24, 24]),
    );
    for (i, ((n, _), t)) in on_page.iter().zip(&thumbs).enumerate() {
        let (cx, cy) = (
            (i as u32 % SHEET_COLUMNS) * SHEET_CELL,
            (i as u32 / SHEET_COLUMNS) * SHEET_CELL,
        );
        let (x, y) = (
            cx + (SHEET_CELL - t.width()) / 2,
            cy + (SHEET_CELL - t.height()) / 2,
        );
        image::imageops::replace(&mut sheet, t, x as i64, y as i64);
        draw_number(&mut sheet, cx + 4, cy + 4, *n);
    }
    Ok(vec![Part::Json(info), Part::Jpeg(encode_jpeg(&sheet)?)])
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
fn draw_number(img: &mut image::RgbImage, x: u32, y: u32, n: usize) {
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

fn parse_mask(v: &Value) -> Result<Mask> {
    serde_json::from_value(v.clone()).map_err(|e| Error(format!("bad mask: {e}. {MASK_DOC}")))
}

/// Refuse a `faces` Mask that names a face the Photo doesn't have.
fn check_mask(lib: &Library, photo: &Photo, m: &Mask) -> Result<()> {
    if let Shape::Faces { face: Some(i) }
    | Shape::Person {
        person: Some(i), ..
    } = m.shape
    {
        let n = render::analysis(lib, photo)?.faces.len();
        if i >= n {
            return Err(Error(format!(
                "Photo {} has {n} face(s), numbered from 0 left to right; there is no face {i}",
                photo.id
            )));
        }
    }
    Ok(())
}

/// Slider deltas from edit_recipe-style paths (no `correction.` paths in a local).
fn local_adjust(
    base: &crate::recipe::Adjust,
    set: &BTreeMap<String, f64>,
    relative: bool,
) -> Result<crate::recipe::Adjust> {
    let mut tmp = Recipe {
        global: base.clone(),
        ..Default::default()
    };
    for (path, &v) in set {
        if path.starts_with("correction.") {
            return Err(Error(format!(
                "{path}: a local adjustment has sliders only, not the Correction"
            )));
        }
        tmp.set(path, v, relative)?;
    }
    Ok(tmp.global)
}

fn default_name(m: &Mask) -> String {
    let n = match m.shape {
        Shape::Faces { face: Some(i) } => return format!("Face {}", i + 1),
        Shape::Faces { face: None } => "Faces",
        Shape::Subject => "Subject",
        Shape::Background => "Background",
        Shape::Skin => "Skin",
        Shape::Hair => "Hair",
        Shape::Sky => "Sky",
        Shape::Person {
            person: Some(i), ..
        } => return format!("Person {}", i + 1),
        Shape::Person { person: None, .. } => "People",
        Shape::Brush { .. } => "Brush",
        Shape::Linear { .. } | Shape::Gradient { .. } => "Gradient",
        Shape::Radial { .. } => "Radial",
    };
    if m.invert {
        format!("Not {}", n.to_lowercase())
    } else {
        n.into()
    }
}

/// A local id not yet used in this Recipe, e.g. `sky-2`.
fn new_local_id(r: &Recipe, kind: &str) -> String {
    (1..)
        .map(|n| {
            if n == 1 {
                kind.to_string()
            } else {
                format!("{kind}-{n}")
            }
        })
        .find(|id| r.local.iter().all(|l| &l.id != id))
        .expect("an unused id")
}

fn add_mask(lib: &mut Library, a: AddMask, batch: Option<BatchId>) -> Result<Vec<Part>> {
    let mask = parse_mask(&a.mask)?;
    let adjust = local_adjust(&Default::default(), &a.adjust, false)?;
    let mut next = Vec::new();
    for &id in &a.photo_ids {
        let photo = find_photo(lib, id)?;
        check_mask(lib, &photo, &mask)?;
        let mut r = current_recipe(lib, id)?;
        let local_id = new_local_id(&r, mask.kind());
        r.local.push(Local {
            id: local_id.clone(),
            name: a.name.clone().unwrap_or_else(|| default_name(&mask)),
            mask: mask.clone(),
            adjust: adjust.clone(),
            source: Some("ai".into()),
            other: Default::default(),
        });
        next.push((id, local_id, r));
    }
    let batch = batch_or(lib, batch, "add_mask")?;
    let mut out = BTreeMap::new();
    for (id, local_id, r) in next {
        let version = lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
        out.insert(id, json!({ "version": version, "local_id": local_id }));
    }
    one(json!({ "batch_id": batch, "photos": out }))
}

fn edit_mask(lib: &mut Library, a: EditMask, batch: Option<BatchId>) -> Result<Vec<Part>> {
    let photo = find_photo(lib, a.photo_id)?;
    let mut r = current_recipe(lib, a.photo_id)?;
    let l = r.local_mut(&a.local_id)?;
    if let Some(m) = &a.mask {
        let m = parse_mask(m)?;
        check_mask(lib, &photo, &m)?;
        l.mask = m;
    }
    if let Some(n) = a.name {
        l.name = n;
    }
    l.adjust = local_adjust(&l.adjust, &a.set, a.relative)?;
    let local = l.clone();
    let batch = batch_or(lib, batch, "edit_mask")?;
    let version = lib.write_recipe(a.photo_id, &r.to_value(), Author::Ai, Some(batch))?;
    one(json!({ "batch_id": batch, "version": version, "local": local }))
}

fn mean(p: &[f32]) -> f64 {
    let s: f64 = p.par_iter().map(|&v| v as f64).sum();
    (s / p.len().max(1) as f64 * 1000.0).round() / 1000.0
}

fn analyze_photo(lib: &Library, photo: &Photo) -> Result<Vec<Part>> {
    use crate::analyzers::{self, eyes, faces, signals};
    let ids: Vec<&str> = analyzers::REGISTRY.iter().map(|a| a.id()).collect();
    let facts = analyzers::facts(lib, photo, &ids)?;
    let r3 = |v: f32| (v as f64 * 1000.0).round() / 1000.0;
    let reads = facts.get("eyes").map(eyes::read).unwrap_or_default();
    let face_list: Vec<Value> = facts
        .get("faces")
        .map(faces::read)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mut v = json!({ "face": i, "x": r3(f.x), "y": r3(f.y), "w": r3(f.w), "h": r3(f.h), "score": r3(f.score) });
            if let Some(e) = reads.iter().find(|e| e.face == i && e.read) {
                v["eyes_closed"] = json!(e.eyes_closed);
                v["blink"] = json!(e.blink);
                v["focus"] = json!(e.focus);
            }
            v
        })
        .collect();
    let seg = &facts["segment"].value["coverage"];
    let num = |v: &Value| v.as_f64().unwrap_or(0.0);
    let people = 1.0 - num(&seg["background"]);
    let skin = num(&seg["face_skin"]) + num(&seg["body_skin"]);
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    one(json!({
        "photo_id": photo.id,
        "faces": face_list,
        "coverage": { "people": round(people), "skin": round(skin),
                      "hair": seg["hair"], "sky": facts["sky"].value["coverage"] },
        "signals": facts.get("signals").map(signals::read),
        "summary": analyzers::caption(&facts),
    }))
}

fn view_mask(lib: &Library, a: ViewMask) -> Result<Vec<Part>> {
    let photo = find_photo(lib, a.photo_id)?;
    let recipe = current_recipe(lib, a.photo_id)?;
    let mask = match (&a.local_id, &a.mask) {
        (Some(id), None) => recipe
            .local
            .iter()
            .find(|l| &l.id == id)
            .map(|l| l.mask.clone())
            .ok_or_else(|| Error(format!("no local adjustment {id:?} on Photo {}", photo.id)))?,
        (None, Some(m)) => parse_mask(m)?,
        _ => return Err(Error("pass either local_id or mask".into())),
    };
    check_mask(lib, &photo, &mask)?;
    let edge = a.long_edge.unwrap_or(1024).clamp(64, MAX_PREVIEW_EDGE) as usize;
    // Masks live on the whole frame, so look at it uncropped and crop the view last.
    let whole = Recipe {
        crop: Default::default(),
        ..recipe.clone()
    };
    let img = render::render_in(lib, &photo, &whole, Some(edge))?;
    let guide: Vec<f32> = img
        .data
        .chunks(3)
        .map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0)
        .collect();
    let analysis = if mask.needs_analysis() {
        Some(render::analysis(lib, &photo)?)
    } else {
        None
    };
    let canvas = Canvas {
        width: img.width,
        height: img.height,
        guide: &guide,
        analysis: analysis.as_deref(),
    };
    let w = canvas.plane(&mask)?;
    let out: Vec<u8> = if a.view == MaskView::Weights {
        w.iter()
            .flat_map(|&k| [(k * 255.0 + 0.5) as u8; 3])
            .collect()
    } else {
        img.data
            .chunks(3)
            .zip(&w)
            .flat_map(|(p, &k)| {
                let k = k * 0.55;
                let tint = [255.0, 40.0, 40.0];
                std::array::from_fn::<u8, 3, _>(|c| (p[c] as f32 * (1.0 - k) + tint[c] * k) as u8)
            })
            .collect()
    };
    let view = crate::geometry::apply(
        crate::RgbImage {
            width: img.width,
            height: img.height,
            data: out,
        },
        &recipe.crop,
    );
    Ok(vec![
        Part::Json(
            json!({ "photo_id": photo.id, "kind": mask.kind(), "coverage": mean(&w),
            "width": view.width, "height": view.height }),
        ),
        Part::Jpeg(render::encode_jpeg(&view, 85)?),
    ])
}
