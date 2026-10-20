//! Keeping edits safe (ADR 0009): schema upgrades, backups and restore, and a writer
//! killed mid-write never losing or tearing what it had committed.

use argus_engine::library::{Author, ImportOptions, Library, PhotoFilter, Verdict, SCHEMA_VERSION};
use rusqlite::Connection;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

fn write_jpeg(path: &Path, seed: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..64 * 48 * 3)
        .map(|i| (i as u8).wrapping_mul(seed))
        .collect();
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, 64, 48, jpeg_encoder::ColorType::Rgb)
        .unwrap();
}

/// A Library with one imported Shoot of two Photos.
fn library(dir: &Path) -> (PathBuf, Vec<i64>) {
    let src = dir.join("card");
    write_jpeg(&src.join("a.jpg"), 3);
    write_jpeg(&src.join("b.jpg"), 5);
    let root = dir.join("library");
    let mut lib = Library::open(&root).unwrap();
    lib.import(&src, &ImportOptions::default()).unwrap();
    let ids = lib
        .list_photos(&PhotoFilter::default())
        .unwrap()
        .iter()
        .map(|p| p.id)
        .collect();
    (root, ids)
}

/// Every table's columns (name, type, notnull, pk), sorted, so column order from
/// `ALTER TABLE ADD COLUMN` doesn't matter.
fn shape(db: &Path) -> BTreeMap<String, Vec<(String, String, bool, i64)>> {
    let c = Connection::open(db).unwrap();
    let tables: Vec<String> = c
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    tables
        .into_iter()
        .map(|t| {
            let mut cols: Vec<_> = c
                .prepare(&format!("PRAGMA table_info({t})"))
                .unwrap()
                .query_map([], |r| Ok((r.get(1)?, r.get(2)?, r.get(3)?, r.get(5)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            cols.sort();
            (t, cols)
        })
        .collect()
}

#[test]
fn an_old_library_upgrades_to_the_same_schema_as_a_new_one_and_keeps_its_data() {
    let tmp = tempfile::tempdir().unwrap();
    let old = tmp.path().join("old");
    std::fs::create_dir_all(&old).unwrap();
    {
        let c = Connection::open(old.join("library.db")).unwrap();
        c.execute_batch(include_str!("data/schema-v1.sql")).unwrap();
        c.execute_batch(
            "PRAGMA user_version = 1;
             INSERT INTO shoots VALUES (1, '2026-10-17_Tahoe', '/card', '2026-10-17T10:00:00Z');
             INSERT INTO photos VALUES (1, 1, '/card/a.jpg', 'a.jpg', 'h1', 10, 'jpeg',
               '2026-10-17T09:00:00', 64, 48, 'Sony', 'previews/a.jpg', '2026-10-17T10:00:00Z');
             INSERT INTO verdicts VALUES (1, 'pick', 'person', NULL, NULL, '2026-10-17T10:01:00Z');
             INSERT INTO recipe_versions VALUES (1, 1, '{\"global\":{\"exposure\":0.5}}', 'person', NULL, '2026-10-17T10:02:00Z');",
        )
        .unwrap();
    }
    let lib = Library::open(&old).unwrap();
    let fresh = tmp.path().join("fresh");
    drop(Library::open(&fresh).unwrap());
    assert_eq!(
        shape(&old.join("library.db")),
        shape(&fresh.join("library.db")),
        "an upgraded Library must match schema.sql: append a step in migrate.rs and change schema.sql together"
    );

    let photos = lib.list_photos(&PhotoFilter::default()).unwrap();
    assert_eq!(photos.len(), 1);
    assert_eq!(photos[0].verdict, Verdict::Pick);
    assert_eq!(
        lib.read_recipe(1).unwrap().unwrap().recipe,
        json!({"global":{"exposure":0.5}})
    );
    let pre: Vec<_> = lib
        .backups()
        .unwrap()
        .into_iter()
        .filter(|b| b.kind == format!("pre-v{SCHEMA_VERSION}"))
        .collect();
    assert_eq!(pre.len(), 1, "the old database is copied before upgrading");
    let c = Connection::open(&pre[0].path).unwrap();
    let v: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, 1, "the pre-upgrade copy is the old version, untouched");
}

#[test]
fn upgrading_reads_exposure_settings_from_online_originals() {
    let raw = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/raw/DSC03945.ARW");
    if !raw.exists() {
        eprintln!("library_safety: no fixtures/raw, skipping the exposure backfill");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let old = tmp.path().join("old");
    std::fs::create_dir_all(&old).unwrap();
    {
        let c = Connection::open(old.join("library.db")).unwrap();
        c.execute_batch(include_str!("data/schema-v1.sql")).unwrap();
        c.execute_batch(
            "PRAGMA user_version = 1;
             INSERT INTO shoots VALUES (1, 'old', '/card', '2026-10-17T10:00:00Z');",
        )
        .unwrap();
        c.execute(
            "INSERT INTO photos VALUES (1, 1, ?1, 'DSC03945.ARW', 'h1', 10, 'raw',
               '2026-10-17T09:00:00', 7952, 5304, 'Sony', 'previews/a.jpg', '2026-10-17T10:00:00Z'),
             (2, 1, '/offline/b.ARW', 'b.ARW', 'h2', 10, 'raw',
               '2026-10-17T09:00:00', 7952, 5304, 'Sony', 'previews/b.jpg', '2026-10-17T10:00:00Z')",
            [raw.canonicalize().unwrap().to_string_lossy()],
        )
        .unwrap();
    }
    let lib = Library::open(&old).unwrap();
    let photos = lib.list_photos(&PhotoFilter::default()).unwrap();
    let e = &photos.iter().find(|p| p.id == 1).unwrap().exposure;
    assert_eq!(
        (e.aperture, e.iso, e.focal),
        (Some(5.0), Some(100), Some(28.0))
    );
    assert_eq!(e.lens.as_deref(), Some("FE 28mm F2"));
    let offline = &photos.iter().find(|p| p.id == 2).unwrap().exposure;
    assert_eq!(offline.iso, None, "an offline original stays blank");
}

#[test]
fn a_library_from_a_newer_argus_is_refused_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("lib");
    drop(Library::open(&root).unwrap());
    let db = root.join("library.db");
    Connection::open(&db)
        .unwrap()
        .execute_batch(&format!("PRAGMA user_version = {}", SCHEMA_VERSION + 1))
        .unwrap();
    let err = Library::open(&root).err().expect("refused").to_string();
    assert!(err.contains("newer Argus"), "{err}");
    let v: i64 = Connection::open(&db)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, SCHEMA_VERSION + 1);
}

#[test]
fn daily_backups_are_made_once_a_day_and_pruned_to_the_newest_fourteen() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, _) = library(tmp.path());
    let lib = Library::open(&root).unwrap();
    let made = lib.backup_if_due().unwrap().expect("first backup");
    // Canonical on both sides: macOS temp dirs live under /var, a link to /private/var.
    let backups = root.join("backups").canonicalize().unwrap();
    assert!(made.canonicalize().unwrap().starts_with(&backups));
    assert!(
        lib.backup_if_due().unwrap().is_none(),
        "not due again today"
    );

    // Twenty old daily backups and one tagged one, all a week old.
    let week_ago = SystemTime::now() - Duration::from_secs(7 * 86_400);
    std::fs::remove_file(&made).unwrap();
    for day in 1..=20 {
        let p = root.join(format!("backups/2026-09-{day:02}_120000.db"));
        std::fs::copy(root.join("library.db"), &p).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(week_ago)
            .unwrap();
    }
    std::fs::copy(
        root.join("library.db"),
        root.join("backups/2026-09-01_000000_pre-v2.db"),
    )
    .unwrap();
    lib.backup_if_due().unwrap().expect("a week old: due");
    let all = lib.backups().unwrap();
    let daily: Vec<_> = all.iter().filter(|b| b.kind == "daily").collect();
    assert_eq!(daily.len(), 14);
    assert!(
        daily[0].name.as_str() > "2026-10",
        "newest kept: {}",
        daily[0].name
    );
    assert_eq!(daily[13].name, "2026-09-08_120000.db");
    assert!(
        all.iter().any(|b| b.kind == "pre-v2"),
        "tagged backups are never pruned"
    );
    assert!(
        !root.join("backups").read_dir().unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")),
        "no temp files left behind"
    );
}

#[test]
fn restoring_a_backup_puts_back_verdicts_and_recipes_and_keeps_the_current_state() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, ids) = library(tmp.path());
    let mut lib = Library::open(&root).unwrap();
    lib.set_verdict(&ids[..1], Verdict::Pick, Author::Person, None, None)
        .unwrap();
    lib.write_recipe(
        ids[0],
        &json!({"global":{"exposure":1}}),
        Author::Person,
        None,
    )
    .unwrap();
    let good = lib.backup_now().unwrap();

    // Later: a bad batch, seen by a second open connection (the app, say).
    let other = Library::open(&root).unwrap();
    lib.set_verdict(&ids, Verdict::Reject, Author::Ai, None, None)
        .unwrap();
    lib.write_recipe(ids[0], &json!({"global":{"exposure":-3}}), Author::Ai, None)
        .unwrap();

    let name = good.file_name().unwrap().to_string_lossy().to_string();
    let before = lib.restore_backup(&name).unwrap();
    assert!(before.to_string_lossy().contains("before-restore"));
    for l in [&lib, &other] {
        let p = l.list_photos(&PhotoFilter::default()).unwrap();
        assert_eq!(p[0].verdict, Verdict::Pick);
        assert_eq!(p[1].verdict, Verdict::Unflagged);
        assert_eq!(
            l.read_recipe(ids[0]).unwrap().unwrap().recipe,
            json!({"global":{"exposure":1}})
        );
    }
    assert!(lib.restore_backup("nope.db").is_err());
}

/// Run by [`killing_a_writer_mid_write_loses_nothing_it_committed`] in a child process:
/// write Recipe Versions and flags as fast as possible, printing each one after its
/// commit returns. Does nothing in a normal test run.
#[test]
fn kill_child_writer() {
    let Some(root) = std::env::var_os("ARGUS_KILL_CHILD") else {
        return;
    };
    let mut lib = Library::open(Path::new(&root)).unwrap();
    let ids: Vec<i64> = lib
        .list_photos(&PhotoFilter::default())
        .unwrap()
        .iter()
        .map(|p| p.id)
        .collect();
    for n in 0..usize::MAX {
        let id = ids[n % ids.len()];
        let batch = lib.begin_batch(Author::Ai, "kill test").unwrap();
        let v = lib
            .write_recipe(
                id,
                &json!({"global":{"exposure": n}}),
                Author::Ai,
                Some(batch),
            )
            .unwrap();
        let verdict = if n % 2 == 0 {
            Verdict::Pick
        } else {
            Verdict::Reject
        };
        lib.set_verdict(&[id], verdict, Author::Ai, None, Some(batch))
            .unwrap();
        if n % 7 == 3 {
            lib.undo_batch(batch, Author::Person).unwrap();
        }
        println!("committed {id} {v} {n}");
    }
}

#[test]
fn killing_a_writer_mid_write_loses_nothing_it_committed() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, _) = library(tmp.path());
    let exe = std::env::current_exe().unwrap();
    let mut total = 0;
    for round in 0..8u64 {
        let mut child = Command::new(&exe)
            .args([
                "library_safety::kill_child_writer",
                "--exact",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("ARGUS_KILL_CHILD", &root)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        // Let it write for a while, then SIGKILL it wherever it is.
        let stop = Instant::now() + Duration::from_millis(150 + round * 37);
        let mut committed: Vec<(i64, i64)> = Vec::new();
        let mut line = String::new();
        while Instant::now() < stop {
            line.clear();
            if out.read_line(&mut line).unwrap() == 0 {
                break;
            }
            if let Some(rest) = line.trim().strip_prefix("committed ") {
                let f: Vec<i64> = rest.split(' ').map(|x| x.parse().unwrap()).collect();
                committed.push((f[0], f[1]));
            }
        }
        child.kill().unwrap();
        // Anything it printed after we stopped reading was also committed.
        for line in out.lines().map_while(Result::ok) {
            if let Some(rest) = line.trim().strip_prefix("committed ") {
                let f: Vec<i64> = rest.split(' ').map(|x| x.parse().unwrap()).collect();
                committed.push((f[0], f[1]));
            }
        }
        child.wait().unwrap();
        assert!(
            !committed.is_empty(),
            "round {round}: the writer never got going"
        );
        total += committed.len();

        let lib = Library::open(&root).unwrap();
        lib.check().unwrap();
        for (photo, version) in &committed {
            let have = lib.read_recipe(*photo).unwrap().unwrap().version;
            assert!(
                have >= *version,
                "round {round}: Photo {photo} lost Version {version} (has {have})"
            );
        }
        // Versions are whole and gapless: no torn or half-numbered writes.
        for p in lib.list_photos(&PhotoFilter::default()).unwrap() {
            let vs = lib.versions(p.id).unwrap();
            for (i, v) in vs.iter().enumerate() {
                assert_eq!(v.version, i as i64 + 1);
                assert!(v.recipe.is_object());
            }
        }
    }
    assert!(total > 50, "only {total} writes across all rounds");
}
