//! Claim and run the next job.

use super::*;

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
    run_next_claim(tools, compute, min_priority, held, &[])
}

/// [`run_next_holding`], with `first`'s jobs (the Shoot being prepared) first and free of
/// the holds, save [`crate::prepare::LATER`].
pub fn run_next_claim(
    tools: &Tools,
    compute: Compute,
    min_priority: i64,
    held: &[&str],
    first: &[ShootId],
) -> Result<Option<Job>> {
    let mut lib = Library::open(tools.library())?;
    let claim = crate::library::Claim {
        min_priority,
        held,
        held_below: PRIORITY_FOCUS,
        first: (!first.is_empty()).then_some((first, crate::prepare::LATER)),
    };
    let Some(job) = lib.claim_with(&claim)? else {
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
    let since = crate::sleep::Since::now();
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
    settle(&lib, &job, &outcome, &since)?;
    Ok(Some(lib.job(job.id)?))
}

/// Record how a job ended. One that failed across the Mac's sleep (a model's connection
/// cut when the lid closed) goes back in the queue as "Paused while asleep" and runs again
/// from where its own work left off, instead of counting as failed.
pub fn settle(
    lib: &Library,
    job: &Job,
    outcome: &Result<Value>,
    since: &crate::sleep::Since,
) -> Result<()> {
    match outcome {
        Ok(v) => lib.finish_job(job.id, Ok(v)),
        Err(e) if since.did_sleep() => {
            crate::logs::note(&format!(
                "job {} ({}) stopped across a {} s sleep, queued again: {e}",
                job.id,
                job.label,
                since.slept().as_secs()
            ));
            lib.interrupt_job(job.id, "asleep")
        }
        Err(e) => lib.finish_job(job.id, Err(&e.0)),
    }
}

/// Run every queued job, whatever the queue's holds (tests and `argus work --once --drain`). Returns how many ran.
pub fn drain(tools: &Tools, compute: Compute) -> Result<usize> {
    let mut n = 0;
    while run_next(tools, compute)?.is_some() {
        n += 1;
    }
    Ok(n)
}
