//! Finishing what a quit, a crash or a closed lid interrupted (Greg, 2026-11-02): a run
//! ([`crate::library::Run`]) whose process is gone goes on the Queue as `resume_run`, ahead
//! of the AI work, and carries on from the Photo it reached.

use super::*;

/// Queue a `resume_run` for every run whose process is gone, and for every Import paused
/// for its card once the card is back. Returns how many were queued.
pub fn queue_orphaned_runs(lib: &mut Library) -> Result<usize> {
    let mut n = 0;
    for r in lib.orphaned_runs()? {
        lib.set_run_status(r.id, "paused", Some("quit"))?;
        n += enqueue(lib, "resume_run", &json!({ "run_id": r.id }), None, None)?.1 as usize;
    }
    for r in lib.unfinished_runs()? {
        let folder = r.args["tool"]["folder"].as_str().map(PathBuf::from);
        if r.note.as_deref() == Some("card") && folder.is_some_and(|f| f.is_dir()) {
            n += enqueue(lib, "resume_run", &json!({ "run_id": r.id }), None, None)?.1 as usize;
        }
    }
    Ok(n)
}

/// The `resume_run` job: an Export's remaining Photos, or the rest of an Import.
pub(super) fn resume_run(tools: &Tools, job: &Job) -> Result<Value> {
    #[derive(Deserialize)]
    struct A {
        run_id: crate::library::RunId,
    }
    let a: A = kinds::args(job)?;
    let lib = Library::open(tools.library())?;
    let run = lib.run(a.run_id)?;
    if !matches!(run.status.as_str(), "running" | "paused") {
        return Ok(json!({ "run_id": run.id, "skipped": run.status }));
    }
    match run.kind.as_str() {
        "export" => resume_export(tools, job, &lib, &run),
        "import" => {
            let mut args = run.args.get("tool").cloned().unwrap_or_else(|| json!({}));
            args["resume_run"] = json!(run.id);
            drop(lib);
            json_of(tools.call("import_folder", args)?)
        }
        other => Err(Error(format!("run {} has unknown kind {other}", run.id))),
    }
}

fn resume_export(
    tools: &Tools,
    job: &Job,
    lib: &Library,
    run: &crate::library::Run,
) -> Result<Value> {
    let ids: Vec<PhotoId> = serde_json::from_value(run.args["photo_ids"].clone())
        .map_err(|e| Error(format!("run {}: bad photo_ids ({e})", run.id)))?;
    let template = run.args["export"].clone();
    lib.set_run_status(run.id, "running", None)?;
    let awake = format!("run:{}", run.id);
    crate::power::hold_awake(&awake, true);
    let total = ids.len();
    let (mut exported, mut resumed, mut failed) = (0, 0, Vec::new());
    for (i, &id) in ids.iter().enumerate() {
        let _ = lib.job_progress(job.id, i as i64, total as i64);
        // Stopped from the Background work panel: what's whole stays, the rest is cleared.
        if lib.job(job.id)?.status == JobStatus::Cancelled {
            crate::power::hold_awake(&awake, false);
            lib.end_run(run.id, true)?;
            return Ok(json!({ "run_id": run.id, "stopped": true, "exported": exported }));
        }
        let mut args = template.clone();
        args["photo_ids"] = json!([id]);
        args["sequence_start"] = json!(i + 1);
        args["sequence_total"] = json!(total);
        args["run_id"] = json!(run.id);
        let out = json_of(tools.call("export_photos", args)?)?;
        for x in out["exported"].as_array().into_iter().flatten() {
            if x["resumed"] == true {
                resumed += 1;
            } else {
                exported += 1;
            }
        }
        failed.extend(out["failed"].as_array().into_iter().flatten().cloned());
    }
    let _ = lib.job_progress(job.id, total as i64, total as i64);
    // The window sends the files to Apple Photos or Google Photos once the whole Export is
    // out; a quit before then leaves that to the resumed run.
    let paths: Vec<PathBuf> = lib
        .run_items(run.id)?
        .into_iter()
        .filter_map(|(_, p, done)| p.filter(|_| done))
        .collect();
    let mut sent = Vec::new();
    for to in run.args["send"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if paths.is_empty() {
            break;
        }
        let mut args = json!({ "to": to, "paths": paths });
        if let Some(a) = run.args.get("album").and_then(Value::as_str) {
            args["album"] = json!(a);
        }
        let mut lib = Library::open(tools.library())?;
        enqueue(&mut lib, "send_photos", &args, None, None)?;
        sent.push(to.to_string());
    }
    crate::power::hold_awake(&awake, false);
    lib.end_run(run.id, false)?;
    Ok(
        json!({ "run_id": run.id, "exported": exported, "already": resumed, "failed": failed, "sending": sent }),
    )
}

fn json_of(parts: Vec<crate::tools::Part>) -> Result<Value> {
    parts
        .into_iter()
        .find_map(|p| match p {
            crate::tools::Part::Json(v) => Some(v),
            _ => None,
        })
        .ok_or_else(|| Error("no result".into()))
}
