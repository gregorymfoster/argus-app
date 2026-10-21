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
//! The seam is [`Provider`]: tests use a fake; the app picks one with [`provider`]:
//! [`Ollama`] (local, default), [`ClaudeCode`] (the person's Claude plan) or [`Claude`] (API key).

use crate::analyzers;
use crate::compute::{Compute, OllamaProcesses, Throttle};
use crate::library::{Author, Library, PhotoFilter, PhotoId, ShootId, Verdict};
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
    /// How likely the person keeps it, from its faces ([`crate::taste::score`]).
    pub taste: f32,
    /// Which burst it belongs to (a run of look-alike frames shot seconds apart), if any.
    pub burst: Option<usize>,
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

impl ContactSheet {
    pub fn build(lib: &Library, shoot: ShootId) -> Result<Self> {
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
        let taste: Vec<f32> = facts.iter().map(crate::taste::score).collect();
        let frames: Vec<crate::bursts::Frame> = photos
            .iter()
            .zip(&facts)
            .zip(&taste)
            .map(|((p, f), &t)| crate::bursts::Frame {
                id: p.id,
                at: seconds(&p.captured_at),
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
    let result = cull_sheet(&sheet, provider, max, progress)?;
    let result = crate::taste::rate(&sheet, result);
    apply(lib, &sheet, &result)
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
    let secs: Vec<i64> = photos.iter().map(|p| seconds(&p.captured_at)).collect();
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

/// Seconds since an arbitrary epoch from `YYYY-MM-DDTHH:MM:SS...`; 0 if unparseable.
pub(crate) fn seconds(t: &str) -> i64 {
    let num = |a: usize, b: usize| t.get(a..b).and_then(|s| s.parse::<i64>().ok());
    let (Some(y), Some(m), Some(d)) = (num(0, 4), num(5, 7), num(8, 10)) else {
        return 0;
    };
    let (hh, mm, ss) = (
        num(11, 13).unwrap_or(0),
        num(14, 16).unwrap_or(0),
        num(17, 19).unwrap_or(0),
    );
    // Days from civil (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe;
    days * 86_400 + hh * 3600 + mm * 60 + ss
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

/// Claude over the Messages API (raw HTTP; there is no official Rust SDK).
pub struct Claude {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
}

const MODEL: &str = "claude-opus-5-5";
const MAX_IMAGES_PER_REQUEST: usize = 100;

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

/// The schema a Provider answers in, for [`taste`].
pub fn schema() -> Value {
    match taste() {
        Taste::Signals => technical_schema(),
        Taste::Model => result_schema(),
    }
}

/// The last line of a request.
fn ask() -> &'static str {
    match taste() {
        Taste::Signals => "Check every photo of this shoot for technical failures.",
        Taste::Model => "Cull and rate this shoot.",
    }
}

/// The cull system prompt: the `cull` of the harness variant ARGUS_VARIANT names, else TECHNICAL
/// or SYSTEM for [`taste`] (an unreadable variant file is reported and the built-in one is used).
fn system() -> String {
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

impl Claude {
    /// Key from ANTHROPIC_API_KEY, else the macOS Keychain item `argus-anthropic`.
    /// ANTHROPIC_BASE_URL overrides the endpoint (the prototype's live test uses a mock).
    pub fn from_env() -> Result<Self> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
            .or_else(keychain_key)
            .ok_or_else(|| {
                Error(
                    "no Claude API key: set ANTHROPIC_API_KEY, or store one with \
                     `security add-generic-password -s argus-anthropic -a argus -w`"
                        .into(),
                )
            })?;
        Ok(Self {
            api_key: key,
            model: MODEL.into(),
            base_url: std::env::var("ANTHROPIC_BASE_URL")
                .unwrap_or_else(|_| "https://api.anthropic.com".into()),
        })
    }

    /// The Messages API request body for a Contact sheet. Public for tests.
    pub fn request_body(&self, sheet: &ContactSheet) -> Result<Value> {
        if sheet.photos.len() > MAX_IMAGES_PER_REQUEST {
            return Err(Error(format!(
                "Shoot has {} photos; culling more than {MAX_IMAGES_PER_REQUEST} at once isn't supported yet",
                sheet.photos.len()
            )));
        }
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut content = vec![json!({
            "type": "text",
            "text": format!("{} Each image is preceded by its number.", sheet.intro())
        })];
        for p in &sheet.photos {
            content.push(json!({
                "type": "text",
                "text": p.caption()
            }));
            content.push(json!({
                "type": "image",
                "source": { "type": "base64", "media_type": "image/jpeg", "data": b64.encode(&p.jpeg) }
            }));
        }
        content.push(json!({
            "type": "text",
            "text": format!("{} Return every photo number exactly once.", ask())
        }));
        let schema = schema();
        Ok(json!({
            "model": self.model,
            "max_tokens": 16000,
            "system": system(),
            "output_config": { "effort": "high", "format": { "type": "json_schema", "schema": schema } },
            "fallbacks": "default",
            "messages": [{ "role": "user", "content": content }]
        }))
    }
}

impl Provider for Claude {
    fn cull(&self, sheet: &ContactSheet) -> Result<CullResult> {
        let body = self.request_body(sheet)?;
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(600))
            .build();
        let resp = agent
            .post(&format!("{}/v1/messages", self.base_url))
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", "2023-06-01")
            .set("anthropic-beta", "server-side-fallback-2026-07-01")
            .set("content-type", "application/json")
            .send_json(body);
        let resp: Value = match resp {
            Ok(r) => r
                .into_json()
                .map_err(|e| Error(format!("Claude response: {e}")))?,
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                return Err(Error(format!("Claude API {code}: {text}")));
            }
            Err(e) => return Err(Error(format!("Claude API: {e}"))),
        };
        parse_response(&resp)
    }
}

/// Pull the CullResult out of a Messages API response, checking stop_reason first.
pub fn parse_response(resp: &Value) -> Result<CullResult> {
    match resp["stop_reason"].as_str() {
        Some("end_turn") => {}
        Some("refusal") => {
            return Err(Error(format!(
                "Claude declined to cull this shoot ({})",
                resp["stop_details"]["category"]
                    .as_str()
                    .unwrap_or("no category")
            )))
        }
        Some("max_tokens") => {
            return Err(Error(
                "Claude ran out of output tokens before finishing".into(),
            ))
        }
        other => return Err(Error(format!("unexpected stop_reason {other:?}"))),
    }
    let text = resp["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect::<String>();
    serde_json::from_str(&text).map_err(|e| Error(format!("Claude returned unexpected JSON: {e}")))
}

/// Check the shape of an Anthropic API key before it is stored: `sk-ant-` and URL-safe
/// characters only, so it can go to `security -i` on stdin without quoting.
pub fn check_api_key(key: &str) -> Result<&str> {
    let key = key.trim();
    let safe = key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if key.starts_with("sk-ant-") && key.len() > 20 && safe {
        Ok(key)
    } else {
        Err(Error(
            "that doesn't look like an Anthropic API key (they start with sk-ant-)".into(),
        ))
    }
}

/// Keep the Claude API key in the macOS Keychain item `argus-anthropic`, where
/// [`Claude::from_env`] finds it. The key goes over stdin, never in a process's arguments.
pub fn store_api_key(key: &str) -> Result<()> {
    use std::io::Write;
    let key = check_api_key(key)?;
    if !cfg!(target_os = "macos") {
        return Err(Error(
            "the Keychain is macOS only: set ANTHROPIC_API_KEY instead".into(),
        ));
    }
    let mut child = std::process::Command::new("security")
        .arg("-i")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Error(format!("couldn't run security: {e}")))?;
    let line = format!("add-generic-password -U -s argus-anthropic -a argus -w {key}\n");
    child
        .stdin
        .take()
        .ok_or_else(|| Error("couldn't talk to security".into()))?
        .write_all(line.as_bytes())
        .map_err(|e| Error(format!("couldn't talk to security: {e}")))?;
    let out = child
        .wait_with_output()
        .map_err(|e| Error(format!("security failed: {e}")))?;
    if out.status.success() && keychain_key().as_deref() == Some(key) {
        Ok(())
    } else {
        Err(Error(format!(
            "couldn't save the key in the Keychain: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

fn keychain_key() -> Option<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", "argus-anthropic", "-w"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|k| !k.is_empty())
}

/// A model Ollama has pulled on this Mac.
#[derive(Debug, Clone, Serialize)]
pub struct InstalledModel {
    pub name: String,
    pub bytes: u64,
    /// In memory now, so the next request starts at once.
    pub loaded: bool,
}

/// A local vision model through Ollama's HTTP API (the private, default Provider).
pub struct Ollama {
    pub base_url: String,
    pub model: String,
    /// Context window to ask for; a Shoot's images need far more than Ollama's default.
    pub num_ctx: u32,
    /// How much of the Mac the model may take (see [`crate::compute`]).
    pub compute: Compute,
}

/// The local vision model that culled Greg's bench Shoots best (about 17 GB at Q4); it
/// beat gemma4:31b on star AUC and Best Photos (2026-10-18, see ADR 0006).
pub const DEFAULT_OLLAMA_MODEL: &str = "qwen3.8:27b";

/// The model chosen in Settings › AI and Compute (kept in the Library as `ollama_model`).
static CHOSEN_MODEL: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

/// Use `model` for Ollama from now on in this process (None: back to the default).
/// ARGUS_OLLAMA_MODEL still wins, so tests and benches can pin one.
pub fn choose_ollama_model(model: Option<String>) {
    *CHOSEN_MODEL.write().unwrap() = model.filter(|m| !m.trim().is_empty());
}

impl Ollama {
    /// OLLAMA_HOST (default 127.0.0.1:11434), ARGUS_OLLAMA_MODEL (else the model chosen in
    /// Settings, else [`DEFAULT_OLLAMA_MODEL`]), ARGUS_OLLAMA_CTX, ARGUS_COMPUTE.
    pub fn from_env() -> Self {
        let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "127.0.0.1:11434".into());
        let base_url = if host.starts_with("http") {
            host
        } else {
            format!("http://{host}")
        };
        Self {
            base_url,
            model: std::env::var("ARGUS_OLLAMA_MODEL")
                .ok()
                .or_else(|| CHOSEN_MODEL.read().unwrap().clone())
                .unwrap_or_else(|| DEFAULT_OLLAMA_MODEL.into()),
            num_ctx: std::env::var("ARGUS_OLLAMA_CTX")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(65_536),
            compute: Compute::from_env(),
        }
    }

    /// The /api/chat request body for a Contact sheet. Public for tests.
    pub fn request_body(&self, sheet: &ContactSheet) -> Value {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut messages = vec![
            json!({ "role": "system", "content": system() }),
            json!({ "role": "user", "content": format!(
                "{} Each one follows as its own message.", sheet.intro()) }),
        ];
        for p in &sheet.photos {
            messages.push(json!({
                "role": "user",
                "content": p.caption(),
                "images": [b64.encode(&p.jpeg)]
            }));
        }
        messages.push(json!({ "role": "user", "content": format!(
            "{} Return JSON with every photo number from 1 to {} exactly once.",
            ask(), sheet.photos.len()) }));
        let mut body = json!({
            "model": self.model,
            "stream": true,
            "format": schema(),
            "keep_alive": "30m",
            "options": self.options(),
            "messages": messages
        });
        // A harness variant can turn thinking on or off (bench/tune.py); unset, Ollama decides.
        if let Some(think) = crate::variant::current().ok().and_then(|v| v.think) {
            body["think"] = json!(think);
        }
        body
    }

    /// Model options. Any difference makes Ollama reload the model, so the load request
    /// and the cull must send exactly these.
    pub(crate) fn options(&self) -> Value {
        let mut options = json!({ "num_ctx": self.num_ctx, "temperature": 0 });
        if let Some(n) = self.compute.plan().num_batch {
            options["num_batch"] = json!(n);
        }
        options
    }

    /// Loads the model without asking anything. Public for tests.
    pub fn load_body(&self) -> Value {
        // Keep it loaded well past the cull; Ollama's 5-minute default can lapse while a
        // throttled cull is still sending.
        json!({ "model": self.model, "stream": false, "messages": [], "keep_alive": "30m", "options": self.options() })
    }

    /// The models Ollama has pulled, largest first, and which are loaded now (`/api/tags`,
    /// `/api/ps`). Err when Ollama isn't running.
    pub fn installed(&self) -> Result<Vec<InstalledModel>> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(3))
            .build();
        let get = |path: &str| -> Result<Value> {
            agent
                .get(&format!("{}{path}", self.base_url))
                .call()
                .map_err(|e| Error(format!("can't reach Ollama at {} ({e})", self.base_url)))?
                .into_json()
                .map_err(|e| Error(format!("Ollama {path}: {e}")))
        };
        let tags = get("/api/tags")?;
        let loaded: Vec<String> = get("/api/ps")
            .ok()
            .and_then(|v| v["models"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|m| m["name"].as_str().map(str::to_owned))
            .collect();
        let mut models: Vec<InstalledModel> = tags["models"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|m| {
                let name = m["name"].as_str()?.to_owned();
                Some(InstalledModel {
                    loaded: loaded.contains(&name),
                    bytes: m["size"].as_u64().unwrap_or(0),
                    name,
                })
            })
            .collect();
        models.sort_by_key(|m| std::cmp::Reverse(m.bytes));
        Ok(models)
    }

    /// One request, once no other Argus process is using the model (a waiting job says
    /// on whom through [`crate::progress::waiting_here`]).
    pub(crate) fn post(&self, agent: &ureq::Agent, body: Value) -> Result<Value> {
        let _turn = crate::model_lock::take(&crate::progress::waiting_here);
        send_chat(agent, &self.base_url, &self.model, body)?
            .into_json()
            .map_err(|e| Error(format!("Ollama response: {e}")))
    }

    /// Send a streaming /api/chat request and gather the reply's content, telling `seen`
    /// after every piece how far it has got. Public for tests.
    pub fn stream(
        &self,
        agent: &ureq::Agent,
        body: Value,
        seen: &dyn Fn(Streamed),
    ) -> Result<String> {
        self.stream_within(
            agent,
            body,
            Stall::from_env(Duration::from_secs(3600)),
            seen,
        )
    }

    /// [`Self::stream`], giving up with an error when the model sends nothing for longer
    /// than `stall` allows: a request Ollama dropped (its runner restarted) or never
    /// started fails where the person sees it, instead of a bar that waits all night.
    pub fn stream_within(
        &self,
        agent: &ureq::Agent,
        body: Value,
        stall: Stall,
        seen: &dyn Fn(Streamed),
    ) -> Result<String> {
        use std::io::BufRead;
        use std::sync::mpsc::RecvTimeoutError;
        let t = Instant::now();
        // The request reads on its own thread so a silent socket can't hold this one; on
        // a stall that thread is left to end at the agent's read timeout.
        let (tx, lines) = std::sync::mpsc::channel::<Result<String>>();
        let (agent2, base, model) = (agent.clone(), self.base_url.clone(), self.model.clone());
        std::thread::Builder::new()
            .name("argus-ollama-stream".into())
            .spawn(move || {
                let r = match send_chat(&agent2, &base, &model, body) {
                    Ok(r) => r,
                    Err(e) => return drop(tx.send(Err(e))),
                };
                for line in std::io::BufReader::new(r.into_reader()).lines() {
                    let line = line.map_err(|e| Error(format!("Ollama stream: {e}")));
                    let end = line.is_err();
                    if tx.send(line).is_err() || end {
                        return;
                    }
                }
            })
            .map_err(|e| Error(format!("Ollama stream thread: {e}")))?;
        let mut content = String::new();
        let mut so_far = Streamed::default();
        let mut scanned = 0;
        let mut finished = false;
        loop {
            let wait = if so_far.tokens == 0 {
                stall.first
            } else {
                stall.gap
            };
            let line = match lines.recv_timeout(wait) {
                Ok(line) => line?,
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(Error(format!(
                        "the model stopped answering: nothing from Ollama for {} ({}). Ollama \
                         may have restarted or be busy with another app; try again",
                        minutes(wait),
                        if so_far.tokens == 0 {
                            "it never started replying".to_owned()
                        } else {
                            format!("{} tokens in", so_far.tokens)
                        }
                    )))
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(&line)
                .map_err(|e| Error(format!("Ollama stream: {e} in {line:.200}")))?;
            if let Some(e) = v["error"].as_str() {
                return Err(Error(format!("Ollama: {e}")));
            }
            let piece = v["message"]["content"].as_str().unwrap_or_default();
            let thinking = v["message"]["thinking"].as_str().unwrap_or_default();
            if !piece.is_empty() || !thinking.is_empty() {
                so_far.tokens += 1;
            }
            content.push_str(piece);
            let (found, next) = count_photo_keys(content.as_bytes(), scanned);
            so_far.photos += found;
            scanned = next;
            seen(so_far);
            if v["done"].as_bool() == Some(true) {
                crate::agent::log_timing(&self.model, &v, t.elapsed());
                finished = true;
                break;
            }
        }
        if !finished {
            return Err(Error(format!(
                "Ollama's reply broke off after {} tokens (its model runner may have \
                 restarted); try again",
                so_far.tokens
            )));
        }
        Ok(content)
    }
}

fn send_chat(
    agent: &ureq::Agent,
    base_url: &str,
    model: &str,
    body: Value,
) -> Result<ureq::Response> {
    match agent.post(&format!("{base_url}/api/chat")).send_json(body) {
        Ok(r) => Ok(r),
        Err(ureq::Error::Status(code, r)) => {
            let text = r.into_string().unwrap_or_default();
            let hint = if text.contains("not found") {
                format!(" (run `ollama pull {model}`)")
            } else {
                String::new()
            };
            Err(Error(format!("Ollama {code}: {text}{hint}")))
        }
        Err(e) => Err(Error(format!(
            "can't reach Ollama at {base_url} ({e}); is it running?"
        ))),
    }
}

fn minutes(d: Duration) -> String {
    let m = d.as_secs() / 60;
    if m == 0 {
        format!("{} s", d.as_secs())
    } else {
        format!("{m} min")
    }
}

/// How long a streaming request may stay silent before it counts as dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stall {
    /// Until the first token: the model loads, reads every image and may queue in Ollama.
    pub first: Duration,
    /// Between tokens once it is replying.
    pub gap: Duration,
}

impl Stall {
    /// ARGUS_OLLAMA_FIRST_TOKEN_S and ARGUS_OLLAMA_STALL_S (default 5 min) override.
    fn from_env(first: Duration) -> Self {
        let secs = |k: &str| {
            std::env::var(k)
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .map(Duration::from_secs_f64)
        };
        Stall {
            first: secs("ARGUS_OLLAMA_FIRST_TOKEN_S").unwrap_or(first),
            gap: secs("ARGUS_OLLAMA_STALL_S").unwrap_or(Duration::from_secs(300)),
        }
    }

    /// For a cull of `sheet` at `compute`: 5 min plus the time to read the sheet's prompt
    /// at half the 63 tokens/s Qwen 3.8 27B read at Full on Greg's M4 Max (2026-10-20:
    /// 41.7k tokens in 657 s), twice that at Balanced and four times at Gentle, whose
    /// pauses slow the read. Image tokens follow pixels (one per 28x28 patch), so bigger
    /// previews and face close-ups get longer; a flat 6 s a Photo once timed out on them.
    pub fn for_cull(sheet: &ContactSheet, compute: Compute) -> Self {
        let tokens: u64 = sheet
            .photos
            .iter()
            .map(|p| {
                let (w, h) = image::ImageReader::new(std::io::Cursor::new(&p.jpeg))
                    .with_guessed_format()
                    .ok()
                    .and_then(|r| r.into_dimensions().ok())
                    .unwrap_or((SHEET_LONG_EDGE, SHEET_LONG_EDGE * 2 / 3));
                150 + u64::from(w) * u64::from(h) / (28 * 28)
            })
            .sum();
        let full = 300 + tokens / 30;
        let factor = match compute {
            Compute::Full => 1,
            Compute::Balanced => 2,
            Compute::Gentle => 4,
        };
        Self::from_env(Duration::from_secs(full * factor))
    }
}

/// Count the `"n"` keys (one per Photo answered) in `json[from..]`; returns how many and
/// where the next scan starts, so a key split across two pieces is counted once.
/// Escaped quotes inside strings (`\"n\"`) never match.
pub fn count_photo_keys(json: &[u8], from: usize) -> (usize, usize) {
    const KEY: &[u8] = b"\"n\"";
    if json.len() < KEY.len() {
        return (0, from);
    }
    let last = json.len() - KEY.len();
    let found = (from..=last)
        .filter(|&i| &json[i..i + KEY.len()] == KEY)
        .count();
    (found, (last + 1).max(from))
}

impl Provider for Ollama {
    fn cull(&self, sheet: &ContactSheet) -> Result<CullResult> {
        self.cull_streaming(sheet, &|_| {})
    }

    /// Qwen 3.8 27B read a 67-Photo sheet at about 12 s a Photo at Gentle on Greg's Mac
    /// (2026-10-20, 45 prompt tokens/s); faster levels scale by their share of the Mac.
    fn read_pace(&self) -> Option<f64> {
        Some(match self.compute {
            Compute::Full => 4.0,
            Compute::Balanced => 7.0,
            Compute::Gentle => 12.0,
        })
    }

    fn cull_streaming(&self, sheet: &ContactSheet, seen: &dyn Fn(Streamed)) -> Result<CullResult> {
        // A cull holds the model for many minutes: background Queues in this and other
        // Argus processes start nothing new meanwhile, so they don't share the model.
        let _foreground = crate::compute::Foreground::begin();
        let stall = Stall::for_cull(sheet, self.compute);
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(12 * 3600)) // slow is fine; Gentle can take hours
            // Ends a reading thread that `stream_within` gave up on.
            .timeout_read(stall.first.max(stall.gap) + Duration::from_secs(60))
            .build();
        // Load the model at full speed first: pausing a loading model makes the load
        // crawl, and the throttle is only meant for the cull itself. Loading waits behind
        // whatever else Ollama is running, so it gets the same limit as the first token.
        let loader = ureq::AgentBuilder::new().timeout(stall.first).build();
        self.post(&loader, self.load_body()).map_err(|e| {
            if e.0.contains("timed out") || e.0.contains("Timeout") {
                Error(format!(
                    "Ollama didn't load {} within {}: another app may be using it; try again",
                    self.model,
                    minutes(stall.first)
                ))
            } else {
                e
            }
        })?;
        // Only throttle an Ollama on this Mac; a remote one isn't ours to pause.
        let local = ["127.0.0.1", "localhost", "[::1]", "0.0.0.0"]
            .iter()
            .any(|h| self.base_url.contains(h));
        let _throttle = local.then(|| Throttle::start(self.compute, OllamaProcesses));
        let text = self.stream_within(&agent, self.request_body(sheet), stall, seen)?;
        parse_json_reply(&text)
    }

    fn local_model(&self) -> bool {
        true
    }
}

/// Claude Code on this Mac (`claude -p`), so a cull runs on the person's Claude plan
/// instead of an API key. See ADR 0006. The Contact sheet goes to a temp folder that
/// Claude Code reads with its Read tool.
pub struct ClaudeCode {
    pub bin: String,
    pub model: String,
}

impl ClaudeCode {
    pub fn from_env() -> Self {
        Self {
            bin: std::env::var("ARGUS_CLAUDE_BIN").unwrap_or_else(|_| "claude".into()),
            model: std::env::var("ARGUS_CLAUDE_MODEL").unwrap_or_else(|_| "opus".into()),
        }
    }
}

impl Provider for ClaudeCode {
    fn cull(&self, sheet: &ContactSheet) -> Result<CullResult> {
        let dir = std::env::temp_dir().join(format!(
            "argus-sheet-{}-{}",
            sheet.shoot,
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).map_err(|e| Error(format!("sheet dir: {e}")))?;
        let mut list = String::new();
        for p in &sheet.photos {
            let path = dir.join(format!("{:03}.jpg", p.n));
            std::fs::write(&path, &p.jpeg).map_err(|e| Error(format!("sheet: {e}")))?;
            list += &format!("{}: {}\n", p.caption(), path.display());
        }
        let prompt = format!(
            "{}\n\n{} Read every image file below \
             with the Read tool before deciding anything.\n\n{list}\nReply with only a JSON object \
             (no prose, no code fence) matching this JSON Schema, with every photo number exactly once:\n{}",
            system(),
            sheet.intro(),
            schema()
        );
        let t = Instant::now();
        let out = std::process::Command::new(&self.bin)
            .args([
                "-p",
                &prompt,
                "--output-format",
                "json",
                "--allowedTools",
                "Read",
                "--model",
                &self.model,
            ])
            .current_dir(&dir)
            .output();
        let _ = std::fs::remove_dir_all(&dir);
        let out = out.map_err(|e| {
            Error(format!(
                "can't run `{}` (is Claude Code installed?): {e}",
                self.bin
            ))
        })?;
        crate::agent::log_claude(&self.model, &out, t.elapsed());
        let stdout = String::from_utf8_lossy(&out.stdout);
        let v: Value = serde_json::from_str(&stdout).map_err(|_| {
            Error(format!(
                "Claude Code failed: {}{}",
                String::from_utf8_lossy(&out.stderr).trim(),
                stdout.trim()
            ))
        })?;
        if v["is_error"] == true {
            return Err(Error(format!(
                "Claude Code: {} (run `claude auth login` if you aren't signed in)",
                v["result"].as_str().unwrap_or("error")
            )));
        }
        parse_json_reply(v["result"].as_str().unwrap_or_default())
    }
}

/// Parse a CullResult from model text, tolerating prose or a code fence around the JSON.
pub fn parse_json_reply(text: &str) -> Result<CullResult> {
    let (Some(a), Some(b)) = (text.find('{'), text.rfind('}')) else {
        return Err(Error(format!(
            "model returned no JSON: {}",
            text.chars().take(200).collect::<String>()
        )));
    };
    serde_json::from_str(&text[a..=b])
        .map_err(|e| Error(format!("model returned unexpected JSON: {e}")))
}

/// Pick a Provider by name: `ollama` (default, local), `claude-code` (your Claude plan via
/// Claude Code), or `claude-api` (an Anthropic API key). ARGUS_PROVIDER sets the default.
pub fn provider(name: Option<&str>, compute: Option<Compute>) -> Result<Box<dyn Provider>> {
    let env = std::env::var("ARGUS_PROVIDER").ok();
    match name.or(env.as_deref()).unwrap_or("ollama") {
        "ollama" => {
            let mut o = Ollama::from_env();
            if let Some(c) = compute {
                o.compute = c;
            }
            Ok(Box::new(o))
        }
        "claude-code" => Ok(Box::new(ClaudeCode::from_env())),
        "claude-api" | "claude" => Ok(Box::new(Claude::from_env()?)),
        other => Err(Error(format!(
            "unknown provider {other:?}; use ollama, claude-code or claude-api"
        ))),
    }
}
