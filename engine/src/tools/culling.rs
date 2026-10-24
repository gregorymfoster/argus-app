//! The AI passes over a Shoot: tagging, Stacks, Best Photos, the AI cull and re-rating.

use super::*;

pub(super) fn specs() -> Vec<ToolSpec> {
    let id = json!({ "type": "integer" });
    let ids = json!({ "type": "array", "items": { "type": "integer" }, "minItems": 1 });
    vec![
        ToolSpec {
            name: "tag_photos",
            description: "The AI suggests a caption and Keywords for Photos (a Shoot's, rejects skipped, or photo_ids), one model call per Photo. Suggestions stay dashed until the person keeps them; the AI never names People. Photos it tagged before are skipped unless redo. Queued in the background (it waits for an idle Mac on the local model) unless wait is true, which runs it now; now puts the queued job first.",
            input_schema: json!({ "type": "object", "properties": {
                "shoot_id": id, "photo_ids": ids, "redo": { "type": "boolean" },
                "wait": { "type": "boolean" }, "now": { "type": "boolean" },
                "provider": { "type": "string", "enum": ["ollama", "claude-code", "claude-api"] } } }),
            read_only: false,
        },
        ToolSpec {
            name: "best_photos",
            description: "A Shoot's Best Photos, best first: the AI cull's short list of the strongest frames (about one in ten, at most one per Stack), leaving out any the person rejected. Empty until the Shoot is culled.",
            input_schema: json!({ "type": "object", "properties": {
                "shoot_id": { "type": "integer", "description": "Defaults to the newest Shoot" },
                "limit": { "type": "integer", "minimum": 1 } } }),
            read_only: true,
        },
        ToolSpec {
            name: "list_stacks",
            description: "List a Shoot's Stacks (bursts or repeated shots of one moment), each as Photo ids best first.",
            input_schema: json!({ "type": "object", "required": ["shoot_id"], "properties": { "shoot_id": id } }),
            read_only: true,
        },
        ToolSpec {
            name: "stack_bursts",
            description: "Find a Shoot's bursts (frames seconds apart that look alike) and join them into its Stacks, keeping the Stacks it has. Returns every Stack, best frame first. Import does this by itself once a Shoot's Photos are analyzed; a cull keeps the bursts too.",
            input_schema: json!({ "type": "object", "required": ["shoot_id"], "properties": { "shoot_id": id } }),
            read_only: false,
        },
        ToolSpec {
            name: "set_stacks",
            description: "Replace all of a Shoot's Stacks. Each inner list is one Stack of Photo ids, best frame first; lists of one are ignored.",
            input_schema: json!({ "type": "object", "required": ["shoot_id", "stacks"], "properties": {
                "shoot_id": id,
                "stacks": { "type": "array", "items": { "type": "array", "items": { "type": "integer" } } } } }),
            read_only: false,
        },
        ToolSpec {
            name: "cull_shoot",
            description: "Have Argus cull a whole Shoot in one pass: Verdicts with reasons, star Ratings, Stacks and the Shoot's Best Photos, keeping the person's own flags and stars. By default it needs no model and takes seconds; check \"model\" asks a model too, and local models can take many minutes. If you can see images yourself, you can instead cull with get_contact_sheet, set_verdict, set_rating and set_stacks.",
            input_schema: json!({ "type": "object", "properties": {
                "shoot_id": { "type": "integer", "description": "Defaults to the newest Shoot" },
                "provider": { "type": "string", "enum": ["ollama", "claude-code", "claude-api", "rules"],
                    "description": "The model for check \"model\"; rules is the same as check \"rules\"" },
                "check": { "type": "string", "enum": ["rules", "model"],
                    "description": "Who looks for technically broken frames: rules (default, from sharpness, seconds) or the model as a second opinion (also shut eyes and subtle shake; local models take many minutes)" },
                "compute": { "type": "string", "enum": ["gentle", "balanced", "full"],
                    "description": "How much of the Mac a local model may take; gentle unless the person said otherwise" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "rerate_shoot",
            description: "Learn as you cull: refresh a culled Shoot's AI star Ratings from the person's own Picks and Rejects so far (here and on other Shoots), so the likely keepers rise, and return `order`: every Photo but the Rejects, the likeliest keeper first (Keepers first). Only the AI's stars change, never flags, Stacks, Best Photos or the person's stars; not an undo step. Takes under a second.",
            input_schema: json!({ "type": "object", "required": ["shoot_id"], "properties": { "shoot_id": id } }),
            read_only: false,
        },
    ]
}

impl Tools {
    pub(super) fn call_culling(
        &self,
        name: &str,
        args: Value,
        mut lib: Library,
        _batch: Option<BatchId>,
    ) -> Result<Vec<Part>> {
        match name {
            "tag_photos" => {
                let a: TagPhotos = parse(args)?;
                if a.wait {
                    let chat = self.chat(
                        a.provider.as_deref(),
                        Some(Compute::from_env()),
                        crate::tagging::schema(),
                    )?;
                    let photos =
                        crate::tagging::photos(&lib, a.shoot_id, a.photo_ids.as_deref(), a.redo)?;
                    drop(lib);
                    let n = crate::tagging::tag(&self.library, chat.as_ref(), &photos, &|_, _| {})?;
                    let lib = Library::open(&self.library)?;
                    return one(
                        json!({ "status": "ready", "tagged": n, "tags": lib.tags(&photos)? }),
                    );
                }
                let mut args = json!({ "redo": a.redo });
                if let Some(s) = a.shoot_id {
                    args["shoot_id"] = json!(s);
                }
                if let Some(p) = &a.photo_ids {
                    args["photo_ids"] = json!(p);
                }
                if let Some(p) = &a.provider {
                    args["provider"] = json!(p);
                }
                let priority = a.now.then_some(jobs::PRIORITY_FOCUS);
                let (job_id, created) =
                    jobs::enqueue(&mut lib, "tag_photos", &args, priority, None)?;
                one(json!({ "status": "queued", "job_id": job_id, "created": created }))
            }
            "best_photos" => {
                let a: BestPhotos = parse(args)?;
                let shoot = match a.shoot_id {
                    Some(s) => s,
                    None => newest_shoot(&lib)?,
                };
                let photos = shoot_photos(&lib, shoot)?;
                let best: Vec<PhotoView> = lib
                    .best_photos(shoot)?
                    .iter()
                    .filter_map(|b| photos.iter().find(|p| p.id == b.photo_id))
                    .take(a.limit.unwrap_or(usize::MAX))
                    .map(PhotoView::from)
                    .collect();
                one(best)
            }
            "list_stacks" => {
                let a: ShootArg = parse(args)?;
                one(stacks(&lib, a.shoot_id)?)
            }
            "stack_bursts" => {
                let a: ShootArg = parse(args)?;
                crate::bursts::stack_shoot(&mut lib, a.shoot_id, true)?;
                one(stacks(&lib, a.shoot_id)?)
            }
            "set_stacks" => {
                let a: SetStacks = parse(args)?;
                lib.replace_stacks(a.shoot_id, &a.stacks)?;
                one(stacks(&lib, a.shoot_id)?)
            }
            "cull_shoot" => {
                let a: CullShoot = parse(args)?;
                let shoot = match a.shoot_id {
                    Some(s) => s,
                    None => newest_shoot(&lib)?,
                };
                // Without a level it runs as the queue would now: Full while the Mac is idle
                // or overnight, else the queue's own level.
                let compute = match a.compute {
                    Some(c) => c,
                    None => jobs::effective_compute(&jobs::config(&lib)?),
                };
                let provider = cull::cull_provider(a.check, a.provider.as_deref(), |name| {
                    (self.providers)(name, Some(compute))
                })?;
                // The app's cull bar reads this task (`progress`).
                let task = crate::progress::Task::start("cull", "Reading the photos", 0);
                // Cancel (`cancel_task`) reaches the model's streaming reply through this.
                let _in = task.enter();
                let _who = crate::model_lock::as_who(format!(
                    "the AI cull in {}",
                    crate::model_lock::who()
                ));
                one(cull::cull_shoot_with(
                    &mut lib,
                    shoot,
                    provider.as_ref(),
                    cull::max_per_request(),
                    &|p| {
                        let (done, total) = p.bar();
                        task.set(done, total);
                        task.relabel(p.stage());
                        task.set_waiting(p.waiting.clone());
                    },
                )?)
            }
            "rerate_shoot" => {
                let a: ShootArg = parse(args)?;
                one(cull::rerate(&mut lib, a.shoot_id)?)
            }
            other => Err(unknown_tool(other)),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TagPhotos {
    shoot_id: Option<ShootId>,
    photo_ids: Option<Vec<PhotoId>>,
    #[serde(default)]
    redo: bool,
    #[serde(default)]
    wait: bool,
    #[serde(default)]
    now: bool,
    provider: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BestPhotos {
    shoot_id: Option<ShootId>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetStacks {
    shoot_id: ShootId,
    stacks: Vec<Vec<PhotoId>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CullShoot {
    shoot_id: Option<ShootId>,
    provider: Option<String>,
    compute: Option<Compute>,
    check: Option<cull::Check>,
}

fn stacks(lib: &Library, shoot: ShootId) -> Result<Vec<Vec<PhotoId>>> {
    let mut by_stack: BTreeMap<i64, Vec<(i64, PhotoId)>> = BTreeMap::new();
    for p in shoot_photos(lib, shoot)? {
        if let Some(s) = p.stack {
            by_stack
                .entry(s)
                .or_default()
                .push((p.stack_rank.unwrap_or(i64::MAX), p.id));
        }
    }
    Ok(by_stack
        .into_values()
        .map(|mut m| {
            m.sort();
            m.into_iter().map(|(_, id)| id).collect()
        })
        .collect())
}
