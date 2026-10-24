//! AI cull through its seam: a fake Provider stands in for Claude.

use argus_engine::compute::Compute;
use argus_engine::cull::{
    apply, cull_shoot, cull_shoot_with, split_chunks, ContactSheet, CullPhoto, CullResult,
    Provider, SheetPhoto,
};
use argus_engine::library::{Author, ImportOptions, Library, PhotoFilter, Verdict};
use argus_engine::providers::{count_photo_keys, parse_json_reply, parse_response, Claude, Ollama};
use argus_engine::signals::Signals;
use std::cell::RefCell;
use std::path::Path;

fn write_jpeg(path: &Path, seed: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..320 * 240 * 3)
        .map(|i| (i as u8).wrapping_mul(seed))
        .collect();
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, 320, 240, jpeg_encoder::ColorType::Rgb)
        .unwrap();
}

fn library_with(n: u8) -> (tempfile::TempDir, Library, i64) {
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..n {
        write_jpeg(&tmp.path().join(format!("shoot/{i:02}.jpg")), i * 2 + 1);
    }
    let mut lib = Library::open(&tmp.path().join("lib")).unwrap();
    let shoot = lib
        .import(&tmp.path().join("shoot"), &ImportOptions::default())
        .unwrap()
        .shoot_id
        .unwrap();
    (tmp, lib, shoot)
}

fn photo(
    n: usize,
    verdict: Verdict,
    reason: &str,
    stack: Option<u32>,
    rank: Option<u32>,
) -> CullPhoto {
    CullPhoto {
        n,
        verdict,
        reason: reason.into(),
        stack,
        rank,
        stars: None,
        best: None,
    }
}

fn rated(mut c: CullPhoto, stars: u8, best: Option<u32>) -> CullPhoto {
    c.stars = Some(stars);
    c.best = best;
    c
}

struct Fake(CullResult);
impl Provider for Fake {
    fn cull(&self, sheet: &ContactSheet) -> argus_engine::Result<CullResult> {
        assert!(sheet.photos.iter().all(|p| !p.jpeg.is_empty()));
        Ok(self.0.clone())
    }
}

#[test]
fn cull_writes_ai_verdicts_and_stacks_but_keeps_person_flags() {
    let (_tmp, mut lib, shoot) = library_with(4);
    let ids: Vec<_> = lib
        .list_photos(&PhotoFilter::default())
        .unwrap()
        .iter()
        .map(|p| p.id)
        .collect();
    lib.set_verdict(&ids[3..], Verdict::Pick, Author::Person, None, None)
        .unwrap();

    let fake = Fake(CullResult {
        summary: "One burst of three; kept the sharpest.".into(),
        photos: vec![
            photo(1, Verdict::Reject, "soft", Some(7), Some(2)),
            photo(2, Verdict::Pick, "best of 3", Some(7), Some(1)),
            photo(3, Verdict::Unflagged, "close call", Some(7), Some(3)),
            photo(4, Verdict::Reject, "eyes closed", None, None),
        ],
    });
    let r = cull_shoot(&mut lib, shoot, &fake).unwrap();
    assert_eq!(
        (
            r.picks,
            r.rejects,
            r.unflagged,
            r.stacks,
            r.kept_person_verdicts
        ),
        (0, 1, 2, 1, 1)
    );

    let photos = lib.list_photos(&PhotoFilter::default()).unwrap();
    assert_eq!(photos[0].verdict, Verdict::Reject);
    assert_eq!(photos[1].verdict, Verdict::Unflagged, "the AI never picks");
    assert_eq!(photos[1].verdict_by, Some(Author::Ai));
    assert_eq!(photos[1].reason.as_deref(), Some("best of 3"));
    assert_eq!(photos[3].verdict, Verdict::Pick, "the person's flag wins");
    assert_eq!(photos[3].verdict_by, Some(Author::Person));
    let stack = photos[0].stack.expect("burst is stacked");
    assert!(photos[..3].iter().all(|p| p.stack == Some(stack)));
    assert_eq!(photos[1].stack_rank, Some(0), "best frame ranks first");
    assert_eq!(photos[3].stack, None);

    // Culling again replaces the Stacks rather than adding more.
    cull_shoot(&mut lib, shoot, &fake).unwrap();
    let again = lib.list_photos(&PhotoFilter::default()).unwrap();
    assert_eq!(again.iter().filter(|p| p.stack.is_some()).count(), 3);
}

#[test]
fn ai_cull_is_cautious_it_never_picks_and_rejects_only_one_star_frames() {
    let (_tmp, mut lib, shoot) = library_with(4);
    let fake = Fake(CullResult {
        summary: String::new(),
        photos: vec![
            rated(
                photo(1, Verdict::Pick, "great moment", None, None),
                5,
                Some(1),
            ),
            rated(
                photo(2, Verdict::Reject, "missed focus", None, None),
                1,
                None,
            ),
            rated(
                photo(3, Verdict::Reject, "weaker of 2", None, None),
                2,
                None,
            ),
            rated(photo(4, Verdict::Unflagged, "fine", None, None), 3, None),
        ],
    });
    let r = cull_shoot(&mut lib, shoot, &fake).unwrap();
    assert_eq!((r.picks, r.rejects, r.unflagged), (0, 1, 3));
    let photos = lib.list_photos(&PhotoFilter::default()).unwrap();
    let verdicts: Vec<_> = photos.iter().map(|p| p.verdict).collect();
    use Verdict::*;
    assert_eq!(verdicts, [Unflagged, Reject, Unflagged, Unflagged]);
    assert_eq!(photos[0].stars, 5, "the stars still carry the AI's taste");
    assert_eq!(photos[2].reason.as_deref(), Some("weaker of 2"));
}

#[test]
fn cull_skips_made_up_and_missing_photo_numbers() {
    let (_tmp, mut lib, shoot) = library_with(2);
    let sheet = ContactSheet::build(&lib, shoot).unwrap();
    let sloppy = CullResult {
        summary: String::new(),
        photos: vec![
            photo(1, Verdict::Pick, "ok", None, None),
            photo(9, Verdict::Pick, "?", None, None),
        ],
    };
    let r = apply(&mut lib, &sheet, &sloppy).unwrap();
    assert_eq!((r.unflagged, r.ignored, r.missing), (1, 1, 1));
}

#[test]
fn ollama_request_sends_each_photo_with_its_number_and_the_schema() {
    let (_tmp, lib, shoot) = library_with(3);
    let sheet = ContactSheet::build(&lib, shoot).unwrap();
    let body = Ollama {
        base_url: String::new(),
        model: "gemma4:31b".into(),
        num_ctx: 65_536,
        compute: Compute::Full,
    }
    .request_body(&sheet);
    let msgs = body["messages"].as_array().unwrap();
    let with_images: Vec<_> = msgs.iter().filter(|m| m["images"].is_array()).collect();
    assert_eq!(with_images.len(), 3);
    assert!(
        body["options"].get("num_thread").is_none(),
        "Full leaves threads to Ollama"
    );
    assert!(with_images[1]["content"]
        .as_str()
        .unwrap()
        .starts_with("Photo 2 "));
    assert_eq!(body["format"]["required"][1], "photos");
    assert_eq!(
        body["stream"], true,
        "streamed, so progress moves inside a request"
    );
}

#[test]
fn ollama_loads_the_model_with_the_same_options_it_culls_with() {
    // Any option difference makes Ollama reload the model mid-cull.
    let (_tmp, lib, shoot) = library_with(1);
    let sheet = ContactSheet::build(&lib, shoot).unwrap();
    for compute in [Compute::Gentle, Compute::Balanced, Compute::Full] {
        let o = Ollama {
            base_url: String::new(),
            model: "gemma4:31b".into(),
            num_ctx: 65_536,
            compute,
        };
        assert_eq!(o.load_body()["options"], o.request_body(&sheet)["options"]);
        assert_eq!(o.load_body()["messages"], serde_json::json!([]));
    }
}

#[test]
fn json_replies_survive_prose_and_fences() {
    let r =
        parse_json_reply("Here you go:\n```json\n{\"summary\":\"s\",\"photos\":[]}\n```").unwrap();
    assert_eq!(r.summary, "s");
    assert!(parse_json_reply("no json here").is_err());
}

#[test]
fn claude_request_carries_every_photo_and_a_strict_schema() {
    let (_tmp, lib, shoot) = library_with(3);
    let sheet = ContactSheet::build(&lib, shoot).unwrap();
    let claude = Claude {
        api_key: "k".into(),
        model: "claude-opus-5-5".into(),
        base_url: String::new(),
    };
    let body = claude.request_body(&sheet).unwrap();
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content.iter().filter(|b| b["type"] == "image").count(), 3);
    assert_eq!(body["model"], "claude-opus-5-5");
    let schema = &body["output_config"]["format"]["schema"];
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["photos"]["items"]["additionalProperties"],
        false
    );
}

#[test]
fn parses_claude_responses_and_refusals() {
    let ok = serde_json::json!({
        "stop_reason": "end_turn",
        "content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": "{\"summary\":\"s\",\"photos\":[{\"n\":1,\"verdict\":\"pick\",\"reason\":\"r\",\"stack\":null,\"rank\":null}]}"}]
    });
    let r = parse_response(&ok).unwrap();
    assert_eq!(r.photos[0].verdict, Verdict::Pick);
    let refused = serde_json::json!({"stop_reason": "refusal", "stop_details": {"category": "cyber"}, "content": []});
    assert!(parse_response(&refused).unwrap_err().0.contains("declined"));
}

#[test]
fn cull_rates_every_photo_and_keeps_person_stars() {
    let (_tmp, mut lib, shoot) = library_with(5);
    let ids: Vec<_> = lib
        .list_photos(&PhotoFilter::default())
        .unwrap()
        .iter()
        .map(|p| p.id)
        .collect();
    lib.set_rating(&ids[4..], 2, Author::Person, None).unwrap();

    let fake = Fake(CullResult {
        summary: "".into(),
        photos: vec![
            rated(
                photo(1, Verdict::Pick, "best of 2", Some(1), Some(1)),
                5,
                Some(1),
            ),
            // Same Stack as the best photo: dropped from Best Photos.
            rated(
                photo(2, Verdict::Unflagged, "close", Some(1), Some(2)),
                3,
                Some(2),
            ),
            rated(
                photo(3, Verdict::Pick, "lovely light", None, None),
                4,
                Some(3),
            ),
            rated(
                photo(4, Verdict::Reject, "missed focus", None, None),
                1,
                None,
            ),
            rated(photo(5, Verdict::Pick, "", None, None), 5, Some(4)),
        ],
    });
    let r = cull_shoot(&mut lib, shoot, &fake).unwrap();
    assert_eq!(r.rated, 4);
    assert_eq!(r.stars, [0, 1, 0, 1, 1, 1]);
    assert_eq!(r.kept_person_ratings, 1);
    assert_eq!(r.best, vec![ids[0], ids[2], ids[4]]);

    let photos = lib.list_photos(&PhotoFilter::default()).unwrap();
    let stars: Vec<_> = photos.iter().map(|p| (p.stars, p.stars_by)).collect();
    assert_eq!(stars[0], (5, Some(Author::Ai)));
    assert_eq!(stars[3], (1, Some(Author::Ai)));
    assert_eq!(
        stars[4],
        (2, Some(Author::Person)),
        "the person's stars win"
    );
    assert_eq!(photos[2].best, Some(2));
    assert_eq!(photos[1].best, None);

    // A Photo the person rejects drops out of Best Photos.
    lib.set_verdict(&[ids[2]], Verdict::Reject, Author::Person, None, None)
        .unwrap();
    let best: Vec<_> = lib
        .best_photos(shoot)
        .unwrap()
        .iter()
        .map(|b| b.photo_id)
        .collect();
    assert_eq!(best, vec![ids[0], ids[4]]);
}

#[test]
fn person_overriding_ai_stars_is_logged_and_undo_puts_stars_back() {
    let (_tmp, mut lib, shoot) = library_with(2);
    let ids: Vec<_> = lib
        .list_photos(&PhotoFilter::default())
        .unwrap()
        .iter()
        .map(|p| p.id)
        .collect();
    lib.set_rating(&ids[..1], 3, Author::Person, None).unwrap();
    let before = lib.override_count().unwrap();
    let fake = Fake(CullResult {
        summary: "".into(),
        photos: vec![
            rated(photo(1, Verdict::Pick, "", None, None), 5, None),
            rated(photo(2, Verdict::Pick, "", None, None), 4, None),
        ],
    });
    cull_shoot(&mut lib, shoot, &fake).unwrap();
    let (batch, _) = lib.last_batch(Some(Author::Ai)).unwrap().unwrap();
    lib.set_rating(&ids[1..], 2, Author::Person, None).unwrap();
    assert_eq!(lib.override_count().unwrap(), before + 1);

    let undo = lib.undo_batch(batch, Author::Person).unwrap();
    assert_eq!(undo.ratings, vec![ids[1]]);
    let photos = lib.list_photos(&PhotoFilter::default()).unwrap();
    assert_eq!(
        (photos[0].stars, photos[0].stars_by),
        (3, Some(Author::Person))
    );
    assert_eq!(photos[1].stars, 0, "never rated before the cull");
    assert_eq!(photos[1].stars_by, None);
}

fn sheet_photo(n: usize, at: &str) -> SheetPhoto {
    SheetPhoto {
        n,
        id: n as i64,
        file_name: format!("{n}.jpg"),
        captured_at: at.into(),
        jpeg: vec![],
        signals: Signals::default(),
        sharpness_pct: 50,
        facts: String::new(),
        taste: 0.0,
        burst: None,
        tilt: None,
        sharpness: Default::default(),
        shut_eyes: false,
    }
}

#[test]
fn chunks_split_at_the_longest_pause_so_bursts_stay_together() {
    // Ten frames: a burst at 12:00:00-05, a 10-minute pause, then a burst from 12:10.
    let times = [
        "2024-08-10T12:00:00",
        "2024-08-10T12:00:01",
        "2024-08-10T12:00:02",
        "2024-08-10T12:00:03",
        "2024-08-10T12:00:04",
        "2024-08-10T12:00:05",
        "2024-08-10T12:10:00",
        "2024-08-10T12:10:01",
        "2024-08-10T12:10:02",
        "2024-08-10T12:10:03",
    ];
    let photos: Vec<_> = times
        .iter()
        .enumerate()
        .map(|(i, t)| sheet_photo(i + 1, t))
        .collect();
    assert_eq!(split_chunks(&photos, 7), vec![0..6, 6..10]);
    assert_eq!(split_chunks(&photos, 10), vec![0..10]);
    let small = split_chunks(&photos, 3);
    assert!(small.iter().all(|r| r.len() <= 3), "{small:?}");
    assert_eq!(small.iter().map(|r| r.len()).sum::<usize>(), 10);
}

/// Rates by file order and puts the first two Photos of every sheet up as Best Photos;
/// records the size of each sheet it saw.
struct Chunky(RefCell<Vec<usize>>);
impl Provider for Chunky {
    fn cull(&self, sheet: &ContactSheet) -> argus_engine::Result<CullResult> {
        self.0.borrow_mut().push(sheet.photos.len());
        let last_pass = sheet
            .context
            .as_deref()
            .is_some_and(|c| c.contains("candidates"));
        Ok(CullResult {
            summary: format!("{} photos", sheet.photos.len()),
            photos: sheet
                .photos
                .iter()
                .map(|p| {
                    // The final pass prefers later frames, so its order must win.
                    let best = if last_pass {
                        Some((sheet.photos.len() - p.n + 1) as u32)
                    } else {
                        (p.n <= 2).then_some(p.n as u32)
                    };
                    rated(
                        photo(p.n, Verdict::Pick, "", Some(1), Some(p.n as u32)),
                        4,
                        best,
                    )
                })
                .collect(),
        })
    }
}

#[test]
fn large_shoots_cull_in_chunks_then_pick_best_photos_across_them() {
    let (_tmp, mut lib, shoot) = library_with(7);
    let ids: Vec<_> = lib
        .list_photos(&PhotoFilter::default())
        .unwrap()
        .iter()
        .map(|p| p.id)
        .collect();
    let chunky = Chunky(RefCell::new(vec![]));
    let seen = std::sync::Mutex::new(vec![]);
    let r = cull_shoot_with(&mut lib, shoot, &chunky, 3, &|p| {
        seen.lock().unwrap().push(p)
    })
    .unwrap();
    let seen: Vec<_> = seen.into_inner().unwrap();
    let sizes = chunky.0.borrow().clone();
    // Three chunks of at most 3, then one pass over the candidates: each chunk's best,
    // capped at one request.
    assert_eq!(&sizes[..3].iter().sum::<usize>(), &7);
    assert!(sizes[..3].iter().all(|&s| s <= 3));
    assert_eq!(sizes[3], 3);
    let first = seen.first().unwrap();
    let last = seen.last().unwrap();
    assert_eq!((first.done, first.total, first.photos), (0, 4, 0));
    assert_eq!((last.done, last.total), (4, 4));
    // In Photos: 7 in the chunks plus the final pass's 3 candidates.
    assert_eq!((last.photos, last.photos_total), (10, 10));
    // The bar counts the Shoot's 7 Photos, whatever the passes add up to.
    assert_eq!(last.bar(), (7, 7));
    assert!(seen
        .iter()
        .any(|p| p.stage() == "Reading the photos, part 2 of 3"));
    assert!(seen.iter().any(|p| p.stage() == "Choosing Best Photos"));
    assert!(
        seen.windows(2).all(|w| w[0].photos <= w[1].photos),
        "never goes back"
    );
    assert_eq!(r.rated, 7);
    // Stacks never span chunks: three Stacks, one per chunk.
    assert_eq!(r.stacks, 3);
    // Best Photos follow the final pass (latest candidate first), one per Stack.
    assert_eq!(r.best.len(), 3);
    let photos = lib.list_photos(&PhotoFilter::default()).unwrap();
    let first_best = photos.iter().find(|p| p.best == Some(1)).unwrap();
    assert!(ids[3..].contains(&first_best.id), "{:?}", r.best);
}

/// Chunks put their first two Photos up as Best Photos; the final pass names only one.
struct Stingy;
impl Provider for Stingy {
    fn cull(&self, sheet: &ContactSheet) -> argus_engine::Result<CullResult> {
        let last_pass = sheet
            .context
            .as_deref()
            .is_some_and(|c| c.contains("candidates"));
        Ok(CullResult {
            summary: String::new(),
            photos: sheet
                .photos
                .iter()
                .map(|p| {
                    let best = if last_pass {
                        (p.n == 1).then_some(1)
                    } else {
                        (p.n <= 2).then_some(p.n as u32)
                    };
                    rated(photo(p.n, Verdict::Unflagged, "", None, None), 4, best)
                })
                .collect(),
        })
    }
}

#[test]
fn chunked_culls_keep_about_one_best_photo_in_ten_of_the_whole_shoot() {
    let (_tmp, mut lib, shoot) = library_with(40);
    let r = cull_shoot_with(&mut lib, shoot, &Stingy, 10, &|_| {}).unwrap();
    assert_eq!(r.best.len(), 4, "{:?}", r.best);
}

#[test]
fn sheet_captions_carry_signals() {
    let (_tmp, lib, shoot) = library_with(3);
    let sheet = ContactSheet::build(&lib, shoot).unwrap();
    let pcts: Vec<_> = sheet.photos.iter().map(|p| p.sharpness_pct).collect();
    assert!(pcts.contains(&100) && pcts.contains(&0), "{pcts:?}");
    let c = sheet.photos[0].caption();
    assert!(c.starts_with("Photo 1 ("), "{c}");
    assert!(c.contains("sharpness") && c.contains("brightness"), "{c}");
}

/// Leaves the first two Photos out of its first reply, as local models sometimes do,
/// and answers whatever it is asked about after that; records each sheet's size.
struct Forgetful(RefCell<Vec<usize>>);
impl Provider for Forgetful {
    fn cull(&self, sheet: &ContactSheet) -> argus_engine::Result<CullResult> {
        let first = self.0.borrow().is_empty();
        self.0.borrow_mut().push(sheet.photos.len());
        Ok(CullResult {
            summary: String::new(),
            photos: sheet
                .photos
                .iter()
                .filter(|p| !first || p.n > 2)
                .map(|p| rated(photo(p.n, Verdict::Pick, "", Some(1), None), 3, None))
                .collect(),
        })
    }
}

#[test]
fn photos_a_reply_leaves_out_are_asked_about_again() {
    let (_tmp, mut lib, shoot) = library_with(5);
    let forgetful = Forgetful(RefCell::new(vec![]));
    let r = cull_shoot(&mut lib, shoot, &forgetful).unwrap();
    // One request for the Shoot, then one for the two it skipped.
    assert_eq!(*forgetful.0.borrow(), vec![5, 2]);
    assert_eq!((r.missing, r.rated, r.unflagged), (0, 5, 5));
    // The second reply's Stack stays apart from the first's.
    assert_eq!(r.stacks, 2);
}

#[test]
fn photo_keys_are_counted_once_even_split_across_pieces() {
    let json = br#"{"summary":"a \"n\" b","photos":[{"n":1,"reason":"x"},{"n":2}]}"#;
    assert_eq!(count_photo_keys(json, 0).0, 2, "escaped quotes don't count");
    // Fed piece by piece, a key cut in half is still counted exactly once.
    let mut found = 0;
    let mut from = 0;
    for end in 1..=json.len() {
        let (f, next) = count_photo_keys(&json[..end], from);
        found += f;
        from = next;
    }
    assert_eq!(found, 2);
}

/// A fake Ollama: answers the model load at once, then streams the cull reply in small
/// pieces after a "thinking" piece, as /api/chat does with `stream: true`.
fn fake_ollama(reply: String) -> String {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(2) {
            let mut stream = stream.unwrap();
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut len = 0;
            loop {
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let req: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let mut out = String::new();
            if req["stream"] == true {
                let line = |v: serde_json::Value| format!("{v}\n");
                out += &line(
                    serde_json::json!({"message": {"content": "", "thinking": "hmm"}, "done": false}),
                );
                for piece in reply.as_bytes().chunks(7) {
                    let piece = std::str::from_utf8(piece).unwrap();
                    out += &line(serde_json::json!({"message": {"content": piece}, "done": false}));
                }
                out += &line(
                    serde_json::json!({"message": {"content": ""}, "done": true, "eval_count": 9}),
                );
            } else {
                out = r#"{"message":{"content":""},"done":true}"#.into();
            }
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                out.len()
            )
            .unwrap();
        }
    });
    format!("http://{addr}")
}

#[test]
fn ollama_streams_its_reply_and_progress_moves_photo_by_photo() {
    let (_tmp, mut lib, shoot) = library_with(3);
    let reply = serde_json::json!({
        "summary": "ok",
        "photos": (1..=3).map(|n| serde_json::json!({
            "n": n, "verdict": "pick", "stars": 4, "reason": "sharp", "stack": null, "rank": null, "best": null
        })).collect::<Vec<_>>()
    })
    .to_string();
    let ollama = Ollama {
        base_url: fake_ollama(reply),
        model: "test-model".into(),
        num_ctx: 4096,
        compute: Compute::Full,
    };
    let seen = std::sync::Mutex::new(vec![]);
    let r = cull_shoot_with(&mut lib, shoot, &ollama, 100, &|p| {
        seen.lock().unwrap().push(p)
    })
    .unwrap();
    assert_eq!(r.unflagged, 3);
    let seen = seen.into_inner().unwrap();
    let photos: Vec<usize> = seen.iter().map(|p| p.photos).collect();
    for n in 1..=3 {
        assert!(
            photos.contains(&n),
            "reported Photo {n} as it streamed in: {photos:?}"
        );
    }
    assert!(
        seen.iter().any(|p| p.tokens > 0 && p.done == 0),
        "tokens counted mid-request"
    );
    assert!(seen.iter().any(|p| p.stage() == "Culling"));
    let last = seen.last().unwrap();
    assert_eq!((last.photos, last.photos_total, last.done), (3, 3, 1));
}

/// Answers after a pause without streaming, like the Claude Providers.
struct Slow;
impl Provider for Slow {
    fn cull(&self, sheet: &ContactSheet) -> argus_engine::Result<CullResult> {
        std::thread::sleep(std::time::Duration::from_millis(400));
        Ok(CullResult {
            summary: String::new(),
            photos: sheet
                .photos
                .iter()
                .map(|p| photo(p.n, Verdict::Pick, "", None, None))
                .collect(),
        })
    }
}

#[test]
fn a_silent_request_still_sends_a_heartbeat() {
    std::env::set_var("ARGUS_CULL_HEARTBEAT_MS", "50");
    let (_tmp, mut lib, shoot) = library_with(2);
    let seen = std::sync::Mutex::new(vec![]);
    cull_shoot_with(&mut lib, shoot, &Slow, 100, &|p| {
        seen.lock().unwrap().push(p)
    })
    .unwrap();
    let seen = seen.into_inner().unwrap();
    let waiting = seen.iter().filter(|p| p.done == 0).count();
    // One report as the request starts, then heartbeats while it is silent.
    assert!(waiting >= 4, "{waiting} reports while waiting: {seen:?}");
    assert_eq!(seen[0].stage(), "Reading the photos");
}

/// Reads like a local model: silent for a while, at a known pace per Photo.
struct Reader;
impl Provider for Reader {
    fn cull(&self, sheet: &ContactSheet) -> argus_engine::Result<CullResult> {
        Slow.cull(sheet)
    }
    fn read_pace(&self) -> Option<f64> {
        Some(0.02)
    }
}

#[test]
fn the_bar_moves_while_the_model_reads_the_sheet() {
    std::env::set_var("ARGUS_CULL_HEARTBEAT_MS", "50");
    let (_tmp, mut lib, shoot) = library_with(10);
    let seen = std::sync::Mutex::new(vec![]);
    cull_shoot_with(&mut lib, shoot, &Reader, 100, &|p| {
        seen.lock().unwrap().push(p)
    })
    .unwrap();
    let seen = seen.into_inner().unwrap();
    let reading: Vec<_> = seen
        .iter()
        .filter(|p| p.done == 0 && p.tokens == 0)
        .collect();
    // Moves before the model says a word, in the Shoot's Photos, but never claims the
    // whole sheet read (or half the work done) before the model replies.
    assert!(reading.iter().any(|p| p.bar().0 > 0), "{seen:?}");
    assert!(
        reading.iter().all(|p| p.read <= 9 && p.bar().0 < 5),
        "{seen:?}"
    );
    assert!(reading.iter().all(|p| p.bar().1 == 10));
    assert!(reading.iter().all(|p| p.stage() == "Reading the photos"));
    assert!(
        seen.windows(2).all(|w| w[0].bar().0 <= w[1].bar().0),
        "never goes back"
    );
    assert_eq!(seen.last().unwrap().bar(), (10, 10));
}

/// A fake Ollama that reads one request, then sends `head` (raw HTTP, maybe a partial
/// stream) and keeps the connection open, saying nothing more, for `hold`.
fn silent_ollama(head: &'static str, hold: std::time::Duration) -> String {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut r = BufReader::new(stream.try_clone().unwrap());
        let mut len = 0;
        loop {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap();
            }
            if line == "\r\n" {
                break;
            }
        }
        let mut body = vec![0; len];
        r.read_exact(&mut body).unwrap();
        stream.write_all(head.as_bytes()).unwrap();
        stream.flush().unwrap();
        std::thread::sleep(hold);
    });
    format!("http://{addr}")
}

fn ollama_at(base_url: String) -> Ollama {
    Ollama {
        base_url,
        model: "test-model".into(),
        num_ctx: 4096,
        compute: Compute::Full,
    }
}

const STREAM_HEAD: &str =
    "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nTransfer-Encoding: chunked\r\n\r\n";

#[test]
fn a_request_ollama_never_answers_fails_instead_of_waiting_all_night() {
    use argus_engine::providers::Stall;
    use std::time::{Duration, Instant};
    let o = ollama_at(silent_ollama(STREAM_HEAD, Duration::from_secs(5)));
    let agent = ureq::AgentBuilder::new().build();
    let t = Instant::now();
    let stall = Stall {
        first: Duration::from_millis(300),
        gap: Duration::from_millis(300),
    };
    let e = o
        .stream_within(&agent, serde_json::json!({}), stall, &|_| {})
        .unwrap_err();
    assert!(t.elapsed() < Duration::from_secs(3), "gave up at the limit");
    assert!(
        e.0.contains("stopped answering") && e.0.contains("never started"),
        "{e}"
    );
}

#[test]
fn cancel_stops_a_cull_mid_reply_within_a_moment() {
    use argus_engine::progress::{cancel, Task, STOPPED};
    use argus_engine::providers::Stall;
    use std::time::{Duration, Instant};
    // The model is reading a big sheet and won't say anything for a while.
    let o = ollama_at(silent_ollama(STREAM_HEAD, Duration::from_secs(10)));
    let agent = ureq::AgentBuilder::new().build();
    let task = Task::start("cull-cancel-test", "Reading the photos", 0);
    let _in = task.enter();
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(300));
        cancel("cull-cancel-test");
    });
    let t = Instant::now();
    let stall = Stall {
        first: Duration::from_secs(60),
        gap: Duration::from_secs(60),
    };
    let e = o
        .stream_within(&agent, serde_json::json!({}), stall, &|_| {})
        .unwrap_err();
    assert_eq!(e.0, STOPPED);
    assert!(
        t.elapsed() < Duration::from_secs(3),
        "stopped within a moment"
    );
}

#[test]
fn a_reply_that_breaks_off_is_an_error_not_a_bad_parse() {
    use argus_engine::providers::Stall;
    use std::time::Duration;
    // One piece of the stream, then the connection closes (a model runner that died).
    let piece = r#"{"message":{"content":"{\"summary\":"},"done":false}"#;
    let head: &'static str = Box::leak(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n{piece}\n"
        )
        .into_boxed_str(),
    );
    let o = ollama_at(silent_ollama(head, Duration::ZERO));
    let agent = ureq::AgentBuilder::new().build();
    let stall = Stall {
        first: Duration::from_secs(5),
        gap: Duration::from_secs(5),
    };
    let e = o
        .stream_within(&agent, serde_json::json!({}), stall, &|_| {})
        .unwrap_err();
    assert!(e.0.contains("broke off after 1 tokens"), "{e}");
}

#[test]
fn a_cull_waits_its_turn_behind_another_argus_process_and_says_so() {
    use std::time::Duration;
    let _env = crate::env_lock();
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ARGUS_MODEL_LOCK_DIR", dir.path());
    std::env::set_var("ARGUS_MODEL_LOCK_POLL_MS", "20");
    // Another live process (pid 1 always exists) holds the model.
    let theirs = dir.path().join(format!("{:024}-1-0.u", 1));
    std::fs::write(&theirs, "`argus cull`").unwrap();
    assert_eq!(
        argus_engine::model_lock::busy_elsewhere().as_deref(),
        Some("`argus cull` is using the model")
    );

    /// A local-model Provider that answers at once.
    struct Local;
    impl Provider for Local {
        fn cull(&self, sheet: &ContactSheet) -> argus_engine::Result<CullResult> {
            Slow.cull(sheet)
        }
        fn local_model(&self) -> bool {
            true
        }
    }
    let (_tmp, mut lib, shoot) = library_with(2);
    let seen = std::sync::Mutex::new(vec![]);
    std::thread::scope(|s| {
        s.spawn(|| {
            // Free the model once the cull has asked for it and waited a moment.
            while !std::fs::read_dir(dir.path())
                .unwrap()
                .any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".w"))
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            std::thread::sleep(Duration::from_millis(200));
            std::fs::remove_file(&theirs).unwrap();
        });
        cull_shoot_with(&mut lib, shoot, &Local, 100, &|p| {
            seen.lock().unwrap().push(p)
        })
        .unwrap();
    });
    std::env::remove_var("ARGUS_MODEL_LOCK_DIR");
    std::env::remove_var("ARGUS_MODEL_LOCK_POLL_MS");
    let seen = seen.into_inner().unwrap();
    let stages: Vec<String> = seen.iter().map(|p| p.stage()).collect();
    assert!(
        stages.contains(&"Waiting: `argus cull` is using the model".to_owned()),
        "{stages:?}"
    );
    let waited = seen.iter().position(|p| p.waiting.is_some()).unwrap();
    assert!(
        seen[waited..]
            .iter()
            .any(|p| p.waiting.is_none() && p.done == 0),
        "went on once it was free: {stages:?}"
    );
    assert!(
        std::fs::read_dir(dir.path()).unwrap().next().is_none(),
        "own marker removed"
    );
}

#[test]
fn a_group_shot_gets_close_ups_of_its_faces_and_a_caption_for_each() {
    if !crate::has_bench() {
        return;
    }
    // Greg's 2026-10-10 card: six friends on a boat deck, faces about 7% of the frame.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2026-10-10_boat");
    std::fs::create_dir_all(&src).unwrap();
    let group = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/bench/cull/2026-10-10/photos/DSC07782.jpg");
    std::fs::copy(&group, src.join("DSC07782.jpg")).unwrap();
    let mut lib = Library::open(&tmp.path().join("lib")).unwrap();
    let shoot = lib
        .import(&src, &ImportOptions::default())
        .unwrap()
        .shoot_id
        .unwrap();
    let sheet = ContactSheet::build(&lib, shoot).unwrap();
    let p = &sheet.photos[0];
    let caption = p.caption();
    assert!(caption.contains("faces"), "{caption}");
    assert!(caption.contains("close-ups of these faces"), "{caption}");
    let img = image::load_from_memory(&p.jpeg).unwrap();
    assert!(
        img.height() as f32 > img.width() as f32 * 426.0 / 640.0 * 1.1,
        "a strip of close-ups under the frame: {}x{}",
        img.width(),
        img.height()
    );
}

fn sheet_of(tastes: &[(f32, Option<usize>)]) -> ContactSheet {
    ContactSheet {
        shoot: 1,
        shoot_name: "2026-10-10_Boat-Trip".into(),
        photos: tastes
            .iter()
            .enumerate()
            .map(|(i, &(taste, burst))| SheetPhoto {
                taste,
                burst,
                ..sheet_photo(i + 1, "2026-10-10T12:00:00")
            })
            .collect(),
        context: None,
    }
}

fn technical(n: usize, rejects: &[usize]) -> CullResult {
    CullResult {
        summary: "Two frames are soft.".into(),
        photos: (1..=n)
            .map(|i| {
                if rejects.contains(&i) {
                    photo(i, Verdict::Reject, "missed focus", None, None)
                } else {
                    photo(i, Verdict::Unflagged, "ok", None, None)
                }
            })
            .collect(),
    }
}

#[test]
fn the_taste_stage_rates_what_the_technical_stage_kept_by_its_faces() {
    // Twenty frames: 1-3 one burst, 4-5 another, the rest single; taste rises with the number
    // except frame 20, a reject that would taste best.
    let mut tastes: Vec<(f32, Option<usize>)> = (1..=20).map(|i| (i as f32 / 20.0, None)).collect();
    for t in tastes.iter_mut().take(3) {
        t.1 = Some(0);
    }
    tastes[3].1 = Some(1);
    tastes[4].1 = Some(1);
    let sheet = sheet_of(&tastes);
    let r = argus_engine::taste::rate(&sheet, technical(20, &[20]));
    let by = |n: usize| r.photos.iter().find(|c| c.n == n).unwrap();
    assert_eq!(by(20).stars, Some(1), "a reject is a 1-star frame");
    assert_eq!(by(19).stars, Some(5), "the best-tasting kept frame");
    assert_eq!(by(1).stars, Some(2), "the worst-tasting kept frame");
    // Bursts become Stacks, best-tasting frame first.
    assert_eq!((by(3).stack, by(3).rank), (Some(1), Some(1)));
    assert_eq!((by(1).stack, by(1).rank), (Some(1), Some(3)));
    assert_eq!((by(5).stack, by(5).rank), (Some(2), Some(1)));
    assert_eq!(by(10).stack, None);
    // Two Best Photos for twenty frames, the best-tasting kept ones, never the reject.
    let mut best: Vec<(u32, usize)> = r
        .photos
        .iter()
        .filter_map(|c| Some((c.best?, c.n)))
        .collect();
    best.sort();
    assert_eq!(best, vec![(1, 19), (2, 18)]);
}

#[test]
fn inside_a_burst_the_most_upright_heads_come_first() {
    // One burst of four: frame 3 tastes best, but frame 2's heads sit level; frame 4 has no
    // face found, so it counts as the burst's average lean and taste places it.
    let mut sheet = sheet_of(&[
        (0.1, Some(0)),
        (0.2, Some(0)),
        (0.9, Some(0)),
        (0.5, Some(0)),
    ]);
    for (p, tilt) in sheet
        .photos
        .iter_mut()
        .zip([Some(25.0), Some(2.0), Some(10.0), None])
    {
        p.tilt = tilt;
    }
    let r = argus_engine::taste::rate(&sheet, technical(4, &[]));
    let rank = |n: usize| r.photos.iter().find(|c| c.n == n).unwrap().rank.unwrap();
    assert_eq!((rank(2), rank(3), rank(4), rank(1)), (1, 2, 3, 4));
}

#[test]
fn a_shoot_without_faces_gets_level_stars_and_no_invented_best_photos() {
    let sheet = sheet_of(&[(0.0, None); 12]);
    let r = argus_engine::taste::rate(&sheet, technical(12, &[]));
    assert!(r.photos.iter().all(|c| c.stars == Some(3)));
    assert!(r.photos.iter().all(|c| c.best.is_none()));
}

#[test]
fn the_taste_stage_never_overrides_a_models_own_stars_or_best_photos() {
    let sheet = sheet_of(&[(0.9, None), (0.1, None), (0.5, None)]);
    let mut result = technical(3, &[]);
    result.photos[1].stars = Some(5);
    result.photos[1].best = Some(1);
    let r = argus_engine::taste::rate(&sheet, result);
    assert_eq!(r.photos[1].stars, Some(5));
    assert_eq!(r.photos[1].best, Some(1));
    assert_eq!(r.photos[0].best, None, "the model chose the Best Photos");
}

#[test]
fn by_default_the_model_is_asked_only_for_technical_failures() {
    let (_tmp, lib, shoot) = library_with(2);
    let sheet = ContactSheet::build(&lib, shoot).unwrap();
    let body = Ollama::from_env().request_body(&sheet);
    let item = &body["format"]["properties"]["photos"]["items"];
    assert_eq!(
        item["required"],
        serde_json::json!(["n", "verdict", "reason"])
    );
    assert_eq!(
        item["properties"]["verdict"]["enum"],
        serde_json::json!(["reject", "unflagged"])
    );
    let system = body["messages"][0]["content"].as_str().unwrap();
    assert!(system.contains("technical"), "{system}");
    // A technical reply parses as a cull with no stars, Stacks or Best Photos.
    let r = parse_json_reply(
        r#"{"summary":"One is soft.","photos":[{"n":1,"verdict":"reject","reason":"missed focus"},{"n":2,"verdict":"unflagged","reason":"ok"}]}"#,
    )
    .unwrap();
    assert_eq!(
        (r.photos[0].stars, r.photos[0].stack, r.photos[0].best),
        (None, None, None)
    );
}

#[test]
fn bigger_sheets_get_longer_to_start_answering() {
    use argus_engine::providers::Stall;
    let sheet = |edge: u32| {
        let mut jpeg = vec![];
        image::RgbImage::new(edge, edge * 2 / 3)
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        ContactSheet {
            shoot: 1,
            shoot_name: "s".into(),
            photos: (1..=60)
                .map(|n| SheetPhoto {
                    jpeg: jpeg.clone(),
                    ..sheet_photo(n, "2026-10-10T10:00:00")
                })
                .collect(),
            context: None,
        }
    };
    let small = Stall::for_cull(&sheet(768), Compute::Full).first.as_secs();
    let big = Stall::for_cull(&sheet(1600), Compute::Full).first.as_secs();
    // 2026-10-20: about 60 Photos with face close-ups read for 684 s before the first token.
    assert!(big > 2 * 684 && big > 3 * small, "{small} s vs {big} s");
    let gentle = Stall::for_cull(&sheet(768), Compute::Gentle)
        .first
        .as_secs();
    assert_eq!(gentle, 4 * small);
}

#[test]
fn a_frame_much_darker_than_the_moment_it_shares_ranks_below_it() {
    // The twin check's 2 EV-under twin: same moment, same faces, far darker. The Technical
    // check rightly leaves it (a RAW can be rescued), so Taste must put it below the original.
    let mut sheet = sheet_of(&[(0.5, Some(0)), (0.5, Some(0)), (0.5, None)]);
    for (p, b) in sheet.photos.iter_mut().zip([0.45, 0.12, 0.47]) {
        p.signals.brightness = b;
    }
    let r = argus_engine::taste::rate(&sheet, technical(3, &[]));
    let stars = |n: usize| r.photos[n - 1].stars.unwrap();
    assert!(stars(2) < stars(1), "{:?}", r.photos);
    assert_eq!(
        r.photos[1].rank,
        Some(2),
        "the dark twin comes last in its Stack"
    );
}

#[test]
fn a_models_check_sees_only_the_doubtful_frames_and_rejects_only_among_them() {
    use argus_engine::technical::Sharpness;
    /// Rejects every Photo it is shown, and remembers how many that was.
    struct RejectAll(std::sync::Mutex<usize>);
    impl Provider for RejectAll {
        fn cull(&self, sheet: &ContactSheet) -> argus_engine::Result<CullResult> {
            *self.0.lock().unwrap() += sheet.photos.len();
            Ok(CullResult {
                summary: "all bad".into(),
                photos: sheet
                    .photos
                    .iter()
                    .map(|p| photo(p.n, Verdict::Reject, "soft", None, None))
                    .collect(),
            })
        }
    }
    let soft = Sharpness {
        in_shoot: 0.25,
        beside: 1.0,
        streak: 0.1,
        portrait: false,
    };
    let sheet = ContactSheet {
        shoot: 1,
        shoot_name: "s".into(),
        photos: (1..=6)
            .map(|n| SheetPhoto {
                sharpness: if n == 2 { soft } else { Default::default() },
                shut_eyes: n == 5,
                ..sheet_photo(n, "2026-10-10T10:00:00")
            })
            .collect(),
        context: None,
    };
    let model = RejectAll(Default::default());
    let r = argus_engine::cull::cull_doubtful(&sheet, &model, 100, &|_| {}).unwrap();
    assert_eq!(
        *model.0.lock().unwrap(),
        2,
        "shown the soft frame and the shut-eyed one"
    );
    let rejected: Vec<usize> = r
        .photos
        .iter()
        .filter(|c| c.verdict == Verdict::Reject)
        .map(|c| c.n)
        .collect();
    assert_eq!(
        rejected,
        vec![2, 5],
        "its rejects land on the right frames of the whole sheet"
    );
}
