//! Suggestion tools: ask for, read, accept and dismiss Suggestions.

use super::*;

pub(super) fn specs() -> Vec<ToolSpec> {
    let id = json!({ "type": "integer" });
    let ids = json!({ "type": "array", "items": { "type": "integer" }, "minItems": 1 });
    vec![
        ToolSpec {
            name: "suggest_edits",
            description: "Queue Suggestions: 2-3 complete edits (sliders, crop, straighten) the AI works out in the background for each Photo, for the person to tab through and accept. Nothing changes until they accept one. One Photo, a list, or a whole Shoot (rejects skipped, Picks first; Photos that already have Suggestions skipped).",
            input_schema: json!({ "type": "object", "properties": {
                "photo_ids": ids, "shoot_id": id,
                "count": { "type": "integer", "minimum": 1, "maximum": 5 },
                "provider": { "type": "string", "description": "ollama, claude-code or claude-api; default ARGUS_PROVIDER, else ollama" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "match_to_photo",
            description: "\"Apply this look to the rest\": the hero Photo's style becomes a new Look of its Shoot, and every other Photo (or photo_ids) gets 2 matched Suggestions under it, with its own exposure, white balance and crop, worked out in the background. On a RAW the first is the Shoot match: the style of the hero and the Shoot's other hand-edited Photos, moved for this Photo's own light without a model call. Nothing changes until the person accepts them.",
            input_schema: json!({ "type": "object", "required": ["photo_id"], "properties": {
                "photo_id": id, "photo_ids": ids,
                "provider": { "type": "string", "description": "ollama, claude-code or claude-api; default ARGUS_PROVIDER, else ollama" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "pre_edit_shoot",
            description: "Edit as you cull: give every Pick and likely keeper (4 stars and up) of a Shoot that the person hasn't edited a Shoot match Suggestion, the style of the RAWs they edited by hand in it moved for each frame's own light, in the background with no model call. Runs by itself after each hand edit and as the stars refresh; Photos with other matched takes, or whose Shoot match was accepted or dismissed, are left alone. Nothing changes until the person accepts.",
            input_schema: json!({ "type": "object", "required": ["shoot_id"], "properties": { "shoot_id": id } }),
            read_only: false,
        },
        ToolSpec {
            name: "list_suggestions",
            description: "A Photo's or a whole Shoot's Suggestions, open and accepted (dismissed ones too with all: true), best first: title, why, status, the whole Recipe, and for matched ones the reference Photo.",
            input_schema: json!({ "type": "object", "properties": {
                "photo_id": id, "shoot_id": id, "all": { "type": "boolean" } } }),
            read_only: true,
        },
        ToolSpec {
            name: "get_suggestion",
            description: "One Suggestion and a JPEG of the Photo with it applied, at amount (the take's Amount slider: 0 = the Photo as it is, 1 = the take, the default, up to 1.5).",
            input_schema: json!({ "type": "object", "required": ["suggestion_id"], "properties": {
                "suggestion_id": id, "long_edge": { "type": "integer", "minimum": 64 },
                "amount": { "type": "number", "minimum": 0, "maximum": 1.5 } } }),
            read_only: true,
        },
        ToolSpec {
            name: "accept_suggestion",
            description: "Accept a Suggestion, or several (one per Photo) with suggestion_ids: each becomes its Photo's new Recipe Version, all in one undoable batch, at amount (the Amount slider: 0..1.5, default 1). The person's call; the AI never accepts its own.",
            input_schema: json!({ "type": "object", "properties": { "suggestion_id": id, "suggestion_ids": ids,
                "amount": { "type": "number", "minimum": 0, "maximum": 1.5 } } }),
            read_only: false,
        },
        ToolSpec {
            name: "dismiss_suggestion",
            description: "Dismiss a Suggestion so it no longer shows.",
            input_schema: json!({ "type": "object", "required": ["suggestion_id"], "properties": { "suggestion_id": id } }),
            read_only: false,
        },
    ]
}

impl Tools {
    pub(super) fn call_suggestions(
        &self,
        name: &str,
        args: Value,
        mut lib: Library,
        _batch: Option<BatchId>,
    ) -> Result<Vec<Part>> {
        match name {
            "suggest_edits" => {
                let a: SuggestEdits = parse(args)?;
                let mut args = json!({});
                if let Some(c) = a.count {
                    args["count"] = json!(c);
                }
                if let Some(p) = &a.provider {
                    args["provider"] = json!(p);
                }
                let (kind, args) = match (a.photo_ids, a.shoot_id) {
                    (Some(ids), _) if ids.len() == 1 => {
                        args["photo_id"] = json!(ids[0]);
                        ("suggest", args)
                    }
                    (Some(ids), _) => {
                        args["photo_ids"] = json!(ids);
                        ("suggest_shoot", args)
                    }
                    (None, shoot) => {
                        args["shoot_id"] = json!(match shoot {
                            Some(s) => s,
                            None => newest_shoot(&lib)?,
                        });
                        ("suggest_shoot", args)
                    }
                };
                let (job_id, created) = jobs::enqueue(&mut lib, kind, &args, None, None)?;
                one(json!({ "job_id": job_id, "created": created }))
            }
            "match_to_photo" => {
                let a: MatchToPhoto = parse(args)?;
                find_photo(&lib, a.photo_id)?;
                let mut args = json!({ "photo_id": a.photo_id });
                if let Some(ids) = a.photo_ids {
                    args["photo_ids"] = json!(ids);
                }
                if let Some(p) = &a.provider {
                    args["provider"] = json!(p);
                }
                let (job_id, created) = jobs::enqueue(&mut lib, "match_hero", &args, None, None)?;
                one(json!({ "job_id": job_id, "created": created }))
            }
            "pre_edit_shoot" => {
                let a: ShootArg = parse(args)?;
                let (job_id, created) = jobs::enqueue(
                    &mut lib,
                    "pre_edit_shoot",
                    &json!({ "shoot_id": a.shoot_id }),
                    None,
                    None,
                )?;
                one(json!({ "job_id": job_id, "created": created }))
            }
            "list_suggestions" => {
                let a: ListSuggestions = parse(args)?;
                one(lib.suggestions(a.photo_id, a.shoot_id, a.all.unwrap_or(false))?)
            }
            "get_suggestion" => {
                let a: SuggestionArg = parse(args)?;
                let sug = lib.suggestion(a.suggestion_id)?;
                let edge = a.long_edge.unwrap_or(1024).clamp(64, MAX_PREVIEW_EDGE);
                // At another Amount the saved preview doesn't show it: render.
                let amount = a.amount.filter(|&v| v != 1.0);
                let saved = sug
                    .preview
                    .as_ref()
                    .map(|p| lib.root().join(p))
                    .filter(|p| p.exists() && amount.is_none());
                let jpeg = match saved {
                    Some(p) => {
                        let img = image::open(&p)
                            .map_err(|e| Error(format!("{}: {e}", p.display())))?
                            .thumbnail(edge, edge)
                            .to_rgb8();
                        encode_jpeg(&img)?
                    }
                    None => {
                        let photo = find_photo(&lib, sug.photo_id)?;
                        let recipe = Recipe::from_value(&match amount {
                            Some(v) => lib.suggestion_at(&sug, v)?,
                            None => sug.recipe.clone(),
                        })?;
                        let img = render::render_in(
                            &lib,
                            &photo,
                            &with_look(&lib, &photo, &recipe)?,
                            Some(edge as usize),
                        )?;
                        render::encode_jpeg(&img, 85)?
                    }
                };
                Ok(vec![Part::Json(to_json(&sug)?), Part::Jpeg(jpeg)])
            }
            "accept_suggestion" => {
                let a: AcceptSuggestion = parse(args)?;
                let ids: Vec<i64> = a
                    .suggestion_id
                    .into_iter()
                    .chain(a.suggestion_ids.unwrap_or_default())
                    .collect();
                if ids.is_empty() {
                    return Err(Error("give suggestion_id or suggestion_ids".into()));
                }
                let (batch_id, done) = lib.accept_suggestions(&ids, Author::Person, a.amount)?;
                let photos: Vec<Value> = done
                    .iter()
                    .map(|(version, s)| json!({ "photo_id": s.photo_id, "version": version, "title": s.title }))
                    .collect();
                let mut v = json!({ "batch_id": batch_id, "photos": photos });
                if let [(version, s)] = done.as_slice() {
                    v["photo_id"] = json!(s.photo_id);
                    v["version"] = json!(version);
                    v["title"] = json!(s.title);
                }
                one(v)
            }
            "dismiss_suggestion" => {
                let a: SuggestionArg = parse(args)?;
                lib.dismiss_suggestion(a.suggestion_id)?;
                one(json!({ "dismissed": a.suggestion_id }))
            }
            other => Err(unknown_tool(other)),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuggestEdits {
    photo_ids: Option<Vec<PhotoId>>,
    shoot_id: Option<ShootId>,
    count: Option<usize>,
    provider: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MatchToPhoto {
    photo_id: PhotoId,
    photo_ids: Option<Vec<PhotoId>>,
    provider: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListSuggestions {
    photo_id: Option<PhotoId>,
    shoot_id: Option<ShootId>,
    all: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptSuggestion {
    suggestion_id: Option<i64>,
    suggestion_ids: Option<Vec<i64>>,
    amount: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuggestionArg {
    suggestion_id: i64,
    long_edge: Option<u32>,
    amount: Option<f64>,
}
