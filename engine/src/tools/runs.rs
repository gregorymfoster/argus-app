//! Resumable runs: the window's Export (one `export_photos` call per Photo) records itself
//! here, so a quit, a crash or a closed lid never loses it ([`crate::library::Run`]).

use super::*;

pub(super) fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "start_run",
            description: "Record an Export before it starts, so it finishes after a quit or crash: kind export, args {photo_ids, export (export_photos' settings, without photo_ids), send? ([apple_photos, google_photos]: where the files go once all are out, as send_photos), album?}. Pass the returned run_id to each export_photos call, then finish_run. Keeps the Mac from idle sleep while it runs. A run whose process is gone resumes on the Queue from the Photo it reached.",
            input_schema: json!({ "type": "object", "required": ["kind", "args"], "properties": {
                "kind": { "enum": ["export"] },
                "label": { "type": "string" },
                "args": { "type": "object", "required": ["photo_ids", "export"], "properties": {
                    "photo_ids": { "type": "array", "items": { "type": "integer" }, "minItems": 1 },
                    "export": { "type": "object" },
                    "send": { "type": "array", "items": { "enum": ["apple_photos", "google_photos"] } },
                    "album": { "type": "string" } } } } }),
            read_only: false,
        },
        ToolSpec {
            name: "finish_run",
            description: "End a run from start_run: done, or stopped (the person stopped it: the file being written is cleared, the finished ones stay).",
            input_schema: json!({ "type": "object", "required": ["run_id"], "properties": {
                "run_id": { "type": "integer" },
                "stopped": { "type": "boolean" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "list_runs",
            description: "Exports and Imports not finished: running, or paused (note quit: Argus quit or crashed and the Queue finishes it; card: its card is not connected), with how many Photos are done.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
    ]
}

impl Tools {
    pub(super) fn call_runs(
        &self,
        name: &str,
        args: Value,
        lib: Library,
        _batch: Option<BatchId>,
    ) -> Result<Vec<Part>> {
        match name {
            "start_run" => {
                let a: StartRun = parse(args)?;
                let ids = a.args.get("photo_ids").and_then(Value::as_array);
                let total = ids.map_or(0, |v| v.len()) as i64;
                if total == 0 || !a.args.get("export").is_some_and(Value::is_object) {
                    return Err(Error(
                        "start_run needs args.photo_ids and args.export".into(),
                    ));
                }
                let label = a.label.unwrap_or_else(|| format!("Exporting {total}"));
                let id = lib.start_run(&a.kind, &label, &a.args, total)?;
                crate::power::hold_awake(&format!("run:{id}"), true);
                one(json!({ "run_id": id }))
            }
            "finish_run" => {
                let a: FinishRun = parse(args)?;
                one(end_run(&lib, a.run_id, a.stopped.unwrap_or(false))?)
            }
            "list_runs" => one(lib.unfinished_runs()?),
            other => Err(unknown_tool(other)),
        }
    }
}

/// End a run ([`Library::end_run`]) and let the Mac sleep again.
fn end_run(lib: &Library, id: crate::library::RunId, stopped: bool) -> Result<Value> {
    let run = lib.end_run(id, stopped)?;
    crate::power::hold_awake(&format!("run:{id}"), false);
    Ok(serde_json::to_value(run).unwrap_or_default())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartRun {
    kind: String,
    label: Option<String>,
    args: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FinishRun {
    run_id: i64,
    stopped: Option<bool>,
}
