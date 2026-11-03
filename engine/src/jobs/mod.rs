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

mod fanout;
mod focus;
mod kinds;
mod policy;
mod resume;
mod run;
mod worker;

pub use fanout::*;
pub use focus::*;
pub use kinds::*;
pub use policy::*;
pub use resume::queue_orphaned_runs;
pub use run::*;
pub use worker::*;

/// Kinds of job, and what their `args` are.
/// - `suggest` {photo_id, count?, reference_id?, look_id?, provider?}: Suggestions for one Photo
/// - `suggest_shoot` {shoot_id?, photo_ids?, count?, provider?, redo?}: a `suggest` per Photo,
///   rejects skipped, Picks first (and Photos that already have Suggestions skipped unless `redo`)
/// - `match_hero` {photo_id, photo_ids?, provider?}: the hero Photo's style becomes a Look and
///   every other Photo (or `photo_ids`) gets matched Suggestions under it
/// - `cull_shoot` (alias `rate_shoot`) {shoot_id?, provider?, check?}: the AI cull
/// - `check_shoot` {shoot_id?}: the AI cull with the rules' Technical check (no model, seconds),
///   so it is never held for an idle Mac; Import queues it
/// - `render_previews` {shoot_id?}: Argus's own previews over the camera's, queued by `import_folder`
/// - `analyze` {shoot_id?, photo_ids?}: the specialist models' Facts (ADR 0013), queued after Import
/// - `rank_presets` {shoot_id, provider?}: the AI's suggested order of the Presets for a Shoot
/// - `tag_photos` {shoot_id?, photo_ids?, provider?, redo?}: the AI's suggested caption and
///   Keywords for each Photo ([`crate::tagging`]), rejects skipped; Import queues it with the cull
/// - `learn_edits` {}: the Library's own Edit head from the person's exported RAWs
///   ([`crate::edit_head::learn_library`]), queued after an export
/// - `download_model` {}: the search-by-meaning model into the models folder
///   ([`crate::meaning::download`], progress in MB), then an `analyze` of the whole Library
/// - `denoise` {photo_id}: AI Denoise's base for one RAW ([`crate::denoise`]), queued when the
///   person saves a Denoise Amount, or opens a Photo whose AI edit has one; with `precompute`,
///   queued for each RAW Pick at the lowest priority so it runs while the Mac is idle, kept
///   under [`crate::denoise::CACHE_CAP`]
/// - `pre_edit_shoot` {shoot_id, every?}: edit as you cull, a Shoot match Suggestion for each Pick and
///   likely keeper from the Shoot's hand edits ([`crate::pre_edit`], no model), queued when the
///   person saves an edit of a RAW and when the AI's stars refresh; with `every` (Lock look) for
///   every frame that isn't a Reject
/// - `backup` {place, shoot_id?, verify?, why?}: copy the Library to a Backup place
///   ([`crate::library::Place`]), or with `verify` re-read the copy; progress in MB. Runs at
///   most [`BACKUP_SLICE`] at a time and queues the rest, so a first backup of a big Library
///   never holds the AI work up for hours
/// - `send_photos` {to: apple_photos|google_photos, paths, album?, shoot_id?}: exported files
///   into the person's photo app, in an album named after the Shoot ([`crate::google_photos`],
///   [`crate::export::add_to_apple_photos`]); progress in files. Runs at most [`SEND_SLICE`]
///   at a time and queues the rest, like `backup`
/// - `resume_run` {run_id}: finish an Export or Import that a quit or crash interrupted
///   ([`crate::library::Run`]), from the Photo it reached; queued when Argus starts
pub const KINDS: &[&str] = &[
    "resume_run",
    "send_photos",
    "backup",
    "learn_edits",
    "rank_presets",
    "tag_photos",
    "analyze",
    "denoise",
    "pre_edit_shoot",
    "download_model",
    "suggest",
    "suggest_shoot",
    "match_hero",
    "cull_shoot",
    "rate_shoot",
    "check_shoot",
    "render_previews",
];

/// The longest one `backup` job copies before it queues the rest.
pub const BACKUP_SLICE: Duration = Duration::from_secs(10 * 60);

/// The longest one `send_photos` job uploads before it queues the rest.
pub const SEND_SLICE: Duration = Duration::from_secs(2 * 60);

/// Priorities: what the person asked for right now goes first.
pub const PRIORITY_MATCH: i64 = 40;
pub const PRIORITY_PICK: i64 = 20;
pub const PRIORITY_OTHER: i64 = 10;
/// An Import's Keywords and captions: after the Shoot's Suggestions.
pub const PRIORITY_KEYWORDS: i64 = 5;
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
