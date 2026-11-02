//! Queue tools: background jobs and the Queue's settings.

use super::*;

pub(super) fn specs() -> Vec<ToolSpec> {
    let id = json!({ "type": "integer" });
    let ids = json!({ "type": "array", "items": { "type": "integer" }, "minItems": 1 });
    vec![
        ToolSpec {
            name: "enqueue_job",
            description: "Queue background AI work. kind: suggest {photo_id, count?, reference_id?, look_id?}, suggest_shoot {shoot_id?, photo_ids?, count?, redo?}, match_hero {photo_id, photo_ids?}, cull_shoot (or rate_shoot) {shoot_id?}, tag_photos {shoot_id?, photo_ids?, redo?} (captions and Keywords), render_previews {shoot_id?}, analyze {shoot_id?, photo_ids?} (the specialist models' Facts). Asking twice for the same thing returns the queued job.",
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
            description: "Set how hard the background queue works: compute gentle (default, leaves the Mac to the person), balanced or full; pause it; how many idle minutes before it goes to full on its own (0 = never); whether it goes to full overnight (1:00-7:00); whether the Photo open in Loupe or Edit jumps the queue; how many Suggestions each Photo gets (1-5); run_on_battery to always run background work on battery (off by default); run_anyway to run on battery until the Mac is next plugged in; idle_only (on by default) to hold model work (Suggestions, culls) until the Mac is idle and plugged in, or overnight; run_now (Run it all) to run every held job now at Full, keeping the Mac awake, until the queue is empty; and shoot_match_smart (on by default) for the Shoot match that reads the Shoot's hand edits as a Gaussian process and leans on the Edit head for unfamiliar frames (off: the first rule, a similarity-weighted mean).",
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
                "run_now": { "type": "boolean" },
                "shoot_match_smart": { "type": "boolean" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "prepare_shoot",
            description: "Prepare a Shoot (Import does this by itself): its heavy AI work, Suggestions and a denoised base for every likely keeper, runs now at Full, ahead of other Shoots and past the idle and overnight holds, keeping the Mac awake; on battery it waits for the charger unless run_anyway. stop:true lifts it; gentle:true is Cull now (carries on at the queue's own level so the app stays quick). Returns prepare_status.",
            input_schema: json!({ "type": "object", "properties": {
                "shoot_id": id, "stop": { "type": "boolean" }, "gentle": { "type": "boolean" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "prepare_status",
            description: "How Prepare stands: null when no Shoot is preparing, else shoot_ids, shoot_names, photos, keepers, state (running, battery, paused, done), eta_s, gentle, and stages [{label, kinds, done, total, eta_s, state, progress?}] in order: Previews, Faces focus level, Stacks and AI flags, Edits for likely keepers, Denoise noisy keepers, then Keywords and Backup (left for Finish). last_done is the last finished Prepare (finished_at, stages) for the ready notification. held is what Run it all would run: jobs by kind and eta_s.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
        ToolSpec {
            name: "finish_shoot",
            description: "Finish a Shoot: queue its captions and Keywords and a backup to every connected Backup place (run_backup), and run them now at Full like Prepare. Exports are separate (export_photos). Returns {tag_job, backups: [{place, job_id, created}]} plus prepare_status; backups is empty when no place is connected.",
            input_schema: json!({ "type": "object", "required": ["shoot_id"], "properties": { "shoot_id": id } }),
            read_only: false,
        },
        ToolSpec {
            name: "retry_failed",
            description: "Queue failed background jobs again (say a model was missing and is back). Repeats of the same job run once; returns queue_status plus retried, how many were queued.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: false,
        },
    ]
}

impl Tools {
    pub(super) fn call_queue(
        &self,
        name: &str,
        args: Value,
        mut lib: Library,
        _batch: Option<BatchId>,
    ) -> Result<Vec<Part>> {
        match name {
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
            "suggestions_progress" => {
                let a: PhotoArg = parse(args)?;
                one(jobs::suggestions_progress(&lib, a.photo_id)?)
            }
            "reorder_jobs" => {
                let a: ReorderJobs = parse(args)?;
                one(json!({ "moved": lib.reorder_jobs(&a.job_ids)? }))
            }
            "queue_status" => one(queue_status(&lib)?),
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
                if let Some(v) = a.shoot_match_smart {
                    c.shoot_match_smart = v;
                }
                jobs::set_config(&lib, &c)?;
                one(queue_status(&lib)?)
            }
            "prepare_shoot" => {
                let a: PrepareShoot = parse(args)?;
                if a.stop == Some(true) {
                    crate::prepare::stop(&lib)?;
                } else if let Some(s) = a.shoot_id {
                    crate::prepare::start(&mut lib, s, &crate::library::time::now())?;
                }
                if let Some(g) = a.gentle {
                    let mut c = jobs::config(&lib)?;
                    c.prepare_gentle = g;
                    jobs::set_config(&lib, &c)?;
                }
                one(prepare_status(&lib)?)
            }
            "prepare_status" => one(prepare_status(&lib)?),
            "finish_shoot" => {
                let a: ShootArg = parse(args)?;
                let (tag, _) = jobs::enqueue(
                    &mut lib,
                    "tag_photos",
                    &json!({ "shoot_id": a.shoot_id }),
                    None,
                    None,
                )?;
                let backups = super::backup::queue_backups(
                    &mut lib,
                    &super::backup::RunBackup {
                        id: None,
                        shoot_id: Some(a.shoot_id),
                        verify: None,
                        why: Some("Finish Shoot".into()),
                        priority: None,
                    },
                )?;
                // The finishing work runs now, like Run it all.
                let mut c = jobs::config(&lib)?;
                c.go_ahead = true;
                jobs::set_config(&lib, &c)?;
                let mut v = prepare_status(&lib)?;
                v["tag_job"] = json!(tag);
                v["backups"] = json!(backups);
                one(v)
            }
            "retry_failed" => {
                let n = lib.retry_failed_jobs()?;
                let mut v = queue_status(&lib)?;
                v["retried"] = json!(n);
                one(v)
            }
            other => Err(unknown_tool(other)),
        }
    }
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
struct PrepareShoot {
    shoot_id: Option<crate::library::ShootId>,
    stop: Option<bool>,
    gentle: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShootArg {
    shoot_id: crate::library::ShootId,
}

/// Prepare's state ([`crate::prepare::status`]), the last finished one, and what Run it all
/// would run.
fn prepare_status(lib: &Library) -> Result<Value> {
    let mut held = serde_json::Map::new();
    let mut eta = 0.0;
    for (kind, n) in lib.queued_by_kind(jobs::PRIORITY_FOCUS)? {
        let avg = lib.average_job_seconds(&kind)?.unwrap_or(30.0);
        eta += n as f64 * avg;
        held.insert(kind, json!(n));
    }
    Ok(json!({
        "prepare": crate::prepare::status(lib)?,
        "last_done": crate::prepare::last_done(lib)?,
        "held": { "jobs": held, "eta_s": eta.round() },
    }))
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
    shoot_match_smart: Option<bool>,
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
    v["preparing"] = json!(c.preparing);
    v["shoot_match_smart"] = json!(c.shoot_match_smart);
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
