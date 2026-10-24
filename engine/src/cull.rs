//! AI cull with Shoot context: the Provider sees every Photo of the Shoot at once (the
//! Contact sheet), so it can group bursts into Stacks, pick the best moment across
//! similar shots, and flag the rest. The same pass gives every Photo a star Rating and
//! chooses the Shoot's Best Photos. Person Verdicts and Ratings are never overwritten.
//!
//! A Shoot larger than one request ([`max_per_request`]) is culled in chunks split at
//! the longest gaps in capture time, then one more request picks the Best Photos from
//! the chunks' candidates, so they are chosen across the whole Shoot.
//!
//! A cull reports [`CullProgress`] as the model's reply streams in (Photos answered so
//! far, tokens) and as a heartbeat at least every [`HEARTBEAT`], so a long request never
//! looks stalled.
//!
//! A cull runs in two stages (Greg 2026-10-20). The model checks every Photo for technical
//! failures only (blur, shake, closed eyes, ruined exposure), which local models call well;
//! then [`crate::taste`] gives the stars, burst order and Best Photos from the Facts, because
//! no model ranked Greg's keepers much better than chance. [`Taste::Model`] (a harness
//! variant) asks the model for the whole cull instead.
//!
//! The seam is [`Provider`]: tests use a fake; the app picks a model Provider with
//! [`crate::providers::provider`] (Ollama, Claude Code or the Claude API), whose clients
//! live in [`crate::providers`].

use crate::analyzers;
use crate::library::{time, Author, Library, PhotoFilter, PhotoId, ShootId, Verdict};
use crate::signals::{self, Signals};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The Facts (ADR 0013) each Photo's caption carries.
pub const CULL_FACTS: &[&str] = &["signals", "faces", "eyes", "embedding"];

/// Image-embedding similarity at which two frames show the same scene (a burst), and at
/// which they are near-identical. Measured on the bench: burst frames 0.85-0.95, exact
/// duplicates 0.99+, different scenes 0.3-0.6.
pub(crate) const SAME_SCENE: f32 = 0.85;
pub(crate) const NEAR_IDENTICAL: f32 = 0.97;

/// For each Photo in capture order, how it compares with the one before ("same scene as
/// Photo 11"), or "" when it's a new scene.
fn alike_previous(facts: &[analyzers::Facts]) -> Vec<String> {
    use analyzers::embedding::{similarity, vector};
    let vecs: Vec<Option<Vec<f32>>> = facts
        .iter()
        .map(|f| f.get("embedding").and_then(vector))
        .collect();
    (0..vecs.len())
        .map(|i| {
            let (Some(Some(a)), Some(b)) = (i.checked_sub(1).map(|j| &vecs[j]), &vecs[i]) else {
                return String::new();
            };
            match similarity(a, b) {
                s if s >= NEAR_IDENTICAL => format!("near-identical to Photo {i}"),
                s if s >= SAME_SCENE => format!("same scene as Photo {i}"),
                _ => String::new(),
            }
        })
        .collect()
}

/// Largest main face (share of the long edge) that still gets a close-up: smaller faces are
/// too few pixels on the Contact sheet for the model to judge an expression.
const CLOSE_UP_UNDER: f32 = 0.15;
/// Most close-ups under one photo, and their size on the sheet.
const CLOSE_UPS: usize = 6;
const CLOSE_UP_PX: u32 = 160;

/// Close-ups of a frame's main faces, left to right, cut from the full preview, when they
/// are small in the frame (group shots): the strip goes under the photo on the Contact sheet
/// so the model can see each person's expression (Greg 2026-10-20: faces are one of the
/// biggest factors in a cull). None for frames without small main faces.
fn face_strip(full: &image::RgbImage, facts: &analyzers::Facts) -> Option<image::RgbImage> {
    use analyzers::eyes;
    let faces = analyzers::faces::read(facts.get("faces")?);
    let reads = eyes::read(facts.get("eyes")?);
    let main = eyes::main_faces(&reads);
    let biggest = main.iter().map(|r| r.size).fold(0.0, f32::max);
    if main.is_empty() || biggest >= CLOSE_UP_UNDER {
        return None;
    }
    let main = &main[..main.len().min(CLOSE_UPS)];
    let tile = CLOSE_UP_PX.min(SHEET_LONG_EDGE / main.len() as u32);
    let (w, h) = (full.width() as f32, full.height() as f32);
    let mut strip =
        image::RgbImage::from_pixel(tile * main.len() as u32, tile, image::Rgb([16, 16, 16]));
    for (k, r) in main.iter().enumerate() {
        let f = faces.get(r.face)?;
        let side = (f.w * w).max(f.h * h) * 1.6;
        let (cx, cy) = ((f.x + f.w / 2.0) * w, (f.y + f.h / 2.0) * h);
        let x0 = (cx - side / 2.0).clamp(0.0, (w - side).max(0.0)) as u32;
        let y0 = (cy - side / 2.0).clamp(0.0, (h - side).max(0.0)) as u32;
        let side = (side as u32)
            .min(full.width() - x0)
            .min(full.height() - y0)
            .max(1);
        let crop = image::imageops::crop_imm(full, x0, y0, side, side).to_image();
        let crop =
            image::imageops::resize(&crop, tile, tile, image::imageops::FilterType::Triangle);
        image::imageops::replace(&mut strip, &crop, (k as u32 * tile) as i64, 0);
    }
    Some(strip)
}

/// `top` with `strip` centred under it, on a dark band.
fn below(top: &image::RgbImage, strip: &image::RgbImage) -> image::RgbImage {
    const GAP: u32 = 4;
    let w = top.width().max(strip.width());
    let mut out = image::RgbImage::from_pixel(
        w,
        top.height() + GAP + strip.height(),
        image::Rgb([16, 16, 16]),
    );
    image::imageops::replace(&mut out, top, ((w - top.width()) / 2) as i64, 0);
    image::imageops::replace(
        &mut out,
        strip,
        ((w - strip.width()) / 2) as i64,
        (top.height() + GAP) as i64,
    );
    out
}

/// Long edge of the previews a Provider sees. Cloud Providers never get more (spec 39).
pub const SHEET_LONG_EDGE: u32 = 768;

/// One Photo on the Contact sheet. `n` is its 1-based position in capture order.
pub struct SheetPhoto {
    pub n: usize,
    pub id: PhotoId,
    pub file_name: String,
    pub captured_at: String,
    pub jpeg: Vec<u8>,
    pub signals: Signals,
    /// Sharpness as a percentile of the whole Shoot (100 = the sharpest frame).
    pub sharpness_pct: u8,
    /// What the other specialist models add ("2 faces, all eyes open, faces sharp").
    pub facts: String,
    /// How likely the person keeps it, from its faces and the person's own flags
    /// ([`crate::taste::scores`]); only the order within the Shoot means anything.
    pub taste: f32,
    /// Which burst it belongs to (a run of look-alike frames shot seconds apart), if any.
    pub burst: Option<usize>,
    /// How far its faces' heads lean, in degrees ([`crate::taste::tilt`]); None without faces.
    /// Orders the frames of a burst.
    pub tilt: Option<f32>,
    /// How sharp it is next to its Shoot and its look-alikes ([`crate::technical`]).
    pub sharpness: crate::technical::Sharpness,
    /// A main face has its eyes shut without laughing ([`crate::technical::shut_eyes`]).
    pub shut_eyes: bool,
}

impl SheetPhoto {
    /// The line a Provider reads before this Photo's image.
    pub fn caption(&self) -> String {
        let s = &self.signals;
        let line = format!(
            "Photo {} ({}, {}) sharpness {}th percentile of the shoot, brightness {:.2}, clipped {:.1}%, crushed {:.1}%",
            self.n,
            self.file_name,
            self.captured_at.replace('T', " "),
            self.sharpness_pct,
            s.brightness,
            s.clipped * 100.0,
            s.crushed * 100.0
        );
        if self.facts.is_empty() {
            line
        } else {
            format!("{line}; {}", self.facts)
        }
    }
}

/// The whole Shoot in capture order, small enough to send in one request.
pub struct ContactSheet {
    pub shoot: ShootId,
    pub shoot_name: String,
    pub photos: Vec<SheetPhoto>,
    /// Set when this sheet is part of a larger Shoot (a chunk, or the Best Photos
    /// candidates), to tell the Provider what it is looking at.
    pub context: Option<String>,
}

/// One Photo's image on the sheet (its preview at [`SHEET_LONG_EDGE`], with close-ups of small
/// faces under it) as JPEG, and the close-up strip when there is one.
fn sheet_image(
    p: &crate::library::Photo,
    f: &analyzers::Facts,
) -> Result<(Vec<u8>, Option<image::RgbImage>)> {
    let img = image::open(&p.preview)
        .map_err(|e| Error(format!("preview {}: {e}", p.preview.display())))?;
    let full = img.to_rgb8();
    let img = img.thumbnail(SHEET_LONG_EDGE, SHEET_LONG_EDGE).to_rgb8();
    let strip = face_strip(&full, f);
    let img = match &strip {
        Some(strip) => below(&img, strip),
        None => img,
    };
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 80)
        .encode(
            img.as_raw(),
            img.width() as u16,
            img.height() as u16,
            jpeg_encoder::ColorType::Rgb,
        )
        .map_err(|e| Error(format!("sheet encode: {e}")))?;
    Ok((jpeg, strip))
}

impl ContactSheet {
    pub fn build(lib: &Library, shoot: ShootId) -> Result<Self> {
        Self::build_with(lib, shoot, true)
    }

    /// The sheet without its images (every `jpeg` empty): what Taste and the rules read, in a
    /// fraction of the time, for [`rerate`].
    pub fn facts_only(lib: &Library, shoot: ShootId) -> Result<Self> {
        Self::build_with(lib, shoot, false)
    }

    fn build_with(lib: &Library, shoot: ShootId, images: bool) -> Result<Self> {
        let shoot_name = lib
            .shoots()?
            .into_iter()
            .find(|s| s.id == shoot)
            .ok_or_else(|| Error(format!("no Shoot with id {shoot}")))?
            .name;
        let photos = lib.list_photos(&PhotoFilter {
            shoot: Some(shoot),
            verdict: None,
        })?;
        let facts = analyzers::facts_for(lib, &photos, CULL_FACTS)?;
        let alike = alike_previous(&facts);
        let sharpness = crate::technical::measure(&photos, &facts);
        let taste = crate::taste::scores(lib, shoot, &facts)?;
        let frames: Vec<crate::bursts::Frame> = photos
            .iter()
            .zip(&facts)
            .zip(&taste)
            .map(|((p, f), &t)| crate::bursts::Frame {
                id: p.id,
                at: time::capture_seconds(&p.captured_at),
                embedding: f.get("embedding").and_then(analyzers::embedding::vector),
                quality: t,
            })
            .collect();
        let burst_of: HashMap<PhotoId, usize> = crate::bursts::group(&frames)
            .into_iter()
            .enumerate()
            .flat_map(|(b, ids)| ids.into_iter().map(move |id| (id, b)))
            .collect();
        use rayon::prelude::*;
        let photos = photos
            .par_iter()
            .zip(&facts)
            .enumerate()
            .map(|(i, (p, f))| {
                let (jpeg, strip) = if images {
                    sheet_image(p, f)?
                } else {
                    (Vec::new(), None)
                };
                let mut rest = f.clone();
                rest.remove("embedding");
                rest.remove("faces"); // the eyes caption counts and describes them
                let signals = rest
                    .remove("signals")
                    .map(|s| analyzers::signals::read(&s))
                    .unwrap_or_default();
                Ok(SheetPhoto {
                    n: i + 1,
                    id: p.id,
                    file_name: p.file_name.clone(),
                    captured_at: p.captured_at.clone(),
                    jpeg,
                    signals,
                    sharpness_pct: 50,
                    taste: taste[i],
                    burst: burst_of.get(&p.id).copied(),
                    tilt: crate::taste::tilt(f),
                    sharpness: sharpness[i],
                    shut_eyes: crate::technical::shut_eyes(f),
                    facts: [
                        analyzers::caption(&rest),
                        alike[i].clone(),
                        strip
                            .as_ref()
                            .map(|_| "close-ups of these faces under the photo".into())
                            .unwrap_or_default(),
                    ]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(", "),
                })
            })
            .collect::<Result<Vec<SheetPhoto>>>()?;
        let mut photos = photos;
        let sharp: Vec<f32> = photos.iter().map(|p| p.signals.sharpness).collect();
        for (p, pct) in photos.iter_mut().zip(signals::percentiles(&sharp)) {
            p.sharpness_pct = pct;
        }
        Ok(Self {
            shoot,
            shoot_name,
            photos,
            context: None,
        })
    }

    /// The opening line of a request: the Shoot, how many Photos, and any context.
    pub fn intro(&self) -> String {
        let mut s = format!(
            "Shoot \"{}\": {} photos in capture order.",
            self.shoot_name,
            self.photos.len()
        );
        if let Some(c) = &self.context {
            s = format!("{s} {c}");
        }
        s
    }

    /// A sheet of some of these Photos, renumbered 1..n in the given order.
    fn subset(&self, picks: &[usize], context: String) -> (ContactSheet, Vec<usize>) {
        let mut orig = Vec::new();
        let photos = picks
            .iter()
            .enumerate()
            .map(|(i, &idx)| {
                let p = &self.photos[idx];
                orig.push(p.n);
                SheetPhoto {
                    n: i + 1,
                    id: p.id,
                    file_name: p.file_name.clone(),
                    captured_at: p.captured_at.clone(),
                    jpeg: p.jpeg.clone(),
                    signals: p.signals,
                    sharpness_pct: p.sharpness_pct,
                    facts: p.facts.clone(),
                    taste: p.taste,
                    burst: p.burst,
                    tilt: p.tilt,
                    sharpness: p.sharpness,
                    shut_eyes: p.shut_eyes,
                }
            })
            .collect();
        (
            ContactSheet {
                shoot: self.shoot,
                shoot_name: self.shoot_name.clone(),
                photos,
                context: Some(context),
            },
            orig,
        )
    }
}

/// What a Provider returns, in Contact sheet positions (`n`), not Library ids.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CullResult {
    /// A sentence or two for the person, in the Agent's voice.
    pub summary: String,
    pub photos: Vec<CullPhoto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CullPhoto {
    pub n: usize,
    pub verdict: Verdict,
    pub reason: String,
    /// Photos sharing a number form one Stack (a burst or repeated shot).
    pub stack: Option<u32>,
    /// Rank inside its Stack, 1 = best.
    pub rank: Option<u32>,
    /// Lightroom-style stars, 1 (reject) to 5 (best of the Shoot).
    #[serde(default)]
    pub stars: Option<u8>,
    /// Rank in the Shoot's Best Photos (1 = best), or None if it isn't one of them.
    #[serde(default)]
    pub best: Option<u32>,
}

pub trait Provider {
    fn cull(&self, sheet: &ContactSheet) -> Result<CullResult>;

    /// [`Provider::cull`], telling `seen` what the model has sent so far as it arrives.
    /// A Provider that can't stream answers in one go (the cull's heartbeat still runs).
    fn cull_streaming(&self, sheet: &ContactSheet, seen: &dyn Fn(Streamed)) -> Result<CullResult> {
        let _ = seen;
        self.cull(sheet)
    }

    /// Whether it runs on this Mac's model, which one Argus process uses at a time
    /// ([`crate::model_lock`]).
    fn local_model(&self) -> bool {
        false
    }

    /// Roughly how many seconds it takes to read one Photo of a sheet before it starts
    /// replying, for a bar that moves while a local model reads (None: no guess).
    fn read_pace(&self) -> Option<f64> {
        None
    }

    /// Which Photos of a Shoot it is asked about in a cull's Technical check: all of them,
    /// unless [`cull_provider`] wrapped it in [`Doubtful`].
    fn ask(&self) -> Ask {
        Ask::All
    }
}

/// A model Provider asked only about the doubtful frames of a cull ([`Ask::Doubtful`]).
pub struct Doubtful(pub Box<dyn Provider>);

impl Provider for Doubtful {
    fn cull(&self, sheet: &ContactSheet) -> Result<CullResult> {
        self.0.cull(sheet)
    }
    fn cull_streaming(&self, sheet: &ContactSheet, seen: &dyn Fn(Streamed)) -> Result<CullResult> {
        self.0.cull_streaming(sheet, seen)
    }
    fn local_model(&self) -> bool {
        self.0.local_model()
    }
    fn read_pace(&self) -> Option<f64> {
        self.0.read_pace()
    }
    fn ask(&self) -> Ask {
        Ask::Doubtful
    }
}

/// How much of its reply the model has sent in the request running now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Streamed {
    /// Photos it has started answering for.
    pub photos: usize,
    /// Tokens so far, thinking included.
    pub tokens: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CullReport {
    pub shoot_id: ShootId,
    pub summary: String,
    pub picks: usize,
    pub rejects: usize,
    pub unflagged: usize,
    pub stacks: usize,
    /// Photos given AI stars, and how many of each (index 1 = one star).
    pub rated: usize,
    pub stars: [usize; 6],
    /// The Shoot's Best Photos, best first.
    pub best: Vec<PhotoId>,
    /// Photos the person had already flagged, left as they were.
    pub kept_person_verdicts: usize,
    /// Photos the person had already rated, left as they were.
    pub kept_person_ratings: usize,
    /// Photos the Provider skipped (left as they were) and numbers it made up (ignored).
    pub missing: usize,
    pub ignored: usize,
}

/// How far a cull has got. A Shoot over [`max_per_request`] Photos takes one Provider
/// request per chunk plus one for its Best Photos.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CullProgress {
    /// Provider requests finished, of `total`.
    pub done: usize,
    pub total: usize,
    /// Photos the model has answered for, of `photos_total`: every request's Photos,
    /// with the Best Photos pass estimated until it starts. This is what a bar shows.
    pub photos: usize,
    pub photos_total: usize,
    /// Tokens the model has sent in the request running now (thinking included).
    pub tokens: usize,
    pub elapsed_s: u64,
    /// What it waits on before the model is free ("`argus cull` is using the model").
    pub waiting: Option<String>,
    /// Photos the model has read, of `photos_total`: finished requests' and, while it
    /// reads the running one before replying, an estimate at its reading pace. Reading a
    /// sheet takes a local model many minutes with no word from it.
    pub read: usize,
    /// Photos in the Shoot, the scale [`CullProgress::bar`] reports on.
    pub shoot: usize,
}

impl CullProgress {
    /// What the model is doing, for a progress bar's label.
    pub fn stage(&self) -> String {
        if let Some(w) = &self.waiting {
            return format!("Waiting: {w}");
        }
        if self.done >= self.total {
            return "Saving the cull".into();
        }
        if self.total > 1 && self.done + 1 == self.total {
            return "Choosing Best Photos".into();
        }
        let doing = if self.tokens == 0 {
            "Reading the photos"
        } else {
            "Culling"
        };
        if self.total > 1 {
            format!("{doing}, part {} of {}", self.done + 1, self.total - 1)
        } else {
            doing.into()
        }
    }

    /// Where a progress bar stands, in the Shoot's Photos: reading a Photo is half its
    /// work and answering for it the other half, across every request (the Best Photos
    /// pass included), so the bar and its ETA move from the first minute.
    pub fn bar(&self) -> (usize, usize) {
        let total = self.photos_total.max(1) * 2;
        let work = (self.read + self.photos).min(total);
        (self.shoot * work / total, self.shoot)
    }

    /// One line for logs, as `scripts/progress` reads it: `::progress 37/300 cull: ...`.
    pub fn line(&self) -> String {
        let t = self.elapsed_s;
        format!(
            "::progress {}/{} cull: {}, request {} of {}, {} tokens, {}m{:02}s",
            self.photos,
            self.photos_total,
            self.stage(),
            (self.done + 1).min(self.total),
            self.total,
            self.tokens,
            t / 60,
            t % 60
        )
    }
}

/// Longest a cull goes without reporting progress, even while the model is silent
/// (reading the Contact sheet, or paused by Gentle). ARGUS_CULL_HEARTBEAT_MS overrides.
pub const HEARTBEAT: Duration = Duration::from_secs(30);

/// The reading pace (seconds per Photo) the last cull in this process measured, so the
/// next one's first request starts from it rather than a guess. Stored as f64 bits.
static MEASURED_PACE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A cull's progress so far, shared by the request and the heartbeat.
struct Live {
    started: Instant,
    p: CullProgress,
    /// Photos answered in finished requests, and the size of the one running.
    base: usize,
    asking: usize,
    /// When the running request was sent, and seconds per Photo it is expected to read.
    asked_at: Option<Instant>,
    pace: Option<f64>,
}

impl Live {
    fn snapshot(&self) -> CullProgress {
        let reading = match (self.asked_at, self.pace) {
            _ if self.asking == 0 => 0,
            _ if self.p.tokens > 0 => self.asking,
            // Never claims a sheet read before the model says so.
            (Some(at), Some(pace)) if self.p.waiting.is_none() => {
                ((at.elapsed().as_secs_f64() / pace) as usize).min(self.asking * 9 / 10)
            }
            _ => 0,
        };
        CullProgress {
            elapsed_s: self.started.elapsed().as_secs(),
            read: (self.base + reading).min(self.p.photos_total),
            ..self.p.clone()
        }
    }
}

struct Reporter<'a> {
    live: Mutex<Live>,
    progress: &'a (dyn Fn(CullProgress) + Sync),
}

impl Reporter<'_> {
    fn report(&self) {
        let p = self.live.lock().unwrap().snapshot();
        (self.progress)(p);
    }

    fn update(&self, f: impl FnOnce(&mut Live)) {
        f(&mut self.live.lock().unwrap());
        self.report();
    }

    /// Ask the Provider about `sheet`, counting its streamed Photos into the progress.
    fn ask(&self, provider: &dyn Provider, sheet: &ContactSheet) -> Result<CullResult> {
        let n = sheet.photos.len();
        self.update(|l| {
            l.asking = n;
            l.p.tokens = 0;
            l.asked_at = Some(Instant::now());
        });
        let r = provider.cull_streaming(sheet, &|s: Streamed| {
            let mut l = self.live.lock().unwrap();
            if l.p.tokens == 0 && s.tokens > 0 && l.asking >= 5 {
                // The model has read the sheet: its real pace sets the next request's.
                if let Some(at) = l.asked_at {
                    let pace = at.elapsed().as_secs_f64() / l.asking as f64;
                    l.pace = Some(pace);
                    MEASURED_PACE.store(pace.to_bits(), std::sync::atomic::Ordering::Relaxed);
                }
            }
            l.p.tokens = s.tokens;
            let photos = (l.base + s.photos.min(l.asking)).min(l.p.photos_total);
            if photos != l.p.photos {
                l.p.photos = photos;
                drop(l);
                self.report();
            }
        });
        let mut l = self.live.lock().unwrap();
        l.base += l.asking;
        l.asking = 0;
        l.asked_at = None;
        l.p.photos = l.base.min(l.p.photos_total);
        r
    }
}

/// Most Photos sent in one request: 100, or ARGUS_CULL_CHUNK (for tests and small models).
pub fn max_per_request() -> usize {
    std::env::var("ARGUS_CULL_CHUNK")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n: &usize| n >= 2)
        .unwrap_or(MAX_IMAGES_PER_REQUEST)
}

/// Cull a Shoot: build its Contact sheet, ask the Provider, write AI Verdicts and Ratings
/// (as one batch, so they undo together) and replace the Shoot's Stacks and Best Photos.
pub fn cull_shoot(
    lib: &mut Library,
    shoot: ShootId,
    provider: &dyn Provider,
) -> Result<CullReport> {
    cull_shoot_with(lib, shoot, provider, max_per_request(), &|_| {})
}

/// [`cull_shoot`] with a chunk size and a progress callback (called before and after each
/// request, as Photos stream in, and at least every [`HEARTBEAT`], from any thread).
pub fn cull_shoot_with(
    lib: &mut Library,
    shoot: ShootId,
    provider: &dyn Provider,
    max: usize,
    progress: &(dyn Fn(CullProgress) + Sync),
) -> Result<CullReport> {
    let sheet = ContactSheet::build(lib, shoot)?;
    let result = match (taste(), provider.ask()) {
        (Taste::Signals, Ask::Doubtful) => cull_doubtful(&sheet, provider, max, progress)?,
        _ => cull_sheet(&sheet, provider, max, progress)?,
    };
    // A model's check adds to the rules', never drops one of theirs.
    let result = match taste() {
        Taste::Signals => crate::technical::add_rejects(&sheet, result),
        Taste::Model => result,
    };
    let result = crate::taste::rate(&sheet, result);
    apply(lib, &sheet, &result)
}

/// Which Photos a model's Technical check sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// Only the frames the rules find doubtful ([`crate::technical::doubtful`]): about one in
    /// fifteen, so a local model takes minutes, not most of an hour, and has fewer clear
    /// keepers to throw away (on the bench Shoots it rejected 16 of Greg's picks looking at
    /// everything, 8 looking at the doubtful frames alone).
    Doubtful,
    /// Every Photo, as before 2026-10-22 (ARGUS_CULL_ASK=all).
    All,
}

/// [`Ask`] for a model's check from ARGUS_CULL_ASK (`all` or `doubtful`, the default).
pub fn ask_about() -> Ask {
    match std::env::var("ARGUS_CULL_ASK").as_deref() {
        Ok("all") => Ask::All,
        _ => Ask::Doubtful,
    }
}

/// The rules' answer for every Photo, plus the model's rejects among the doubtful ones, which
/// it sees as a smaller sheet of their own.
pub fn cull_doubtful(
    sheet: &ContactSheet,
    provider: &dyn Provider,
    max: usize,
    progress: &(dyn Fn(CullProgress) + Sync),
) -> Result<CullResult> {
    let mut out = crate::technical::Rules.cull(sheet)?;
    let doubt: Vec<usize> = sheet
        .photos
        .iter()
        .enumerate()
        .filter(|(_, p)| crate::technical::doubtful(p.sharpness, p.shut_eyes).is_some())
        .map(|(i, _)| i)
        .collect();
    crate::logs::note(&format!(
        "cull: asking the model about {} doubtful of {} Photos",
        doubt.len(),
        sheet.photos.len()
    ));
    if doubt.is_empty() {
        return Ok(out);
    }
    let (sub, orig) = sheet.subset(
        &doubt,
        format!(
            "These are the {} of the shoot's {} photos that may be soft, shaken or caught with eyes shut; the rest are sound.",
            doubt.len(),
            sheet.photos.len()
        ),
    );
    let asked = cull_sheet(&sub, provider, max, progress)?;
    for c in asked.photos.iter().filter(|c| c.verdict == Verdict::Reject) {
        let Some(&n) = c.n.checked_sub(1).and_then(|i| orig.get(i)) else {
            continue; // local models sometimes invent numbers
        };
        if let Some(o) = out.photos.iter_mut().find(|o| o.n == n) {
            o.verdict = Verdict::Reject;
            o.reason = c.reason.clone();
        }
    }
    out.summary = asked.summary;
    Ok(out)
}

/// Ask the Provider about a whole sheet, in chunks when it is larger than `max`.
pub fn cull_sheet(
    sheet: &ContactSheet,
    provider: &dyn Provider,
    max: usize,
    progress: &(dyn Fn(CullProgress) + Sync),
) -> Result<CullResult> {
    let max = max.max(2);
    let chunks = if sheet.photos.len() <= max {
        vec![]
    } else {
        split_chunks(&sheet.photos, max)
    };
    let n = sheet.photos.len();
    let rep = Reporter {
        live: Mutex::new(Live {
            started: Instant::now(),
            p: CullProgress {
                done: 0,
                total: if chunks.is_empty() {
                    1
                } else {
                    chunks.len() + 1
                },
                photos: 0,
                // The Best Photos pass sees about one in ten, plus each chunk's best.
                photos_total: if chunks.is_empty() {
                    n
                } else {
                    n + (n / 10 + chunks.len()).min(max)
                },
                tokens: 0,
                elapsed_s: 0,
                waiting: None,
                read: 0,
                shoot: n,
            },
            base: 0,
            asking: 0,
            asked_at: None,
            pace: match f64::from_bits(MEASURED_PACE.load(std::sync::atomic::Ordering::Relaxed)) {
                p if p > 0.0 => Some(p),
                _ => provider.read_pace(),
            },
        }),
        progress,
    };
    let every = std::env::var("ARGUS_CULL_HEARTBEAT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(HEARTBEAT, Duration::from_millis);
    std::thread::scope(|s| {
        let (stop, quiet) = std::sync::mpsc::channel::<()>();
        let rep = &rep;
        s.spawn(move || {
            while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = quiet.recv_timeout(every) {
                rep.report();
            }
        });
        // The whole cull, every chunk, holds this Mac's model: another Argus process's
        // request in between would make Ollama swap options and the cull wait unseen.
        crate::logs::note(&format!(
            "cull: {n} Photos in {} request(s)",
            rep.live.lock().unwrap().p.total
        ));
        let _turn = provider.local_model().then(|| {
            crate::model_lock::take(&|w| {
                crate::logs::note(&format!(
                    "cull: {}",
                    w.map_or("model free, starting".into(), |w| format!("waiting, {w}"))
                ));
                rep.update(|l| l.p.waiting = w.map(str::to_owned))
            })
        });
        let r = if chunks.is_empty() {
            cull_every_photo(sheet, provider, rep)
        } else {
            cull_chunks(sheet, provider, max, &chunks, rep)
        };
        drop(stop);
        match &r {
            Ok(_) => crate::logs::note("cull: model answered every request"),
            Err(e) => crate::logs::note(&format!("cull failed: {e}")),
        }
        if r.is_ok() {
            rep.update(|l| {
                l.p.done = l.p.total;
                l.p.photos = l.p.photos_total;
                l.p.read = l.p.photos_total;
            });
        }
        r
    })
}

fn cull_chunks(
    sheet: &ContactSheet,
    provider: &dyn Provider,
    max: usize,
    chunks: &[std::ops::Range<usize>],
    rep: &Reporter,
) -> Result<CullResult> {
    let mut photos = Vec::new();
    let mut summaries = Vec::new();
    // (chunk best rank, stars, global n) for every candidate the chunks put forward.
    let mut candidates: Vec<(u32, u8, usize)> = Vec::new();
    for (ci, range) in chunks.iter().enumerate() {
        crate::progress::check_stopped()?;
        let idx: Vec<usize> = range.clone().collect();
        let (sub, orig) = sheet.subset(
            &idx,
            format!(
                "They are photos {} to {} of a {}-photo shoot, sent in parts; judge them \
                 against each other and choose Best Photos from these alone.",
                range.start + 1,
                range.end,
                sheet.photos.len()
            ),
        );
        let r = cull_every_photo(&sub, provider, rep)?;
        rep.update(|l| l.p.done = ci + 1);
        summaries.push(r.summary);
        for mut c in r.photos {
            let Some(&n) = c.n.checked_sub(1).and_then(|i| orig.get(i)) else {
                continue; // a number the model made up
            };
            c.n = n;
            // Stacks never span chunks; keep each chunk's numbers apart.
            c.stack = c.stack.map(|s| s + 10_000 * ci as u32);
            if let Some(b) = c.best.take() {
                candidates.push((b, c.stars.unwrap_or(0), n));
            } else if c.stars == Some(5) {
                candidates.push((u32::MAX, 5, n));
            }
            photos.push(c);
        }
    }
    // The final pass sees each chunk's best first, then other 5-star frames.
    candidates.sort_by_key(|&(b, stars, n)| (b, std::cmp::Reverse(stars), n));
    candidates.dedup_by_key(|c| c.2);
    candidates.truncate(max);
    // About one Photo in ten of the whole Shoot. The final pass sees only the candidates,
    // and the prompt's "one in ten" of those once left 3 Best Photos for 261 frames.
    let target = (sheet.photos.len() / 10).max(1);
    let mut best: Vec<usize> = Vec::new();
    crate::progress::check_stopped()?;
    if candidates.len() > 1 {
        rep.update(|l| l.p.photos_total = l.base + candidates.len());
        let mut idx: Vec<usize> = candidates.iter().map(|c| c.2 - 1).collect();
        idx.sort(); // capture order
        let (sub, orig) = sheet.subset(
            &idx,
            format!(
                "They are the strongest candidates from a {}-photo shoot, already culled in \
                 parts. Rate them against each other and choose the shoot's Best Photos \
                 from them: about {target} of these {} photos, not one in ten of them.",
                sheet.photos.len(),
                candidates.len()
            ),
        );
        let r = rep.ask(provider, &sub)?;
        let mut ranked: Vec<(u32, usize)> = r
            .photos
            .iter()
            .filter_map(|c| Some((c.best?, *orig.get(c.n.checked_sub(1)?)?)))
            .collect();
        ranked.sort();
        best = ranked.into_iter().map(|(_, n)| n).collect();
    } else {
        rep.update(|l| l.p.photos_total = l.base);
    }
    // A model that names too few is topped up from the chunks' own choices, in order.
    for c in &candidates {
        if best.len() >= target {
            break;
        }
        if !best.contains(&c.2) {
            best.push(c.2);
        }
    }
    for (rank, n) in best.iter().enumerate() {
        if let Some(c) = photos.iter_mut().find(|c| c.n == *n) {
            c.best = Some(rank as u32 + 1);
        }
    }
    Ok(CullResult {
        summary: summaries.join(" "),
        photos,
    })
}

/// Ask the Provider about a sheet, then once more about any Photos its reply left out
/// (local models sometimes skip a run of them), so every Photo gets a Verdict.
fn cull_every_photo(
    sheet: &ContactSheet,
    provider: &dyn Provider,
    rep: &Reporter,
) -> Result<CullResult> {
    let mut r = rep.ask(provider, sheet)?;
    let seen: HashSet<usize> = r.photos.iter().map(|c| c.n).collect();
    let missing: Vec<usize> = (0..sheet.photos.len())
        .filter(|&i| !seen.contains(&sheet.photos[i].n))
        .collect();
    if missing.is_empty() {
        return Ok(r);
    }
    crate::progress::check_stopped()?;
    let context = format!(
        "{}Your earlier reply left these {} photos out; cull and rate them against the \
         same shoot.",
        sheet
            .context
            .as_ref()
            .map(|c| format!("{c} "))
            .unwrap_or_default(),
        missing.len()
    );
    let (sub, orig) = sheet.subset(&missing, context);
    rep.update(|l| l.p.photos_total += missing.len());
    let extra = rep.ask(provider, &sub)?;
    // Stacks from the second reply stay apart from the first's, and Best Photos stay
    // the first reply's.
    let base = r.photos.iter().filter_map(|c| c.stack).max().unwrap_or(0) + 1;
    let mut added = HashSet::new();
    for mut c in extra.photos {
        let Some(&n) = c.n.checked_sub(1).and_then(|i| orig.get(i)) else {
            continue;
        };
        if seen.contains(&n) || !added.insert(n) {
            continue;
        }
        c.n = n;
        c.stack = c.stack.map(|s| s + base);
        c.best = None;
        r.photos.push(c);
    }
    Ok(r)
}

/// Split Photos (in capture order) into ranges of at most `max`, cutting at the longest
/// pause in capture time near each even split point, so bursts stay in one chunk.
pub fn split_chunks(photos: &[SheetPhoto], max: usize) -> Vec<std::ops::Range<usize>> {
    let n = photos.len();
    let k = n.div_ceil(max);
    let secs: Vec<i64> = photos
        .iter()
        .map(|p| time::capture_seconds(&p.captured_at))
        .collect();
    let mut cuts = vec![0];
    for i in 1..k {
        let start = *cuts.last().unwrap();
        let ideal = start + (n - start).div_ceil(k - i + 1);
        let lo = (ideal.saturating_sub(max / 5)).max(start + 1);
        let hi = (ideal + max / 5).min(start + max).min(n - 1);
        let cut = (lo..=hi.max(lo))
            .max_by_key(|&c| (secs[c] - secs[c - 1], std::cmp::Reverse(c.abs_diff(ideal))))
            .unwrap_or(ideal);
        cuts.push(cut);
    }
    cuts.push(n);
    let mut out: Vec<std::ops::Range<usize>> = cuts.windows(2).map(|w| w[0]..w[1]).collect();
    // A cut chosen early can leave the last range too long; split it evenly.
    while let Some(i) = out.iter().position(|r| r.len() > max) {
        let r = out.remove(i);
        let mid = r.start + r.len() / 2;
        out.insert(i, mid..r.end);
        out.insert(i, r.start..mid);
    }
    out
}

/// What [`rerate`] did.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Rerate {
    pub shoot_id: ShootId,
    /// The person's own Picks and Rejects on this Shoot, which the Taste head learns from.
    pub picks: usize,
    pub rejects: usize,
    /// Whether the person's flags (here or on other Shoots) are enough to learn from yet
    /// ([`crate::taste::MIN_FLAGS`] of each).
    pub learning: bool,
    /// Photos whose AI stars changed.
    pub changed: usize,
    /// Keepers first: every Photo of the Shoot but the Rejects, the likeliest keeper first
    /// ([`crate::taste::keepers_first`]). The UI re-orders only the unflagged frames ahead.
    pub order: Vec<PhotoId>,
}

/// Learn as you cull: re-rate a Shoot's AI stars from the person's flags so far. Only stars the
/// AI already gave change, and not on Rejects; flags, Stacks, Best Photos and the person's own stars stay
/// as they are, and no undo step is recorded (it only refreshes the AI's hints). Takes well
/// under a second for a card: it reads the stored Facts, not the images.
pub fn rerate(lib: &mut Library, shoot: ShootId) -> Result<Rerate> {
    let sheet = ContactSheet::facts_only(lib, shoot)?;
    let current: HashMap<PhotoId, crate::library::Photo> = lib
        .list_photos(&PhotoFilter {
            shoot: Some(shoot),
            verdict: None,
        })?
        .into_iter()
        .map(|p| (p.id, p))
        .collect();
    let mine = |v: Verdict| {
        current
            .values()
            .filter(|p| p.verdict_by == Some(Author::Person) && p.verdict == v)
            .count()
    };
    let flags = crate::taste::flags(lib)?;
    let mut out = Rerate {
        shoot_id: shoot,
        picks: mine(Verdict::Pick),
        rejects: mine(Verdict::Reject),
        learning: crate::taste::Head::learn(&flags, shoot).is_some(),
        changed: 0,
        order: Vec::new(),
    };
    let by_n: HashMap<usize, PhotoId> = sheet.photos.iter().map(|p| (p.n, p.id)).collect();
    out.order = crate::taste::keepers_first(&sheet)
        .into_iter()
        .filter_map(|n| by_n.get(&n).copied())
        .filter(|id| {
            current
                .get(id)
                .is_some_and(|p| p.verdict != Verdict::Reject)
        })
        .collect();
    let result = CullResult {
        summary: String::new(),
        photos: sheet
            .photos
            .iter()
            .map(|p| CullPhoto {
                n: p.n,
                verdict: current
                    .get(&p.id)
                    .map_or(Verdict::Unflagged, |c| match c.verdict {
                        Verdict::Reject => Verdict::Reject,
                        _ => Verdict::Unflagged,
                    }),
                reason: String::new(),
                stack: Some(0),
                rank: None,
                stars: None,
                best: Some(0),
            })
            .collect(),
    };
    let rated = crate::taste::rate(&sheet, result);
    for (p, c) in sheet.photos.iter().zip(&rated.photos) {
        let Some(now) = current.get(&p.id) else {
            continue;
        };
        let (Some(stars), false) = (c.stars, c.verdict == Verdict::Reject) else {
            continue;
        };
        // Only the AI's own stars: a Photo the cull hasn't rated yet waits for the cull.
        if now.stars_by != Some(Author::Ai) || now.stars == stars {
            continue;
        }
        lib.set_rating(&[p.id], stars, Author::Ai, None)?;
        out.changed += 1;
    }
    // New likely keepers, and Picks since the last run, get their Shoot match.
    crate::pre_edit::queue(lib, shoot)?;
    Ok(out)
}

/// Write a [`CullResult`] to the Library. Split out so it is testable without a model.
pub fn apply(lib: &mut Library, sheet: &ContactSheet, result: &CullResult) -> Result<CullReport> {
    let by_n: HashMap<usize, PhotoId> = sheet.photos.iter().map(|p| (p.n, p.id)).collect();
    let current = lib.list_photos(&PhotoFilter {
        shoot: Some(sheet.shoot),
        verdict: None,
    })?;
    let person: HashSet<PhotoId> = current
        .iter()
        .filter(|p| p.verdict_by == Some(Author::Person))
        .map(|p| p.id)
        .collect();
    let person_rated: HashSet<PhotoId> = current
        .iter()
        .filter(|p| p.stars_by == Some(Author::Person))
        .map(|p| p.id)
        .collect();

    let batch = lib.begin_batch(Author::Ai, "AI cull")?;
    let mut seen = HashSet::new();
    let mut stacks: BTreeMap<u32, Vec<(u32, PhotoId)>> = BTreeMap::new();
    let mut best: Vec<(u32, Option<u32>, PhotoId)> = Vec::new();
    let mut star_counts = [0usize; 6];
    let mut kept_ratings = 0;
    let (mut picks, mut rejects, mut unflagged, mut kept, mut ignored) = (0, 0, 0, 0, 0);
    for c in &result.photos {
        let Some(&id) = by_n.get(&c.n) else {
            ignored += 1; // local models sometimes invent numbers; skip, don't fail the cull
            continue;
        };
        if !seen.insert(id) {
            continue;
        }
        if let Some(s) = c.stack {
            stacks
                .entry(s)
                .or_default()
                .push((c.rank.unwrap_or(u32::MAX), id));
        }
        if let Some(b) = c.best {
            best.push((b, c.stack, id));
        }
        if let Some(stars) = c.stars.map(|s| s.clamp(1, 5)) {
            if person_rated.contains(&id) {
                kept_ratings += 1;
            } else {
                lib.set_rating(&[id], stars, Author::Ai, Some(batch))?;
                star_counts[stars as usize] += 1;
            }
        }
        if person.contains(&id) {
            kept += 1;
            continue;
        }
        let verdict = cautious(c);
        lib.set_verdict(&[id], verdict, Author::Ai, Some(&c.reason), Some(batch))?;
        match verdict {
            Verdict::Pick => picks += 1,
            Verdict::Reject => rejects += 1,
            Verdict::Unflagged => unflagged += 1,
        }
    }
    let stacks: Vec<Vec<PhotoId>> = stacks
        .into_values()
        .map(|mut m| {
            m.sort();
            m.into_iter().map(|(_, id)| id).collect()
        })
        .filter(|m: &Vec<PhotoId>| m.len() > 1)
        .collect();
    // The model sometimes leaves a burst apart; join the bursts the Facts show.
    let stacks = crate::bursts::merge(&stacks, &crate::bursts::find(lib, &current)?);
    lib.replace_stacks(sheet.shoot, &stacks)?;
    // Best Photos: in the model's order, never two from one Stack.
    let stack_of: HashMap<PhotoId, usize> = stacks
        .iter()
        .enumerate()
        .flat_map(|(i, m)| m.iter().map(move |&id| (id, i)))
        .collect();
    best.sort_by_key(|&(rank, _, id)| (rank, id));
    let mut stacks_used = HashSet::new();
    let best: Vec<PhotoId> = best
        .into_iter()
        .filter(|&(_, _, id)| stack_of.get(&id).is_none_or(|&s| stacks_used.insert(s)))
        .map(|(_, _, id)| id)
        .collect();
    lib.replace_best_photos(sheet.shoot, &best)?;
    Ok(CullReport {
        shoot_id: sheet.shoot,
        summary: result.summary.clone(),
        picks,
        rejects,
        unflagged,
        stacks: stacks.len(),
        rated: star_counts.iter().sum(),
        stars: star_counts,
        best,
        kept_person_verdicts: kept,
        kept_person_ratings: kept_ratings,
        missing: by_n.len() - seen.len(),
        ignored,
    })
}

/// The Verdict the AI may set (Greg, 2026-10-20): it rates every Photo and rejects only a frame
/// it rated 1 star for a named technical flaw; picking is left to the person's own cull pass.
/// On his clean 2026-10-10 card no model picked his keepers much better than chance, and Qwen
/// rejected 14 of them, while it caught every damaged frame (docs/research/cull-scorer.md).
fn cautious(c: &CullPhoto) -> Verdict {
    match c.verdict {
        Verdict::Reject if c.stars.is_none_or(|s| s <= 1) => Verdict::Reject,
        _ => Verdict::Unflagged,
    }
}

pub(crate) const MAX_IMAGES_PER_REQUEST: usize = 100;

/// JSON Schema for [`CullResult`], shared by every Provider.
pub fn result_schema() -> Value {
    let nullable_int = json!({ "anyOf": [{ "type": "integer" }, { "type": "null" }] });
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "photos"],
        "properties": {
            "summary": { "type": "string" },
            "photos": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["n", "verdict", "stars", "reason", "stack", "rank", "best"],
                    "properties": {
                        "n": { "type": "integer" },
                        "verdict": { "type": "string", "enum": ["pick", "reject", "unflagged"] },
                        "stars": { "type": "integer", "enum": [1, 2, 3, 4, 5] },
                        "reason": { "type": "string" },
                        "stack": nullable_int,
                        "rank": nullable_int,
                        "best": nullable_int
                    }
                }
            }
        }
    })
}

/// JSON Schema for a technical-only cull ([`Taste::Signals`]): a Verdict and a reason per
/// Photo, nothing about taste. It parses as a [`CullResult`] with no stars, Stacks or Best Photos.
pub fn technical_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "photos"],
        "properties": {
            "summary": { "type": "string" },
            "photos": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["n", "verdict", "reason"],
                    "properties": {
                        "n": { "type": "integer" },
                        "verdict": { "type": "string", "enum": ["reject", "unflagged"] },
                        "reason": { "type": "string" }
                    }
                }
            }
        }
    })
}

/// Who rates taste in a cull. `Signals` (the default) asks the model only for technical
/// failures and leaves stars, burst order and Best Photos to [`crate::taste`]; `Model` asks the
/// model for all of it, as culls did before 2026-10-20.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Taste {
    Signals,
    Model,
}

/// The cull's split: the harness variant's `cull_taste`, else `Model` when the variant brings
/// its own `cull` prompt (written for the whole job), else `Signals`.
pub fn taste() -> Taste {
    match crate::variant::current() {
        Ok(v) => v.cull_taste.unwrap_or(if v.cull.is_some() {
            Taste::Model
        } else {
            Taste::Signals
        }),
        Err(_) => Taste::Signals,
    }
}

/// Who does a cull's Technical check. `Rules` (the default since 2026-10-21, Greg) answers it
/// from sharpness in seconds ([`crate::technical`]); `Model` asks the cull's Provider, a
/// second opinion that also catches shut eyes and subtle shake, and keeps the rules' rejects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Check {
    Rules,
    Model,
}

impl std::str::FromStr for Check {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "rules" => Ok(Check::Rules),
            "model" => Ok(Check::Model),
            other => Err(Error(format!(
                "unknown check {other:?}; use rules or model"
            ))),
        }
    }
}

/// The Technical check a cull runs: `explicit`, else ARGUS_CULL_CHECK, else the model when the
/// harness variant asks it for the whole cull ([`Taste::Model`]), else the rules.
pub fn check(explicit: Option<Check>) -> Check {
    explicit
        .or_else(|| std::env::var("ARGUS_CULL_CHECK").ok()?.parse().ok())
        .unwrap_or(match taste() {
            Taste::Model => Check::Model,
            Taste::Signals => Check::Rules,
        })
}

/// The Provider a cull asks: the rules for [`Check::Rules`] (or `provider` "rules"), else the
/// named model Provider ([`crate::providers::provider`]).
pub fn cull_provider(
    explicit: Option<Check>,
    name: Option<&str>,
    model: impl FnOnce(Option<&str>) -> Result<Box<dyn Provider>>,
) -> Result<Box<dyn Provider>> {
    if name == Some("rules") || check(explicit) == Check::Rules {
        Ok(Box::new(crate::technical::Rules))
    } else if taste() == Taste::Signals && ask_about() == Ask::Doubtful {
        Ok(Box::new(Doubtful(model(name)?)))
    } else {
        model(name)
    }
}

/// The schema a Provider answers in, for [`taste`].
pub fn schema() -> Value {
    match taste() {
        Taste::Signals => technical_schema(),
        Taste::Model => result_schema(),
    }
}

/// The last line of a request.
pub(crate) fn ask() -> &'static str {
    match taste() {
        Taste::Signals => "Check every photo of this shoot for technical failures.",
        Taste::Model => "Cull and rate this shoot.",
    }
}

/// The cull system prompt: the `cull` of the harness variant ARGUS_VARIANT names, else TECHNICAL
/// or SYSTEM for [`taste`] (an unreadable variant file is reported and the built-in one is used).
pub(crate) fn system() -> String {
    let built_in = || match taste() {
        Taste::Signals => TECHNICAL.to_string(),
        Taste::Model => SYSTEM.to_string(),
    };
    match crate::variant::current() {
        Ok(v) => v.cull.unwrap_or_else(built_in),
        Err(e) => {
            eprintln!("argus cull: {e}; using the built-in prompt");
            built_in()
        }
    }
}

/// The technical stage's prompt: only what is broken, never what is good.
const TECHNICAL: &str = "You check photo shoots for technical failures, for a photographer \
who does the real cull afterwards. You see every photo of one shoot, in capture order. Your \
only job is to find frames that are technically broken. Whether a sound frame is a good photo \
is not your call: other tools rate that, so never judge composition, moment or mood.

For each photo give a Verdict:
- reject: only a frame with a concrete technical flaw you can name: missed focus or motion \
blur on the subject, camera shake, the main subject's eyes fully closed when they aren't \
laughing (a half-blink is not a technical failure: leave it, other tools weigh it), exposure beyond rescue (the subject blown to white or lost in black), or the \
main subject cut off by the frame edge in a way no crop can fix. A wrong reject hides a \
keeper from the photographer, so when in doubt, don't.
- unflagged: every other frame, including ordinary ones, weaker versions of another frame and \
deliberately soft, dark or moody frames.

Each photo comes with measured signals: sharpness as a percentile within this shoot (low means \
softer than most frames here), mean brightness from 0 to 1, and the share of clipped highlights \
and crushed shadows. Treat them as hints beside your own eyes: a low sharpness percentile backs \
up a frame that looks soft, but a shallow-focus portrait can be low and still sharp where it \
matters. A face model reads the main faces left to right (smiling or not, half-blink, eyes \
closed, soft); check it against the image. When the faces are small in the frame, close-ups of \
them, left to right, sit under the photo.

Give every reject a reason of two or three words a photographer can scan in a grid, like \
\"missed focus\", \"eyes closed\", \"motion blur\", \"blown out\"; give every other photo the \
reason \"ok\". The summary is one plain sentence to the photographer about what you found.";

const SYSTEM: &str = "You cull photo shoots for a photographer. You see every photo of one \
shoot, in capture order, before deciding anything, and you judge each photo against the \
rest of the shoot rather than in isolation.

For each photo decide a Verdict. The photographer does the real cull afterwards, so you only \
set aside frames that are plainly broken and leave every judgment of taste to your stars:
- reject: only a frame with a concrete technical flaw you can name: missed focus or motion blur \
on the subject, closed eyes on the main subject, or exposure beyond rescue. Rate it 1 star. Never \
reject a frame because it is ordinary, a weaker version of another frame, or a scene you like \
less: a wrong reject hides a keeper from the photographer.
- unflagged: every other frame, including the ones you would keep. Your stars say which are best.
- pick: never. The photographer picks.

Group near-identical frames (bursts, repeated attempts at the same shot) into Stacks by giving \
them the same stack number, and rank the frames inside each Stack (1 = best). Look across the \
whole shoot for similar scenes and the same people, so you keep the strongest moment rather than \
several versions of it. Single photos get stack and rank null.

Rate every photo with Lightroom stars, judged against the rest of this shoot:
- 5: one of the very best of the shoot, a frame worth printing. Most shoots have only a few.
- 4: excellent, clearly worth editing and sharing.
- 3: a good, solid keeper.
- 2: usable but flawed, a weaker version of a better frame, or someone looks off in it.
- 1: a technical failure (and only then a reject).
Use the whole range: spread the shoot across 2 to 5 so the photographer can sort by stars.

Then choose the shoot's Best Photos: the strongest frames, the ones you would show someone \
first. Take about one photo in ten (at least one), never two from the same Stack or two \
versions of the same moment, and rank them with best 1 = the single best photo. Every other \
photo gets best null.

Each photo comes with measured signals: sharpness as a percentile within this shoot (low means \
softer than most frames here), mean brightness from 0 to 1, and the share of clipped \
highlights and crushed shadows. Treat them as hints beside your own eyes: a low sharpness \
percentile backs up a frame that looks soft, but a deliberate shallow-focus or dark, moody \
frame can still be among the best.

Faces matter most. For every frame with people, ask of each person: would they be happy with \
how they look in this photo, or would they ask you not to share it? A half-blink, eyes shut \
without a laugh, a mouth caught mid-word, an awkward grimace, looking down, a face turned away \
or soft focus on a face makes a frame weaker, even when everything else is good. In a burst or \
group shot, rank highest the frame where everyone looks good, and rate a frame below the \
others when someone in it looks off and a near-identical frame has them looking good. Natural \
expressions are the goal: closed eyes in a real laugh or a kiss can be the moment. A face model \
reads the main faces left to right (smiling or not, half-blink, mouth open mid-word, looking \
down, turned away, soft); treat it as a hint and check it against the image. When the faces \
are small in the frame, close-ups of them, left to right, sit under the photo. An image model also notes \
when a photo shows the same scene as the one before it (likely the same Stack) or is \
near-identical to it.

Give every photo a short reason a photographer can scan in a grid: a few words, like \
\"eyes closed\", \"missed focus\", \"best of 6\", \"great expression, soft light\". \
The summary is one or two plain sentences to the photographer about the shoot and what you did.";
