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
                jobs::set_config(&lib, &c)?;
                one(queue_status(&lib)?)
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
