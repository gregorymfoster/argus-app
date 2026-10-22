//! The background queue and Suggestions against a scripted model: a Shoot fans out into
//! per-Photo jobs (Picks first, rejects skipped), each saves Suggestions without touching
//! the Photo, accepting one is an undoable Recipe Version, and "apply to the rest" makes a
//! Look from the hero and matched Suggestions for the others.

use argus_engine::agent::{Chat, Msg};
use argus_engine::compute::Compute;
use argus_engine::jobs;
use argus_engine::library::Library;
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

/// What the scripted model was asked: (system prompt, number of images).
type Seen = Arc<Mutex<Vec<(String, usize)>>>;

/// Replies like a model would: three takes when asked fresh, two matches when matching.
/// A reply of `garbage` replaces every answer (to test failures).
struct Fake {
    seen: Seen,
    garbage: bool,
}

impl Chat for Fake {
    fn next(&self, system: &str, convo: &[Msg]) -> Result<String> {
        let images = convo.iter().map(|m| m.images.len()).sum();
        self.seen.lock().unwrap().push((system.to_string(), images));
        if self.garbage {
            return Ok("I think it looks great!".into());
        }
        Ok(if system.contains("reference Photo") {
            json!({ "suggestions": [
                { "title": "Closest match", "why": "Brighter to match.", "set": { "correction.exposure": 0.4, "correction.wbK": -150 } },
                { "title": "Match, tighter", "why": "Cropped in.", "set": { "correction.exposure": 0.4, "crop.x": 0.1, "crop.y": 0.1, "crop.w": 0.8, "crop.h": 0.8 } },
                { "title": "Extra", "why": "More than asked.", "set": {} }
            ] })
        } else {
            json!({ "suggestions": [
                { "title": "Clean and natural", "why": "Lifts the shadows.", "set": { "exposure": 0.3, "shadows": 25 } },
                { "title": "Bright and airy", "why": "More light.", "set": { "exposure": 0.8, "highlights": -30, "made_up": 4 } },
                { "title": "Black and white", "why": "Moody.", "set": { "saturation": -100, "contrast": 30, "crop.angle": -2 } }
            ] })
        }
        .to_string())
    }
}

fn library(n: u8, garbage: bool) -> (tempfile::TempDir, Tools, Vec<i64>, Seen) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2026-10-12_tahoe");
    for i in 0..n {
        write_jpeg(&src.join(format!("p{i:02}.jpg")), i * 2 + 3);
    }
    let seen: Seen = Arc::default();
    let s = seen.clone();
    let tools = Tools::new(tmp.path().join("Argus.library")).with_chats(move |_, _, _| {
        Ok(Box::new(Fake {
            seen: s.clone(),
            garbage,
        }) as Box<dyn Chat>)
    });
    tools
        .call("import_folder", json!({ "folder": src }))
        .unwrap();
    // Import queues the specialist models (tests/analyzers.rs covers them); these tests
    // are about the jobs they queue themselves.
    for j in json1(&tools, "list_jobs", json!({ "status": "queued" }))
        .as_array()
        .unwrap()
    {
        if j["kind"] == "analyze" {
            json1(&tools, "cancel_job", json!({ "job_id": j["id"] }));
        }
    }
    let mut ids: Vec<i64> = json1(&tools, "list_photos", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_i64().unwrap())
        .collect();
    ids.sort();
    (tmp, tools, ids, seen)
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

fn recipe(tools: &Tools, id: i64) -> Value {
    json1(tools, "read_recipe", json!({ "photo_id": id }))
}

#[test]
fn a_shoot_gets_suggestions_picks_first_rejects_skipped_and_nothing_changes_until_accepted() {
    let (_t, tools, ids, seen) = library(4, false);
    tools
        .call(
            "set_verdict",
            json!({ "photo_ids": [ids[3]], "verdict": "pick", "reason": "sharp" }),
        )
        .unwrap();
    tools
        .call(
            "set_verdict",
            json!({ "photo_ids": [ids[1]], "verdict": "reject", "reason": "blur" }),
        )
        .unwrap();

    let q = json1(&tools, "suggest_edits", json!({}));
    assert_eq!(q["created"], true);
    // Asking again while it's queued is the same job.
    assert_eq!(
        json1(&tools, "suggest_edits", json!({}))["job_id"],
        q["job_id"]
    );

    // The fan-out runs first and queues one job per Photo worth editing.
    let fan = jobs::run_next(&tools, Compute::Full).unwrap().unwrap();
    assert_eq!(fan.kind, "suggest_shoot");
    assert_eq!(
        fan.result.as_ref().unwrap()["queued"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let status = json1(&tools, "queue_status", json!({}));
    assert_eq!(status["queued"], 3);
    assert_eq!(status["compute"], "gentle");

    // The Pick goes first.
    let first = jobs::run_next(&tools, Compute::Full).unwrap().unwrap();
    assert_eq!(first.args["photo_id"], ids[3]);
    assert_eq!(
        first.status,
        argus_engine::library::JobStatus::Done,
        "{:?}",
        first.error
    );
    assert_eq!(jobs::drain(&tools, Compute::Full).unwrap(), 2);
    assert_eq!(proposals(&seen), 3, "one proposal request per Photo");
    assert_eq!(
        seen.lock().unwrap().len(),
        3,
        "no review round by default, even at Full"
    );
    assert_eq!(
        seen.lock().unwrap()[0].1,
        2,
        "a fresh Suggestion shows the model the Photo and Auto's edit"
    );

    let rejected = json1(&tools, "list_suggestions", json!({ "photo_id": ids[1] }));
    assert_eq!(rejected.as_array().unwrap().len(), 0);
    let list = json1(&tools, "list_suggestions", json!({ "photo_id": ids[0] }));
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 3);
    let titles: Vec<&str> = (0..3).map(|i| list[i]["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Natural", "Punchy", "Bright"]);
    // Each character starts from its own look, on top of Auto and the Adaptive layer.
    assert!(list[0]["recipe"]["global"]["clarity"].as_f64().unwrap() > 0.0);
    let exposure = |i: usize| {
        list[i]["recipe"]["global"]["exposure"]
            .as_f64()
            .unwrap_or(0.0)
    };
    assert!(
        exposure(2) > exposure(0),
        "Bright starts brighter than Natural"
    );
    assert_eq!(list[0]["recipe"]["global"]["shadows"], 25.0);
    assert!(
        list[1]["recipe"].get("made_up").is_none(),
        "a made-up setting is dropped"
    );
    assert_eq!(list[2]["recipe"]["crop"]["angle"], -2.0);
    let preview = tools.library().join(list[0]["preview"].as_str().unwrap());
    assert!(preview.exists(), "{preview:?}");

    // Untouched until the person accepts.
    assert!(recipe(&tools, ids[0]).is_null());
    let parts = tools
        .call("get_suggestion", json!({ "suggestion_id": list[2]["id"] }))
        .unwrap();
    assert!(matches!(parts[1], Part::Jpeg(_)));

    let acc = json1(
        &tools,
        "accept_suggestion",
        json!({ "suggestion_id": list[0]["id"] }),
    );
    assert_eq!(acc["version"], 1);
    assert_eq!(recipe(&tools, ids[0])["recipe"]["global"]["shadows"], 25.0);
    // Switching to another take: the newest accepted one is the accepted one.
    let second = json1(
        &tools,
        "accept_suggestion",
        json!({ "suggestion_id": list[1]["id"] }),
    );
    let all = json1(
        &tools,
        "list_suggestions",
        json!({ "photo_id": ids[0], "all": true }),
    );
    let accepted: Vec<_> = all
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["status"] == "accepted")
        .collect();
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0]["id"], list[1]["id"]);
    // Each accept undoes on its own.
    tools
        .call("undo_batch", json!({ "batch_id": second["batch_id"] }))
        .unwrap();
    assert_eq!(recipe(&tools, ids[0])["recipe"]["global"]["shadows"], 25.0);

    json1(
        &tools,
        "dismiss_suggestion",
        json!({ "suggestion_id": list[2]["id"] }),
    );
    let open = json1(&tools, "list_suggestions", json!({ "photo_id": ids[0] }));
    assert!(open
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["id"] != list[2]["id"]));
    assert_eq!(
        open.as_array().unwrap().len(),
        2,
        "the accepted take stays listed"
    );

    // The first take on several Photos at once is one batch, one undo.
    let firsts: Vec<Value> = [ids[2], ids[3]]
        .iter()
        .map(|&id| json1(&tools, "list_suggestions", json!({ "photo_id": id }))[0]["id"].clone())
        .collect();
    let bulk = json1(
        &tools,
        "accept_suggestion",
        json!({ "suggestion_ids": firsts }),
    );
    assert_eq!(bulk["photos"].as_array().unwrap().len(), 2);
    assert_eq!(recipe(&tools, ids[2])["recipe"]["global"]["shadows"], 25.0);
    tools
        .call("undo_batch", json!({ "batch_id": bulk["batch_id"] }))
        .unwrap();
    let shadows = recipe(&tools, ids[2])["recipe"]["global"]["shadows"].as_f64();
    assert!(
        shadows.unwrap_or(0.0) == 0.0,
        "undo put it back: {shadows:?}"
    );

    // A second run skips Photos that already have Suggestions.
    tools.call("suggest_edits", json!({})).unwrap();
    jobs::drain(&tools, Compute::Full).unwrap();
    assert_eq!(proposals(&seen), 3);
}

/// Model requests that proposed Suggestions, not reviewed them.
fn proposals(seen: &Seen) -> usize {
    seen.lock()
        .unwrap()
        .iter()
        .filter(|(system, _)| !system.contains("checking edits"))
        .count()
}

#[test]
fn apply_to_the_rest_makes_a_look_from_the_hero_and_matched_suggestions_for_the_others() {
    let (_t, tools, ids, seen) = library(3, false);
    let hero = ids[0];
    tools
        .call(
            "edit_recipe",
            json!({ "photo_ids": [hero], "set": { "exposure": 0.5, "temp": 12, "contrast": 20 } }),
        )
        .unwrap();
    // A fresh Suggestion already waiting on one target stays alongside the matched ones.
    tools
        .call("suggest_edits", json!({ "photo_ids": [ids[2]] }))
        .unwrap();
    jobs::drain(&tools, Compute::Full).unwrap();

    let q = json1(&tools, "match_to_photo", json!({ "photo_id": hero }));
    assert_eq!(q["created"], true);
    let job = jobs::run_next(&tools, Compute::Full).unwrap().unwrap();
    let result = job.result.unwrap();
    let look_id = result["look_id"].as_str().unwrap().to_string();
    let stem = json1(&tools, "list_photos", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == hero)
        .unwrap()["file_name"]
        .as_str()
        .unwrap()
        .trim_end_matches(".jpg")
        .to_string();
    assert_eq!(look_id, format!("from-{stem}"));
    assert_eq!(
        result["queued"].as_array().unwrap().len(),
        2,
        "every Photo but the hero"
    );

    let looks = json1(&tools, "list_looks", json!({ "shoot_id": 1 }));
    let look = looks
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["id"] == look_id.as_str())
        .unwrap()
        .clone();
    assert_eq!(look["adjust"]["exposure"], 0.5);
    assert_eq!(look["adjust"]["contrast"], 20.0);
    // Saving the Look assigns it to nobody: the hero itself is unchanged.
    assert!(recipe(&tools, hero)["recipe"]["look"].is_null());

    jobs::drain(&tools, Compute::Full).unwrap();
    let matched_request = seen
        .lock()
        .unwrap()
        .iter()
        .rfind(|(system, _)| !system.contains("checking edits"))
        .cloned()
        .unwrap();
    assert!(matched_request.0.contains("reference Photo"));
    assert_eq!(
        matched_request.1, 2,
        "the model sees the reference and the target"
    );

    let list = json1(&tools, "list_suggestions", json!({ "photo_id": ids[2] }));
    let list = list.as_array().unwrap();
    let matched: Vec<_> = list.iter().filter(|s| s["reference_id"] == hero).collect();
    assert_eq!(matched.len(), 2, "count 2, the extra proposal dropped");
    assert_eq!(list.len(), 5, "the 3 fresh Suggestions stay");
    assert_eq!(matched[0]["recipe"]["look"], look_id.as_str());
    assert_eq!(matched[0]["recipe"]["correction"]["exposure"], 0.4);
    assert_eq!(matched[1]["recipe"]["crop"]["w"], 0.8);

    // Accepting puts the Photo under the hero's Look with its own correction.
    json1(
        &tools,
        "accept_suggestion",
        json!({ "suggestion_id": matched[0]["id"] }),
    );
    let r = recipe(&tools, ids[2]);
    assert_eq!(r["recipe"]["look"], look_id.as_str());
    assert_eq!(r["recipe"]["correction"]["wbK"], -150.0);

    // Applying from the hero again replaces the earlier matches and makes a second Look.
    tools
        .call("match_to_photo", json!({ "photo_id": hero }))
        .unwrap();
    jobs::drain(&tools, Compute::Full).unwrap();
    let list = json1(&tools, "list_suggestions", json!({ "photo_id": ids[1] }));
    let refs: Vec<_> = list
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["reference_id"] == hero)
        .collect();
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0]["recipe"]["look"], format!("{look_id}-2"));
}

#[test]
fn a_model_that_wont_answer_fails_its_job_and_the_queue_moves_on() {
    let (_t, tools, ids, seen) = library(2, true);
    tools
        .call("suggest_edits", json!({ "photo_ids": [ids[0]] }))
        .unwrap();
    tools
        .call("suggest_edits", json!({ "photo_ids": [ids[1]] }))
        .unwrap();
    let a = jobs::run_next(&tools, Compute::Full).unwrap().unwrap();
    assert_eq!(a.status, argus_engine::library::JobStatus::Failed);
    assert!(a.error.unwrap().contains("JSON"));
    assert_eq!(seen.lock().unwrap().len(), 2, "one retry");
    assert!(jobs::run_next(&tools, Compute::Full).unwrap().is_some());
    let s = json1(&tools, "queue_status", json!({}));
    assert_eq!(s["failed"], 2);
    assert_eq!(s["queued"], 0);

    // The model is back: Retry failed queues them again, and the failed count clears.
    let r = json1(&tools, "retry_failed", json!({}));
    assert_eq!(
        (
            r["retried"].clone(),
            r["queued"].clone(),
            r["failed"].clone()
        ),
        (json!(2), json!(2), json!(0))
    );
    assert_eq!(json1(&tools, "retry_failed", json!({}))["retried"], 0);
}

#[test]
fn on_battery_only_the_open_photos_work_runs() {
    let (_t, tools, ids, _) = library(2, false);
    for id in &ids {
        tools
            .call("suggest_edits", json!({ "photo_ids": [id] }))
            .unwrap();
    }
    let held =
        |t: &Tools| jobs::run_next_at_least(t, Compute::Gentle, jobs::PRIORITY_FOCUS).unwrap();
    assert!(
        held(&tools).is_none(),
        "background work waits for the charger"
    );
    let mut lib = Library::open(tools.library()).unwrap();
    assert_eq!(jobs::focus(&mut lib, ids[1], &[]).unwrap(), 1);
    drop(lib);
    let job = held(&tools).unwrap();
    assert_eq!(job.args["photo_id"], json!(ids[1]));
    assert!(held(&tools).is_none());

    let s = json1(
        &tools,
        "set_queue",
        json!({ "run_anyway": true, "run_on_battery": true }),
    );
    assert_eq!(
        (s["run_anyway"].clone(), s["run_on_battery"].clone()),
        (json!(true), json!(true))
    );
}

#[test]
fn queued_jobs_survive_a_quit_and_cancel_and_settings_stick() {
    let (_t, tools, ids, _) = library(2, false);
    let a = json1(
        &tools,
        "enqueue_job",
        json!({ "kind": "suggest", "args": { "photo_id": ids[0] } }),
    );
    let b = json1(
        &tools,
        "enqueue_job",
        json!({ "kind": "suggest", "args": { "photo_id": ids[1] }, "priority": 99 }),
    );
    assert!(tools
        .call("enqueue_job", json!({ "kind": "mine_bitcoin" }))
        .is_err());

    // A crash mid-job: the job is left running, and comes back on the next start.
    let mut lib = Library::open(tools.library()).unwrap();
    let claimed = lib.claim_job().unwrap().unwrap();
    assert_eq!(
        claimed.id,
        b["job_id"].as_i64().unwrap(),
        "highest priority first"
    );
    assert_eq!(
        json1(&tools, "queue_status", json!({}))["running"]["id"],
        b["job_id"]
    );
    assert_eq!(lib.requeue_stale_jobs().unwrap(), 1);

    assert_eq!(
        json1(&tools, "cancel_job", json!({ "job_id": a["job_id"] }))["cancelled"],
        true
    );
    assert_eq!(jobs::drain(&tools, Compute::Full).unwrap(), 1);
    let done = json1(&tools, "list_jobs", json!({ "status": "done" }));
    assert_eq!(done.as_array().unwrap().len(), 1);

    let s = json1(
        &tools,
        "set_queue",
        json!({ "compute": "full", "paused": true, "overnight": false, "takes": 9 }),
    );
    assert_eq!(s["compute"], "full");
    assert_eq!(
        (s["overnight"].clone(), s["takes"].clone()),
        (json!(false), json!(5))
    );
    // The app's progress line is the same thing under the name the UI calls.
    let p = argus_engine::commands::dispatch(&tools, "progress", json!({})).unwrap();
    assert_eq!(p["paused"], true);
    assert_eq!(p["compute"], "full");
}

#[test]
fn crop_and_straighten_render_through_the_tools() {
    let (_t, tools, ids, _) = library(1, false);
    let size = |tools: &Tools| {
        let v = json1(
            tools,
            "render_photo",
            json!({ "photo_id": ids[0], "long_edge": 160 }),
        );
        (v["width"].as_u64().unwrap(), v["height"].as_u64().unwrap())
    };
    assert_eq!(size(&tools), (160, 120));
    tools
        .call(
            "edit_recipe",
            json!({ "photo_ids": [ids[0]], "set": { "crop.w": 0.5, "crop.x": 0.25 } }),
        )
        .unwrap();
    let (w, h) = size(&tools);
    assert!((w as i64 - 80).abs() <= 1 && h == 120, "{w}x{h}");
    tools
        .call(
            "edit_recipe",
            json!({ "photo_ids": [ids[0]], "set": { "crop.angle": 5 } }),
        )
        .unwrap();
    let r = recipe(&tools, ids[0]);
    assert_eq!(r["recipe"]["crop"]["angle"], 5.0);
    let (w2, h2) = size(&tools);
    assert!(
        w2 <= w && h2 <= h,
        "straightening crops in, never shows corners: {w2}x{h2}"
    );
}

#[test]
fn import_can_queue_its_preview_renders() {
    // import_folder queues `render_previews` after a RAW import; a kind the Queue refuses
    // fails every RAW import from a card.
    let (_t, tools, _, _) = library(1, false);
    let job = json1(
        &tools,
        "enqueue_job",
        json!({ "kind": "render_previews", "args": { "shoot_id": 1 } }),
    );
    assert!(job["job_id"].as_i64().is_some(), "{job}");
}

#[test]
fn raw_import_renders_its_previews_through_the_queue() {
    // The whole path a card takes: import_folder on a real RAW queues render_previews, and
    // the Queue swaps the camera's embedded JPEG for Argus's render. Skips without sample RAWs.
    let raws = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/raw");
    let Some(raw) = std::fs::read_dir(&raws).ok().and_then(|d| {
        d.flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("arw")))
    }) else {
        eprintln!("jobs: no fixtures/raw/*.ARW, skipping RAW import through the Queue");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let card = tmp.path().join("card");
    std::fs::create_dir_all(&card).unwrap();
    std::fs::copy(&raw, card.join(raw.file_name().unwrap())).unwrap();
    let tools = Tools::new(tmp.path().join("Argus.library"));
    let imported = json1(
        &tools,
        "import_folder",
        json!({ "folder": card.to_str().unwrap(), "shoot_name": "2026-10-18_Queue-Test" }),
    );
    let jobs = json1(&tools, "list_jobs", json!({ "kind": "render_previews" }));
    let job = jobs.as_array().and_then(|j| j.first()).cloned();
    assert!(
        job.is_some(),
        "import queued no preview job: {imported} {jobs}"
    );
    let lib = Library::open(tools.library()).unwrap();
    assert_eq!(
        lib.pending_previews(None).unwrap(),
        1,
        "camera preview first"
    );
    jobs::drain(&tools, Compute::Full).unwrap();
    let after = json1(&tools, "list_jobs", json!({ "kind": "render_previews" }));
    assert_eq!(after[0]["status"], "done", "{after}");
    assert_eq!(
        lib.pending_previews(None).unwrap(),
        0,
        "Argus render swapped in"
    );
}

#[test]
fn the_photo_open_in_edit_jumps_the_queue_and_runs_at_full() {
    let (_t, tools, ids, seen) = library(3, false);
    for &id in &ids {
        json1(
            &tools,
            "enqueue_job",
            json!({ "kind": "suggest", "args": { "photo_id": id } }),
        );
    }
    // Greg opens the last Photo; the one before it is next in his filmstrip.
    let r = argus_engine::commands::dispatch(
        &tools,
        "focus_photo",
        json!({ "photo_id": ids[2], "next_ids": [ids[1]] }),
    )
    .unwrap();
    assert_eq!(r["raised"], 2);

    let levels: Arc<Mutex<Vec<Option<Compute>>>> = Arc::default();
    let l = levels.clone();
    let tools = tools.with_chats(move |_, c, _| {
        l.lock().unwrap().push(c);
        Ok(Box::new(Fake {
            seen: seen.clone(),
            garbage: false,
        }) as Box<dyn Chat>)
    });
    let order: Vec<i64> = (0..3)
        .map(|_| {
            let job = jobs::run_next(&tools, Compute::Gentle).unwrap().unwrap();
            job.args["photo_id"].as_i64().unwrap()
        })
        .collect();
    assert_eq!(order, [ids[2], ids[1], ids[0]]);
    assert_eq!(
        *levels.lock().unwrap(),
        [
            Some(Compute::Full),
            Some(Compute::Gentle),
            Some(Compute::Gentle)
        ],
        "only the open Photo skips the queue's Compute level"
    );
}

#[test]
fn with_your_photo_first_off_focus_raises_nothing() {
    let (_t, tools, ids, _) = library(2, false);
    for &id in &ids {
        json1(
            &tools,
            "enqueue_job",
            json!({ "kind": "suggest", "args": { "photo_id": id } }),
        );
    }
    json1(&tools, "set_queue", json!({ "focus_first": false }));
    let r = argus_engine::commands::dispatch(&tools, "focus_photo", json!({ "photo_id": ids[1] }))
        .unwrap();
    assert_eq!(r["raised"], 0);
}

#[test]
fn dragging_queued_jobs_changes_which_runs_next() {
    let (_t, tools, ids, _) = library(3, false);
    let q: Vec<i64> = ids
        .iter()
        .map(|id| {
            json1(
                &tools,
                "enqueue_job",
                json!({ "kind": "suggest", "args": { "photo_id": id } }),
            )["job_id"]
                .as_i64()
                .unwrap()
        })
        .collect();
    // The last queued goes first, then the first; the middle one last.
    let moved = json1(
        &tools,
        "reorder_jobs",
        json!({ "job_ids": [q[2], q[0], q[1]] }),
    );
    assert_eq!(moved["moved"], 3);
    let mut lib = Library::open(tools.library()).unwrap();
    let order: Vec<i64> = (0..3)
        .map(|_| lib.claim_job().unwrap().unwrap().id)
        .collect();
    assert_eq!(order, vec![q[2], q[0], q[1]]);
}

#[test]
fn the_open_photos_work_stays_first_whatever_the_order() {
    let (_t, tools, ids, _) = library(2, false);
    // The open Photo's work stays first: dragging under it doesn't lift the rest into its band.
    let focus = json1(
        &tools,
        "enqueue_job",
        json!({ "kind": "suggest", "args": { "photo_id": ids[0], "count": 2 }, "priority": 100 }),
    )["job_id"]
        .as_i64()
        .unwrap();
    let other = json1(
        &tools,
        "enqueue_job",
        json!({ "kind": "suggest", "args": { "photo_id": ids[1], "count": 2 } }),
    )["job_id"]
        .as_i64()
        .unwrap();
    assert_eq!(
        json1(&tools, "reorder_jobs", json!({ "job_ids": [other, focus] }))["moved"],
        1
    );
    let mut lib = Library::open(tools.library()).unwrap();
    assert_eq!(lib.claim_job().unwrap().unwrap().id, focus);
}

/// Proposes four takes, then on review blows one out, nudges another and gives two
/// changes no critique.
struct Reviewer;

impl Chat for Reviewer {
    fn next(&self, system: &str, _: &[Msg]) -> Result<String> {
        Ok(if system.contains("checking edits") {
            json!({ "suggestions": [
                { "see": "", "title": "", "why": "", "set": { "contrast": 40 }, "done": false },
                { "see": "brighter", "title": "Brighter", "why": "", "set": { "exposure": 5.0 }, "done": false },
                { "see": "a touch brighter", "title": "", "why": "", "set": { "exposure": 0.3 }, "done": false },
                { "see": "", "title": "", "why": "", "set": { "exposure": 1.0 }, "done": false }
            ] })
        } else {
            json!({ "suggestions": [
                { "see": "fine", "title": "Natural", "why": "As shot.", "set": {}, "done": false },
                { "see": "dim", "title": "Brighter", "why": "More light.", "set": { "exposure": 0.1 }, "done": false },
                { "see": "flat", "title": "Punchy", "why": "", "set": { "contrast": 20 }, "done": false },
                { "see": "soft", "title": "Soft", "why": "", "set": { "contrast": -10 }, "done": false }
            ] })
        }
        .to_string())
    }
}

#[test]
fn review_rounds_keep_a_better_revision_and_refuse_one_that_blows_out() {
    let (_t, tools, ids, _) = library(1, false);
    let args = argus_engine::suggest::SuggestArgs {
        photo_id: ids[0],
        count: Some(4),
        ..Default::default()
    };
    argus_engine::suggest::suggest_photo(&tools, &Reviewer, &args, None, 2).unwrap();
    let list = json1(&tools, "list_suggestions", json!({ "photo_id": ids[0] }));
    let exposure = |i: usize| {
        list[i]["recipe"]["global"]["exposure"]
            .as_f64()
            .unwrap_or(0.0)
    };
    assert_ne!(
        list[0]["recipe"]["global"]["contrast"], 40.0,
        "a change with no critique is not applied"
    );
    assert_eq!(
        exposure(1),
        0.1,
        "+5 stops clips the frame, so the take stays as it was"
    );
    assert_eq!(exposure(2), 0.3, "the small fix is kept");
    assert_eq!(
        list[2]["title"], "Bright",
        "a fresh take keeps its character's name whatever the model calls it"
    );
    let auto = tools
        .auto_recipe(ids[0], &argus_engine::recipe::Recipe::default())
        .unwrap()
        .global;
    assert_eq!(
        exposure(3),
        auto.exposure - 0.3,
        "a change with no critique is not applied: the take stays at its start (Moody's -0.3)"
    );
}

#[test]
fn the_natural_take_keeps_autos_exposure_and_highlights() {
    // On the edit bench the model's own brightness moves were what put its first take
    // further from Greg's edits than Auto: the natural take takes those two from Auto.
    let (_t, tools, ids, _) = library(1, false);
    let args = argus_engine::suggest::SuggestArgs {
        photo_id: ids[0],
        count: Some(4),
        ..Default::default()
    };
    argus_engine::suggest::suggest_photo(&tools, &Reviewer, &args, None, 1).unwrap();
    let auto = tools
        .auto_recipe(ids[0], &argus_engine::recipe::Recipe::default())
        .unwrap()
        .global;
    let list = json1(&tools, "list_suggestions", json!({ "photo_id": ids[0] }));
    let global = |i: usize, k: &str| list[i]["recipe"]["global"][k].as_f64().unwrap_or(0.0);
    assert_eq!(global(0, "exposure"), auto.exposure);
    assert_eq!(global(0, "highlights"), auto.highlights);
    assert_eq!(
        global(2, "exposure"),
        0.3,
        "the other takes are the model's own"
    );
}

/// Answers like [`Fake`], first reading the Photo's Suggestions progress as the Edit strip
/// would mid-run, and cancelling its job when told to (the strip's Cancel).
struct Peek {
    library: std::path::PathBuf,
    photo: i64,
    cancel: bool,
    saw: Arc<Mutex<Vec<Value>>>,
}

impl Chat for Peek {
    fn next(&self, system: &str, convo: &[Msg]) -> Result<String> {
        let tools = Tools::new(self.library.clone());
        let p = json1(
            &tools,
            "suggestions_progress",
            json!({ "photo_id": self.photo }),
        );
        if self.cancel {
            json1(&tools, "cancel_job", json!({ "job_id": p["job"]["id"] }));
        }
        self.saw.lock().unwrap().push(p);
        Fake {
            seen: Arc::default(),
            garbage: false,
        }
        .next(system, convo)
    }
}

fn peeking(tools: Tools, photo: i64, cancel: bool) -> (Tools, Arc<Mutex<Vec<Value>>>) {
    let saw: Arc<Mutex<Vec<Value>>> = Arc::default();
    let (s, library) = (saw.clone(), tools.library().to_path_buf());
    let tools = tools.with_chats(move |_, _, _| {
        Ok(Box::new(Peek {
            library: library.clone(),
            photo,
            cancel,
            saw: s.clone(),
        }) as Box<dyn Chat>)
    });
    (tools, saw)
}

#[test]
fn new_takes_say_what_runs_before_them_then_their_stage_and_when_they_land() {
    let (_t, tools, ids, _) = library(2, false);
    assert_eq!(
        json1(
            &tools,
            "suggestions_progress",
            json!({ "photo_id": ids[0] })
        )["job"],
        Value::Null
    );
    // A background job for another Photo was queued first, then R on this one.
    json1(
        &tools,
        "suggest_edits",
        json!({ "photo_ids": [ids[1]], "count": 2 }),
    );
    json1(
        &tools,
        "suggest_edits",
        json!({ "photo_ids": [ids[0]], "count": 2 }),
    );
    let p = json1(
        &tools,
        "suggestions_progress",
        json!({ "photo_id": ids[0] }),
    )["job"]
        .clone();
    assert_eq!(p["status"], "queued");
    assert_eq!(p["stage"], "Queued");
    assert_eq!(p["takes"], 2);
    assert_eq!(p["ahead"], 1);
    let other = json1(&tools, "list_photos", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == ids[1])
        .unwrap()["file_name"]
        .clone();
    assert_eq!(
        p["behind"],
        format!("Suggestions for {}", other.as_str().unwrap())
    );
    assert_eq!(p["eta_s"], Value::Null, "no timings yet, so no made-up ETA");

    // Opening it in Edit puts it first.
    argus_engine::commands::dispatch(
        &tools,
        "focus_photo",
        json!({ "photo_id": ids[0], "next_ids": [] }),
    )
    .unwrap();
    let p = json1(
        &tools,
        "suggestions_progress",
        json!({ "photo_id": ids[0] }),
    )["job"]
        .clone();
    assert_eq!(
        (p["ahead"].clone(), p["focus"].clone()),
        (json!(0), json!(true))
    );

    let (tools, saw) = peeking(tools, ids[0], false);
    jobs::run_next(&tools, Compute::Gentle).unwrap().unwrap();
    let mid = saw.lock().unwrap()[0]["job"].clone();
    assert_eq!(mid["status"], "running");
    assert_eq!(mid["stage"], "Model thinking");
    assert!(mid["progress"].as_f64().unwrap() > 0.0);
    assert_eq!(
        json1(
            &tools,
            "suggestions_progress",
            json!({ "photo_id": ids[0] })
        )["job"],
        Value::Null,
        "done: the takes are in"
    );
    // With one timing, the other Photo's takes get an ETA.
    let p = json1(
        &tools,
        "suggestions_progress",
        json!({ "photo_id": ids[1] }),
    )["job"]
        .clone();
    assert!(p["eta_s"].as_f64().is_some(), "{p}");
    assert!(p["avg_s"].as_f64().is_some(), "{p}");
}

#[test]
fn cancelling_new_takes_mid_run_saves_none() {
    let (_t, tools, ids, _) = library(1, false);
    json1(
        &tools,
        "suggest_edits",
        json!({ "photo_ids": [ids[0]], "count": 2 }),
    );
    let (tools, _) = peeking(tools, ids[0], true);
    let job = jobs::run_next(&tools, Compute::Gentle).unwrap().unwrap();
    assert_eq!(job.status, argus_engine::library::JobStatus::Cancelled);
    assert_eq!(
        json1(&tools, "list_suggestions", json!({ "photo_id": ids[0] })),
        json!([])
    );
}

#[test]
fn model_work_waits_for_an_idle_mac_unless_told_to_run_now() {
    let (_t, tools, ids, _) = library(2, false);
    for id in &ids {
        tools
            .call("suggest_edits", json!({ "photo_ids": [id] }))
            .unwrap();
    }
    let c = jobs::QueueConfig::default();
    assert!(c.idle_only, "on by default");

    // While the person works, model jobs stay queued; the open Photo's still runs.
    let held = |t: &Tools| {
        jobs::run_next_holding(t, Compute::Gentle, i64::MIN, jobs::MODEL_KINDS).unwrap()
    };
    assert!(held(&tools).is_none(), "Suggestions wait for an idle Mac");
    let mut lib = Library::open(tools.library()).unwrap();
    assert_eq!(jobs::focus(&mut lib, ids[0], &[]).unwrap(), 1);
    drop(lib);
    assert_eq!(held(&tools).unwrap().args["photo_id"], json!(ids[0]));
    assert!(held(&tools).is_none());

    let s = json1(&tools, "queue_status", json!({}));
    assert_eq!(s["idle_only"], json!(true));
    // Only overnight lets every model job through; an idle Mac still holds long runs.
    assert_eq!(
        s["waiting_for_idle"],
        json!(s["boost"] != json!("overnight"))
    );
    let s = json1(&tools, "set_queue", json!({ "run_now": true }));
    assert_eq!(
        (s["run_now"].clone(), s["waiting_for_idle"].clone()),
        (json!(true), json!(false))
    );
    assert!(s["thermal_paused"].is_boolean() && s.get("model_busy").is_some());
}

#[test]
fn a_take_is_accepted_at_its_amount() {
    let (_t, tools, ids, _) = library(1, false);
    let args = argus_engine::suggest::SuggestArgs {
        photo_id: ids[0],
        ..Default::default()
    };
    argus_engine::suggest::suggest_photo(
        &tools,
        &Fake {
            seen: Arc::default(),
            garbage: false,
        },
        &args,
        None,
        0,
    )
    .unwrap();
    let list = json1(&tools, "list_suggestions", json!({ "photo_id": ids[0] }));
    let take = &list[1]["recipe"];
    // Half way between the Photo as it is (nothing yet) and the take, like a preset's Amount.
    let parts = tools
        .call(
            "get_suggestion",
            json!({ "suggestion_id": list[1]["id"], "amount": 0.5 }),
        )
        .unwrap();
    assert!(matches!(parts[1], Part::Jpeg(_)));
    json1(
        &tools,
        "accept_suggestion",
        json!({ "suggestion_id": list[1]["id"], "amount": 0.5 }),
    );
    let got = recipe(&tools, ids[0]);
    for k in ["exposure", "clarity", "vibrance"] {
        let want = take["global"][k].as_f64().unwrap() / 2.0;
        let have = got["recipe"]["global"][k].as_f64().unwrap();
        assert!((have - want).abs() < 1e-6, "{k}: {have} vs {want}");
    }
}
