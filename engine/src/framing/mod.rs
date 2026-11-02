//! Framing: a Photo's straighten and crop together. Crop & straighten proposes it for a
//! Shoot (Greg, 2026-11-01): a straighten where the scene has a horizon or architecture to
//! level by ([`level`]), and, when asked, a punch-in where the people sit loose
//! ([`compose`]). Benches: bench/straighten_bench.py, bench/crop_bench.py;
//! docs/research/framing.md.

pub mod compose;
pub mod level;

use crate::masks::models;
use crate::{Result, RgbImage};

/// What the scene model sees in `img`.
pub fn scene(img: &RgbImage) -> Result<level::Scene> {
    Ok(level::Scene {
        labels: models::scene_labels(img)?,
    })
}

/// The crop `img` calls for, from its faces and people (at the analyzers' small size), as a
/// rectangle on the frame: what [`propose`] reads from Facts, for the bench.
pub fn crop_rect(img: &RgbImage) -> Result<Option<compose::Rect>> {
    let small = crate::masks::shrink(img, crate::analyzers::SMALL_EDGE);
    let faces = models::detect_faces(&small)?;
    let planes = models::segment(&small)?;
    let person: Vec<f32> = planes[0].iter().map(|b| 1.0 - b).collect();
    let subject = compose::Subject {
        faces: compose::Subject::from_faces(&faces),
        people: compose::people_box(&person, models::SEG_SIZE),
    };
    Ok(compose::propose(&subject, &compose::Rules::default()))
}

/// The straighten `img` needs, from the scene model and the lines it trusts.
pub fn straighten(img: &RgbImage) -> Result<level::Level> {
    Ok(level::find(img, Some(&scene(img)?)))
}

use crate::geometry::Crop;
use crate::library::{Author, BatchId, Library, Photo, PhotoId};
use crate::Error;
use rayon::prelude::*;
use serde::Serialize;

/// Turns smaller than this aren't worth a decision; larger than [`MAX_TURN`] are more likely
/// a choice (or a misread) than a slip.
/// On Greg's edits every turn the detector made over 3.5° was a misread, and those under
/// 0.7° barely show (bench/straighten_bench.py, 2026-11-01).
pub const MIN_TURN: f64 = 0.7;
pub const MAX_TURN: f64 = 3.5;

/// What Crop & straighten proposes for one Photo.
#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    pub photo_id: PhotoId,
    /// The crop and straighten to write; None when there's nothing to propose.
    pub crop: Option<Crop>,
    /// Why, in a few words each: "Straighten 1.8°", "Tighter".
    pub reasons: Vec<String>,
    /// Left alone, with why: "already cropped", "nothing to level or tighten".
    pub kept: Option<String>,
    /// The Photo with the proposal, at the asked size, as JPEG (the HTTP command sends it).
    #[serde(skip)]
    pub jpeg: Option<Vec<u8>>,
    pub error: Option<String>,
}

/// Facts the proposal reads.
const FACTS: [&str; 3] = ["faces", "segment", "level"];

/// Proposals for `ids`, rendered at `long_edge` when given. Photos already cropped or
/// straightened are left alone (the person's framing is theirs, and an earlier proposal they
/// accepted is too). Facts are computed once and kept, so a second look is quick.
///
/// `crop` adds the punch-ins. It is off by default in the app: on the edit bench the AI
/// cropped 36 of the 136 frames Greg left whole and 1 of the 3 he cropped, while its
/// straightens matched his eye (docs/research/framing.md).
pub fn propose(
    lib: &Library,
    ids: &[PhotoId],
    crop: bool,
    long_edge: Option<usize>,
) -> Result<Vec<Frame>> {
    propose_with(lib, ids, crop, long_edge, &|_, _| {})
}

/// [`propose`], calling `progress(done, total)` as each Photo's Facts are made.
pub fn propose_with(
    lib: &Library,
    ids: &[PhotoId],
    with_crop: bool,
    long_edge: Option<usize>,
    progress: &dyn Fn(usize, usize),
) -> Result<Vec<Frame>> {
    let photos: Vec<Photo> = ids
        .iter()
        .map(|&id| {
            lib.photo(id)?
                .ok_or_else(|| Error(format!("no Photo with id {id}")))
        })
        .collect::<Result<_>>()?;
    let mut open = Vec::new();
    let mut frames: Vec<Option<Frame>> = Vec::with_capacity(photos.len());
    for p in &photos {
        let r = crate::tools::current_recipe(lib, p.id)?;
        if !r.crop.is_identity() {
            frames.push(Some(kept(p.id, "already cropped")));
        } else {
            frames.push(None);
            open.push(p.clone());
        }
    }
    let facts = crate::analyzers::facts_with(lib, &open, &FACTS, None, progress)?;
    let mut planned: Vec<(Photo, Option<Crop>, Vec<String>)> = Vec::new();
    for (p, f) in open.iter().zip(facts) {
        let (crop, reasons) = plan(p, &f, with_crop);
        planned.push((p.clone(), crop, reasons));
    }
    let root = lib.root().to_path_buf();
    let rendered: Vec<Frame> = planned
        .into_par_iter()
        .map_init(
            || Library::open(&root),
            |lib, (p, crop, reasons)| {
                let Some(crop) = crop else {
                    return kept(
                        p.id,
                        if with_crop {
                            "nothing to level or tighten"
                        } else {
                            "nothing to level"
                        },
                    );
                };
                let jpeg = match (lib, long_edge) {
                    (Ok(lib), Some(e)) => match render(lib, &p, &crop, e) {
                        Ok(j) => Some(j),
                        Err(err) => {
                            return Frame {
                                error: Some(err.to_string()),
                                ..kept(p.id, "couldn't render")
                            }
                        }
                    },
                    _ => None,
                };
                Frame {
                    photo_id: p.id,
                    crop: Some(crop),
                    reasons,
                    kept: None,
                    jpeg,
                    error: None,
                }
            },
        )
        .collect();
    let mut rendered = rendered.into_iter();
    Ok(frames
        .into_iter()
        .map(|f| f.unwrap_or_else(|| rendered.next().expect("one per open Photo")))
        .collect())
}

fn kept(photo_id: PhotoId, why: &str) -> Frame {
    Frame {
        photo_id,
        crop: None,
        reasons: vec![],
        kept: Some(why.into()),
        jpeg: None,
        error: None,
    }
}

/// The straighten and crop a Photo's Facts call for.
fn plan(p: &Photo, f: &crate::analyzers::Facts, with_crop: bool) -> (Option<Crop>, Vec<String>) {
    let mut reasons = Vec::new();
    let angle = f
        .get("level")
        .and_then(crate::analyzers::level::turn)
        .filter(|a| (MIN_TURN..=MAX_TURN).contains(&a.abs()))
        .unwrap_or(0.0);
    if angle != 0.0 {
        reasons.push(format!("Straighten {:.1}°", angle.abs()));
    }
    let faces = f
        .get("faces")
        .map(crate::analyzers::faces::read)
        .unwrap_or_default();
    let people = f
        .get("segment")
        .and_then(|s| crate::analyzers::segment::planes(s).ok())
        .and_then(|planes| {
            // Person = anything but background.
            let person: Vec<f32> = planes[0].iter().map(|b| 1.0 - b).collect();
            compose::people_box(&person, models::SEG_SIZE)
        });
    let subject = compose::Subject {
        faces: compose::Subject::from_faces(&faces),
        people,
    };
    let rect = with_crop
        .then(|| compose::propose(&subject, &compose::Rules::default()))
        .flatten();
    if rect.is_some() {
        reasons.push("Tighter".into());
    }
    if angle == 0.0 && rect.is_none() {
        return (None, reasons);
    }
    let (w, h) = f
        .get("level")
        .and_then(crate::analyzers::level::size)
        .unwrap_or((p.width as f64, p.height as f64));
    (Some(to_crop(angle, rect, w, h)), reasons)
}

/// A rectangle on the unturned frame as the Recipe's crop, which is measured on the largest
/// upright rectangle of the turned frame ([`crate::geometry`]): close enough for the small
/// turns proposed, and clamped so it's always valid.
pub fn to_crop(angle: f64, rect: Option<compose::Rect>, w: f64, h: f64) -> Crop {
    let Some(r) = rect else {
        return Crop {
            angle,
            ..Crop::default()
        };
    };
    let k = crate::geometry::inscribed_scale(w.max(1.0), h.max(1.0), angle);
    let side = (r.w / k).min(1.0);
    let at = |v: f64| ((v - (1.0 - k) / 2.0) / k).clamp(0.0, 1.0 - side);
    Crop {
        angle,
        x: at(r.x),
        y: at(r.y),
        w: side,
        h: side,
    }
    .clamped()
}

fn render(lib: &Library, p: &Photo, crop: &Crop, edge: usize) -> Result<Vec<u8>> {
    let mut r = crate::tools::current_recipe(lib, p.id)?;
    r.crop = *crop;
    let shown = crate::tools::with_look(lib, p, &r)?;
    crate::render::encode_jpeg(&crate::render::render_in(lib, p, &shown, Some(edge))?, 82)
}

/// What [`apply`] did.
#[derive(Debug, Clone, Serialize)]
pub struct Applied {
    pub batch_id: BatchId,
    pub applied: Vec<PhotoId>,
    pub skipped: Vec<crate::shoot_look::Skipped>,
}

/// Write each `(photo, crop)` as the Photo's crop and straighten, changing nothing else, all
/// in `batch` (one Undo). A Photo cropped since the proposal is skipped: never over a crop.
pub fn apply(lib: &mut Library, items: &[(PhotoId, Crop)], batch: BatchId) -> Result<Applied> {
    let mut applied = Vec::new();
    let mut skipped = Vec::new();
    for &(id, crop) in items {
        let skip = |why: &str| crate::shoot_look::Skipped {
            photo_id: id,
            why: why.into(),
        };
        if lib.photo(id)?.is_none() {
            skipped.push(skip("no such Photo"));
            continue;
        }
        let mut r = crate::tools::current_recipe(lib, id)?;
        if !r.crop.is_identity() {
            skipped.push(skip("already cropped"));
            continue;
        }
        r.crop = crop.clamped();
        if r.crop.is_identity() {
            skipped.push(skip("nothing to change"));
            continue;
        }
        lib.write_recipe(id, &r.to_value(), Author::Ai, Some(batch))?;
        lib.log_usage(
            id,
            "framing_applied",
            serde_json::json!({ "batch_id": batch, "crop": r.crop }),
        );
        applied.push(id);
    }
    Ok(Applied {
        batch_id: batch,
        applied,
        skipped,
    })
}
