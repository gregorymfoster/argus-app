//! The open Photo jumps the queue: its Suggestions run first and at Full.

use super::*;

/// The person opened `photo_id` (and `next` follow it in the filmstrip): their queued
/// Suggestions jump ahead of background work. Returns how many jobs moved up.
pub fn focus(lib: &mut Library, photo_id: PhotoId, next: &[PhotoId]) -> Result<usize> {
    // An AI edit's Denoise gets its base when the person first looks (crate::denoise), and
    // Picks from before precomputing existed get theirs queued once.
    crate::denoise::queue_on_open(lib, photo_id)?;
    crate::denoise::backfill_picks(lib)?;
    if !config(lib)?.focus_first {
        return Ok(0);
    }
    let mut n = 0;
    let targets = std::iter::once((photo_id, PRIORITY_FOCUS))
        .chain(next.iter().take(5).map(|&p| (p, PRIORITY_PREFETCH)));
    for (p, priority) in targets {
        let mut raised = 0;
        for flavour in ["fresh", "match"] {
            raised += lib.raise_job(&format!("suggest:{p}:{flavour}"), priority)? as usize;
        }
        if raised == 0 && take_on_open(lib, p)? {
            enqueue(
                lib,
                "suggest",
                &json!({ "photo_id": p }),
                Some(priority),
                None,
            )?;
            raised = 1;
        }
        n += raised;
    }
    Ok(n)
}

/// The Shoot's Photos whose model takes wait until the person opens them.
pub(super) fn on_open_key(shoot: ShootId) -> String {
    format!("suggest_on_open:{shoot}")
}

/// `photo` was held back from its Shoot's Suggestions pass: take it off the list, so
/// opening it queues its takes once.
pub(super) fn take_on_open(lib: &mut Library, photo: PhotoId) -> Result<bool> {
    let Some(shoot) = lib.photo(photo)?.map(|p| p.shoot_id) else {
        return Ok(false);
    };
    let key = on_open_key(shoot);
    let mut ids: Vec<PhotoId> = lib
        .setting(&key)?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let Some(at) = ids.iter().position(|&i| i == photo) else {
        return Ok(false);
    };
    ids.remove(at);
    lib.set_setting(&key, &json!(ids))?;
    Ok(true)
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
