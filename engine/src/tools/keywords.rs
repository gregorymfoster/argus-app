//! Keyword, caption, Album and search tools.

use super::*;

pub(super) fn specs() -> Vec<ToolSpec> {
    let id = json!({ "type": "integer" });
    let ids = json!({ "type": "array", "items": { "type": "integer" }, "minItems": 1 });
    vec![
        ToolSpec {
            name: "read_tags",
            description: "Each Photo's caption and Keywords, Lightroom style: hierarchical paths under a Facet (People, Places, Events, Activities, Subjects), e.g. Places|Portugal|Lisbon. Each says who added it and whether it is still an AI suggestion the person hasn't kept. Dismissed suggestions are left out.",
            input_schema: json!({ "type": "object", "required": ["photo_ids"], "properties": { "photo_ids": ids } }),
            read_only: true,
        },
        ToolSpec {
            name: "set_keywords",
            description: TAGGING_DOC,
            input_schema: json!({ "type": "object", "required": ["photo_ids"], "properties": {
                "photo_ids": ids,
                "add": { "type": "array", "items": { "type": "string" }, "description": "Keywords to add, kept at once" },
                "remove": { "type": "array", "items": { "type": "string" }, "description": "Keywords to take off (an AI suggestion is dismissed)" },
                "accept": { "type": "array", "items": { "type": "string" }, "description": "AI suggestions to keep" },
                "accept_all": { "type": "boolean", "description": "Keep every AI suggestion, caption included" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "set_caption",
            description: "Set the caption (IPTC Description: one or two plain sentences saying who, what, where) of Photos, as the person's own. Empty text removes it. Written to the XMP sidecar on export.",
            input_schema: json!({ "type": "object", "required": ["photo_ids", "text"], "properties": {
                "photo_ids": ids, "text": { "type": "string" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "search_photos",
            description: "Find Photos by words, best first: every word must match the start of a word in a Photo's Keywords (kept ones count most, AI suggestions too), its caption, its file name or its Shoot's name. Returns how many matched and in how many Shoots, the best `limit` (default 24) with what matched, and Library Keywords that start like the last word.",
            input_schema: json!({ "type": "object", "required": ["query"], "properties": {
                "query": { "type": "string" }, "shoot_id": id, "limit": { "type": "integer", "minimum": 1 } } }),
            read_only: true,
        },
        ToolSpec {
            name: "meaning_model",
            description: "Whether search by meaning (finding Photos that look like the words, not only by Keywords) is installed: its model is a one-time 407 MB download into the models folder. `download: true` queues the download (progress on the Queue), after which every Photo is analysed so search can find it.",
            input_schema: json!({ "type": "object", "properties": { "download": { "type": "boolean" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "list_albums",
            description: "The Library's Albums: saved searches (like Lightroom's smart collections) shown in the sidebar, each with its query and how many Photos it finds now.",
            input_schema: json!({ "type": "object", "properties": {} }),
            read_only: true,
        },
        ToolSpec {
            name: "save_album",
            description: "Save a search (search_photos words) as an Album. It finds its Photos again each time, so it keeps up with Imports and new Keywords.",
            input_schema: json!({ "type": "object", "required": ["name", "query"], "properties": {
                "name": { "type": "string" }, "query": { "type": "string" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "edit_album",
            description: "Rename an Album, or change the search it saves.",
            input_schema: json!({ "type": "object", "required": ["album_id"], "properties": {
                "album_id": id, "name": { "type": "string" }, "query": { "type": "string" } } }),
            read_only: false,
        },
        ToolSpec {
            name: "delete_album",
            description: "Delete an Album. Its Photos stay in the Library.",
            input_schema: json!({ "type": "object", "required": ["album_id"], "properties": { "album_id": id } }),
            read_only: false,
        },
        ToolSpec {
            name: "list_keywords",
            description: "Keywords in use across the Library, most used first, with how many Photos keep each. prefix narrows to those whose path or any level starts with it. Reuse these names when tagging so the Library stays consistent.",
            input_schema: json!({ "type": "object", "properties": {
                "prefix": { "type": "string" }, "limit": { "type": "integer", "minimum": 1 } } }),
            read_only: true,
        },
    ]
}

impl Tools {
    pub(super) fn call_keywords(
        &self,
        name: &str,
        args: Value,
        mut lib: Library,
        _batch: Option<BatchId>,
    ) -> Result<Vec<Part>> {
        match name {
            "read_tags" => {
                let a: PhotoIds = parse(args)?;
                one(lib.tags(&a.photo_ids)?)
            }
            "set_keywords" => {
                let a: SetKeywords = parse(args)?;
                let add = keywords_arg(&a.add)?;
                let remove = keywords_arg(&a.remove)?;
                let accept = a.accept.as_deref().map(keywords_arg).transpose()?;
                let mut changed = 0;
                if !add.is_empty() {
                    changed += lib.add_keywords(&a.photo_ids, &add)?.changed;
                }
                if !remove.is_empty() {
                    changed += lib.remove_keywords(&a.photo_ids, &remove)?.changed;
                }
                if a.accept_all {
                    changed += lib.accept_tags(&a.photo_ids, None)?.changed;
                } else if let Some(k) = &accept {
                    changed += lib.accept_tags(&a.photo_ids, Some(k))?.changed;
                }
                one(json!({ "changed": changed, "tags": lib.tags(&a.photo_ids)? }))
            }
            "set_caption" => {
                let a: SetCaption = parse(args)?;
                let r = lib.set_caption(&a.photo_ids, &a.text)?;
                one(json!({ "changed": r.changed, "tags": lib.tags(&a.photo_ids)? }))
            }
            "search_photos" => {
                let a: SearchPhotos = parse(args)?;
                let hits = crate::meaning::search(&lib, &a.query, a.shoot_id)?;
                let shoots: std::collections::BTreeSet<ShootId> =
                    hits.iter().map(|h| h.shoot_id).collect();
                let last = a.query.split_whitespace().last().unwrap_or("");
                let keywords = lib.keyword_counts(Some(last), 5)?;
                let count = hits.len();
                let photos: Vec<Value> = hits
                    .into_iter()
                    .take(a.limit.unwrap_or(24))
                    .map(|h| {
                        let p = find_photo(&lib, h.photo_id)?;
                        let preview = p.preview.strip_prefix(lib.root()).unwrap_or(&p.preview).to_string_lossy().replace('\\', "/");
                        Ok(json!({ "photo_id": h.photo_id, "shoot_id": h.shoot_id, "file_name": p.file_name,
                                   "preview": preview, "score": h.score, "matched": h.matched }))
                    })
                    .collect::<Result<_>>()?;
                one(
                    json!({ "count": count, "shoots": shoots.len(), "photos": photos, "keywords": keywords }),
                )
            }
            "meaning_model" => {
                let a: MeaningModel = parse(args)?;
                let mut v = json!(crate::meaning::status());
                if a.download && !crate::meaning::available() {
                    let (job, _) =
                        jobs::enqueue(&mut lib, "download_model", &json!({}), None, None)?;
                    v["job_id"] = json!(job);
                }
                one(v)
            }
            "list_albums" => {
                let albums: Vec<Value> = lib
                    .albums()?
                    .into_iter()
                    .map(|al| {
                        let n = album_photos(&lib, &al.query).map(|p| p.len()).unwrap_or(0);
                        let mut v = json!(al);
                        v["photos"] = json!(n);
                        v
                    })
                    .collect();
                one(albums)
            }
            "save_album" => {
                let a: SaveAlbum = parse(args)?;
                one(lib.save_album(&a.name, &json!({ "text": a.query }))?)
            }
            "edit_album" => {
                let a: EditAlbum = parse(args)?;
                let q = a.query.as_ref().map(|t| json!({ "text": t }));
                lib.edit_album(a.album_id, a.name.as_deref(), q.as_ref())?;
                one(json!({ "album_id": a.album_id }))
            }
            "delete_album" => {
                let a: AlbumArg = parse(args)?;
                lib.delete_album(a.album_id)?;
                one(json!({ "deleted": a.album_id }))
            }
            "list_keywords" => {
                let a: ListKeywords = parse(args)?;
                one(lib.keyword_counts(a.prefix.as_deref(), a.limit.unwrap_or(50))?)
            }
            other => Err(unknown_tool(other)),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PhotoIds {
    photo_ids: Vec<PhotoId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetKeywords {
    photo_ids: Vec<PhotoId>,
    #[serde(default)]
    add: Vec<String>,
    #[serde(default)]
    remove: Vec<String>,
    accept: Option<Vec<String>>,
    #[serde(default)]
    accept_all: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetCaption {
    photo_ids: Vec<PhotoId>,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchPhotos {
    query: String,
    shoot_id: Option<ShootId>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveAlbum {
    name: String,
    query: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditAlbum {
    album_id: i64,
    name: Option<String>,
    query: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MeaningModel {
    #[serde(default)]
    download: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AlbumArg {
    album_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListKeywords {
    prefix: Option<String>,
    limit: Option<usize>,
}

const TAGGING_DOC: &str = "Add, remove or keep Keywords on Photos, as the person asked. Keywords are hierarchical paths joined with |, under People, Places, Events, Activities or Subjects (a path without one goes under Subjects): Places|Portugal|Lisbon, Events|Birthday, Subjects|Animals|dog. Reuse names from list_keywords. add keeps them at once; remove takes them off (an AI suggestion is remembered as dismissed); accept keeps named AI suggestions and accept_all keeps all of them and the caption. Name a person under People only when the person named them. Written to the XMP sidecar (dc:subject, lr:hierarchicalSubject) on export.";

/// Keywords from a caller, each read with [`naming::Keyword::parse`]; one that has no
/// words left is an error so the caller can fix it.
fn keywords_arg(texts: &[String]) -> Result<Vec<crate::naming::Keyword>> {
    texts
        .iter()
        .map(|t| {
            crate::naming::Keyword::parse(t).ok_or_else(|| Error(format!("not a Keyword: {t:?}")))
        })
        .collect()
}
