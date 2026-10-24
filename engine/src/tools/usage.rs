//! Usage log and style profile tools.

use super::*;

pub(super) fn specs() -> Vec<ToolSpec> {
    let id = json!({ "type": "integer" });
    vec![
        ToolSpec {
            name: "usage_log",
            description: "The usage log Argus keeps in this Library (never uploaded): each Photo's starting Recipe, every AI take with its Provider and model, what the person accepted, flagged and exported. Returns counts; recording turns logging on or off (Settings › General, on by default); clear deletes the whole log; photo_id adds that Photo's events and folded history (kept take, sliders moved from it).",
            input_schema: json!({ "type": "object", "properties": {
                "recording": { "type": "boolean" }, "clear": { "type": "boolean" }, "photo_id": id } }),
            read_only: false,
        },
        ToolSpec {
            name: "style_profile",
            description: "The person's style profile, learned from the usage log: how they usually correct the AI's takes before exporting (median slider changes on Photos where they kept a take, shrunk while there are few). Fresh Suggestions add these moves to every take; a take name with enough corrections of its own gets its own (by_take). Empty below min_photos corrections. corrections, a list of {take, moved: {slider: change}}, learns from that list instead of this Library (benches, tests).",
            input_schema: json!({ "type": "object", "properties": {
                "corrections": { "type": "array", "items": { "type": "object" } } } }),
            read_only: true,
        },
        ToolSpec {
            name: "export_usage_bench",
            description: "Write the usage log as bench cases in fixtures/bench's layout (edit/cases.json with start and exported renders, cull/<Shoot>/labels.json with the person's and the AI's Verdicts and stars), for `python3 bench/score.py --bench <folder>`. images: false writes only the JSON.",
            input_schema: json!({ "type": "object", "required": ["folder"], "properties": {
                "folder": { "type": "string" }, "images": { "type": "boolean" },
                "long_edge": { "type": "integer", "minimum": 64, "maximum": 2048, "description": "Default 640, as the bench fixtures" } } }),
            read_only: false,
        },
    ]
}

impl Tools {
    pub(super) fn call_usage(
        &self,
        name: &str,
        args: Value,
        lib: Library,
        _batch: Option<BatchId>,
    ) -> Result<Vec<Part>> {
        match name {
            "usage_log" => {
                let a: UsageLogArgs = parse(args)?;
                if let Some(on) = a.recording {
                    lib.set_recording_usage(on)?;
                }
                let cleared = if a.clear {
                    Some(lib.clear_usage()?)
                } else {
                    None
                };
                let mut out = to_json(lib.usage_summary()?)?;
                out["cleared"] = json!(cleared);
                if let Some(id) = a.photo_id {
                    let events = lib.usage_events(Some(id))?;
                    out["history"] = to_json(crate::usage::history(&events))?;
                    out["photo_events"] = to_json(events)?;
                }
                one(out)
            }
            "style_profile" => {
                let a: StyleArgs = parse(args)?;
                let p = match &a.corrections {
                    Some(c) => crate::style::Profile::learn(&crate::style::corrections_from(c)?),
                    None => crate::style::Profile::of_library(&lib)?,
                };
                let mut out = to_json(&p)?;
                out["line"] = json!(crate::style::Profile::line(&p.moves));
                out["min_photos"] = json!(crate::style::MIN_PHOTOS);
                one(out)
            }
            "export_usage_bench" => {
                let a: ExportUsageBench = parse(args)?;
                let folder = export::expand_home(&a.folder);
                one(crate::usage::export_bench(
                    self,
                    &folder,
                    a.images.unwrap_or(true),
                    a.long_edge.unwrap_or(640),
                )?)
            }
            other => Err(unknown_tool(other)),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StyleArgs {
    corrections: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UsageLogArgs {
    recording: Option<bool>,
    #[serde(default)]
    clear: bool,
    photo_id: Option<PhotoId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportUsageBench {
    folder: String,
    images: Option<bool>,
    long_edge: Option<u32>,
}
