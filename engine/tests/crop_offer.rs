//! Crop like this: a hand crop is offered to the frames of the same shape shot within two
//! minutes of it, joins a Shoot match that is already waiting, changes only the crop when
//! accepted, respects Not for these, and is taken back when the hero's crop goes.

use argus_engine::library::{Author, Library};
use argus_engine::tools::{Part, Tools};
use serde_json::{json, Value};
use std::path::Path;
use std::time::{Duration, SystemTime};

fn write_jpeg(path: &Path, w: u16, h: u16, seed: u8, at: SystemTime) {
    let data: Vec<u8> = (0..w as usize * h as usize * 3)
        .map(|i| 60 + ((i as u8).wrapping_mul(seed) % 120))
        .collect();
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, w, h, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    // No EXIF: the capture time is the file's.
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

fn call(tools: &Tools, name: &str, args: Value) -> Value {
    match tools
        .call(name, args)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .remove(0)
    {
        Part::Json(v) => v,
        _ => panic!("{name}: not JSON"),
    }
}

#[test]
fn a_hand_crop_is_offered_to_the_frames_around_it() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2026-10-25_Golden-Hour");
    std::fs::create_dir_all(&src).unwrap();
    let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_000_000);
    let at = |s: u64| t0 + Duration::from_secs(s);
    // (name, seconds after the hero, portrait)
    let frames = [
        ("A0_hero.jpg", 0, false),
        ("A1_near.jpg", 30, false),
        ("A2_portrait.jpg", 20, true),
        ("A3_framed.jpg", 40, false),
        ("A4_matched.jpg", 60, false),
        ("A5_far.jpg", 300, false),
        ("A6_reject.jpg", 50, false),
    ];
    for (i, (f, s, portrait)) in frames.iter().enumerate() {
        let (w, h) = if *portrait { (120, 160) } else { (160, 120) };
        write_jpeg(&src.join(f), w, h, i as u8 * 7 + 3, at(*s));
    }
    let root = tmp.path().join("Argus.library");
    let tools = Tools::new(root.clone());
    call(&tools, "import_folder", json!({ "folder": src }));
    let photos = call(&tools, "list_photos", json!({}));
    let id = |name: &str| {
        photos
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["file_name"] == name)
            .unwrap_or_else(|| panic!("{name} not imported: {photos}"))["id"]
            .as_i64()
            .unwrap()
    };
    let shoots: std::collections::HashSet<_> = photos
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["shoot_id"].clone())
        .collect();
    assert_eq!(shoots.len(), 1, "one Shoot: {photos}");
    let [hero, near, portrait, framed, matched, far, reject] = [
        "A0_hero.jpg",
        "A1_near.jpg",
        "A2_portrait.jpg",
        "A3_framed.jpg",
        "A4_matched.jpg",
        "A5_far.jpg",
        "A6_reject.jpg",
    ]
    .map(id);
    call(
        &tools,
        "set_verdict",
        json!({ "photo_ids": [reject], "verdict": "reject" }),
    );
    {
        let mut lib = Library::open(&root).unwrap();
        // Already framed by an accepted take, and a Shoot match waiting on another frame.
        lib.write_recipe(
            framed,
            &json!({ "crop": { "x": 0.2, "y": 0.2, "w": 0.5, "h": 0.5 } }),
            Author::Ai,
            None,
        )
        .unwrap();
        lib.replace_suggestions(
            matched,
            None,
            Some(near),
            &[(
                "Shoot match".into(),
                "The style of the Photo you edited.".into(),
                json!({ "global": { "exposure": 0.4 } }),
                None,
            )],
        )
        .unwrap();
    }

    let crop = json!({ "angle": 0.0, "x": 0.1, "y": 0.15, "w": 0.6, "h": 0.6 });
    let edit = |recipe: Value| {
        argus_engine::commands::dispatch(
            &tools,
            "edit",
            json!({ "photo_id": hero, "recipe": recipe }),
        )
        .unwrap()
    };
    edit(json!({ "global": { "exposure": 0.2 }, "crop": crop }));
    let open = |photo: i64| -> Vec<Value> {
        call(&tools, "list_suggestions", json!({ "photo_id": photo }))
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["status"] == "open")
            .cloned()
            .collect()
    };
    let near_takes = open(near);
    assert_eq!(near_takes.len(), 1, "{near_takes:?}");
    assert_eq!(near_takes[0]["title"], "Crop like A0_hero");
    assert_eq!(near_takes[0]["reference_id"], hero);
    assert_eq!(near_takes[0]["recipe"]["crop"], crop);
    for (who, p) in [
        ("portrait", portrait),
        ("framed", framed),
        ("far", far),
        ("reject", reject),
    ] {
        assert!(open(p).is_empty(), "{who} is left alone: {:?}", open(p));
    }
    let m = open(matched);
    assert_eq!(m.len(), 1, "the crop joins the Shoot match: {m:?}");
    assert_eq!(m[0]["title"], "Shoot match");
    assert_eq!(m[0]["recipe"]["crop"], crop);
    assert_eq!(m[0]["recipe"]["global"]["exposure"], 0.4);
    assert!(m[0]["note"]
        .as_str()
        .unwrap()
        .ends_with("With A0_hero's crop."));

    // A slider save on the hero offers nothing twice.
    edit(json!({ "global": { "exposure": 0.3 }, "crop": crop }));
    assert_eq!(open(near).len(), 1);
    assert_eq!(open(near)[0]["id"], near_takes[0]["id"]);

    // Accepting the offer changes only the crop, even after another edit since.
    Library::open(&root)
        .unwrap()
        .write_recipe(near, &json!({ "global": { "temp": 9 } }), Author::Ai, None)
        .unwrap();
    call(
        &tools,
        "accept_suggestion",
        json!({ "suggestion_ids": [near_takes[0]["id"], m[0]["id"]] }),
    );
    let r = |p: i64| call(&tools, "read_recipe", json!({ "photo_id": p }))["recipe"].clone();
    assert_eq!(r(near)["crop"], crop);
    assert_eq!(r(near)["global"]["temp"], 9.0);
    assert_eq!(r(matched)["crop"], crop);
    assert_eq!(r(matched)["global"]["exposure"], 0.4);

    // A look accepted later keeps the frame's crop, as a preset does.
    let mut lib = Library::open(&root).unwrap();
    let look = lib
        .replace_suggestions(
            near,
            None,
            None,
            &[(
                "Warm".into(),
                "A fresh take.".into(),
                json!({ "global": { "temp": 20 } }),
                None,
            )],
        )
        .unwrap();
    drop(lib);
    call(
        &tools,
        "accept_suggestion",
        json!({ "suggestion_id": look[0] }),
    );
    assert_eq!(r(near)["crop"], crop);
    assert_eq!(r(near)["global"]["temp"], 20.0);

    // Not for these: a dismissed offer isn't made again, even for a new crop; taking the
    // hero's crop away takes the open offers back.
    let crop2 = json!({ "angle": 0.0, "x": 0.0, "y": 0.0, "w": 0.7, "h": 0.7 });
    let offers = call(&tools, "offer_crop", json!({ "photo_id": hero }));
    assert_eq!(
        offers["offered"],
        json!([]),
        "near and matched are framed now: {offers}"
    );
    Library::open(&root)
        .unwrap()
        .write_recipe(near, &json!({}), Author::Ai, None)
        .unwrap();
    edit(json!({ "crop": crop2 }));
    let again = open(near);
    assert_eq!(again.len(), 1, "the new crop, once: {again:?}");
    let offer = again
        .iter()
        .find(|s| s["title"] == "Crop like A0_hero")
        .unwrap();
    assert_eq!(offer["recipe"]["crop"], crop2);
    call(
        &tools,
        "dismiss_suggestion",
        json!({ "suggestion_id": offer["id"] }),
    );
    edit(json!({ "crop": { "x": 0.05, "y": 0.0, "w": 0.7, "h": 0.7 } }));
    assert!(
        open(near).iter().all(|s| s["title"] != "Crop like A0_hero"),
        "Not for these stands"
    );
    let r2 = call(&tools, "offer_crop", json!({ "photo_id": hero }));
    assert_eq!(r2["offered"], json!([]));
    // Back to the whole frame: nothing left to offer.
    Library::open(&root)
        .unwrap()
        .replace_suggestions(
            far,
            None,
            Some(hero),
            &[(
                "Crop like A0_hero".into(),
                "stale".into(),
                json!({ "crop": crop }),
                None,
            )],
        )
        .unwrap();
    edit(json!({}));
    assert!(open(far).is_empty(), "taken back: {:?}", open(far));
}
