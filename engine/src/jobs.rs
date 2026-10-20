//! The background queue: AI work that runs while the person is away (Suggestions for a
//! whole Shoot, "apply to the rest", culls). Jobs live in the Library, so a quit or crash
//! loses nothing: the next start puts running jobs back in the queue.
//!
//! Anyone enqueues (the Agent, the post-import pipeline, the person); one [`Worker`] per
//! process works through them, highest priority first. Between jobs it pauses by the
//! queue's Compute level so a Gentle queue leaves the Mac to the person, and it goes to
//! Full on its own while the Mac has been idle a while.

use crate::compute::Compute;
use crate::library::{Job, JobId, JobStatus, Library, PhotoId, ShootId, Verdict};
use crate::power;
use crate::recipe::Recipe;
use crate::suggest::{self, SuggestArgs};
use crate::tools::Tools;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Kinds of job, and what their `args` are.
/// - `suggest` {photo_id, count?, reference_id?, look_id?, provider?}: Suggestions for one Photo
/// - `suggest_shoot` {shoot_id?, photo_ids?, count?, provider?, redo?}: a `suggest` per Photo,
///   rejects skipped, Picks first (and Photos that already have Suggestions skipped unless `redo`)
/// - `match_hero` {photo_id, photo_ids?, provider?}: the hero Photo's style becomes a Look and
///   every other Photo (or `photo_ids`) gets matched Suggestions under it
/// - `cull_shoot` (alias `rate_shoot`) {shoot_id?, provider?}: the AI cull
/// - `render_previews` {shoot_id?}: Argus's own previews over the camera's, queued by `import_folder`
/// - `analyze` {shoot_id?, photo_ids?}: the specialist models' Facts (ADR 0013), queued after Import
pub const KINDS: &[&str] = &[
    "analyze",
    "suggest",
    "suggest_shoot",
    "match_hero",
    "cull_shoot",
    "rate_shoot",
    "render_previews",
];

/// Priorities: what the person asked for right now goes first.
pub const PRIORITY_MATCH: i64 = 40;
pub const PRIORITY_PICK: i64 = 20;
pub const PRIORITY_OTHER: i64 = 10;
/// A whole-Shoot fan-out runs before the per-Photo jobs it makes are queued.
pub const PRIORITY_SHOOT: i64 = 50;
/// The specialist models' Facts, right after previews: the cull and Suggestions read them.
pub const PRIORITY_ANALYZE: i64 = 55;
/// Previews first of all: the person is looking at the grid.
pub const PRIORITY_PREVIEWS: i64 = 60;
/// The Photo open in Loupe or Edit: ahead of everything, and run at Full (Greg,
/// 2026-10-18: AI work on the Photo he's on goes before any batch).
pub const PRIORITY_FOCUS: i64 = 100;
/// The next few Photos in the filmstrip, so they're ready when he gets there.
pub const PRIORITY_PREFETCH: i64 = 70;

/// The person opened `photo_id` (and `next` follow it in the filmstrip): their queued
/// Suggestions jump ahead of background work. Returns how many jobs moved up.
pub fn focus(lib: &mut Library, photo_id: PhotoId, next: &[PhotoId]) -> Result<usize> {
    if !config(lib)?.focus_first {
        return Ok(0);
    }
    let mut n = 0;
    let targets = std::iter::once((photo_id, PRIORITY_FOCUS))
        .chain(next.iter().take(5).map(|&p| (p, PRIORITY_PREFETCH)));
    for (p, priority) in targets {
        for flavour in ["fresh", "match"] {
            n += lib.raise_job(&format!("suggest:{p}:{flavour}"), priority)? as usize;
        }
    }
    Ok(n)
}

/// How a Photo's Suggestions are coming along, for the Edit strip: the queued or running
/// `suggest` job (fresh takes first, else matched ones) with its stage, how many takes it
/// makes, what runs before it and when its takes should land. `{"job": null}` when none.
pub fn suggestions_progress(lib: &Library, photo_id: PhotoId) -> Result<Value> {
    let fresh = lib.active_job(&format!("suggest:{photo_id}:fresh"))?;
    let matched = lib.active_job(&format!("suggest:{photo_id}:match"))?;
    let job = match (fresh, matched) {
        (Some(f), Some(m)) if m.status == JobStatus::Running && f.status != JobStatus::Running => m,
        (Some(f), _) => f,
        (None, Some(m)) => m,
        (None, None) => return Ok(json!({ "job": null })),
    };
    let is_match = job.args.get("reference_id").is_some_and(|v| !v.is_null());
    let takes = job
        .args
        .get("count")
        .and_then(Value::as_i64)
        .unwrap_or(if is_match {
            2
        } else {
            config(lib)?.takes as i64
        });
    let avg = lib.average_job_seconds("suggest")?;
    let elapsed = lib.job_elapsed_seconds(job.id)?;
    // Seconds a job has left: its kind's recent average less what it has run, or for a job
    // that counts its own items, its pace so far.
    let left = |j: &Job| -> Result<Option<f64>> {
        let ran = lib.job_elapsed_seconds(j.id)?.unwrap_or(0.0);
        if j.kind != "suggest" && j.done > 0 && j.total > j.done {
            return Ok(Some(ran * (j.total - j.done) as f64 / j.done as f64));
        }
        Ok(lib
            .average_job_seconds(&j.kind)?
            .map(|a| (a - ran).max(if ran > 0.0 { 5.0 } else { 0.0 })))
    };
    let mut out = json!({
        "id": job.id,
        "status": job.status,
        "label": job.label,
        "matched": is_match,
        "takes": takes,
        "focus": job.priority >= PRIORITY_FOCUS,
        "avg_s": avg.map(f64::round),
    });
    if job.status == JobStatus::Running {
        let ran = elapsed.unwrap_or(0.0);
        let by_steps = if job.total > 0 {
            job.done as f64 / job.total as f64
        } else {
            0.0
        };
        let by_time = avg.map_or(0.0, |a| (ran / a.max(1.0)).min(0.95));
        out["stage"] = json!(suggest::stage(job.done, job.total));
        out["step"] = json!(job.done);
        out["steps"] = json!(job.total);
        out["elapsed_s"] = json!(ran.round());
        out["progress"] = json!((by_steps.max(by_time) * 100.0).round() / 100.0);
        out["eta_s"] = json!(left(&job)?.map(f64::round));
    } else {
        let ahead = lib.jobs_ahead(job.id)?;
        let mut wait = Some(0.0);
        for j in &ahead {
            wait = match (wait, left(j)?) {
                (Some(w), Some(l)) => Some(w + l),
                _ => None,
            };
        }
        out["stage"] = json!("Queued");
        out["ahead"] = json!(ahead.len());
        out["behind"] = json!(ahead.first().map(|j| j.label.clone()));
        out["wait_s"] = json!(wait.map(f64::round));
        out["eta_s"] = json!(wait.zip(avg).map(|(w, a)| (w + a).round()));
    }
    Ok(json!({ "job": out }))
}

/// The queue's own settings, kept in the Library.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueConfig {
    pub compute: Compute,
    /// Stop taking new jobs (the one running finishes).
    pub paused: bool,
    /// Go to Full while the Mac has been idle this many minutes (0 = never).
    pub idle_boost_min: u32,
    /// Go to Full overnight, between [`OVERNIGHT`] local hours.
    pub overnight: bool,
    /// The Photo open in Loupe or Edit jumps the queue (`focus_photo`).
    pub focus_first: bool,
    /// Suggestions per Photo when a job doesn't say.
    pub takes: u8,
    /// Run background work on battery too. Off: on battery only the open Photo's work runs
    /// (Greg, 2026-10-18).
    pub run_on_battery: bool,
    /// "Run anyway" from the Background work panel: run on battery until the next time the
    /// Mac is plugged in.
    pub battery_override: bool,
    /// Background model work (Suggestions, culls) waits until the Mac is idle and plugged
    /// in, or overnight (Greg, 2026-10-19, after an overnight cull fought the CLI). The
    /// open Photo's work still runs.
    pub idle_only: bool,
    /// "Run now" from the Background work panel: run model work while the person works,
    /// until the queue is empty.
    pub go_ahead: bool,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            compute: Compute::from_env(),
            paused: false,
            idle_boost_min: 3,
            overnight: true,
            focus_first: true,
            takes: 3,
            run_on_battery: false,
            battery_override: false,
            idle_only: true,
            go_ahead: false,
        }
    }
}

/// The overnight window, in local hours: from 1:00 up to 7:00.
pub const OVERNIGHT: (u32, u32) = (1, 7);

const CONFIG_KEY: &str = "queue";

pub fn config(lib: &Library) -> Result<QueueConfig> {
    Ok(lib
        .setting(CONFIG_KEY)?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

pub fn set_config(lib: &Library, c: &QueueConfig) -> Result<()> {
    lib.set_setting(
        CONFIG_KEY,
        &serde_json::to_value(c).expect("config serializes"),
    )
}

/// Queue a job of a known kind. `dedupe_key` defaults to one per kind and target, so
/// asking twice doesn't do the work twice. Returns (job id, newly queued).
pub fn enqueue(
    lib: &mut Library,
    kind: &str,
    args: &Value,
    priority: Option<i64>,
    dedupe_key: Option<&str>,
) -> Result<(JobId, bool)> {
    if !KINDS.contains(&kind) {
        return Err(Error(format!(
            "unknown job kind {kind:?}; use one of {}",
            KINDS.join(", ")
        )));
    }
    let args = if args.is_null() {
        json!({})
    } else {
        args.clone()
    };
    let id = |k: &str| args.get(k).and_then(Value::as_i64);
    let name = |p: Option<i64>| -> String {
        p.and_then(|p| lib.photo(p).ok().flatten())
            .map(|p| p.file_name)
            .unwrap_or_else(|| "a Photo".into())
    };
    // "AI cull · 2026-10-10_Boat-Tour": which Shoot, so the queue reads at a glance.
    let shoot = |what: &str| -> String {
        let named = id("shoot_id").and_then(|s| {
            lib.shoots()
                .ok()?
                .into_iter()
                .find(|x| x.id == s)
                .map(|x| x.name)
        });
        match named {
            Some(n) => format!("{what} · {n}"),
            None => what.to_string(),
        }
    };
    let (label, key, default_priority) = match kind {
        "suggest" => {
            let p = id("photo_id").ok_or_else(|| Error("suggest needs photo_id".into()))?;
            let matched = id("reference_id").is_some();
            (
                format!(
                    "{} for {}",
                    if matched { "Matching" } else { "Suggestions" },
                    name(Some(p))
                ),
                format!("suggest:{p}:{}", if matched { "match" } else { "fresh" }),
                if matched {
                    PRIORITY_MATCH
                } else {
                    PRIORITY_OTHER
                },
            )
        }
        "suggest_shoot" => (
            shoot("Suggestions"),
            format!("suggest_shoot:{}", id("shoot_id").unwrap_or(0)),
            PRIORITY_SHOOT,
        ),
        "match_hero" => {
            let p = id("photo_id").ok_or_else(|| Error("match_hero needs photo_id".into()))?;
            (
                format!("Apply {}'s look to the rest", name(Some(p))),
                format!("match_hero:{p}"),
                PRIORITY_SHOOT,
            )
        }
        "analyze" => (
            shoot("Analysing photos"),
            format!("analyze:{}", id("shoot_id").unwrap_or(0)),
            PRIORITY_ANALYZE,
        ),
        "render_previews" => (
            shoot("Previews"),
            format!("render_previews:{}", id("shoot_id").unwrap_or(0)),
            PRIORITY_PREVIEWS,
        ),
        _ => (
            shoot("AI cull"),
            format!("cull_shoot:{}", id("shoot_id").unwrap_or(0)),
            PRIORITY_SHOOT,
        ),
    };
    let kind = if kind == "rate_shoot" {
        "cull_shoot"
    } else {
        kind
    };
    lib.enqueue_job(
        kind,
        &args,
        priority.unwrap_or(default_priority),
        &label,
        Some(dedupe_key.unwrap_or(&key)),
    )
}

/// Claim and run the next job, if there is one. Returns it, finished.
pub fn run_next(tools: &Tools, compute: Compute) -> Result<Option<Job>> {
    run_next_at_least(tools, compute, i64::MIN)
}

/// [`run_next`], only taking a job of at least `min_priority`.
pub fn run_next_at_least(
    tools: &Tools,
    compute: Compute,
    min_priority: i64,
) -> Result<Option<Job>> {
    run_next_holding(tools, compute, min_priority, &[])
}

/// [`run_next_at_least`], passing over jobs of the `held` kinds unless the person waits
/// on them (the open Photo's).
pub fn run_next_holding(
    tools: &Tools,
    compute: Compute,
    min_priority: i64,
    held: &[&str],
) -> Result<Option<Job>> {
    let mut lib = Library::open(tools.library())?;
    let Some(job) = lib.claim_job_holding(min_priority, held, PRIORITY_FOCUS)? else {
        return Ok(None);
    };
    drop(lib);
    // What the queue shows for it now (`queue_status`), and what another Argus process
    // waiting on the model sees.
    let task = crate::progress::Task::for_job(job.id, job.label.clone());
    let _in = task.enter();
    let _who = crate::model_lock::as_who(format!("{} in {}", job.label, crate::model_lock::who()));
    // The person is waiting on this one.
    let compute = if job.priority >= PRIORITY_FOCUS {
        Compute::Full
    } else {
        compute
    };
    let used_before = crate::meter::totals();
    let span = crate::debug_log::span(
        "job",
        serde_json::json!({
            "job_id": job.id,
            "job_kind": job.kind,
            "label": job.label,
            "priority": job.priority,
            "compute": format!("{compute:?}").to_lowercase(),
            "args": job.args,
        }),
    );
    let outcome = execute(tools, &job, compute);
    if let Err(e) = &outcome {
        crate::logs::note(&format!("job {} ({}) failed: {e}", job.id, job.label));
    }
    span.end(serde_json::json!({
        "ok": outcome.is_ok(),
        "error": outcome.as_ref().err().map(|e| e.0.clone()),
        "used": crate::meter::totals().since(&used_before).to_json(),
    }));
    let lib = Library::open(tools.library())?;
    match &outcome {
        Ok(v) => lib.finish_job(job.id, Ok(v))?,
        Err(e) => lib.finish_job(job.id, Err(&e.0))?,
    }
    Ok(Some(lib.job(job.id)?))
}

/// Run every queued job, whatever the queue's holds (tests and `argus work --once --drain`). Returns how many ran.
pub fn drain(tools: &Tools, compute: Compute) -> Result<usize> {
    let mut n = 0;
    while run_next(tools, compute)?.is_some() {
        n += 1;
    }
    Ok(n)
}

fn execute(tools: &Tools, job: &Job, compute: Compute) -> Result<Value> {
    match job.kind.as_str() {
        "suggest" => {
            let mut a: SuggestArgs = args(job)?;
            if a.count.is_none() && a.reference_id.is_none() {
                a.count = Some(config(&Library::open(tools.library())?)?.takes as usize);
            }
            let chat = tools.chat(a.provider.as_deref(), Some(compute), suggest::schema())?;
            let ids = suggest::suggest_photo(
                tools,
                chat.as_ref(),
                &a,
                Some(job.id),
                suggest::rounds(compute),
            )?;
            Ok(json!({ "photo_id": a.photo_id, "suggestion_ids": ids }))
        }
        "suggest_shoot" => suggest_shoot(tools, job),
        "match_hero" => match_hero(tools, job),
        "cull_shoot" => {
            #[derive(Deserialize)]
            struct A {
                shoot_id: Option<ShootId>,
                provider: Option<String>,
                compute: Option<Compute>,
            }
            let a: A = args(job)?;
            let mut lib = Library::open(tools.library())?;
            let shoot = match a.shoot_id {
                Some(s) => s,
                None => newest_shoot(&lib)?,
            };
            let provider =
                tools.provider(a.provider.as_deref(), Some(a.compute.unwrap_or(compute)))?;
            let progress = std::sync::Mutex::new(Library::open(tools.library())?);
            let id = job.id;
            // In Photos answered, so the bar and its ETA move inside a long request.
            let report = crate::cull::cull_shoot_with(
                &mut lib,
                shoot,
                provider.as_ref(),
                crate::cull::max_per_request(),
                &|p| {
                    crate::progress::label_here(&p.stage());
                    crate::progress::waiting_here(p.waiting.as_deref());
                    if let Ok(l) = progress.lock() {
                        let _ = l.job_progress(id, p.photos as i64, p.photos_total as i64);
                    }
                },
            )?;
            serde_json::to_value(report).map_err(|e| Error(format!("cull: {e}")))
        }
        "render_previews" => {
            #[derive(Deserialize)]
            struct A {
                shoot_id: Option<ShootId>,
            }
            let a: A = args(job)?;
            let lib = Library::open(tools.library())?;
            let progress = std::sync::Mutex::new(Library::open(tools.library())?);
            let id = job.id;
            let report = lib.render_previews(a.shoot_id, compute, &|done, total| {
                if let Ok(l) = progress.lock() {
                    let _ = l.job_progress(id, done as i64, total as i64);
                }
            })?;
            serde_json::to_value(report).map_err(|e| Error(format!("previews: {e}")))
        }
        "analyze" => {
            #[derive(Deserialize)]
            struct A {
                shoot_id: Option<ShootId>,
                photo_ids: Option<Vec<PhotoId>>,
            }
            let a: A = args(job)?;
            let lib = Library::open(tools.library())?;
            let photos: Vec<_> = lib
                .list_photos(&crate::library::PhotoFilter {
                    shoot: a.shoot_id,
                    verdict: None,
                })?
                .into_iter()
                .filter(|p| a.photo_ids.as_ref().is_none_or(|ids| ids.contains(&p.id)))
                .collect();
            let progress = Library::open(tools.library())?;
            let id = job.id;
            let ids = crate::analyzers::background_ids(compute);
            crate::analyzers::facts_with(&lib, &photos, &ids, Some(compute), &|done, total| {
                let _ = progress.job_progress(id, done as i64, total as i64);
            })?;
            // Stack the bursts now, so the grid shows them before any cull. A Shoot that
            // already has Stacks (a cull's, or the person's) keeps them.
            let mut lib = lib;
            let mut stacked = 0;
            let shoots: std::collections::BTreeSet<_> = photos.iter().map(|p| p.shoot_id).collect();
            for shoot in shoots {
                let all = lib.list_photos(&crate::library::PhotoFilter {
                    shoot: Some(shoot),
                    verdict: None,
                })?;
                if all.iter().all(|p| p.stack.is_none()) {
                    stacked += crate::bursts::stack_shoot(&mut lib, shoot, false)?.len();
                }
            }
            Ok(json!({ "photos": photos.len(), "analyzers": ids, "stacks": stacked }))
        }
        other => Err(Error(format!("unknown job kind {other:?}"))),
    }
}

fn args<T: serde::de::DeserializeOwned>(job: &Job) -> Result<T> {
    serde_json::from_value(job.args.clone())
        .map_err(|e| Error(format!("{} job {}: bad args ({e})", job.kind, job.id)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShootArgs {
    shoot_id: Option<ShootId>,
    photo_ids: Option<Vec<PhotoId>>,
    count: Option<usize>,
    provider: Option<String>,
    #[serde(default)]
    redo: bool,
}

/// The Photos of a Shoot worth editing, best first: the AI's Best Photos in rank order,
/// then Picks, then by stars, then capture order. Rejects are left out.
fn worth_editing(lib: &Library, shoot: ShootId) -> Result<Vec<(PhotoId, bool)>> {
    let mut photos: Vec<_> = lib
        .list_photos(&crate::library::PhotoFilter {
            shoot: Some(shoot),
            verdict: None,
        })?
        .into_iter()
        .filter(|p| p.verdict != Verdict::Reject)
        .collect();
    // A stable sort keeps capture order among equals.
    photos.sort_by_key(|p| {
        (
            p.best.unwrap_or(i64::MAX),
            p.verdict != Verdict::Pick,
            std::cmp::Reverse(p.stars),
            p.stack_rank.unwrap_or(0) > 0,
        )
    });
    Ok(photos
        .into_iter()
        .map(|p| (p.id, p.verdict == Verdict::Pick))
        .collect())
}

fn newest_shoot(lib: &Library) -> Result<ShootId> {
    lib.shoots()?
        .iter()
        .map(|s| s.id)
        .max()
        .ok_or_else(|| Error("the Library has no Shoots yet".into()))
}

fn suggest_shoot(tools: &Tools, job: &Job) -> Result<Value> {
    let a: ShootArgs = args(job)?;
    let mut lib = Library::open(tools.library())?;
    let targets: Vec<(PhotoId, bool)> = match a.photo_ids {
        Some(ids) => ids.into_iter().map(|id| (id, false)).collect(),
        None => {
            let shoot = match a.shoot_id {
                Some(s) => s,
                None => newest_shoot(&lib)?,
            };
            worth_editing(&lib, shoot)?
        }
    };
    let mut queued = Vec::new();
    let mut skipped = 0;
    let total = targets.len() as i64;
    for (i, (photo, pick)) in targets.into_iter().enumerate() {
        let has = lib
            .suggestions(Some(photo), None, false)?
            .iter()
            .any(|s| s.reference_id.is_none());
        if has && !a.redo {
            skipped += 1;
            continue;
        }
        let mut args = json!({ "photo_id": photo });
        if let Some(c) = a.count {
            args["count"] = json!(c);
        }
        if let Some(p) = &a.provider {
            args["provider"] = json!(p);
        }
        let priority = if pick { PRIORITY_PICK } else { PRIORITY_OTHER };
        let (id, _) = enqueue(&mut lib, "suggest", &args, Some(priority), None)?;
        queued.push(id);
        lib.job_progress(job.id, i as i64 + 1, total)?;
    }
    Ok(json!({ "queued": queued, "skipped": skipped }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeroArgs {
    photo_id: PhotoId,
    photo_ids: Option<Vec<PhotoId>>,
    provider: Option<String>,
}

/// "Apply to the rest": the hero's whole style (its Look and its own sliders) becomes a
/// new Look of the Shoot; every target gets matched Suggestions under it. Nothing changes
/// until the person accepts them.
fn match_hero(tools: &Tools, job: &Job) -> Result<Value> {
    let a: HeroArgs = args(job)?;
    let mut lib = Library::open(tools.library())?;
    let hero = lib
        .photo(a.photo_id)?
        .ok_or_else(|| Error(format!("no Photo with id {}", a.photo_id)))?;
    let recipe = match lib.read_recipe(hero.id)? {
        Some(v) => Recipe::from_value(&v.recipe)?,
        None => Recipe::default(),
    };
    let mut style = recipe.global.clone();
    if let Some(id) = &recipe.look {
        if let Some(look) = lib.look(hero.shoot_id, id)? {
            style = look.adjust.plus(&recipe.global);
        }
    }
    let stem = hero
        .file_name
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(&hero.file_name)
        .to_string();
    let looks = lib.looks(hero.shoot_id)?;
    let base_id: String = format!("from-{stem}")
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let mut look_id = base_id.clone();
    let mut n = 2;
    while looks.iter().any(|l| l.id == look_id) {
        look_id = format!("{base_id}-{n}");
        n += 1;
    }
    let look = crate::library::Look {
        id: look_id.clone(),
        name: format!("Look from {stem}"),
        adjust: style,
    };
    lib.save_look(hero.shoot_id, &look, None)?;

    let targets: Vec<PhotoId> = match a.photo_ids {
        Some(ids) => ids,
        None => worth_editing(&lib, hero.shoot_id)?
            .into_iter()
            .map(|(id, _)| id)
            .collect(),
    };
    let mut queued = Vec::new();
    for id in targets.into_iter().filter(|&id| id != hero.id) {
        let mut args =
            json!({ "photo_id": id, "reference_id": hero.id, "look_id": look_id, "count": 2 });
        if let Some(p) = &a.provider {
            args["provider"] = json!(p);
        }
        // A newer hero replaces an older one's queued matching for the same Photo.
        let key = format!("suggest:{id}:match");
        if let Some(old) = lib
            .jobs(
                Some(crate::library::JobStatus::Queued),
                Some("suggest"),
                10_000,
            )?
            .into_iter()
            .find(|j| j.args.get("reference_id").is_some() && j.args["photo_id"] == json!(id))
        {
            lib.cancel_job(old.id)?;
        }
        let (jid, _) = enqueue(&mut lib, "suggest", &args, Some(PRIORITY_MATCH), Some(&key))?;
        queued.push(jid);
    }
    Ok(json!({ "look_id": look_id, "queued": queued }))
}

/// Seconds the person hasn't touched the keyboard or mouse (macOS), or None.
pub fn idle_seconds() -> Option<f64> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let out = std::process::Command::new("ioreg")
        .args(["-c", "IOHIDSystem", "-d", "4"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains("\"HIDIdleTime\""))?;
    let ns: f64 = line.rsplit('=').next()?.trim().parse().ok()?;
    Some(ns / 1e9)
}

/// The local hour of day, 0-23.
fn local_hour() -> u32 {
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&now, &mut tm) };
    tm.tm_hour as u32
}

/// Why the queue runs at Full beyond its own level right now: "idle" or "overnight".
/// Never on battery.
pub fn boost(c: &QueueConfig) -> Option<&'static str> {
    if power::on_battery() {
        return None;
    }
    boost_at(c, idle_seconds(), local_hour())
}

/// The kinds that run the big local model, which wait for an idle Mac ([`QueueConfig::idle_only`]).
pub const MODEL_KINDS: &[&str] = &["suggest", "suggest_shoot", "match_hero", "cull_shoot"];

/// Long model jobs: a whole-Shoot cull, or a Shoot's Suggestions fanning out.
const LONG_KINDS: &[&str] = &["suggest_shoot", "cull_shoot"];

/// The most per-Photo model jobs (a few minutes each on a local model) the idle boost runs
/// back to back; a bigger backlog is a long run and waits for overnight, like a cull.
pub const IDLE_BATCH: usize = 3;

/// Whether model work waits for the person to step away: idle_only, not overridden, and
/// neither idle long enough nor overnight (`boost` is [`boost`]'s answer).
pub fn waits_for_idle(c: &QueueConfig, boost: Option<&str>) -> bool {
    c.idle_only && !c.go_ahead && boost.is_none()
}

/// The job kinds the queue passes over now (the open Photo's always run). With idle_only,
/// model work waits for the person to step away, and long runs (a cull, a Shoot's
/// Suggestions, more than [`IDLE_BATCH`] Photos' worth) wait for overnight or Run now: a
/// few idle minutes are no promise of hours (Greg, 2026-10-19: heavy runs overnight or on
/// his go). `backlog` is how many per-Photo model jobs are queued.
pub fn held_kinds(c: &QueueConfig, boost: Option<&str>, backlog: usize) -> &'static [&'static str] {
    if !c.idle_only || c.go_ahead || boost == Some("overnight") {
        &[]
    } else if boost == Some("idle") && backlog <= IDLE_BATCH {
        LONG_KINDS
    } else {
        MODEL_KINDS
    }
}

/// Per-Photo model jobs queued below the open Photo's priority.
pub fn model_backlog(lib: &Library) -> Result<usize> {
    let mut n = 0;
    for kind in ["suggest", "match_hero"] {
        n += lib
            .jobs(Some(crate::library::JobStatus::Queued), Some(kind), 10_000)?
            .iter()
            .filter(|j| j.priority < PRIORITY_FOCUS)
            .count();
    }
    Ok(n)
}

/// Background work waits for the charger: on battery, unless the person said otherwise,
/// only the open Photo's jobs run.
pub fn held_on_battery(c: &QueueConfig, on_battery: bool) -> bool {
    on_battery && !c.run_on_battery && !c.battery_override
}

fn boost_at(c: &QueueConfig, idle_s: Option<f64>, hour: u32) -> Option<&'static str> {
    if c.overnight && (OVERNIGHT.0..OVERNIGHT.1).contains(&hour) {
        return Some("overnight");
    }
    match idle_s {
        Some(s) if c.idle_boost_min > 0 && s >= c.idle_boost_min as f64 * 60.0 => Some("idle"),
        _ => None,
    }
}

/// The Compute level to run the next job at: the queue's, or Full while the Mac is idle
/// or overnight.
pub fn effective_compute(c: &QueueConfig) -> Compute {
    match boost(c) {
        Some(_) => Compute::Full,
        None => c.compute,
    }
}

/// Rest between jobs, by Compute level: a Gentle queue leaves gaps for the person.
fn rest(c: Compute) -> Duration {
    match c {
        Compute::Gentle => Duration::from_secs(5),
        Compute::Balanced => Duration::from_secs(2),
        Compute::Full => Duration::ZERO,
    }
}

/// A background thread working through the queue until dropped.
/// Whether the queue may start a job now and, if so, how it picks one. The app's worker and
/// `argus work` both go by it, so a shell run keeps the app's holds (battery, heat, idle and
/// overnight waits, pause).
pub enum Gate {
    /// Start nothing: why.
    Wait(&'static str),
    Run {
        compute: Compute,
        /// On battery or with the Mac running hot, only the open Photo's work.
        min_priority: i64,
        /// Kinds passed over now ([`held_kinds`]).
        held: &'static [&'static str],
    },
}

pub fn gate(lib: &Library, cfg: &QueueConfig) -> Gate {
    if cfg.paused {
        return Gate::Wait("the queue is paused");
    }
    // An Agent request or a cull (here or in another Argus process, such as `argus cull`)
    // has the model; start nothing new until it's done.
    if crate::compute::interactive() || crate::compute::foreground() {
        return Gate::Wait("the model is busy");
    }
    let min_priority = if held_on_battery(cfg, power::on_battery()) || power::thermal_serious() {
        PRIORITY_FOCUS
    } else {
        i64::MIN
    };
    let why_full = boost(cfg);
    let backlog = model_backlog(lib).unwrap_or(usize::MAX);
    Gate::Run {
        compute: why_full.map_or(cfg.compute, |_| Compute::Full),
        min_priority,
        held: held_kinds(cfg, why_full, backlog),
    }
}

pub struct Worker {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    /// Work on whichever Library `current` names (it can change, as when the person opens
    /// another one), with tools made by `make` so tests and the app can swap Providers.
    pub fn start(
        current: impl Fn() -> Option<PathBuf> + Send + 'static,
        make: impl Fn(PathBuf) -> Tools + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = std::thread::Builder::new()
            .name("argus-queue".into())
            .spawn(move || {
                // Parked between rounds; `stop` unparks it, so an idle queue wakes once per
                // round instead of five times a second.
                let sleep = |d: Duration| {
                    let end = std::time::Instant::now() + d;
                    while !flag.load(Ordering::Relaxed) {
                        let left = end.saturating_duration_since(std::time::Instant::now());
                        if left.is_zero() {
                            break;
                        }
                        std::thread::park_timeout(left);
                    }
                };
                let mut open: Option<(PathBuf, Tools)> = None;
                while !flag.load(Ordering::Relaxed) {
                    let Some(library) = current() else {
                        sleep(Duration::from_secs(2));
                        continue;
                    };
                    if open.as_ref().map(|(p, _)| p) != Some(&library) {
                        // A job a quit left running starts again.
                        if let Ok(lib) = Library::open(&library) {
                            let _ = lib.requeue_stale_jobs();
                        }
                        open = Some((library.clone(), make(library.clone())));
                    }
                    let tools = &open.as_ref().expect("set above").1;
                    let Ok(lib) = Library::open(&library) else {
                        sleep(Duration::from_secs(2));
                        continue;
                    };
                    let cfg = config(&lib).unwrap_or_default();
                    if cfg.battery_override && !power::on_battery() {
                        // Plugged in again: the next unplug waits for the charger as usual.
                        let _ = set_config(
                            &lib,
                            &QueueConfig {
                                battery_override: false,
                                ..cfg
                            },
                        );
                    }
                    // Nothing queued (the usual idle state): no power, heat or model checks.
                    if !lib.has_queued_jobs().unwrap_or(true) {
                        // "Run now" lasts until the queue is empty.
                        if cfg.go_ahead && lib.queue_summary().is_ok_and(|q| q.running.is_none()) {
                            let _ = set_config(
                                &lib,
                                &QueueConfig {
                                    go_ahead: false,
                                    ..cfg
                                },
                            );
                        }
                        drop(lib);
                        sleep(Duration::from_secs(2));
                        continue;
                    }
                    let Gate::Run {
                        compute,
                        min_priority,
                        held,
                    } = gate(&lib, &cfg)
                    else {
                        drop(lib);
                        sleep(Duration::from_secs(2));
                        continue;
                    };
                    drop(lib);
                    match run_next_holding(tools, compute, min_priority, held) {
                        Ok(Some(_)) => sleep(rest(compute)),
                        Ok(None) => sleep(Duration::from_secs(2)),
                        Err(e) => {
                            crate::logs::note(&format!("argus queue: {e}"));
                            sleep(Duration::from_secs(10));
                        }
                    }
                }
            })
            .expect("spawn queue worker");
        Worker {
            stop,
            handle: Some(handle),
        }
    }

    /// Stop after the current job.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            h.thread().unpark();
            let _ = h.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = &self.handle {
            h.thread().unpark();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_model_runs_wait_for_overnight_even_when_idle() {
        let c = QueueConfig::default();
        assert_eq!(
            held_kinds(&c, None, 0),
            MODEL_KINDS,
            "nothing while the person works"
        );
        assert_eq!(
            held_kinds(&c, Some("idle"), IDLE_BATCH),
            LONG_KINDS,
            "a few Photos at idle"
        );
        assert_eq!(
            held_kinds(&c, Some("idle"), 95),
            MODEL_KINDS,
            "a big batch waits"
        );
        assert!(held_kinds(&c, Some("overnight"), 95).is_empty());
        assert!(held_kinds(
            &QueueConfig {
                go_ahead: true,
                ..c
            },
            None,
            95
        )
        .is_empty());
        assert!(held_kinds(
            &QueueConfig {
                idle_only: false,
                ..c
            },
            None,
            95
        )
        .is_empty());
    }

    #[test]
    fn full_overnight_and_when_idle() {
        let c = QueueConfig {
            compute: Compute::Gentle,
            ..QueueConfig::default()
        };
        assert_eq!(boost_at(&c, None, 3), Some("overnight"));
        assert_eq!(boost_at(&c, None, 7), None);
        assert_eq!(boost_at(&c, Some(600.0), 14), Some("idle"));
        assert_eq!(boost_at(&c, Some(30.0), 14), None);
        let off = QueueConfig {
            overnight: false,
            idle_boost_min: 0,
            ..c
        };
        assert_eq!(boost_at(&off, Some(1e6), 3), None);
    }

    #[test]
    fn an_old_config_reads_with_the_new_defaults() {
        let c: QueueConfig = serde_json::from_value(
            json!({ "compute": "full", "paused": true, "idle_boost_min": 5 }),
        )
        .unwrap();
        assert!(c.overnight && c.focus_first && c.paused);
        assert!(!c.run_on_battery && !c.battery_override);
    }

    #[test]
    fn battery_holds_background_work_unless_told() {
        let c = QueueConfig::default();
        assert!(held_on_battery(&c, true));
        assert!(!held_on_battery(&c, false));
        let anyway = QueueConfig {
            battery_override: true,
            ..c
        };
        assert!(!held_on_battery(&anyway, true));
        let always = QueueConfig {
            run_on_battery: true,
            ..c
        };
        assert!(!held_on_battery(&always, true));
        assert_eq!(c.takes, 3);
    }
}
