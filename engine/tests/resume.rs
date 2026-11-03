//! Work that survives closing the lid, a quit or a crash (Greg, 2026-11-02): an Export or
//! Import killed part way finishes on the next start from the Photo it reached, writing each
//! file once, and a Queue job that fails across a sleep waits to run again.
//!
//! A kill is simulated by leaving exactly what a killed process leaves on disk and in the
//! Library (a claimed empty name, a half-written temporary file, a run still `running` under
//! a process that is gone), then starting again the way the app does.

use argus_engine::compute::Compute;
use argus_engine::jobs;
use argus_engine::library::{ImportOptions, JobStatus, Library};
use argus_engine::sleep::Since;
use argus_engine::tools::{Part, Tools};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn write_jpeg(path: &Path, seed: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..160 * 120 * 3)
        .map(|i| (i as u8).wrapping_mul(seed))
        .collect();
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, 160, 120, jpeg_encoder::ColorType::Rgb)
        .unwrap();
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

/// Every file in `dir`, hidden ones too, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut n: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    n.sort();
    n
}

/// The process that drove `run` is gone (the app quit or crashed).
fn orphan(root: &Path, run: i64) {
    let db = rusqlite::Connection::open(root.join("library.db")).unwrap();
    db.execute(
        "UPDATE runs SET pid = ?2 WHERE id = ?1",
        rusqlite::params![run, i32::MAX as i64 + 1],
    )
    .unwrap();
}

/// What the app does when it starts on a Library: jobs and runs left part way go back on
/// the Queue. Returns how many runs it queued.
fn start_again(root: &Path) -> usize {
    let mut lib = Library::open(root).unwrap();
    lib.requeue_stale_jobs().unwrap();
    jobs::queue_orphaned_runs(&mut lib).unwrap()
}

struct Lib {
    tmp: tempfile::TempDir,
    tools: Tools,
    root: PathBuf,
    ids: Vec<i64>,
}

fn library(n: u8) -> Lib {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("photos");
    for i in 0..n {
        write_jpeg(&src.join(format!("DSC0{i}.JPG")), i * 2 + 3);
    }
    let root = tmp.path().join("library");
    let tools = Tools::new(root.clone());
    json1(
        &tools,
        "import_folder",
        json!({ "folder": src, "shoot_name": "2026-10-18_Tahoe" }),
    );
    let ids = json1(&tools, "list_photos", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_i64().unwrap())
        .collect();
    Lib {
        tmp,
        tools,
        root,
        ids,
    }
}

#[test]
fn an_export_killed_part_way_finishes_on_the_next_start_writing_each_file_once() {
    let l = library(3);
    let out = l.tmp.path().join("out");
    let settings = json!({ "folder": out, "long_edge": 64, "shoot_folder": false });
    let run = json1(
        &l.tools,
        "start_run",
        json!({ "kind": "export", "args": { "photo_ids": l.ids, "export": settings } }),
    )["run_id"]
        .as_i64()
        .unwrap();
    let export = |i: usize| {
        let mut a = settings.clone();
        a["photo_ids"] = json!([l.ids[i]]);
        a["sequence_start"] = json!(i + 1);
        a["sequence_total"] = json!(3);
        a["run_id"] = json!(run);
        json1(&l.tools, "export_photos", a)
    };
    // The first Photo went out whole.
    let first = export(0);
    assert_eq!(
        first["exported"][0]["path"].as_str().unwrap(),
        out.join("2026-10-18_Tahoe_DSC00.jpg").to_str().unwrap()
    );
    // The lid closed on a laptop that then ran flat, mid-way through the second: its name
    // was claimed (an empty file) and half its bytes sat in the temporary file.
    let lib = Library::open(&l.root).unwrap();
    let second = out.join("2026-10-18_Tahoe_DSC01.jpg");
    std::fs::write(&second, b"").unwrap();
    std::fs::write(
        out.join(".2026-10-18_Tahoe_DSC01.jpg.argus-tmp"),
        b"half a jpeg",
    )
    .unwrap();
    lib.claim_run_item(run, l.ids[1], &second).unwrap();
    drop(lib);
    orphan(&l.root, run);

    // Next start: the Queue finishes it, ahead of anything else.
    assert_eq!(start_again(&l.root), 1);
    let runs = json1(&l.tools, "list_runs", json!({}));
    assert_eq!(runs[0]["status"], "paused");
    assert_eq!(runs[0]["note"], "quit");
    let job = jobs::run_next(&l.tools, Compute::Gentle).unwrap().unwrap();
    assert_eq!(job.kind, "resume_run");
    assert_eq!(job.status, JobStatus::Done, "{:?}", job.error);
    let r = job.result.unwrap();
    assert_eq!(
        (r["exported"].as_i64(), r["already"].as_i64()),
        (Some(2), Some(1)),
        "{r}"
    );

    // Each file once, under the name it first claimed: no -2, nothing half-written left.
    assert_eq!(
        names(&out),
        [
            "2026-10-18_Tahoe_DSC00.jpg",
            "2026-10-18_Tahoe_DSC01.jpg",
            "2026-10-18_Tahoe_DSC02.jpg"
        ]
    );
    for n in names(&out) {
        assert!(
            std::fs::metadata(out.join(&n)).unwrap().len() > 100,
            "{n} is whole"
        );
        image::open(out.join(&n)).unwrap();
    }
    assert_eq!(json1(&l.tools, "list_runs", json!({})), json!([]));
    // Started again once more, nothing is redone.
    assert_eq!(start_again(&l.root), 0);
}

#[test]
fn a_file_renamed_into_place_just_before_the_kill_is_not_written_twice() {
    let l = library(1);
    let out = l.tmp.path().join("out");
    let settings = json!({ "folder": out, "long_edge": 64, "shoot_folder": false });
    let run = json1(
        &l.tools,
        "start_run",
        json!({ "kind": "export", "args": { "photo_ids": l.ids, "export": settings } }),
    )["run_id"]
        .as_i64()
        .unwrap();
    // Written and renamed into place, but the process died before it noted the Photo done.
    let mut a = settings.clone();
    a["photo_ids"] = json!(l.ids);
    let path = PathBuf::from(
        json1(&l.tools, "export_photos", a)["exported"][0]["path"]
            .as_str()
            .unwrap(),
    );
    Library::open(&l.root)
        .unwrap()
        .claim_run_item(run, l.ids[0], &path)
        .unwrap();
    orphan(&l.root, run);
    assert_eq!(start_again(&l.root), 1);
    let job = jobs::run_next(&l.tools, Compute::Gentle).unwrap().unwrap();
    assert_eq!(job.result.unwrap()["already"], 1);
    assert_eq!(names(&out), ["2026-10-18_Tahoe_DSC00.jpg"]);
}

#[test]
fn a_stopped_export_clears_the_file_it_was_writing_and_keeps_the_whole_ones() {
    let l = library(2);
    let out = l.tmp.path().join("out");
    let settings = json!({ "folder": out, "long_edge": 64, "shoot_folder": false });
    let run = json1(
        &l.tools,
        "start_run",
        json!({ "kind": "export", "args": { "photo_ids": l.ids, "export": settings } }),
    )["run_id"]
        .as_i64()
        .unwrap();
    let mut a = settings.clone();
    a["photo_ids"] = json!([l.ids[0]]);
    a["run_id"] = json!(run);
    json1(&l.tools, "export_photos", a);
    let claimed = out.join("2026-10-18_Tahoe_DSC01.jpg");
    std::fs::write(&claimed, b"").unwrap();
    std::fs::write(out.join(".2026-10-18_Tahoe_DSC01.jpg.argus-tmp"), b"half").unwrap();
    Library::open(&l.root)
        .unwrap()
        .claim_run_item(run, l.ids[1], &claimed)
        .unwrap();
    let r = json1(
        &l.tools,
        "finish_run",
        json!({ "run_id": run, "stopped": true }),
    );
    assert_eq!(r["status"], "stopped");
    assert_eq!(names(&out), ["2026-10-18_Tahoe_DSC00.jpg"]);
    // A stopped run is never resumed.
    orphan(&l.root, run);
    assert_eq!(start_again(&l.root), 0);
}

#[test]
fn an_import_killed_part_way_finishes_into_the_same_shoot_and_copy_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let card = tmp.path().join("card");
    for i in 0..4u8 {
        write_jpeg(&card.join(format!("DSC0{i}.JPG")), i * 2 + 3);
    }
    let root = tmp.path().join("library");
    let tools = Tools::new(root.clone());
    // An Import of the whole card, killed after two Photos: what the Library holds then.
    let mut lib = Library::open(&root).unwrap();
    let tool = json!({ "folder": card, "shoot_name": "Tahoe", "copy": true });
    let run = lib
        .start_run("import", "Importing Tahoe", &json!({ "tool": tool }), 0)
        .unwrap();
    let first_two: Vec<PathBuf> = (0..2).map(|i| card.join(format!("DSC0{i}.JPG"))).collect();
    let report = lib
        .import(
            &card,
            &ImportOptions {
                shoot_name: Some("Tahoe".into()),
                files: Some(first_two),
                copy: true,
                run: Some(run),
                ..Default::default()
            },
        )
        .unwrap();
    let shoot = report.shoot_id.unwrap();
    let copy_dir = root.join(report.copied_to.unwrap());
    // ...and the third was half copied.
    std::fs::write(copy_dir.join(".DSC02.JPG.0123456789ab.argus-tmp"), b"half").unwrap();
    drop(lib);
    orphan(&root, run);

    assert_eq!(start_again(&root), 1);
    let job = jobs::run_next(&tools, Compute::Gentle).unwrap().unwrap();
    assert_eq!(job.kind, "resume_run");
    assert_eq!(job.status, JobStatus::Done, "{:?}", job.error);
    let r = job.result.unwrap();
    assert_eq!(r["shoot_id"], shoot, "{r}");
    assert_eq!(
        (
            r["imported"].as_i64(),
            r["resumed_with"].as_i64(),
            r["duplicates"].as_i64()
        ),
        (Some(2), Some(2), Some(0)),
        "{r}"
    );

    // One Shoot of four, its originals copied once each, nothing half-written.
    let shoots = json1(&tools, "list_shoots", json!({}));
    assert_eq!(shoots.as_array().unwrap().len(), 1, "{shoots}");
    assert_eq!(
        json1(&tools, "list_photos", json!({}))
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let copy_dir = root.join(r["copied_to"].as_str().unwrap());
    assert_eq!(
        names(&copy_dir),
        ["DSC00.JPG", "DSC01.JPG", "DSC02.JPG", "DSC03.JPG"]
    );
    assert_eq!(json1(&tools, "list_runs", json!({})), json!([]));
}

#[test]
fn an_import_whose_card_is_gone_waits_for_it_and_carries_on_when_it_is_back() {
    let tmp = tempfile::tempdir().unwrap();
    let card = tmp.path().join("card");
    let root = tmp.path().join("library");
    let tools = Tools::new(root.clone());
    let lib = Library::open(&root).unwrap();
    let run = lib
        .start_run(
            "import",
            "Importing card",
            &json!({ "tool": { "folder": card } }),
            0,
        )
        .unwrap();
    drop(lib);
    orphan(&root, run);
    // Argus restarts with the card still out of the reader.
    assert_eq!(start_again(&root), 1);
    let job = jobs::run_next(&tools, Compute::Gentle).unwrap().unwrap();
    assert_eq!(job.result.unwrap()["waiting"], "card");
    let runs = json1(&tools, "list_runs", json!({}));
    assert_eq!(
        (runs[0]["status"].as_str(), runs[0]["note"].as_str()),
        (Some("paused"), Some("card"))
    );
    assert_eq!(start_again(&root), 0, "nothing to do while the card is out");
    // The card is back.
    write_jpeg(&card.join("DSC00.JPG"), 3);
    assert_eq!(start_again(&root), 1);
    let job = jobs::run_next(&tools, Compute::Gentle).unwrap().unwrap();
    assert_eq!(job.result.unwrap()["imported"], 1);
    assert_eq!(json1(&tools, "list_runs", json!({})), json!([]));
}

#[test]
fn a_job_that_fails_across_a_sleep_waits_to_run_again_instead_of_failing() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(tmp.path()).unwrap();
    let (id, _) = jobs::enqueue(&mut lib, "learn_edits", &json!({}), None, None).unwrap();
    let job = lib.claim_job().unwrap().unwrap();
    assert_eq!(job.id, id);
    let cut: argus_engine::Result<Value> = Err(argus_engine::Error("connection reset".into()));
    // The lid closed for ten minutes while it ran, and its model connection died.
    jobs::settle(
        &lib,
        &job,
        &cut,
        &Since::slept_for(Duration::from_secs(600)),
    )
    .unwrap();
    let j = lib.job(id).unwrap();
    assert_eq!(
        (j.status, j.interrupted.as_deref()),
        (JobStatus::Queued, Some("asleep"))
    );
    // Running again clears the mark; a failure with no sleep is a real failure.
    let job = lib.claim_job().unwrap().unwrap();
    assert_eq!(job.interrupted, None);
    jobs::settle(&lib, &job, &cut, &Since::now()).unwrap();
    assert_eq!(lib.job(id).unwrap().status, JobStatus::Failed);
}

#[test]
fn a_job_a_quit_left_running_is_queued_again_and_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(tmp.path()).unwrap();
    let (id, _) = jobs::enqueue(&mut lib, "learn_edits", &json!({}), None, None).unwrap();
    lib.claim_job().unwrap().unwrap();
    lib.job_progress(id, 3, 10).unwrap();
    assert_eq!(lib.requeue_stale_jobs().unwrap(), 1);
    let j = lib.job(id).unwrap();
    assert_eq!(
        (j.status, j.interrupted.as_deref()),
        (JobStatus::Queued, Some("quit"))
    );
}
