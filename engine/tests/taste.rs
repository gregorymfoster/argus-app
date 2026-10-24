//! The taste head: what the person's own Picks and Rejects teach the cull.

use argus_engine::analyzers;
use argus_engine::cull::ContactSheet;
use argus_engine::library::{Author, ImportOptions, Library, PhotoFilter, Verdict};
use argus_engine::taste::{blend, Flag, Head, MIN_FLAGS};
use std::path::Path;

fn axis(i: usize, sign: f32) -> Vec<f32> {
    let mut v = vec![0.0; analyzers::embedding::DIMS];
    v[i] = sign;
    v
}

fn flags(shoot: i64, pick: usize, reject: usize, n: usize) -> Vec<Flag> {
    (0..n)
        .flat_map(|_| {
            [
                Flag {
                    shoot,
                    vector: axis(pick, 1.0),
                    pick: true,
                    edit: false,
                },
                Flag {
                    shoot,
                    vector: axis(reject, 1.0),
                    pick: false,
                    edit: false,
                },
            ]
        })
        .collect()
}

#[test]
fn the_head_points_from_what_the_person_rejects_to_what_they_pick() {
    let head = Head::learn(&flags(1, 0, 1, MIN_FLAGS), 2).unwrap();
    assert!(head.score(&axis(0, 1.0)) > head.score(&axis(1, 1.0)));
    assert!(
        Head::learn(&flags(1, 0, 1, MIN_FLAGS - 1), 2).is_none(),
        "too few flags to read anything into"
    );
}

#[test]
fn flags_on_the_card_being_culled_count_as_much_as_all_the_history() {
    // Years of history say axis 0; the first frames flagged on this card say axis 2.
    let mut all = flags(1, 0, 1, 50);
    all.extend(flags(7, 2, 3, MIN_FLAGS));
    let head = Head::learn(&all, 7).unwrap();
    let (history, card) = (head.score(&axis(0, 1.0)), head.score(&axis(2, 1.0)));
    assert!((history - card).abs() < 1e-5, "{history} vs {card}");
    assert!(head.score(&axis(3, 1.0)) < 0.0);
}

#[test]
fn edits_on_other_shoots_teach_a_direction_of_their_own() {
    // Flags on other Shoots say axis 0; which frames the person edited there says axis 2.
    let mut all = flags(1, 0, 1, 50);
    let mut edits = flags(1, 2, 3, MIN_FLAGS);
    edits.iter_mut().for_each(|f| f.edit = true);
    all.extend(edits.clone());
    let head = Head::learn(&all, 7).unwrap();
    let (flagged, edited) = (head.score(&axis(0, 1.0)), head.score(&axis(2, 1.0)));
    assert!((flagged - edited).abs() < 1e-5, "{flagged} vs {edited}");
    // Edits on the card being culled are left out: the card's own flags speak for it.
    assert!(Head::learn(&edits, 1).is_none());
}

#[test]
fn blending_gives_each_taste_equal_weight_and_ignores_a_level_one() {
    // No faces anywhere: the head alone orders the frames.
    let t = blend(&[0.0; 3], &[0.2, -0.1, 0.5]);
    assert!(t[2] > t[0] && t[0] > t[1]);
    // A frame the head can't score sits at its middle.
    let t = blend(&[1.0, 0.0, 0.0], &[f32::NAN, 1.0, -1.0]);
    assert!(t[0] > t[1] && t[1] > t[2]);
}

/// A 160x120 JPEG: a gradient in one tint, a little different per `seed`.
fn write_jpeg(path: &Path, tint: [u8; 3], seed: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut data = Vec::with_capacity(160 * 120 * 3);
    for y in 0..120u32 {
        for x in 0..160u32 {
            for c in tint {
                data.push(((x * 200 / 160 + y / 4 + seed * 7) % 256) as u8 / 2 + c / 2);
            }
        }
    }
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, 160, 120, jpeg_encoder::ColorType::Rgb)
        .unwrap();
}

const WARM: [u8; 3] = [230, 120, 40];
const COOL: [u8; 3] = [40, 110, 230];

/// A Shoot of `warm` warm frames and `cool` cool ones; `seed` keeps its frames unlike any other
/// Shoot's (Import skips a file it already has).
fn shoot(lib: &mut Library, dir: &Path, name: &str, warm: u32, cool: u32, seed: u32) -> i64 {
    for i in 0..warm {
        write_jpeg(&dir.join(format!("{name}/w{i:02}.jpg")), WARM, seed + i);
    }
    for i in 0..cool {
        write_jpeg(&dir.join(format!("{name}/c{i:02}.jpg")), COOL, seed + i);
    }
    lib.import(
        &dir.join(name),
        &ImportOptions {
            shoot_name: Some(name.into()),
            ..Default::default()
        },
    )
    .unwrap()
    .shoot_id
    .unwrap()
}

fn flag_shoot(lib: &mut Library, shoot: i64, author: Author) {
    let photos = lib
        .list_photos(&PhotoFilter {
            shoot: Some(shoot),
            verdict: None,
        })
        .unwrap();
    analyzers::facts_for(lib, &photos, &["embedding"]).unwrap();
    for p in &photos {
        let v = if p.file_name.starts_with('w') {
            Verdict::Pick
        } else {
            Verdict::Reject
        };
        lib.set_verdict(&[p.id], v, author, None, None).unwrap();
    }
}

fn tastes(lib: &Library, shoot: i64) -> (Vec<f32>, Vec<f32>) {
    let sheet = ContactSheet::build(lib, shoot).unwrap();
    let (warm, cool): (Vec<_>, Vec<_>) = sheet
        .photos
        .iter()
        .partition(|p| p.file_name.starts_with('w'));
    (
        warm.iter().map(|p| p.taste).collect(),
        cool.iter().map(|p| p.taste).collect(),
    )
}

#[test]
fn a_cull_ranks_frames_like_the_ones_the_person_picked_on_other_shoots_higher() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(&tmp.path().join("lib")).unwrap();
    let before = shoot(&mut lib, tmp.path(), "2026-10-01_Beach", 6, 6, 0);
    flag_shoot(&mut lib, before, Author::Person);
    let card = shoot(&mut lib, tmp.path(), "2026-10-08_Park", 3, 3, 10);
    let (warm, cool) = tastes(&lib, card);
    let worst_warm = warm.iter().copied().fold(f32::INFINITY, f32::min);
    let best_cool = cool.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(worst_warm > best_cool, "warm {warm:?} vs cool {cool:?}");
}

fn edit_shoot(lib: &mut Library, shoot: i64, author: Author) {
    let photos = lib
        .list_photos(&PhotoFilter {
            shoot: Some(shoot),
            verdict: None,
        })
        .unwrap();
    analyzers::facts_for(lib, &photos, &["embedding"]).unwrap();
    for p in photos.iter().filter(|p| p.file_name.starts_with('w')) {
        lib.write_recipe(p.id, &serde_json::json!({ "exposure": 0.3 }), author, None)
            .unwrap();
    }
}

#[test]
fn a_cull_ranks_frames_like_the_ones_the_person_edited_on_other_shoots_higher() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(&tmp.path().join("lib")).unwrap();
    let before = shoot(&mut lib, tmp.path(), "2026-10-01_Beach", 6, 6, 0);
    edit_shoot(&mut lib, before, Author::Person);
    let card = shoot(&mut lib, tmp.path(), "2026-10-08_Park", 3, 3, 10);
    let (warm, cool) = tastes(&lib, card);
    let worst_warm = warm.iter().copied().fold(f32::INFINITY, f32::min);
    let best_cool = cool.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!(worst_warm > best_cool, "warm {warm:?} vs cool {cool:?}");
}

#[test]
fn the_ais_own_edits_teach_the_head_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(&tmp.path().join("lib")).unwrap();
    let before = shoot(&mut lib, tmp.path(), "2026-10-01_Beach", 6, 6, 0);
    edit_shoot(&mut lib, before, Author::Ai);
    let card = shoot(&mut lib, tmp.path(), "2026-10-08_Park", 3, 3, 10);
    let (warm, cool) = tastes(&lib, card);
    assert!(warm.iter().chain(&cool).all(|&t| t == 0.0));
}

#[test]
fn the_ais_own_flags_teach_the_head_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(&tmp.path().join("lib")).unwrap();
    let before = shoot(&mut lib, tmp.path(), "2026-10-01_Beach", 6, 6, 0);
    flag_shoot(&mut lib, before, Author::Ai);
    let card = shoot(&mut lib, tmp.path(), "2026-10-08_Park", 3, 3, 10);
    let (warm, cool) = tastes(&lib, card);
    assert!(
        warm.iter().chain(&cool).all(|&t| t == 0.0),
        "no faces and no flags of the person's: level taste"
    );
}

#[test]
fn rerating_as_the_person_flags_lifts_the_frames_like_their_picks() {
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = Library::open(&tmp.path().join("lib")).unwrap();
    let card = shoot(&mut lib, tmp.path(), "2026-10-08_Park", 7, 7, 10);
    argus_engine::cull::cull_shoot(&mut lib, card, &argus_engine::technical::Rules).unwrap();
    let photos = lib
        .list_photos(&PhotoFilter {
            shoot: Some(card),
            verdict: None,
        })
        .unwrap();
    assert!(
        photos.iter().all(|p| p.stars_by == Some(Author::Ai)),
        "the cull rated them all"
    );
    // The person flags five of each and stars one warm frame themselves.
    let (warm, cool): (Vec<_>, Vec<_>) = photos.iter().partition(|p| p.file_name.starts_with('w'));
    for p in warm.iter().take(5) {
        lib.set_verdict(&[p.id], Verdict::Pick, Author::Person, None, None)
            .unwrap();
    }
    for p in cool.iter().take(5) {
        lib.set_verdict(&[p.id], Verdict::Reject, Author::Person, None, None)
            .unwrap();
    }
    lib.set_rating(&[warm[6].id], 2, Author::Person, None)
        .unwrap();
    let r = argus_engine::cull::rerate(&mut lib, card).unwrap();
    assert!(
        r.learning && r.picks == 5 && r.rejects == 5 && r.changed > 0,
        "{r:?}"
    );
    let now = |id| {
        lib.list_photos(&PhotoFilter::default())
            .unwrap()
            .into_iter()
            .find(|p| p.id == id)
            .unwrap()
    };
    let (w, c) = (now(warm[5].id), now(cool[5].id));
    assert!(
        w.stars > c.stars,
        "unflagged warm {} vs cool {}",
        w.stars,
        c.stars
    );
    assert_eq!(w.verdict, Verdict::Unflagged, "flags untouched");
    let mine = now(warm[6].id);
    assert_eq!(
        (mine.stars, mine.stars_by),
        (2, Some(Author::Person)),
        "the person's stars stay"
    );
    // Keepers first: the order puts the unflagged frames like their Picks ahead of the rest,
    // and leaves out what they rejected.
    let at = |id| r.order.iter().position(|&o| o == id);
    for p in cool.iter().take(5) {
        assert_eq!(at(p.id), None, "a Reject has no place in the order");
    }
    let (w, c) = (at(warm[5].id).unwrap(), at(cool[5].id).unwrap());
    assert!(w < c, "warm at {w}, cool at {c}: {:?}", r.order);
    assert_eq!(r.order.len(), 14 - 5, "every frame but the Rejects");
}

mod technical {
    use argus_engine::technical::{broken, doubtful, Sharpness};

    #[test]
    fn a_frame_far_softer_than_its_shoot_or_its_sharp_twin_is_broken() {
        let s = |in_shoot, beside| Sharpness {
            in_shoot,
            beside,
            streak: 0.1,
            portrait: false,
        };
        assert!(
            broken(s(0.05, 1.0)).is_some(),
            "blurred against the whole Shoot"
        );
        assert!(
            broken(s(0.3, 0.1)).is_some(),
            "shaken beside a sharp frame of the moment"
        );
        assert_eq!(
            broken(s(0.3, 0.5)),
            None,
            "a little softer than its sibling"
        );
        assert_eq!(
            broken(s(0.8, 0.1)),
            None,
            "sharp for its Shoot, whatever its sibling"
        );
        assert_eq!(broken(Sharpness::default()), None);
        let portrait = Sharpness {
            portrait: true,
            ..s(0.08, 1.0)
        };
        assert_eq!(
            broken(portrait),
            None,
            "a shallow-focus portrait reads soft beside scenes"
        );
        assert_eq!(
            argus_engine::technical::doubtful(portrait, false),
            Some("soft"),
            "but a model may look"
        );
        assert!(
            broken(Sharpness {
                portrait: true,
                ..s(0.08, 0.1)
            })
            .is_some(),
            "a sharp sibling still gives a missed portrait away"
        );
    }

    #[test]
    fn a_soft_frame_whose_edges_all_run_one_way_is_shaken() {
        let s = |in_shoot, streak| Sharpness {
            in_shoot,
            beside: 1.0,
            streak,
            portrait: false,
        };
        assert_eq!(broken(s(0.4, 0.85)), Some("camera shake"));
        assert_eq!(
            broken(s(1.2, 0.85)),
            None,
            "streaky but sharp: stripes, a fence"
        );
        assert_eq!(
            broken(s(0.4, 0.3)),
            None,
            "soft every which way is not shake"
        );
    }

    #[test]
    fn soft_streaky_or_shut_eyed_frames_are_doubtful_not_broken() {
        let s = |in_shoot, beside, streak| Sharpness {
            in_shoot,
            beside,
            streak,
            portrait: false,
        };
        assert_eq!(doubtful(s(1.0, 1.0, 0.1), false), None);
        assert_eq!(doubtful(s(0.25, 1.0, 0.1), false), Some("soft"));
        assert_eq!(doubtful(s(0.6, 0.4, 0.1), false), Some("soft"));
        assert_eq!(
            doubtful(s(0.7, 1.0, 0.5), false),
            Some("streaky, maybe shake")
        );
        assert_eq!(doubtful(s(1.0, 1.0, 0.1), true), Some("eyes shut"));
        assert_eq!(
            doubtful(s(0.05, 1.0, 0.1), true),
            None,
            "broken already: a reject"
        );
    }
}
