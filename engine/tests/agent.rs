//! The Agent's loop against a scripted model: the request becomes tool calls, every change
//! lands in one batch, and one undo reverts the lot.

use argus_engine::agent::{self, Ask, Chat, Msg, Role, Turn};
use argus_engine::tools::{Part, Tools};
use argus_engine::Result;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::path::Path;

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

fn library(n: u8) -> (tempfile::TempDir, Tools, Vec<i64>) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2026-10-12_tahoe");
    for i in 0..n {
        write_jpeg(&src.join(format!("p{i:02}.jpg")), i * 2 + 3);
    }
    let tools = Tools::new(tmp.path().join("Argus.library"));
    tools
        .call("import_folder", json!({ "folder": src }))
        .unwrap();
    let ids = json1(&tools, "list_photos", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_i64().unwrap())
        .collect();
    (tmp, tools, ids)
}

fn json1(tools: &Tools, name: &str, args: Value) -> Value {
    match tools
        .call(name, args)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .remove(0)
    {
        Part::Json(v) => v,
        p => panic!("{name}: expected JSON, got {p:?}"),
    }
}

/// Replies with the scripted turns in order and records every conversation it was shown.
struct Script {
    turns: RefCell<Vec<String>>,
    seen: RefCell<Vec<Vec<Msg>>>,
}

impl Script {
    fn new(turns: &[Value]) -> Self {
        Self {
            turns: RefCell::new(turns.iter().rev().map(|t| t.to_string()).collect()),
            seen: RefCell::new(vec![]),
        }
    }
}

impl Chat for Script {
    fn next(&self, _system: &str, convo: &[Msg]) -> Result<String> {
        self.seen.borrow_mut().push(convo.to_vec());
        Ok(self
            .turns
            .borrow_mut()
            .pop()
            .expect("the Agent asked for more turns than scripted"))
    }
}

#[test]
fn warmer_less_contrast_is_one_relative_edit_in_one_batch_and_one_undo() {
    let (_t, tools, ids) = library(3);
    tools
        .call(
            "edit_recipe",
            json!({ "photo_ids": [ids[1]], "set": { "temp": 5 } }),
        )
        .unwrap();
    let model = Script::new(&[
        json!({ "say": "Warming it up", "calls": [
            { "tool": "edit_recipe", "args": { "photo_ids": [ids[1]], "set": { "temp": 12, "contrast": -15 }, "relative": true } },
            { "tool": "render_photo", "args": { "photo_id": ids[1], "long_edge": 128 } } ] }),
        json!({ "say": "Temperature +12 (now +17) and Contrast -15.", "calls": [] }),
    ]);
    let ask = Ask {
        prompt: "warmer, less contrast".into(),
        photo_id: Some(ids[1]),
        ..Default::default()
    };
    let r = agent::run_with(&tools, &ask, &model).unwrap();

    assert_eq!(r.text, "Temperature +12 (now +17) and Contrast -15.");
    assert_eq!(r.recipes, vec![ids[1]]);
    assert_eq!(r.steps.len(), 2);
    assert!(r.steps.iter().all(|s| s.ok));
    let recipe = json1(&tools, "read_recipe", json!({ "photo_id": ids[1] }));
    assert_eq!(
        recipe["recipe"]["global"],
        json!({ "temp": 17.0, "contrast": -15.0 })
    );
    assert_eq!(recipe["batch_id"], json!(r.batch_id));

    // The model saw the Shoot first: the Contact sheet and the Photo in focus as images.
    let first = &model.seen.borrow()[0];
    assert!(
        first[0].text.contains("In focus: Photo"),
        "{}",
        first[0].text
    );
    // With its light in numbers and Auto's values as a starting point.
    assert!(
        first[0].text.contains("Its light: luma mean")
            && first[0]
                .text
                .contains("Auto (Lightroom-style, deterministic) would set: {"),
        "{}",
        first[0].text
    );
    assert_eq!(
        first[0].images.len(),
        2,
        "Contact sheet and the focused render"
    );
    assert_eq!(first.last().unwrap().text, "warmer, less contrast");
    // Then the tool results, with the render it asked for.
    let second = &model.seen.borrow()[1];
    let results = second.last().unwrap();
    assert_eq!(results.role, Role::User);
    assert!(
        results.text.contains("[1] edit_recipe ->"),
        "{}",
        results.text
    );
    assert_eq!(results.images.len(), 1);

    // One undo puts it back to the person's own Temperature +5.
    tools
        .call("undo_batch", json!({ "batch_id": r.batch_id }))
        .unwrap();
    let back = json1(&tools, "read_recipe", json!({ "photo_id": ids[1] }));
    assert_eq!(back["recipe"]["global"], json!({ "temp": 5.0 }));
    let history = json1(&tools, "list_versions", json!({ "photo_id": ids[1] }));
    assert_eq!(history[1]["label"], "Agent: warmer, less contrast");
}

#[test]
fn whole_shoot_consistency_uses_a_look_and_per_photo_corrections_in_one_batch() {
    let (_t, tools, ids) = library(3);
    let shoot = json1(&tools, "list_shoots", json!({}))[0]["id"].clone();
    let model = Script::new(&[
        json!({ "say": "Matching to the brightest frame", "calls": [
            { "tool": "set_look", "args": { "shoot_id": shoot, "name": "Warm film", "set": { "temp": 20, "contrast": -10 }, "photo_ids": ids } },
            { "tool": "edit_recipe", "args": { "photo_ids": [ids[0]], "set": { "correction.exposure": 0.3 } } },
            { "tool": "edit_recipe", "args": { "photo_ids": [ids[2]], "set": { "correction.wbK": -150 } } } ] }),
        json!({ "say": "Done.", "calls": [] }),
    ]);
    let r = agent::run_with(
        &tools,
        &Ask {
            prompt: "make the whole shoot consistent".into(),
            ..Default::default()
        },
        &model,
    )
    .unwrap();
    assert!(r.looks);
    let mut want = ids.clone();
    want.sort();
    assert_eq!(r.recipes, want);
    let ctx = &model.seen.borrow()[0][0];
    assert!(
        ctx.text.contains("No single Photo is in focus"),
        "{}",
        ctx.text
    );
    assert_eq!(ctx.images.len(), 1, "just the Contact sheet");

    tools
        .call("undo_batch", json!({ "batch_id": r.batch_id }))
        .unwrap();
    assert_eq!(
        json1(&tools, "list_looks", json!({ "shoot_id": shoot })),
        json!([])
    );
    for id in ids {
        assert_eq!(
            json1(&tools, "read_recipe", json!({ "photo_id": id }))["recipe"]["look"],
            Value::Null
        );
    }
}

#[test]
fn tool_errors_and_bad_json_go_back_to_the_model_to_fix() {
    let (_t, tools, ids) = library(1);
    let model = Script::new(&[
        json!("I'll make it brighter!"),
        json!({ "say": "", "calls": [
            { "tool": "edit_recipe", "args": { "photo_ids": [ids[0]], "set": { "brightness": 20 } } },
            { "tool": "export_photos", "args": { "photo_ids": [ids[0]], "folder": "/tmp/x" } } ] }),
        json!({ "say": "", "calls": [{ "tool": "edit_recipe", "args": { "photo_ids": [ids[0]], "set": { "exposure": 0.3 } } }] }),
        json!({ "say": "Exposure +0.30.", "calls": [] }),
    ]);
    let ask = Ask {
        prompt: "brighter".into(),
        photo_id: Some(ids[0]),
        history: vec![
            Turn {
                role: Role::User,
                text: "hello".into(),
            },
            Turn {
                role: Role::Agent,
                text: "Hi.".into(),
            },
        ],
        ..Default::default()
    };
    let r = agent::run_with(&tools, &ask, &model).unwrap();
    assert_eq!(r.text, "Exposure +0.30.");
    assert_eq!(
        r.steps.iter().map(|s| s.ok).collect::<Vec<_>>(),
        [false, false, true]
    );
    let seen = model.seen.borrow();
    assert!(
        seen[1].last().unwrap().text.contains("not valid JSON")
            || seen[1].last().unwrap().text.contains("no JSON")
    );
    let errs = &seen[2].last().unwrap().text;
    assert!(errs.contains("unknown setting \"brightness\""), "{errs}");
    assert!(errs.contains("not one of your tools"), "{errs}");
    assert_eq!(seen[0][1].text, "hello", "history comes after the context");
}

#[test]
fn a_question_changes_nothing_and_opens_no_batch() {
    let (_t, tools, ids) = library(1);
    let model = Script::new(&[
        json!({ "say": "", "calls": [{ "tool": "read_recipe", "args": { "photo_id": ids[0] } }] }),
        json!({ "say": "It hasn't been edited yet.", "calls": [] }),
    ]);
    let r = agent::run_with(
        &tools,
        &Ask {
            prompt: "what did you change?".into(),
            photo_id: Some(ids[0]),
            ..Default::default()
        },
        &model,
    )
    .unwrap();
    assert_eq!(r.batch_id, None);
    assert!(r.recipes.is_empty());
}
