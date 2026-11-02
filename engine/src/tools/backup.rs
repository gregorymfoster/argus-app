//! Backup tools: Backup places (copies of the Library on another disk, or in the Google
//! Drive or iCloud Drive folder) and running them. See `library::places` and ADR 0017.

use super::*;
use crate::library::{find_places, Copies, Place};

pub(super) fn specs() -> Vec<ToolSpec> {
    let copies = json!({ "type": "string", "enum": ["edits", "originals", "everything"],
        "description": "edits: the database only; originals (default): originals and the database; everything: previews too" });
    let when =
        json!({ "type": "string", "enum": ["after_import", "daily", "on_connect", "manual"] });
    vec![
        ToolSpec {
            name: "backup_places",
            description: "The Library's Backup places (copies of the Library on an external disk, a network disk, or in the Google Drive or iCloud Drive folder) with their status: connected, free space, what they copy and when, Photos in the copy and waiting, last backup, last check, history. Also how many Photos have at least two copies elsewhere.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
        ToolSpec {
            name: "find_backup_places",
            description: "Folders on this Mac that would make good Backup places: the Google Drive, Dropbox and OneDrive app folders, iCloud Drive, and connected disks. Each with its free space and whether it's already in use.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
        ToolSpec {
            name: "estimate_backup",
            description: "How big a first backup of the Library to a folder would be, whether it fits, and roughly how long writing it takes. Reads sizes only.",
            input_schema: json!({ "type": "object", "required": ["folder"], "properties": {
                "folder": { "type": "string", "description": "Absolute path" },
                "copies": copies.clone(),
                "include_elsewhere": { "type": "boolean" } } }),
            read_only: true,
        },
        ToolSpec {
            name: "add_backup_place",
            description: "Add a Backup place: Argus keeps a copy of the Library in <folder>/Argus-Backup/. Pass run: true to start the first backup on the Queue.",
            input_schema: json!({ "type": "object", "required": ["folder"], "properties": {
                "folder": { "type": "string", "description": "Absolute path to an existing folder" },
                "name": { "type": "string" },
                "copies": copies.clone(),
                "include_elsewhere": { "type": "boolean", "description": "Also copy originals the Library reads where they are" },
                "when": when.clone(),
                "keep_snapshots": { "type": "integer", "minimum": 0 },
                "verify_monthly": { "type": "boolean" },
                "run": { "type": "boolean" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "update_backup_place",
            description: "Change a Backup place's name, what it copies, when it runs, how many database copies it keeps, or its monthly check.",
            input_schema: json!({ "type": "object", "required": ["id"], "properties": {
                "id": { "type": "string" },
                "name": { "type": "string" },
                "copies": copies,
                "include_elsewhere": { "type": "boolean" },
                "when": when,
                "keep_snapshots": { "type": "integer", "minimum": 0 },
                "verify_monthly": { "type": "boolean" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "remove_backup_place",
            description: "Stop backing up to a place. Its copy stays where it is; Argus never deletes a backup.",
            input_schema: json!({ "type": "object", "required": ["id"], "properties": { "id": { "type": "string" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "run_backup",
            description: "Back the Library up now, on the Queue (progress and ETA in list_jobs). id: one place (default every connected place that copies). shoot_id: only that Shoot's originals plus the database (Finish Shoot). verify: re-read every file in the copy and check it instead. Returns the queued jobs.",
            input_schema: json!({ "type": "object", "properties": {
                "id": { "type": "string" },
                "shoot_id": { "type": "integer" },
                "verify": { "type": "boolean" },
                "why": { "type": "string", "description": "Shown in the place's history, e.g. \"Finish Shoot\"" },
                "priority": { "type": "integer" } } }),
            read_only: false,
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddPlace {
    folder: String,
    name: Option<String>,
    copies: Option<Copies>,
    include_elsewhere: Option<bool>,
    when: Option<crate::library::BackupWhen>,
    keep_snapshots: Option<usize>,
    verify_monthly: Option<bool>,
    run: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdatePlace {
    id: String,
    name: Option<String>,
    copies: Option<Copies>,
    include_elsewhere: Option<bool>,
    when: Option<crate::library::BackupWhen>,
    keep_snapshots: Option<usize>,
    verify_monthly: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EstimateBackup {
    folder: String,
    copies: Option<Copies>,
    include_elsewhere: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: String,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunBackup {
    pub id: Option<String>,
    pub shoot_id: Option<ShootId>,
    pub verify: Option<bool>,
    pub why: Option<String>,
    pub priority: Option<i64>,
}

/// Places with a backup job queued or running now.
fn running(lib: &Library) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for status in [
        crate::library::JobStatus::Running,
        crate::library::JobStatus::Queued,
    ] {
        for j in lib.jobs(Some(status), Some("backup"), 100)? {
            if let Some(id) = j.args.get("place").and_then(Value::as_str) {
                out.push(id.to_string());
            }
        }
    }
    Ok(out)
}

/// Queue backups: one place, or every connected one (Edits-only places skip a Shoot's run
/// only if nothing changed; the database always goes). Returns the job ids.
pub(crate) fn queue_backups(lib: &mut Library, a: &RunBackup) -> Result<Vec<Value>> {
    let places: Vec<Place> = match &a.id {
        Some(id) => vec![lib.backup_place(id)?],
        None => lib
            .backup_places()?
            .into_iter()
            .filter(Place::available)
            .collect(),
    };
    let mut out = Vec::new();
    for p in places {
        let mut args = json!({ "place": p.id });
        if let Some(s) = a.shoot_id {
            args["shoot_id"] = json!(s);
        }
        if a.verify == Some(true) {
            args["verify"] = json!(true);
        }
        if let Some(w) = &a.why {
            args["why"] = json!(w);
        }
        let (job_id, created) = jobs::enqueue(lib, "backup", &args, a.priority, None)?;
        out.push(json!({ "place": p.id, "job_id": job_id, "created": created }));
    }
    Ok(out)
}

impl Tools {
    pub(super) fn call_backup(
        &self,
        name: &str,
        args: Value,
        mut lib: Library,
        _batch: Option<BatchId>,
    ) -> Result<Vec<Part>> {
        match name {
            "backup_places" => {
                let places = lib.backup_status(&running(&lib)?)?;
                one(json!({
                    "places": places,
                    "photos": lib.photo_count()?,
                    "two_copies": lib.photos_backed_up(2)?,
                    "one_copy": lib.photos_backed_up(1)?,
                }))
            }
            "find_backup_places" => {
                let home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_default();
                let volumes = std::env::var_os("ARGUS_VOLUMES")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("/Volumes"));
                let used: Vec<PathBuf> =
                    lib.backup_places()?.into_iter().map(|p| p.folder).collect();
                one(find_places(&home, &volumes, &used))
            }
            "estimate_backup" => {
                let a: EstimateBackup = parse(args)?;
                one(lib.estimate_backup(
                    Path::new(&a.folder),
                    a.copies.unwrap_or_default(),
                    a.include_elsewhere.unwrap_or(false),
                )?)
            }
            "add_backup_place" => {
                let a: AddPlace = parse(args)?;
                let place = lib.add_backup_place(Place {
                    id: String::new(),
                    name: a.name.unwrap_or_default(),
                    folder: PathBuf::from(a.folder),
                    copies: a.copies.unwrap_or_default(),
                    include_elsewhere: a.include_elsewhere.unwrap_or(false),
                    when: a.when.unwrap_or_default(),
                    keep_snapshots: a.keep_snapshots.unwrap_or(30),
                    verify_monthly: a.verify_monthly.unwrap_or(true),
                    copy_name: String::new(),
                })?;
                let jobs = if a.run == Some(true) {
                    queue_backups(
                        &mut lib,
                        &RunBackup {
                            id: Some(place.id.clone()),
                            why: Some("First backup".into()),
                            ..Default::default()
                        },
                    )?
                } else {
                    Vec::new()
                };
                one(json!({ "place": place, "jobs": jobs }))
            }
            "update_backup_place" => {
                let a: UpdatePlace = parse(args)?;
                let mut p = lib.backup_place(&a.id)?;
                if let Some(v) = a.name {
                    p.name = v;
                }
                if let Some(v) = a.copies {
                    p.copies = v;
                }
                if let Some(v) = a.include_elsewhere {
                    p.include_elsewhere = v;
                }
                if let Some(v) = a.when {
                    p.when = v;
                }
                if let Some(v) = a.keep_snapshots {
                    p.keep_snapshots = v;
                }
                if let Some(v) = a.verify_monthly {
                    p.verify_monthly = v;
                }
                one(lib.update_backup_place(&p)?)
            }
            "remove_backup_place" => {
                let a: Id = parse(args)?;
                one(lib.remove_backup_place(&a.id)?)
            }
            "run_backup" => {
                let a: RunBackup = parse(args)?;
                one(queue_backups(&mut lib, &a)?)
            }
            other => Err(unknown_tool(other)),
        }
    }
}
