//! People tools: faces, naming and hiding them.

use super::*;

pub(super) fn specs() -> Vec<ToolSpec> {
    let id = json!({ "type": "integer" });
    vec![
        ToolSpec {
            name: "list_people",
            description: "The People view: everyone the person has named (with how many Photos and a cover face), and groups of unnamed faces that look like one person, biggest first, each with the named person it looks most like (maybe) when alike enough to ask. Faces are {photo_id, face}, face numbered from 0 left to right as analyze_photo lists them. Grouping runs on this Mac and never names anyone.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
        ToolSpec {
            name: "read_faces",
            description: "Queue the face models for Photos whose faces aren't read yet (list_people counts them as unread_photos), so they join the People view. Runs in the background Queue at the Compute level.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: false,
        },
        ToolSpec {
            name: "get_person",
            description: "One named person: their Photos in capture order, their faces, and unnamed faces that might be them (suggestions, most alike first) for the person to confirm with name_faces or turn down with not_person.",
            input_schema: json!({ "type": "object", "required": ["person_id"], "properties": { "person_id": id } }),
            read_only: true,
        },
        ToolSpec {
            name: "name_faces",
            description: "Name faces as someone: only with a name the person gave you, never one you guessed. A new name makes a person; an existing one (any case) adds to them. Each Photo keeps the Keyword People|<name>.",
            input_schema: json!({ "type": "object", "required": ["faces", "name"], "properties": {
                "faces": json!({ "type": "array", "minItems": 1, "items": { "type": "object", "required": ["photo_id", "face"],
                "properties": { "photo_id": { "type": "integer" }, "face": { "type": "integer", "minimum": 0 } } } }), "name": { "type": "string" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "hide_faces",
            description: "Hide faces from the People view as not someone to name (strangers, a poster), taking any name off them; unhide brings them back to name.",
            input_schema: json!({ "type": "object", "required": ["faces"], "properties": {
                "faces": json!({ "type": "array", "minItems": 1, "items": { "type": "object", "required": ["photo_id", "face"],
                "properties": { "photo_id": { "type": "integer" }, "face": { "type": "integer", "minimum": 0 } } } }), "unhide": { "type": "boolean" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "not_person",
            description: "The person says these faces aren't this person: they're unnamed if they were, and never suggested as them again.",
            input_schema: json!({ "type": "object", "required": ["faces", "person_id"], "properties": {
                "faces": json!({ "type": "array", "minItems": 1, "items": { "type": "object", "required": ["photo_id", "face"],
                "properties": { "photo_id": { "type": "integer" }, "face": { "type": "integer", "minimum": 0 } } } }), "person_id": id } }),
            read_only: false,
        },
        ToolSpec {
            name: "rename_person",
            description: "Rename a person, and the People Keyword on their Photos. A name someone else already has merges the two.",
            input_schema: json!({ "type": "object", "required": ["person_id", "name"], "properties": {
                "person_id": id, "name": { "type": "string" } } }),
            read_only: false,
        },
    ]
}

impl Tools {
    pub(super) fn call_people(
        &self,
        name: &str,
        args: Value,
        mut lib: Library,
        _batch: Option<BatchId>,
    ) -> Result<Vec<Part>> {
        match name {
            "list_people" => one(crate::people::overview(&lib)?),
            "read_faces" => {
                let (job, photos) = crate::people::read_faces(&mut lib)?;
                one(json!({ "job_id": job, "photos": photos }))
            }
            "get_person" => {
                let a: PersonArg = parse(args)?;
                one(crate::people::person(&lib, a.person_id)?)
            }
            "name_faces" => {
                let a: NameFaces = parse(args)?;
                one(lib.name_faces(&a.faces, &a.name)?)
            }
            "hide_faces" => {
                let a: HideFaces = parse(args)?;
                one(json!({ "changed": lib.hide_faces(&a.faces, !a.unhide)? }))
            }
            "not_person" => {
                let a: NotPerson = parse(args)?;
                one(json!({ "changed": lib.not_person(&a.faces, a.person_id)? }))
            }
            "rename_person" => {
                let a: RenamePerson = parse(args)?;
                one(lib.rename_person(a.person_id, &a.name)?)
            }
            other => Err(unknown_tool(other)),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonArg {
    person_id: crate::library::PersonId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NameFaces {
    faces: Vec<crate::library::FaceRef>,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HideFaces {
    faces: Vec<crate::library::FaceRef>,
    #[serde(default)]
    unhide: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NotPerson {
    faces: Vec<crate::library::FaceRef>,
    person_id: crate::library::PersonId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenamePerson {
    person_id: crate::library::PersonId,
    name: String,
}
