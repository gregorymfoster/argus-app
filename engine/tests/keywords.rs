//! Keywords and captions through the tool layer: the person's own are kept at once, the
//! AI's (`tag_photos`, against a scripted model) stay suggestions until kept, People are
//! never the AI's to name, a dismissed suggestion isn't offered again, and what is kept
//! goes into the XMP sidecar and the exported JPEG and leaves and comes back with its Photo.

use argus_engine::agent::{Chat, Msg};
use argus_engine::commands::dispatch;
use argus_engine::compute::Compute;
use argus_engine::jobs;
use argus_engine::tools::{Part, Tools};
use argus_engine::Result;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};

fn write_jpeg(path: &Path, seed: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..160 * 120 * 3)
        .map(|i| 60 + ((i as u8).wrapping_mul(seed) % 120))
        .collect();
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, 160, 120, jpeg_encoder::ColorType::Rgb)
        .unwrap();
}

/// Answers every Photo the same way, and keeps what it was asked.
struct Fake {
    asked: Arc<Mutex<Vec<String>>>,
}

impl Chat for Fake {
    fn next(&self, _system: &str, convo: &[Msg]) -> Result<String> {
        assert_eq!(convo[0].images.len(), 1, "the model sees the Photo");
        self.asked.lock().unwrap().push(convo[0].text.clone());
        Ok(json!({
            "caption": "A wooden pier on a calm lake at dusk.",
            "keywords": ["Places|USA|Lake Tahoe", "Subjects|pier", "lake", "People|Greg", "Subjects|Nature|sunset"]
        })
        .to_string())
    }
}

fn json1(tools: &Tools, name: &str, args: Value) -> Value {
    match tools.call(name, args).unwrap().remove(0) {
        Part::Json(v) => v,
        Part::Jpeg(_) => panic!("{name} gave an image"),
    }
}

fn setup() -> (tempfile::TempDir, Tools, Vec<i64>, Arc<Mutex<Vec<String>>>) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2026-10-12_Tahoe");
    for i in 0..3u8 {
        write_jpeg(&src.join(format!("DSC0{i}.JPG")), i * 5 + 3);
    }
    let asked: Arc<Mutex<Vec<String>>> = Arc::default();
    let a = asked.clone();
    let tools = Tools::new(tmp.path().join("lib"))
        .with_chats(move |_, _, _| Ok(Box::new(Fake { asked: a.clone() }) as Box<dyn Chat>));
    let r = json1(&tools, "import_folder", json!({ "folder": src }));
    for j in json1(&tools, "list_jobs", json!({ "status": "queued" }))
        .as_array()
        .unwrap()
    {
        if j["kind"] == "analyze" {
            json1(&tools, "cancel_job", json!({ "job_id": j["id"] }));
        }
    }
    // Capture order (DSC00, DSC01, DSC02), which list_photos gives. Not sorted by id: Import's
    // workers finish in any order, so under load DSC01 can get the first id, and the Album
    // check below ("in capture order") failed about one run in ten (2026-10-22).
    let photos = json1(&tools, "list_photos", json!({ "shoot_id": r["shoot_id"] }));
    let ids: Vec<i64> = photos
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_i64().unwrap())
        .collect();
    (tmp, tools, ids, asked)
}

fn keywords(tools: &Tools, id: i64) -> Vec<(String, bool)> {
    let t = json1(tools, "read_tags", json!({ "photo_ids": [id] }));
    t[0]["keywords"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| {
            (
                k["keyword"].as_str().unwrap().to_string(),
                k["suggested"].as_bool().unwrap(),
            )
        })
        .collect()
}

#[test]
fn the_persons_keywords_are_kept_and_listed_for_autocomplete() {
    let (_tmp, tools, ids, _) = setup();
    let r = json1(
        &tools,
        "set_keywords",
        json!({ "photo_ids": [ids[0], ids[1]], "add": ["Places > Portugal > Lisbon", "dog", "Events|Birthday"] }),
    );
    assert_eq!(r["changed"], 6);
    assert_eq!(
        keywords(&tools, ids[0]),
        [
            ("Places|Portugal|Lisbon".to_string(), false),
            ("Events|Birthday".to_string(), false),
            ("Subjects|dog".to_string(), false),
        ],
        "Facets in order, a bare word under Subjects"
    );
    json1(
        &tools,
        "set_keywords",
        json!({ "photo_ids": [ids[1]], "add": ["Places|Portugal|Porto"] }),
    );
    let all = json1(&tools, "list_keywords", json!({}));
    assert_eq!(all[0]["photos"], 2);
    let por = json1(&tools, "list_keywords", json!({ "prefix": "por" }));
    let names: Vec<&str> = por
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["keyword"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["Places|Portugal|Lisbon", "Places|Portugal|Porto"],
        "any level matches"
    );

    json1(
        &tools,
        "set_keywords",
        json!({ "photo_ids": [ids[0]], "remove": ["Subjects|dog"] }),
    );
    assert_eq!(
        keywords(&tools, ids[0]).len(),
        2,
        "the person's own is simply gone"
    );
    assert!(tools
        .call(
            "set_keywords",
            json!({ "photo_ids": [ids[0]], "add": ["| ,"] })
        )
        .is_err());
    assert!(tools
        .call("read_tags", json!({ "photo_ids": [999] }))
        .is_err());

    let c = json1(
        &tools,
        "set_caption",
        json!({ "photo_ids": [ids[0]], "text": "  Lunch in Alfama. " }),
    );
    assert_eq!(c["tags"][0]["caption"]["text"], "Lunch in Alfama.");
    assert_eq!(c["tags"][0]["caption"]["suggested"], false);
    let c = json1(
        &tools,
        "set_caption",
        json!({ "photo_ids": [ids[0]], "text": "" }),
    );
    assert!(c["tags"][0]["caption"].is_null());
}

#[test]
fn the_ais_tags_are_suggestions_until_kept_and_never_name_people() {
    let (_tmp, tools, ids, asked) = setup();
    json1(
        &tools,
        "set_keywords",
        json!({ "photo_ids": [ids[0]], "add": ["Places|USA|Lake Tahoe"] }),
    );
    json1(
        &tools,
        "set_caption",
        json!({ "photo_ids": [ids[1]], "text": "My own words." }),
    );

    let q = json1(&tools, "tag_photos", json!({}));
    assert_eq!(q["status"], "queued");
    // Run the queue until the tagging job has gone (the Import queued others before it).
    let mut ran = false;
    while let Some(job) = jobs::run_next(&tools, Compute::Full).unwrap() {
        ran |= job.kind == "tag_photos";
    }
    assert!(ran);
    let prompts = asked.lock().unwrap().clone();
    assert_eq!(prompts.len(), 3, "one model call per Photo");
    assert!(
        prompts[1].contains("Places|USA|Lake Tahoe"),
        "the Library's Keywords are offered for reuse"
    );
    assert!(prompts[1].contains("Tahoe"), "and the Shoot's name");

    let k0 = keywords(&tools, ids[0]);
    assert!(
        k0.contains(&("Places|USA|Lake Tahoe".into(), false)),
        "the person's stays kept"
    );
    assert!(k0.contains(&("Subjects|pier".into(), true)));
    assert!(k0.contains(&("Subjects|lake".into(), true)));
    assert!(
        !k0.iter().any(|(k, _)| k.starts_with("People")),
        "never names People: {k0:?}"
    );
    let t = json1(
        &tools,
        "read_tags",
        json!({ "photo_ids": [ids[0], ids[1]] }),
    );
    assert_eq!(t[0]["caption"]["suggested"], true);
    assert_eq!(t[0]["caption"]["author"], "ai");
    assert_eq!(
        t[1]["caption"]["text"], "My own words.",
        "a kept caption is never replaced"
    );

    // Keep one, dismiss one; a redo offers neither again and changes nothing kept.
    json1(
        &tools,
        "set_keywords",
        json!({ "photo_ids": [ids[0]], "accept": ["Subjects|pier"], "remove": ["Subjects|lake"] }),
    );
    let q = json1(
        &tools,
        "tag_photos",
        json!({ "photo_ids": [ids[0]], "wait": true }),
    );
    assert_eq!(q["tagged"], 0, "already tagged, so skipped without redo");
    let q = json1(
        &tools,
        "tag_photos",
        json!({ "photo_ids": [ids[0]], "wait": true, "redo": true }),
    );
    assert_eq!(q["tagged"], 1);
    let k0 = keywords(&tools, ids[0]);
    assert!(k0.contains(&("Subjects|pier".into(), false)));
    assert!(
        !k0.iter().any(|(k, _)| k == "Subjects|lake"),
        "dismissed stays dismissed: {k0:?}"
    );

    // Accept all keeps every suggestion and the caption.
    json1(
        &tools,
        "set_keywords",
        json!({ "photo_ids": [ids[2]], "accept_all": true }),
    );
    let t = json1(&tools, "read_tags", json!({ "photo_ids": [ids[2]] }));
    assert_eq!(t[0]["caption"]["suggested"], false);
    assert!(t[0]["keywords"]
        .as_array()
        .unwrap()
        .iter()
        .all(|k| k["suggested"] == false));
}

#[test]
fn kept_tags_go_into_the_xmp_and_the_jpeg_and_travel_with_remove_and_restore() {
    let (tmp, tools, ids, _) = setup();
    json1(
        &tools,
        "set_keywords",
        json!({ "photo_ids": [ids[0]], "add": ["Places|Portugal|Lisbon", "Subjects|Food & drink"] }),
    );
    json1(
        &tools,
        "set_caption",
        json!({ "photo_ids": [ids[0]], "text": "Lunch <outside>." }),
    );
    tools
        .call(
            "tag_photos",
            json!({ "photo_ids": [ids[0]], "wait": true, "redo": true }),
        )
        .unwrap();

    let out = tmp.path().join("out");
    let r = json1(
        &tools,
        "export_photos",
        json!({ "photo_ids": [ids[0]], "folder": out, "format": "original", "naming": "original", "shoot_folder": false }),
    );
    let xmp =
        std::fs::read_to_string(r["exported"][0]["sidecar"].as_str().expect("a sidecar")).unwrap();
    for want in [
        "<rdf:li>Portugal</rdf:li>",
        "<rdf:li>Lisbon</rdf:li>",
        "<rdf:li>Food &amp; drink</rdf:li>",
        "<rdf:li>Places|Portugal|Lisbon</rdf:li>",
        "<lr:hierarchicalSubject>",
        "<rdf:li xml:lang=\"x-default\">Lunch &lt;outside&gt;.</rdf:li>",
        "crs:Exposure2012",
    ] {
        assert!(xmp.contains(want), "missing {want} in\n{xmp}");
    }
    assert!(
        !xmp.contains("pier"),
        "AI suggestions not kept stay out:\n{xmp}"
    );

    let r = json1(
        &tools,
        "export_photos",
        json!({ "photo_ids": [ids[0], ids[1]], "folder": tmp.path().join("jpeg"), "long_edge": 64, "naming": "original", "shoot_folder": false }),
    );
    let path = |i: usize| r["exported"][i]["path"].as_str().unwrap().to_string();
    let jpeg = std::fs::read(path(0)).unwrap();
    let text = String::from_utf8_lossy(&jpeg);
    assert!(
        text.contains("http://ns.adobe.com/xap/1.0/\0"),
        "an XMP packet"
    );
    assert!(text.contains("<rdf:li>Places|Portugal|Lisbon</rdf:li>"));
    assert!(image::load_from_memory(&jpeg).is_ok(), "still a JPEG");
    let plain = std::fs::read(path(1)).unwrap();
    assert!(
        !String::from_utf8_lossy(&plain).contains("xmpmeta"),
        "no tags, no packet"
    );

    dispatch(&tools, "remove_photos", json!({ "photo_ids": [ids[0]] })).unwrap();
    dispatch(&tools, "restore_removed", json!({})).unwrap();
    let k = keywords(&tools, ids[0]);
    assert!(
        k.contains(&("Places|Portugal|Lisbon".into(), false)),
        "back with its Photo: {k:?}"
    );
    assert!(k.contains(&("Subjects|pier".into(), true)));
}

#[test]
fn search_finds_photos_by_keywords_captions_and_names_and_albums_keep_up() {
    let (_tmp, tools, ids, _) = setup();
    json1(
        &tools,
        "set_keywords",
        json!({ "photo_ids": [ids[0]], "add": ["Subjects|Nature|autumn", "Subjects|friends"] }),
    );
    json1(
        &tools,
        "set_keywords",
        json!({ "photo_ids": [ids[1]], "add": ["Subjects|Nature|autumn"] }),
    );
    json1(
        &tools,
        "set_caption",
        json!({ "photo_ids": [ids[2]], "text": "Friends by the lake in autumn." }),
    );

    let r = json1(&tools, "search_photos", json!({ "query": "Autumn frie" }));
    assert_eq!(r["count"], 2, "every word must match: {r}");
    assert_eq!(
        r["photos"][0]["photo_id"], ids[0],
        "kept Keywords rank above a caption"
    );
    assert_eq!(r["photos"][1]["photo_id"], ids[2]);
    assert_eq!(r["photos"][1]["matched"], json!(["caption"]));
    assert_eq!(r["keywords"][0]["keyword"], "Subjects|friends");
    assert_eq!(
        json1(&tools, "search_photos", json!({ "query": "DSC01" }))["count"],
        1,
        "file names"
    );
    assert_eq!(
        json1(&tools, "search_photos", json!({ "query": "tahoe" }))["count"],
        3,
        "Shoot names"
    );
    assert_eq!(
        json1(&tools, "search_photos", json!({ "query": "  " }))["count"],
        0
    );

    let a = json1(
        &tools,
        "save_album",
        json!({ "name": "Autumn", "query": "autumn" }),
    );
    let albums = json1(&tools, "list_albums", json!({}));
    assert_eq!(albums[0]["photos"], 3);
    json1(
        &tools,
        "set_keywords",
        json!({ "photo_ids": [ids[1]], "remove": ["Subjects|Nature|autumn"] }),
    );
    assert_eq!(
        json1(&tools, "list_albums", json!({}))[0]["photos"],
        2,
        "an Album keeps up"
    );
    let shown = dispatch(&tools, "photos", json!({ "album_id": a["id"] })).unwrap();
    let shown: Vec<i64> = shown
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_i64().unwrap())
        .collect();
    // Capture order, which Import's parallel copy can make differ from id order.
    let order: Vec<i64> = json1(&tools, "list_photos", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_i64().unwrap())
        .filter(|id| [ids[0], ids[2]].contains(id))
        .collect();
    assert_eq!(shown, order, "the grid shows an Album in capture order");
    json1(
        &tools,
        "edit_album",
        json!({ "album_id": a["id"], "name": "Fall" }),
    );
    assert_eq!(json1(&tools, "list_albums", json!({}))[0]["name"], "Fall");
    assert!(tools
        .call("save_album", json!({ "name": " ", "query": "x" }))
        .is_err());
    json1(&tools, "delete_album", json!({ "album_id": a["id"] }));
    assert!(json1(&tools, "list_albums", json!({}))
        .as_array()
        .unwrap()
        .is_empty());
}
