//! Preset tools: list, rank, apply, save and delete Presets.

use super::*;

pub(super) fn specs() -> Vec<ToolSpec> {
    let id = json!({ "type": "integer" });
    let ids = json!({ "type": "array", "items": { "type": "integer" }, "minItems": 1 });
    vec![
        ToolSpec {
            name: "list_presets",
            description: "Argus's Presets, as in Lightroom's Presets panel: the person's own (group Yours, saved with save_preset, each with the settings groups it holds) first, then the built-in groups (Essentials, Portrait, Landscape, Film, Cinematic, Vintage, Black and white, Seasons, Subject, and Adaptive ones that add Masks for faces, sky or subject) with each Preset's id, name and what it suits. With shoot_id, also the order the AI suggests for that Shoot when one has been worked out (rank_presets).",
            input_schema: json!({ "type": "object", "properties": { "shoot_id": id } }),
            read_only: true,
        },
        ToolSpec {
            name: "rank_presets",
            description: "The AI's suggested order of the Presets for a Shoot: a few representative Photos (Best Photos, then Picks) are each rendered with every Preset and the model picks what suits them, best first, with a few words why. A suggestion order, not a verdict. Returns the cached order when there is one; otherwise queues it in the background (a few model calls) unless wait is true, which runs it now and returns it. refresh redoes it; now puts the queued job first.",
            input_schema: json!({ "type": "object", "required": ["shoot_id"], "properties": {
                "shoot_id": id, "refresh": { "type": "boolean" }, "wait": { "type": "boolean" }, "now": { "type": "boolean" },
                "provider": { "type": "string", "enum": ["ollama", "claude-code", "claude-api"] } } }),
            read_only: false,
        },
        ToolSpec {
            name: "apply_preset",
            description: "Apply a Preset (list_presets) to Photos at an Amount, as one undoable batch, like clicking it in Lightroom. Its settings add to each Photo's own edit (so it stacks on Auto); B&W ones set saturation. A Look-style Preset replaces the one applied before; Adaptive ones (Masks) stack. amount 0..200 percent (default 100); applying again with another amount re-scales it; remove takes it off, leaving the person's own changes.",
            input_schema: json!({ "type": "object", "required": ["photo_ids", "preset"], "properties": {
                "photo_ids": ids, "preset": { "type": "string" },
                "amount": { "type": "number", "minimum": 0, "maximum": 200 },
                "remove": { "type": "boolean" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "save_preset",
            description: "Lightroom's Create Preset: save one Photo's settings (as it looks, Look included) as one of the person's own Presets, group Yours, kept in the Library. groups are the settings groups it holds: wb, light, color, effects, detail, optics, calibration, retouch, masks; by default all but wb and masks. Applied with apply_preset, its settings are set rather than added, a setting of those groups it leaves out goes back to neutral, and amount moves each from the Photo's value toward it. With id as well as photo_id it updates that Preset with the Photo's settings; with id and no photo_id it only renames it. Names are unique among the person's Presets.",
            input_schema: json!({ "type": "object", "required": ["name"], "properties": {
                "name": { "type": "string" }, "photo_id": id, "id": { "type": "string" },
                "groups": { "type": "array", "items": { "enum": crate::presets::PRESET_GROUPS } } } }),
            read_only: false,
        },
        ToolSpec {
            name: "delete_preset",
            description: "Delete one of the person's own Presets (group Yours). Photos it was applied to keep their settings.",
            input_schema: json!({ "type": "object", "required": ["id"], "properties": { "id": { "type": "string" } } }),
            read_only: false,
        },
    ]
}

impl Tools {
    pub(super) fn call_presets(
        &self,
        name: &str,
        args: Value,
        mut lib: Library,
        batch: Option<BatchId>,
    ) -> Result<Vec<Part>> {
        match name {
            "list_presets" => {
                let a: OptShoot = parse(args)?;
                list_presets(&lib, a.shoot_id)
            }
            "rank_presets" => {
                let a: RankPresets = parse(args)?;
                shoot_photos(&lib, a.shoot_id)?;
                if !a.refresh {
                    if let Some(r) = crate::preset_rank::cached(&lib, a.shoot_id)? {
                        return one(json!({ "status": "ready", "ranking": r }));
                    }
                }
                if a.wait {
                    let compute = Compute::from_env();
                    let task = crate::progress::Task::start(
                        "rank_presets",
                        "Ranking Presets".to_string(),
                        crate::preset_rank::PHOTOS,
                    );
                    let r = crate::preset_rank::rank(
                        &lib,
                        a.shoot_id,
                        a.provider.as_deref(),
                        compute,
                        &|_, _| task.tick(),
                    )?;
                    return one(json!({ "status": "ready", "ranking": r }));
                }
                let mut args = json!({ "shoot_id": a.shoot_id });
                if let Some(p) = &a.provider {
                    args["provider"] = json!(p);
                }
                let priority = a.now.then_some(jobs::PRIORITY_FOCUS);
                let (job_id, created) =
                    jobs::enqueue(&mut lib, "rank_presets", &args, priority, None)?;
                one(json!({ "status": "queued", "job_id": job_id, "created": created }))
            }
            "apply_preset" => apply_preset(&mut lib, parse(args)?, batch),
            "save_preset" => save_preset(&lib, parse(args)?),
            "delete_preset" => {
                let a: DeletePreset = parse(args)?;
                crate::presets::delete(&lib, &a.id)?;
                one(json!({ "deleted": a.id }))
            }
            other => Err(unknown_tool(other)),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RankPresets {
    shoot_id: ShootId,
    #[serde(default)]
    refresh: bool,
    #[serde(default)]
    wait: bool,
    #[serde(default)]
    now: bool,
    provider: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OptShoot {
    shoot_id: Option<ShootId>,
}

fn list_presets(lib: &Library, shoot: Option<ShootId>) -> Result<Vec<Part>> {
    let yours = crate::presets::yours(lib)?;
    let presets: Vec<Value> = yours
        .iter()
        .chain(crate::presets::builtin())
        .map(|p| {
            let mut v = json!({ "id": p.id, "name": p.name, "group": p.group, "why": p.why,
            "masks": p.local.iter().map(|l| l.mask.clone()).collect::<Vec<_>>() });
            if p.is_yours() {
                v["settings"] = json!(p.groups);
            }
            v
        })
        .collect();
    let mut groups: Vec<&str> = crate::presets::groups()
        .iter()
        .map(String::as_str)
        .collect();
    if !yours.is_empty() {
        groups.insert(0, crate::presets::YOURS);
    }
    let ranking = match shoot {
        Some(s) => crate::preset_rank::cached(lib, s)?.map(|r| json!(r)),
        None => None,
    };
    one(json!({ "groups": groups, "presets": presets, "ranking": ranking }))
}

/// Lightroom's Create Preset, Update with current settings and Rename ([`crate::presets`]).
fn save_preset(lib: &Library, a: SavePreset) -> Result<Vec<Part>> {
    let p = match (a.photo_id, &a.id) {
        (None, Some(id)) => crate::presets::rename(lib, id, &a.name)?,
        (Some(photo_id), _) => {
            let photo = find_photo(lib, photo_id)?;
            // What the Photo looks like, its Look included.
            let recipe = with_look(lib, &photo, &current_recipe(lib, photo_id)?)?;
            let groups: Vec<&str> = match &a.groups {
                Some(g) => g.iter().map(String::as_str).collect(),
                None => crate::presets::default_groups(),
            };
            let p = crate::presets::from_recipe(&a.name, &recipe, &groups)?;
            crate::presets::save(lib, p, a.id.as_deref())?
        }
        (None, None) => {
            return Err(Error(
                "pass photo_id to save a Preset from, or id to rename one".into(),
            ))
        }
    };
    one(json!({ "id": p.id, "name": p.name, "group": p.group, "settings": p.groups }))
}

fn apply_preset(lib: &mut Library, a: ApplyPreset, batch: Option<BatchId>) -> Result<Vec<Part>> {
    let yours = crate::presets::yours(lib)?;
    let p = &crate::presets::find_in(lib, &a.preset)?;
    let amount = a.amount.unwrap_or(100.0).clamp(0.0, 200.0) / 100.0;
    let mut todo = Vec::new();
    for &id in &a.photo_ids {
        find_photo(lib, id)?;
        let own = current_recipe(lib, id)?;
        let r = if a.remove {
            crate::presets::remove(&own, p)?
        } else {
            crate::presets::apply_among(&own, p, amount, &yours)?
        };
        todo.push((id, r));
    }
    let label = if a.remove {
        format!("Removed {}", p.name)
    } else {
        format!("Preset: {}", p.name)
    };
    let batch = batch_or(lib, batch, &label)?;
    let mut out = BTreeMap::new();
    for (id, r) in todo {
        let version = lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
        out.insert(id, json!({ "version": version }));
    }
    one(json!({ "batch_id": batch, "preset": p.id, "amount": amount * 100.0, "photos": out }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyPreset {
    photo_ids: Vec<PhotoId>,
    preset: String,
    amount: Option<f64>,
    #[serde(default)]
    remove: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavePreset {
    name: String,
    photo_id: Option<PhotoId>,
    groups: Option<Vec<String>>,
    id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeletePreset {
    id: String,
}
