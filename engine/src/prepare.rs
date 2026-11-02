//! Prepare: the heavy work for a new Shoot, done up front so the person can walk away and
//! come back to a Shoot where nothing waits (Greg, 2026-11-02, "everything up front").
//!
//! Import starts it by itself. The Shoot's jobs then go first in the Queue and skip the
//! idle and overnight holds: previews, the Analyzers, the cull, model Suggestions and a
//! denoised base for every likely keeper that is noisy enough to want one. The queue runs
//! it at Full and keeps the Mac awake ([`crate::power::Awake`]); "Cull now" drops it to
//! Gentle so the Loupe stays quick. On battery it waits for the charger like any background
//! work, unless the person says run anyway. Keywords and the backup wait for Finish.
//!
//! One Shoot prepares at a time: the newest Import's. [`status`] says how each stage stands
//! and when it will be ready, from the same per-kind averages the Queue's ETA uses.

use crate::jobs::{self, QueueConfig};
use crate::library::{Library, ShootId};
use crate::Result;
use serde_json::{json, Value};

/// Setting holding the Shoot being prepared and since when.
const KEY: &str = "prepare";
/// Setting holding the last finished Prepare, for the "ready" notification.
const DONE_KEY: &str = "prepare_done";

/// Job kinds of a Shoot that Prepare leaves for Finish.
pub const LATER: &[&str] = &["tag_photos", "backup", "learn_edits"];

/// The stages the Work screen shows, in order: label and the job kinds behind it.
pub const STAGES: &[(&str, &[&str])] = &[
    ("Previews", &["render_previews"]),
    ("Faces, focus, level", &["analyze"]),
    ("Stacks and AI flags", &["check_shoot", "cull_shoot"]),
    ("Edits for likely keepers", &["suggest_shoot", "suggest"]),
    ("Denoise noisy keepers", &["denoise"]),
    ("Keywords", &["tag_photos"]),
    ("Backup", &["backup"]),
];

/// Seconds a job of `kind` takes when the Library hasn't run one yet (Greg's Mac, 2026-11).
fn guess_seconds(kind: &str) -> f64 {
    match kind {
        "suggest" => 30.0,
        "denoise" => 35.0,
        "tag_photos" | "render_previews" | "analyze" => 120.0,
        "backup" => 60.0,
        _ => 10.0,
    }
}

/// Start preparing `shoot`, counting its jobs created from `since` (RFC 3339; the Import's
/// start). A card that splits into several Shoots prepares them together. Queues the Shoot's
/// Suggestions pass with Prepare's denoise, replacing the plain one Import queued.
pub fn start(lib: &mut Library, shoot: ShootId, since: &str) -> Result<Value> {
    let mut args = json!({ "shoot_id": shoot, "prepare": true });
    if let Some(j) = lib
        .jobs(
            Some(crate::library::JobStatus::Queued),
            Some("suggest_shoot"),
            1000,
        )?
        .into_iter()
        .find(|j| j.args["shoot_id"] == json!(shoot) && j.args.get("prepare").is_none())
    {
        if let Some(p) = j.args.get("provider") {
            args["provider"] = p.clone();
        }
        lib.cancel_job(j.id)?;
    }
    jobs::enqueue(
        lib,
        "suggest_shoot",
        &args,
        None,
        Some(&format!("suggest_shoot:{shoot}:prepare")),
    )?;
    let mut c = jobs::config(lib)?;
    let (mut list, mut from) = if c.preparing {
        current(lib)?
    } else {
        (Vec::new(), since.to_string())
    };
    if !list.contains(&shoot) {
        list.push(shoot);
    }
    if since < from.as_str() {
        from = since.to_string();
    }
    lib.set_setting(KEY, &json!({ "shoots": list, "since": from }))?;
    c.preparing = true;
    c.prepare_gentle = false;
    jobs::set_config(lib, &c)?;
    status(lib)
}

/// Stop preparing: the Shoots' remaining jobs go back to the usual holds.
pub fn stop(lib: &Library) -> Result<()> {
    let mut c = jobs::config(lib)?;
    c.preparing = false;
    c.prepare_gentle = false;
    jobs::set_config(lib, &c)
}

/// The Shoots being prepared, when Prepare is on.
pub fn shoots(lib: &Library) -> Result<Vec<ShootId>> {
    Ok(current(lib)?.0)
}

/// The Shoots in the last Prepare and since when.
fn current(lib: &Library) -> Result<(Vec<ShootId>, String)> {
    let v = lib.setting(KEY)?.unwrap_or(Value::Null);
    let list = v["shoots"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default();
    let since = v["since"].as_str().unwrap_or("0000").to_string();
    Ok((list, since))
}

/// One stage of a Shoot's work.
fn stage(
    lib: &Library,
    label: &str,
    kinds: &[&str],
    rows: &[crate::library::ShootJobs],
    later: bool,
) -> Result<Value> {
    let mut done = 0;
    let mut left = 0;
    let mut failed = 0;
    let mut eta = 0.0;
    let mut seconds = 0.0;
    let mut running = false;
    for r in rows.iter().filter(|r| kinds.contains(&r.kind.as_str())) {
        // The pass that fans out into per-Photo jobs isn't a unit of its own.
        if r.kind == "suggest_shoot" {
            running |= r.running > 0 || r.queued > 0;
            continue;
        }
        done += r.done;
        failed += r.failed;
        left += r.queued + r.running;
        running |= r.running > 0;
        seconds += r.seconds;
        let avg = lib
            .average_job_seconds(&r.kind)?
            .unwrap_or_else(|| guess_seconds(&r.kind));
        eta += (r.queued + r.running) as f64 * avg;
    }
    // A per-Shoot job (previews, Analyzers) reports its own progress: show it in Photos.
    let unit = if ["render_previews", "analyze"]
        .iter()
        .any(|k| kinds.contains(k))
    {
        lib.jobs(Some(crate::library::JobStatus::Running), None, 10)?
            .into_iter()
            .find(|j| kinds.contains(&j.kind.as_str()) && j.total > 0)
            .map(|j| json!({ "done": j.done, "total": j.total }))
    } else {
        None
    };
    let total = done + failed + left;
    Ok(json!({
        "label": label,
        "kinds": kinds,
        "done": done + failed,
        "failed": failed,
        "total": total,
        "running": running,
        "eta_s": if left > 0 { Some(eta.round()) } else { None },
        "seconds": seconds.round(),
        "progress": unit,
        "later": later,
        "state": if total == 0 && !running { if later { "later" } else { "none" } }
                 else if left > 0 || running { "running" } else { "done" },
    }))
}

/// How the Shoots being prepared stand: `null` when Prepare is off. Their stages, the
/// seconds left and the state: "running", "battery" (waiting for the charger), "paused" or
/// "done".
pub fn status(lib: &Library) -> Result<Value> {
    if !jobs::config(lib)?.preparing {
        return Ok(Value::Null);
    }
    let (list, since) = current(lib)?;
    shoots_status(lib, &list, &since)
}

pub fn shoots_status(lib: &Library, list: &[ShootId], since: &str) -> Result<Value> {
    let c = jobs::config(lib)?;
    let mut rows = Vec::new();
    for &s in list {
        rows.extend(lib.shoot_jobs(s, since)?);
    }
    let mut stages = Vec::new();
    let mut eta = 0.0;
    let mut busy = false;
    for (label, kinds) in STAGES {
        let later = kinds.iter().all(|k| LATER.contains(k));
        let s = stage(lib, label, kinds, &rows, later)?;
        if !later {
            eta += s["eta_s"].as_f64().unwrap_or(0.0);
            busy |= s["state"] == "running";
        }
        stages.push(s);
    }
    let mut photos = 0;
    for &s in list {
        photos += lib
            .list_photos(&crate::library::PhotoFilter {
                shoot: Some(s),
                verdict: None,
            })?
            .len();
    }
    let names: Vec<String> = lib
        .shoots()?
        .into_iter()
        .filter(|s| list.contains(&s.id))
        .map(|s| s.name)
        .collect();
    let state = if !busy {
        "done"
    } else if c.paused {
        "paused"
    } else if jobs::held_on_battery(&c, crate::power::on_battery()) {
        "battery"
    } else {
        "running"
    };
    let keepers: i64 = rows
        .iter()
        .filter(|r| r.kind == "suggest")
        .map(|r| r.queued + r.running + r.done + r.failed)
        .sum();
    Ok(json!({
        "shoot_ids": list,
        "shoot_names": names,
        "photos": photos,
        "keepers": keepers,
        "since": since,
        "state": state,
        "gentle": c.prepare_gentle,
        "eta_s": if busy { Some(eta.round()) } else { None },
        "stages": stages,
    }))
}

/// Called by the worker between jobs: once the Prepare work is all done, note it for the
/// "ready" notification and lift Prepare. Returns whether it just finished.
pub fn settle(lib: &Library) -> Result<bool> {
    if !jobs::config(lib)?.preparing {
        return Ok(false);
    }
    let s = status(lib)?;
    if s["state"] != "done" {
        return Ok(false);
    }
    let mut done = s.clone();
    done["finished_at"] = json!(crate::library::time::now());
    lib.set_setting(DONE_KEY, &done)?;
    stop(lib)?;
    Ok(true)
}

/// The last finished Prepare (for the notification and the Work screen's "Ready" line).
pub fn last_done(lib: &Library) -> Result<Value> {
    Ok(lib.setting(DONE_KEY)?.unwrap_or(Value::Null))
}

/// Whether the queue should hold the Mac awake: Prepare or Run it all has work to do.
pub fn wants_awake(c: &QueueConfig, has_work: bool) -> bool {
    has_work && (c.preparing || c.go_ahead) && !c.paused
}
