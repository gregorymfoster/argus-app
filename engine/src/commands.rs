//! Commands: what the UI calls, by name with JSON arguments, over any transport.
//!
//! `argus serve` (HTTP) and the Tauri app (`invoke`) both call [`dispatch`]. Every tool
//! in [`crate::tools`] is a command under its own name (`list_shoots`, `import_folder`,
//! `cull_shoot`, ...), so the UI, the Agent and MCP share one implementation. A few
//! commands exist only for the UI, because the person, not a model, is acting, and one
//! tells the UI which AI Providers there are:
//!
//! | command     | args                                      | returns                       |
//! |-------------|-------------------------------------------|-------------------------------|
//! | `providers` | {}                                        | {default, providers: [{id, label, model, note, down?}]}; `down` says why one can't run now (Ollama not running) |
//! | `models`    | {}                                        | {model, pinned, models: [{name, bytes, loaded}], error?} |
//! | `set_model` | {model}                                   | {model}                       |
//! | `set_api_key` | {key}                                   | {ok} (macOS Keychain; offers `claude-api` in `providers`) |
//! | `photos` | {shoot_id?, verdict?}                        | [Photo] with `preview`, and `edit`: a stamp of the current Recipe (null if never edited) |
//! | `eye_flags` | {shoot_id?}                               | [{id, closed, soft}]: stored eye Facts only, never computes; Photos with neither left out |
//! | `flag`   | {ids, verdict, author = "person", reason?, label?, batch_id?} | {flagged, batch_id, photos: [CullState]} |
//! | `rate`   | {ids, stars (0-5), author = "person", label?, batch_id?} | {rated, batch_id, photos: [CullState]} |
//! | `photo_states` | {ids}                                | [CullState]                   |
//! | `denoise_status` | {photo_id}                         | {raw, ready, job: {status, done, total, eta_s} or null}: AI Denoise's base for the Edit panel |
//! | `as_person` | {tool, args, label}                     | the tool's result plus `batch_id` |
//! | `edit`   | {photo_id, recipe, label?}                   | {version, batch_id}           |
//! | `thumbnail`    | {path, long_edge = 320}                | {images: [data URL]}          |
//! | `edited_previews` | {photo_ids, long_edge}              | {"<id>": Library-relative JPEG of the Photo with its edits, or null}; cached on disk by what it shows |
//! | `suggest_name` | {paths? \| shoot_id?, captured_at?, provider?} | {name, by, note}       |
//! | `shoot_name`   | {title, captured_at?}                  | {shoot_name, originals}       |
//! | `reveal`       | {shoot_id? \| photo_id? \| path?}      | {path, shown}                 |
//! | `agent`  | {prompt, shoot_id?, photo_id?, history?, provider?} | [`crate::agent::Reply`] |
//! | `progress` | {}                                      | `queue_status` plus `tasks` (scan, import) |
//! | `focus_photo` | {photo_id, next_ids?}                | {raised}                      |
//! | `render_previews` | {shoot_id?, compute?}           | [`crate::library::PreviewReport`] |
//! | `pending_previews` | {shoot_id?}                    | {pending}                     |
//! | `start_import` | as `import_folder`                     | {started: true}               |
//! | `import_result` | {}                                    | {running, report?, error?}    |
//! | `render_detail` | as `render_photo`, long_edge up to full size | as `render_photo`: the zoomed view's sharp render |
//! | `cancel_import` | {}                                    | {cancelled}: stops after the files in hand, keeping them |
//!
//! `thumbnail` and `suggest_name` serve the Import dialog, for files not imported yet;
//! `suggest_name` with a `shoot_id` names a Shoot already in the Library from its previews.
//! `shoot_name` turns what the person typed into the Shoot name and originals folder
//! Argus will use (`2026-10-18_Boat-Trip`), by the rules in [`crate::naming`].
//! `reveal` shows a Shoot's originals folder, one Photo's original, or a folder Export
//! wrote (`path`, which must be an existing folder), in Finder
//! (`open -R`; elsewhere the folder opens in the file manager). `shown` is false when no
//! file manager could be started; `path` says what it would have shown.
//! `render_previews` replaces the camera previews Import leaves on RAWs with Argus's own
//! render (`import_folder` also starts it in the background); `pending_previews` counts
//! what's left.
//!
//! `start_import` runs `import_folder` on its own thread and returns at once, so the app
//! can open the new Shoot (its Photos land as they're copied) without a request in flight
//! across a page load (Tauri resends one the page dropped, which imported twice).
//! `import_result` says how the last one ended. One Import runs at a time.
//!
//! `agent` runs the Agent's tool-use loop ([`crate::agent`]): a request in plain words,
//! carried out through the same tools, as one undoable batch (`undo_batch`).
//!
//! A tool that returns images (`render_photo`, `get_preview`, `get_contact_sheet`) comes
//! back as its JSON object plus `images`: the JPEGs as `data:` URLs, so they travel over
//! any transport that carries JSON.
//!
//! The Library is the only source of truth for Verdicts and stars. A person's `flag` or
//! `rate` is its own batch (`batch_id`), answered with the Photos' [`CullState`] as the
//! Library now has them; an `edit` (one slider drag, one Mask) is a batch too. The UI's
//! Undo is `undo_batch` on that batch, its Redo is
//! `undo_batch` on the Undo's batch, and `photo_states` re-reads Photos after either.
//!
//! [`CullState`]: crate::library::CullState
//!
//! `flag` and `rate` write exactly what they are told, including `author: "ai"` so Undo can put an AI
//! Verdict back (the `set_verdict` tool never overrules the person). A Photo's `preview`
//! is relative to the Library root (`previews/ab/cdef.jpg`), so each transport can serve
//! it its own way.

use crate::cull::{ClaudeCode, Ollama};
use crate::library::{Author, Library, PhotoFilter, PhotoId, ShootId, Verdict};
use crate::tools::{Part, Tools};
use crate::{Error, Result};
use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn person() -> Author {
    Author::Person
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PhotosArgs {
    shoot_id: Option<ShootId>,
    verdict: Option<Verdict>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveArgs {
    #[serde(default)]
    photo_ids: Vec<i64>,
    #[serde(default)]
    shoot_ids: Vec<ShootId>,
    #[serde(default)]
    delete_originals: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemovalArgs {
    removal: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FlagArgs {
    ids: Vec<i64>,
    verdict: Verdict,
    #[serde(default = "person")]
    author: Author,
    reason: Option<String>,
    label: Option<String>,
    batch_id: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RateArgs {
    ids: Vec<i64>,
    stars: u8,
    #[serde(default = "person")]
    author: Author,
    label: Option<String>,
    batch_id: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FocusArgs {
    photo_id: crate::library::PhotoId,
    #[serde(default)]
    next_ids: Vec<crate::library::PhotoId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AsPersonArgs {
    tool: String,
    #[serde(default)]
    args: Value,
    label: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatesArgs {
    ids: Vec<i64>,
}

/// A person's flag or star change is its own batch, so the UI's Undo is `undo_batch` on
/// it (and Redo is `undo_batch` on the Undo). AI writes stay out of batches here: the
/// Agent's "undo the last change" looks for the newest AI batch. `join` adds to a batch
/// the UI already opened, so "keep this one, reject the rest" undoes in one step.
fn person_batch(
    lib: &mut Library,
    author: Author,
    join: Option<i64>,
    label: Option<String>,
    default: String,
) -> Result<Option<i64>> {
    if author != Author::Person {
        return Ok(None);
    }
    match join {
        Some(batch) => Ok(Some(batch)),
        None => lib.begin_batch(author, &label.unwrap_or(default)).map(Some),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewArgs {
    shoot_id: Option<ShootId>,
    compute: Option<crate::compute::Compute>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditedPreviewArgs {
    photo_ids: Vec<PhotoId>,
    long_edge: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThumbnailArgs {
    path: String,
    long_edge: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuggestNameArgs {
    #[serde(default)]
    paths: Vec<String>,
    shoot_id: Option<ShootId>,
    captured_at: Option<String>,
    provider: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevealArgs {
    shoot_id: Option<ShootId>,
    photo_id: Option<i64>,
    /// A folder, such as the one an Export wrote to.
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShootNameArgs {
    title: String,
    captured_at: Option<String>,
}

/// Show `path` in Finder: selected when it's a file, opened when it's a folder.
fn reveal(path: &Path, is_file: bool) -> bool {
    if std::env::var_os("ARGUS_NO_REVEAL").is_some() {
        return false;
    }
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        if is_file {
            c.arg("-R");
        }
        c.arg(path);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(if is_file {
            path.parent().unwrap_or(path)
        } else {
            path
        });
        c
    };
    cmd.spawn().is_ok()
}

fn data_url(jpeg: &[u8]) -> Value {
    Value::String(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    photo_id: i64,
    recipe: Value,
    label: Option<String>,
}

fn args<T: DeserializeOwned>(command: &str, v: Value) -> Result<T> {
    let v = if v.is_null() { json!({}) } else { v };
    serde_json::from_value(v).map_err(|e| Error(format!("{command}: bad arguments: {e}")))
}

/// Run one command. Errors are plain messages for the UI.
/// The last `start_import`: whether one is running, and how the last one ended.
static IMPORT: std::sync::Mutex<(bool, Option<std::result::Result<Value, String>>)> =
    std::sync::Mutex::new((false, None));

/// Polled every few seconds: in the Debug log only when slow or failing.
const POLLS: &[&str] = &[
    "queue_status",
    "progress",
    "import_result",
    "resource_usage",
    "list_jobs",
    "suggestions_progress",
];

/// [`run`], timed into the Debug log with its arguments (cut short) and outcome.
pub fn dispatch(tools: &Tools, command: &str, a: Value) -> Result<Value> {
    if !crate::debug_log::enabled() {
        return run(tools, command, a);
    }
    let args: String = a.to_string().chars().take(300).collect();
    let t = std::time::Instant::now();
    let r = run(tools, command, a);
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    if !POLLS.contains(&command) || ms > 250.0 || r.is_err() {
        crate::debug_log::event(
            "command",
            json!({
                "command": command,
                "duration_ms": (ms * 10.0).round() / 10.0,
                "ok": r.is_ok(),
                "error": r.as_ref().err().map(|e| e.0.clone()),
                "args": args,
            }),
        );
    }
    r
}

fn run(tools: &Tools, command: &str, a: Value) -> Result<Value> {
    match command {
        "resource_usage" => Ok(crate::meter::snapshot(
            a["watch"].as_bool().unwrap_or(true),
            a["points"].as_u64().map(|n| n as usize),
        )),
        "start_import" => {
            {
                let mut state = IMPORT.lock().unwrap();
                if state.0 {
                    return Err(Error("an Import is already running".into()));
                }
                *state = (true, None);
            }
            let library = tools.library().to_path_buf();
            std::thread::spawn(move || {
                let r = dispatch(&Tools::new(library), "import_folder", a).map_err(|e| e.0);
                *IMPORT.lock().unwrap() = (false, Some(r));
            });
            Ok(json!({ "started": true }))
        }
        "import_result" => {
            let state = IMPORT.lock().unwrap();
            Ok(match &state.1 {
                Some(Ok(report)) => json!({ "running": state.0, "report": report }),
                Some(Err(e)) => json!({ "running": state.0, "error": e }),
                None => json!({ "running": state.0 }),
            })
        }
        "cancel_import" => Ok(json!({ "cancelled": crate::progress::cancel("import") })),
        "render_detail" => Ok(parts_json(tools.render_detail(a)?)),
        "render_previews" => {
            let a: PreviewArgs = args(command, a)?;
            let lib = Library::open(tools.library())?;
            let compute = a.compute.unwrap_or_else(crate::compute::Compute::from_env);
            let report = lib.render_previews(a.shoot_id, compute, &|_, _| {})?;
            serde_json::to_value(report).map_err(|e| Error(format!("serialize: {e}")))
        }
        "pending_previews" => {
            let a: PreviewArgs = args(command, a)?;
            let lib = Library::open(tools.library())?;
            Ok(json!({ "pending": lib.pending_previews(a.shoot_id)? }))
        }
        // Edited Photos for the grid, filmstrip and Loupe: { "<id>": "previews/edited/…" | null }.
        "edited_previews" => {
            let a: EditedPreviewArgs = args(command, a)?;
            let lib = Library::open(tools.library())?;
            let files = crate::tools::edited_previews(&lib, &a.photo_ids, a.long_edge)?;
            Ok(json!(files))
        }
        "thumbnail" => {
            let a: ThumbnailArgs = args(command, a)?;
            let edge = a.long_edge.unwrap_or(320).clamp(64, 1024);
            let jpeg = crate::naming::thumbnail(Path::new(&a.path), edge)?;
            Ok(json!({ "images": [data_url(&jpeg)] }))
        }
        "suggest_name" => {
            let a: SuggestNameArgs = args(command, a)?;
            let (mut owned, mut captured_at) = (a.paths, a.captured_at);
            if let Some(shoot) = a.shoot_id {
                // A few previews spread across the Shoot, as Import samples a card.
                let lib = Library::open(tools.library())?;
                let photos = lib.list_photos(&PhotoFilter {
                    shoot: Some(shoot),
                    verdict: None,
                })?;
                let n = crate::naming::NAME_SAMPLE;
                let step = (photos.len() / n).max(1);
                owned = photos
                    .iter()
                    .step_by(step)
                    .take(n)
                    .map(|p| p.preview.to_string_lossy().into_owned())
                    .collect();
                captured_at = captured_at.or_else(|| photos.first().map(|p| p.captured_at.clone()));
            }
            let paths: Vec<&Path> = owned.iter().map(Path::new).collect();
            let s = crate::naming::suggest_shoot_name(
                &paths,
                captured_at.as_deref(),
                a.provider.as_deref(),
            );
            serde_json::to_value(s).map_err(|e| Error(format!("serialize: {e}")))
        }
        "shoot_name" => {
            let a: ShootNameArgs = args(command, a)?;
            let today = crate::library::time::now();
            let date = a
                .captured_at
                .as_deref()
                .and_then(crate::naming::capture_date)
                .unwrap_or(&today[..10]);
            let name = crate::naming::shoot_name(date, &a.title);
            Ok(json!({ "shoot_name": name, "originals": crate::naming::originals_folder(&name) }))
        }
        "reveal" => {
            let a: RevealArgs = args(command, a)?;
            if let Some(path) = a.path {
                let path = crate::export::expand_home(&path);
                if !path.is_dir() {
                    return Err(Error(format!("reveal: no folder at {}", path.display())));
                }
                return Ok(json!({ "path": path, "shown": reveal(&path, false) }));
            }
            let lib = Library::open(tools.library())?;
            let photos = lib.list_photos(&PhotoFilter {
                shoot: a.shoot_id,
                verdict: None,
            })?;
            let path = match (a.photo_id, a.shoot_id) {
                (Some(id), _) => photos
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| std::path::PathBuf::from(&p.path))
                    .ok_or_else(|| Error(format!("no Photo {id}")))?,
                // The folder its first original sits in: originals/<Shoot> when copied in.
                (None, Some(shoot)) => photos
                    .first()
                    .and_then(|p| Path::new(&p.path).parent().map(Path::to_path_buf))
                    .ok_or_else(|| Error(format!("Shoot {shoot} has no Photos")))?,
                (None, None) => return Err(Error("reveal: give a shoot_id or photo_id".into())),
            };
            Ok(json!({ "path": path, "shown": reveal(&path, a.photo_id.is_some()) }))
        }
        "providers" => {
            let default = std::env::var("ARGUS_PROVIDER").unwrap_or_else(|_| "ollama".into());
            let o = Ollama::from_env();
            let mut ollama = json!({ "id": "ollama", "label": "Ollama", "model": o.model,
                                     "note": "On this Mac · nothing leaves it" });
            // So the Agent panel can say so before a cull fails on it.
            if o.installed().is_err() {
                ollama["down"] = json!("Ollama isn't running");
            }
            let mut providers = vec![
                ollama,
                json!({ "id": "claude-code", "label": "Claude", "model": ClaudeCode::from_env().model,
                        "note": "Your Claude plan, through the claude CLI" }),
            ];
            // Only offered once a key is set up: Greg uses no API keys (ADR 0006).
            if crate::cull::Claude::from_env().is_ok() {
                providers.push(
                    json!({ "id": "claude-api", "label": "Claude API", "model": null,
                                       "note": "Your Anthropic API key" }),
                );
            }
            Ok(json!({ "default": default, "providers": providers }))
        }
        // The Ollama models pulled on this Mac, for Settings to choose from: only real ones.
        "models" => {
            let o = Ollama::from_env();
            let pinned = std::env::var("ARGUS_OLLAMA_MODEL").is_ok();
            Ok(match o.installed() {
                Ok(models) => json!({ "model": o.model, "pinned": pinned, "models": models }),
                Err(e) => json!({ "model": o.model, "pinned": pinned, "models": [], "error": e.0 }),
            })
        }
        "set_model" => {
            #[derive(Deserialize)]
            struct A {
                model: String,
            }
            let a: A = args(command, a)?;
            let lib = Library::open(tools.library())?;
            lib.set_setting(crate::tools::OLLAMA_MODEL_KEY, &json!(a.model))?;
            crate::cull::choose_ollama_model(Some(a.model));
            Ok(json!({ "model": Ollama::from_env().model }))
        }
        "set_api_key" => {
            #[derive(Deserialize)]
            struct A {
                key: String,
            }
            let a: A = args(command, a)?;
            crate::cull::store_api_key(&a.key)?;
            Ok(json!({ "ok": true }))
        }
        "agent" => {
            let ask: crate::agent::Ask = args(command, a)?;
            let reply = crate::agent::run(tools, &ask)?;
            serde_json::to_value(reply).map_err(|e| Error(format!("agent: {e}")))
        }
        "photos" => {
            let a: PhotosArgs = args(command, a)?;
            let lib = Library::open(tools.library())?;
            let root = lib.root().to_path_buf();
            let photos = lib.list_photos(&PhotoFilter {
                shoot: a.shoot_id,
                verdict: a.verdict,
            })?;
            let edits = lib.recipe_stamps()?;
            Ok(Value::Array(
                photos
                    .into_iter()
                    .map(|p| {
                        let rel = p.preview.strip_prefix(&root).unwrap_or(&p.preview);
                        let rel = rel.to_string_lossy().replace('\\', "/");
                        let mut v = serde_json::to_value(&p).expect("Photo serializes");
                        v["preview"] = Value::String(rel);
                        v["edit"] = json!(edits.get(&p.id));
                        v
                    })
                    .collect(),
            ))
        }
        "eye_flags" => {
            // The grid's closed-eye and soft-focus badges: read what the background
            // `analyze` job stored, in one query for the whole Shoot.
            use crate::analyzers::{eyes, Analyzer, Fact};
            let a: PhotosArgs = args(command, a)?;
            let lib = Library::open(tools.library())?;
            let mut out = Vec::new();
            for (id, version, value) in lib.fact_values("eyes", a.shoot_id)? {
                if version != eyes::Eyes.version() {
                    continue;
                }
                let reads: Vec<_> = eyes::read(&Fact { value, blob: None })
                    .into_iter()
                    .filter(|r| r.read)
                    .collect();
                let closed = reads.iter().any(|r| r.eyes_closed);
                let soft = !reads.is_empty() && reads.iter().all(|r| r.focus < eyes::SOFT);
                if closed || soft {
                    out.push(json!({ "id": id, "closed": closed, "soft": soft }));
                }
            }
            Ok(Value::Array(out))
        }
        // Remove from Library is the person's own call, so it's a command, not an Agent tool.
        "remove_photos" => {
            let a: RemoveArgs = args(command, a)?;
            let mut lib = Library::open(tools.library())?;
            Ok(json!(lib.remove(
                &a.photo_ids,
                &a.shoot_ids,
                a.delete_originals
            )?))
        }
        "restore_removed" => {
            let a: RemovalArgs = args(command, a)?;
            let mut lib = Library::open(tools.library())?;
            Ok(json!(lib.restore_removed(a.removal)?))
        }
        "list_removed" => Ok(json!(Library::open(tools.library())?.removed()?)),
        "empty_removed" => {
            let a: RemovalArgs = args(command, a)?;
            let mut lib = Library::open(tools.library())?;
            Ok(json!({ "emptied": lib.empty_removed(a.removal)? }))
        }
        "flag" => {
            let a: FlagArgs = args(command, a)?;
            let ids = distinct(&a.ids);
            if ids.is_empty() {
                // Nothing to flag is no Undo step either.
                return Ok(json!({ "flagged": 0, "batch_id": null, "photos": [] }));
            }
            let mut lib = Library::open(tools.library())?;
            let label = format!("Flagged {} {}", ids.len(), a.verdict.as_str());
            let batch = person_batch(&mut lib, a.author, a.batch_id, a.label, label)?;
            lib.set_verdict(&ids, a.verdict, a.author, a.reason.as_deref(), batch)?;
            let photos = lib.cull_states(&ids)?;
            Ok(json!({ "flagged": ids.len(), "batch_id": batch, "photos": photos }))
        }
        "rate" => {
            let a: RateArgs = args(command, a)?;
            let ids = distinct(&a.ids);
            if ids.is_empty() {
                return Ok(json!({ "rated": 0, "batch_id": null, "photos": [] }));
            }
            let mut lib = Library::open(tools.library())?;
            let label = format!("Rated {} at {} stars", ids.len(), a.stars);
            let batch = person_batch(&mut lib, a.author, a.batch_id, a.label, label)?;
            lib.set_rating(&ids, a.stars, a.author, batch)?;
            let photos = lib.cull_states(&ids)?;
            Ok(json!({ "rated": ids.len(), "batch_id": batch, "photos": photos }))
        }
        "denoise_status" => {
            #[derive(Deserialize)]
            struct A {
                photo_id: crate::library::PhotoId,
            }
            let a: A = args(command, a)?;
            let lib = Library::open(tools.library())?;
            let photo = lib
                .photo(a.photo_id)?
                .ok_or_else(|| Error(format!("no Photo {}", a.photo_id)))?;
            let job = lib
                .jobs(None, Some("denoise"), 50)?
                .into_iter()
                .filter(|j| j.args["photo_id"].as_i64() == Some(a.photo_id))
                .find(|j| {
                    matches!(
                        j.status,
                        crate::library::JobStatus::Queued | crate::library::JobStatus::Running
                    )
                });
            let job = match job {
                Some(j) => {
                    let ran = lib.job_elapsed_seconds(j.id)?.unwrap_or(0.0);
                    let eta = (j.done > 0 && j.total > j.done)
                        .then(|| ran * (j.total - j.done) as f64 / j.done as f64);
                    json!({ "status": j.status, "done": j.done, "total": j.total, "eta_s": eta })
                }
                None => Value::Null,
            };
            Ok(json!({
                "raw": crate::denoise::base_file(&photo).is_some(),
                "ready": crate::denoise::ready(&photo).is_some(),
                "job": job,
            }))
        }
        "photo_states" => {
            let a: StatesArgs = args(command, a)?;
            let lib = Library::open(tools.library())?;
            serde_json::to_value(lib.cull_states(&a.ids)?)
                .map_err(|e| Error(format!("photo_states: {e}")))
        }
        "edit" => {
            let a: EditArgs = args(command, a)?;
            if !a.recipe.is_object() {
                return Err(Error("recipe must be a JSON object".into()));
            }
            let mut lib = Library::open(tools.library())?;
            let label = a.label.unwrap_or_else(|| "Edit".into());
            let batch = lib.begin_batch(Author::Person, &label)?;
            let version = lib.write_recipe(a.photo_id, &a.recipe, Author::Person, Some(batch))?;
            Ok(json!({ "version": version, "batch_id": batch }))
        }
        // The app's progress line reads the background queue.
        // The job queue's state, plus what this process is doing outside it (a card scan,
        // an import) as `tasks`, for progress bars with an ETA.
        "progress" => {
            let mut v = dispatch(tools, "queue_status", a)?;
            if let Some(o) = v.as_object_mut() {
                o.insert(
                    "tasks".into(),
                    serde_json::to_value(crate::progress::snapshot()).unwrap_or_default(),
                );
            }
            Ok(v)
        }
        // The person opened a Photo in Loupe or Edit: its queued Suggestions go first and
        // run at Full, the next few in the filmstrip (`next_ids`) right after.
        "focus_photo" => {
            let a: FocusArgs = args(command, a)?;
            let mut lib = Library::open(tools.library())?;
            let raised = crate::jobs::focus(&mut lib, a.photo_id, &a.next_ids)?;
            Ok(json!({ "raised": raised }))
        }
        // A tool the person runs from a screen (the Look screen's sliders, Sync to Look):
        // its changes are a batch of the person's own, so the UI's ⌘Z can undo it and the
        // Agent's "undo the last change" never picks it up.
        "as_person" => {
            let a: AsPersonArgs = args(command, a)?;
            let batch = Library::open(tools.library())?.begin_batch(Author::Person, &a.label)?;
            let mut out = parts_json(tools.call_in(&a.tool, a.args, Some(batch))?);
            if let Some(o) = out.as_object_mut() {
                o.insert("batch_id".into(), json!(batch));
            }
            Ok(out)
        }
        tool => Ok(parts_json(tools.call(tool, a)?)),
    }
}

/// A tool's result as one JSON value, its images as `data:` URLs under `images`.
fn parts_json(parts: Vec<Part>) -> Value {
    let mut out = json!({});
    let mut images = Vec::new();
    for part in parts {
        match part {
            Part::Json(v) => out = v,
            Part::Jpeg(bytes) => images.push(data_url(&bytes)),
        }
    }
    if images.is_empty() {
        return out;
    }
    if !out.is_object() {
        out = json!({ "result": out });
    }
    out["images"] = Value::Array(images);
    out
}

/// Long edge of the grid's thumbnails (`previews/thumbs/...`): crisp on a Retina screen up
/// to tiles of 240 points. Decoding one costs a twentieth of a 2048 px preview, which is
/// what made scrolling a big Shoot heavy (docs/agents/performance.md).
pub const THUMB_EDGE: u32 = 480;
const THUMBS: &str = "previews/thumbs/";

/// Resolve a Library-relative file a transport was asked for (a preview), refusing
/// anything that could escape the Library's previews folder. `previews/thumbs/<preview>`
/// is that preview at [`THUMB_EDGE`], made on first request and kept as cache (Settings ›
/// Library › Clear cache removes them) until the preview changes.
pub fn library_file(lib_root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.trim_start_matches('/');
    let ok = rel.starts_with("previews/")
        && !rel
            .split(['/', '\\'])
            .any(|part| part == ".." || part.is_empty());
    let rel = ok.then_some(rel)?;
    if let Some(of) = rel.strip_prefix(THUMBS) {
        let thumb = lib_root.join(rel);
        let preview = library_file(lib_root, &format!("previews/{of}"))?;
        let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        let fresh = matches!((modified(&thumb), modified(&preview)), (Some(t), Some(p)) if t >= p);
        if !fresh {
            if let Err(e) = write_thumb(&preview, &thumb) {
                eprintln!("thumbnail of {}: {e}", preview.display());
                // The preview itself still shows.
                return Some(preview);
            }
        }
        return Some(thumb);
    }
    let path = lib_root.join(rel);
    // A camera preview Argus has since replaced with its own render: a page that listed
    // the Photos before the swap still asks for the old name.
    if !path.exists() {
        if let Some(stem) = rel.strip_suffix(".camera.jpg") {
            let own = lib_root.join(format!("{stem}.jpg"));
            if own.is_file() {
                return Some(own);
            }
        }
    }
    Some(path)
}

fn write_thumb(preview: &Path, thumb: &Path) -> Result<()> {
    let bytes = std::fs::read(preview).map_err(|e| Error(e.to_string()))?;
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg)
        .map_err(|e| Error(e.to_string()))?
        .thumbnail(THUMB_EDGE, THUMB_EDGE)
        .to_rgb8();
    let (width, height) = img.dimensions();
    let jpeg = crate::render::encode_jpeg(
        &crate::RgbImage {
            width: width as usize,
            height: height as usize,
            data: img.into_raw(),
        },
        82,
    )?;
    let dir = thumb.parent().expect("a thumbnail has a folder");
    std::fs::create_dir_all(dir).map_err(|e| Error(e.to_string()))?;
    // Via a temporary file, so a reader never sees half of one.
    let tmp = thumb.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, jpeg).map_err(|e| Error(e.to_string()))?;
    std::fs::rename(&tmp, thumb).map_err(|e| Error(e.to_string()))
}

/// The ids once each, in the order given: a selection sent twice is still one change per Photo.
fn distinct<T: Copy + Eq + std::hash::Hash>(ids: &[T]) -> Vec<T> {
    let mut seen = std::collections::HashSet::new();
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}
