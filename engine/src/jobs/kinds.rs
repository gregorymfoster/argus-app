//! What each job kind is: how it is queued ([`enqueue`]: label, dedupe key, priority)
//! and what running it does.

use super::*;

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
        "learn_edits" => (
            "Learning from your edits".to_string(),
            "learn_edits".to_string(),
            PRIORITY_OTHER,
        ),
        "rank_presets" => (
            shoot("Ranking Presets"),
            format!("rank_presets:{}", id("shoot_id").unwrap_or(0)),
            PRIORITY_OTHER,
        ),
        "tag_photos" => (
            shoot("Keywords and captions"),
            format!(
                "tag_photos:{}:{}",
                id("shoot_id").unwrap_or(0),
                args.get("photo_ids")
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            ),
            PRIORITY_OTHER,
        ),
        "denoise" => {
            let p = id("photo_id").ok_or_else(|| Error("denoise needs photo_id".into()))?;
            (
                format!("Denoise {}", name(Some(p))),
                format!("denoise:{p}"),
                PRIORITY_OTHER,
            )
        }
        "pre_edit_shoot" => (
            shoot("Shoot match"),
            format!(
                "pre_edit_shoot:{}{}",
                id("shoot_id").unwrap_or(0),
                if args.get("every").and_then(|v| v.as_bool()) == Some(true) {
                    ":every"
                } else {
                    ""
                }
            ),
            PRIORITY_MATCH,
        ),
        "download_model" => (
            "Downloading search by meaning".to_string(),
            "download_model".to_string(),
            PRIORITY_MATCH,
        ),
        "send_photos" => {
            let to = args.get("to").and_then(Value::as_str).unwrap_or("");
            let app = match to {
                "apple_photos" => "Apple Photos",
                "google_photos" => "Google Photos",
                _ => {
                    return Err(Error(
                        "send_photos needs to: apple_photos or google_photos".into(),
                    ))
                }
            };
            let paths = args.get("paths").and_then(Value::as_array);
            let n = paths.map_or(0, |p| p.len());
            if n == 0 {
                return Err(Error("send_photos needs paths".into()));
            }
            let first = paths
                .and_then(|p| p.first())
                .and_then(Value::as_str)
                .unwrap_or("");
            (
                shoot(&format!("Sending {n} to {app}")),
                format!("send_photos:{to}:{n}:{first}"),
                PRIORITY_FOCUS,
            )
        }
        "resume_run" => {
            let run = id("run_id").ok_or_else(|| Error("resume_run needs run_id".into()))?;
            let label = lib
                .run(run)
                .map(|r| r.label)
                .unwrap_or_else(|_| "an Export".into());
            (
                format!("{label} · resumed"),
                format!("resume_run:{run}"),
                // The person started it: ahead of the AI work, and on battery too.
                PRIORITY_FOCUS,
            )
        }
        "backup" => {
            let place = args
                .get("place")
                .and_then(Value::as_str)
                .ok_or_else(|| Error("backup needs place".into()))?
                .to_string();
            let name = lib
                .backup_place(&place)
                .map(|p| p.name)
                .unwrap_or_else(|_| place.clone());
            let verify = args.get("verify").and_then(Value::as_bool) == Some(true);
            (
                shoot(&if verify {
                    format!("Checking the backup on {name}")
                } else {
                    format!("Backing up to {name}")
                }),
                format!(
                    "backup:{place}:{}:{verify}",
                    id("shoot_id").map_or("all".into(), |s| s.to_string())
                ),
                PRIORITY_OTHER,
            )
        }
        "render_previews" => (
            shoot("Previews"),
            format!("render_previews:{}", id("shoot_id").unwrap_or(0)),
            PRIORITY_PREVIEWS,
        ),
        _ => (
            shoot("AI cull"),
            format!(
                "{}:{}",
                if kind == "check_shoot" {
                    kind
                } else {
                    "cull_shoot"
                },
                id("shoot_id").unwrap_or(0)
            ),
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

pub(super) fn execute(tools: &Tools, job: &Job, compute: Compute) -> Result<Value> {
    match job.kind.as_str() {
        "suggest" => {
            let mut a: SuggestArgs = args(job)?;
            if a.count.is_none() && a.reference_id.is_none() {
                a.count = Some(config(&Library::open(tools.library())?)?.takes as usize);
            }
            let mut chat = tools.chat(a.provider.as_deref(), Some(compute), suggest::schema())?;
            chat.cap_output(suggest::MAX_REPLY_TOKENS);
            chat.prefer_models(crate::providers::SUGGEST_OLLAMA_MODELS);
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
        "resume_run" => super::resume::resume_run(tools, job),
        "match_hero" => match_hero(tools, job),
        "pre_edit_shoot" => {
            #[derive(Deserialize)]
            struct A {
                shoot_id: ShootId,
                #[serde(default)]
                every: bool,
            }
            let a: A = args(job)?;
            let mut lib = Library::open(tools.library())?;
            let progress = Library::open(tools.library())?;
            let id = job.id;
            let every = a.every || crate::pre_edit::locked(&lib, a.shoot_id)?;
            let report =
                crate::pre_edit::run(&mut lib, a.shoot_id, every, Some(id), &|done, total| {
                    let _ = progress.job_progress(id, done as i64, total as i64);
                })?;
            serde_json::to_value(report).map_err(|e| Error(format!("pre-edit: {e}")))
        }
        "cull_shoot" | "check_shoot" => {
            #[derive(Deserialize)]
            struct A {
                shoot_id: Option<ShootId>,
                provider: Option<String>,
                compute: Option<Compute>,
                check: Option<crate::cull::Check>,
            }
            let a: A = args(job)?;
            let mut lib = Library::open(tools.library())?;
            let shoot = match a.shoot_id {
                Some(s) => s,
                None => newest_shoot(&lib)?,
            };
            let check = match job.kind.as_str() {
                "check_shoot" => Some(crate::cull::Check::Rules),
                _ => a.check,
            };
            let provider = crate::cull::cull_provider(check, a.provider.as_deref(), |name| {
                tools.provider(name, Some(a.compute.unwrap_or(compute)))
            })?;
            let progress = std::sync::Mutex::new(Library::open(tools.library())?);
            let id = job.id;
            // In the Shoot's Photos, read and answered, so the bar and its ETA move
            // inside a long request.
            let report = crate::cull::cull_shoot_with(
                &mut lib,
                shoot,
                provider.as_ref(),
                crate::cull::max_per_request(),
                &|p| {
                    crate::progress::label_here(&p.stage());
                    crate::progress::waiting_here(p.waiting.as_deref());
                    if let Ok(l) = progress.lock() {
                        let (done, total) = p.bar();
                        let _ = l.job_progress(id, done as i64, total as i64);
                    }
                },
            )?;
            serde_json::to_value(report).map_err(|e| Error(format!("cull: {e}")))
        }
        "send_photos" => send_photos(tools, job),
        "backup" => {
            #[derive(Deserialize)]
            struct A {
                place: String,
                shoot_id: Option<ShootId>,
                #[serde(default)]
                verify: bool,
                why: Option<String>,
            }
            let a: A = args(job)?;
            let lib = Library::open(tools.library())?;
            let progress = Library::open(tools.library())?;
            let id = job.id;
            const MB: u64 = 1 << 20;
            let why = a.why.clone().unwrap_or_else(|| {
                if a.verify {
                    "Monthly check".into()
                } else if a.shoot_id.is_some() {
                    "After Import".into()
                } else {
                    "Backup".into()
                }
            });
            let report = lib.run_backup(
                &a.place,
                a.shoot_id,
                a.verify,
                &why,
                Some(BACKUP_SLICE),
                &|done, total| {
                    let _ =
                        progress.job_progress(id, (done / MB) as i64, total.div_ceil(MB) as i64);
                },
            )?;
            if !report.done {
                // The rest goes back on the Queue, behind whatever is waiting.
                let mut lib = lib;
                let mut next = job.args.clone();
                next["why"] = json!(why);
                enqueue(
                    &mut lib,
                    "backup",
                    &next,
                    Some(job.priority),
                    Some(&format!("backup-rest:{}", job.id)),
                )?;
            }
            serde_json::to_value(report).map_err(|e| Error(format!("backup: {e}")))
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
        "learn_edits" => {
            let lib = Library::open(tools.library())?;
            let l = crate::edit_head::learn_library(&lib)?;
            serde_json::to_value(l).map_err(|e| Error(format!("learn_edits: {e}")))
        }
        "rank_presets" => {
            #[derive(Deserialize)]
            struct A {
                shoot_id: ShootId,
                provider: Option<String>,
            }
            let a: A = args(job)?;
            let lib = Library::open(tools.library())?;
            let progress = Library::open(tools.library())?;
            let id = job.id;
            let r = crate::preset_rank::rank(
                &lib,
                a.shoot_id,
                a.provider.as_deref(),
                compute,
                &|done, total| {
                    let _ = progress.job_progress(id, done as i64, total as i64);
                },
            )?;
            Ok(
                json!({ "top": r.ranked.iter().take(6).map(|x| &x.id).collect::<Vec<_>>(), "photos": r.photos }),
            )
        }
        "download_model" => {
            let progress = Library::open(tools.library())?;
            let id = job.id;
            const MB: u64 = 1 << 20;
            crate::meaning::download(&|done, total| {
                let _ = progress.job_progress(id, (done / MB) as i64, total.div_ceil(MB) as i64);
            })?;
            // Every Photo needs its vector before search can find it by meaning.
            let mut lib = Library::open(tools.library())?;
            crate::analyzers::enqueue(&mut lib, None, None)?;
            Ok(json!({ "installed": crate::meaning::available(), "folder": crate::meaning::dir() }))
        }
        "tag_photos" => {
            #[derive(Deserialize)]
            struct A {
                shoot_id: Option<ShootId>,
                photo_ids: Option<Vec<PhotoId>>,
                provider: Option<String>,
                #[serde(default)]
                redo: bool,
            }
            let a: A = args(job)?;
            let photos = crate::tagging::photos(
                &Library::open(tools.library())?,
                a.shoot_id,
                a.photo_ids.as_deref(),
                a.redo,
            )?;
            let chat = tools.chat(
                a.provider.as_deref(),
                Some(compute),
                crate::tagging::schema(),
            )?;
            // Keywords stay on the default model (its MLX build when installed): on 20
            // Halloween Photos gemma4:26b-a4b gave 44 vague Keywords to qwen3.8:27b's 129.
            let progress = Library::open(tools.library())?;
            let id = job.id;
            let n =
                crate::tagging::tag(tools.library(), chat.as_ref(), &photos, &|done, total| {
                    let _ = progress.job_progress(id, done as i64, total as i64);
                })?;
            Ok(json!({ "tagged": n, "photos": photos.len() }))
        }
        "denoise" => {
            #[derive(Deserialize)]
            struct A {
                photo_id: PhotoId,
                #[serde(default)]
                precompute: bool,
                /// Prepare's: for a likely keeper, Pick or not yet.
                #[serde(default)]
                keeper: bool,
            }
            let a: A = args(job)?;
            let lib = Library::open(tools.library())?;
            let photo = lib
                .photo(a.photo_id)?
                .ok_or_else(|| Error(format!("no Photo {}", a.photo_id)))?;
            let recipe = lib
                .read_recipe(a.photo_id)?
                .map(|v| Recipe::from_value(&v.recipe))
                .transpose()?
                .unwrap_or_default();
            if crate::denoise::ready(&photo).is_some() {
                return Ok(json!({ "photo_id": a.photo_id, "skipped": "already made" }));
            }
            // A Pick's precompute: only for a frame noisy enough to want Denoise, still a Pick.
            // Otherwise: undone or turned off since it was queued, nothing to do.
            if a.precompute && !crate::denoise::wants_precompute(&lib, &photo, &recipe, a.keeper)? {
                return Ok(
                    json!({ "photo_id": a.photo_id, "skipped": "clean frame or no longer a Pick" }),
                );
            }
            if a.precompute
                && job.priority < PRIORITY_FOCUS
                && !crate::denoise::room_to_precompute(tools.library())
            {
                return Ok(json!({ "photo_id": a.photo_id, "skipped": "denoise cache full" }));
            }
            if !a.precompute && crate::denoise::amount(&recipe) == 0.0 {
                return Ok(json!({ "photo_id": a.photo_id, "skipped": "Denoise is off" }));
            }
            let focused = crate::denoise::focused(&lib)?
                .and_then(|f| lib.photo(f).ok().flatten())
                .and_then(|p| crate::denoise::base_file(&p));
            let started = std::time::Instant::now();
            let id = job.id;
            let progress = std::sync::Mutex::new(lib);
            compute.cpu_pool()?.install(|| {
                crate::denoise::compute(&photo, &|done, total| {
                    if let Ok(l) = progress.lock() {
                        let _ = l.job_progress(id, done as i64, total as i64);
                    }
                })
            })?;
            // Stay under the cache cap: the new base and the open Photo's stay.
            let keep: Vec<_> = crate::denoise::base_file(&photo)
                .into_iter()
                .chain(focused)
                .collect();
            let dropped = crate::denoise::evict(tools.library(), crate::denoise::CACHE_CAP, &keep)?;
            Ok(
                json!({ "photo_id": a.photo_id, "seconds": started.elapsed().as_secs(), "evicted": dropped }),
            )
        }
        other => Err(Error(format!("unknown job kind {other:?}"))),
    }
}

pub(super) fn args<T: serde::de::DeserializeOwned>(job: &Job) -> Result<T> {
    serde_json::from_value(job.args.clone())
        .map_err(|e| Error(format!("{} job {}: bad args ({e})", job.kind, job.id)))
}

/// Send exported files to Apple Photos or Google Photos, [`SEND_SLICE`] at a time.
fn send_photos(tools: &Tools, job: &Job) -> Result<Value> {
    #[derive(Deserialize)]
    struct A {
        to: String,
        paths: Vec<PathBuf>,
        album: Option<String>,
    }
    let a: A = args(job)?;
    let lib = Library::open(tools.library())?;
    let progress = Library::open(tools.library())?;
    let id = job.id;
    let until = std::time::Instant::now() + SEND_SLICE;
    let total = a.paths.len();
    let _ = progress.job_progress(id, 0, total as i64);
    let (report, left) = match a.to.as_str() {
        "google_photos" => {
            let c = crate::google_photos::Config::from_env();
            let (sent, left) = crate::google_photos::send(
                &c,
                &lib,
                &a.paths,
                a.album.as_deref(),
                Some(until),
                &|done, total| {
                    let _ = progress.job_progress(id, done as i64, total as i64);
                },
            )?;
            if sent.sent == 0 && !sent.failed.is_empty() {
                return Err(Error(format!(
                    "Google Photos took none of the files: {}",
                    sent.failed[0].1
                )));
            }
            (serde_json::to_value(&sent).unwrap_or_default(), left)
        }
        "apple_photos" => {
            // Photos imports a few files per AppleScript call, so the bar moves.
            const CHUNK: usize = 20;
            let mut done = 0;
            let mut left = Vec::new();
            for chunk in a.paths.chunks(CHUNK) {
                if done > 0 && std::time::Instant::now() > until {
                    left = a.paths[done..].to_vec();
                    break;
                }
                crate::export::add_to_apple_photos(chunk, a.album.as_deref())?;
                done += chunk.len();
                let _ = progress.job_progress(id, done as i64, total as i64);
            }
            (json!({ "sent": done, "album": a.album }), left)
        }
        other => return Err(Error(format!("send_photos: unknown app {other:?}"))),
    };
    if !left.is_empty() {
        let mut lib = lib;
        let mut next = job.args.clone();
        next["paths"] = json!(left);
        enqueue(
            &mut lib,
            "send_photos",
            &next,
            Some(job.priority),
            Some(&format!("send-rest:{}", job.id)),
        )?;
    }
    Ok(report)
}
