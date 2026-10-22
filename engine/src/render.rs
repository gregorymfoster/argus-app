//! Render a Photo from its original with a Recipe: the one path the tools, the CLI and
//! export share. RAW Photos go through [`crate::develop`]; JPEG Photos through
//! [`crate::develop_rgb8`]. Originals are only read.

use crate::library::{read_jpeg_upright, Library, Photo};
use crate::masks::Analysis;
use crate::recipe::Recipe;
use crate::{
    decode, develop, develop_rgb8_scaled, Demosaiced, DevelopOptions, Error, Result, RgbImage,
};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

/// What the Mask models see in a Photo: the `segment`, `sky` and `faces` Facts (ADR
/// 0013), kept in memory while the preview is unchanged so slider drags don't re-read them.
pub fn analysis(lib: &Library, photo: &Photo) -> Result<Arc<Analysis>> {
    analysis_with(lib, photo, false)
}

/// [`analysis`], with the depth map too when `depth` (Lens Blur), computing it the first
/// time.
pub fn analysis_with(lib: &Library, photo: &Photo, depth: bool) -> Result<Arc<Analysis>> {
    type Stamp = (u64, SystemTime);
    type Mem = HashMap<PathBuf, (Stamp, Arc<Analysis>)>;
    static MEM: OnceLock<Mutex<Mem>> = OnceLock::new();
    let mem = MEM.get_or_init(Default::default);
    let stamp = std::fs::metadata(&photo.preview)
        .map(|m| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
        .map_err(|e| Error(format!("preview of Photo {}: {e}", photo.id)))?;
    if let Some((at, a)) = mem.lock().unwrap().get(&photo.preview) {
        if *at == stamp && (!depth || a.depth.is_some()) {
            return Ok(a.clone());
        }
    }
    let mut ids = crate::masks::ANALYSIS_FACTS.to_vec();
    if depth {
        ids.push(crate::masks::DEPTH_FACT);
    }
    let facts = crate::analyzers::facts(lib, photo, &ids)?;
    let a = Arc::new(Analysis::from_facts(&facts)?);
    mem.lock()
        .unwrap()
        .insert(photo.preview.clone(), (stamp, a.clone()));
    Ok(a)
}

/// The Analysis `recipe` needs, if it has AI Masks, retouching or Lens Blur. A Photo that
/// can't be analysed renders with the analysis made from the render itself.
pub fn masks_for(lib: &Library, photo: &Photo, recipe: &Recipe) -> Option<Arc<Analysis>> {
    if !recipe.needs_masks() {
        return None;
    }
    match analysis_with(lib, photo, recipe.lens_blur.active()) {
        Ok(a) => Some(a),
        Err(e) => {
            eprintln!("masks: {e}; analysing the render instead");
            None
        }
    }
}

/// [`render_photo`] with the Analysis its Masks need, read from the Library.
pub fn render_in(
    lib: &Library,
    photo: &Photo,
    recipe: &Recipe,
    long_edge: Option<usize>,
) -> Result<RgbImage> {
    render_photo(photo, recipe, long_edge, masks_for(lib, photo, recipe))
}

/// Render `photo` with `recipe`, the long edge at most `long_edge` (full size if None).
/// `analysis` is the Photo's [`Analysis`] when the Recipe has AI Masks (see [`masks_for`]);
/// without one, AI Masks use an analysis of the render.
pub fn render_photo(
    photo: &Photo,
    recipe: &Recipe,
    long_edge: Option<usize>,
    analysis: Option<Arc<Analysis>>,
) -> Result<RgbImage> {
    let mut img = develop_photo(photo, recipe, long_edge, analysis)?;
    crate::heal::apply(&mut img, &recipe.spots);
    let mut img = crate::geometry::apply(img, &recipe.crop);
    crate::effects::post_crop(&mut img, &recipe.render_adjust());
    Ok(img)
}

fn develop_photo(
    photo: &Photo,
    recipe: &Recipe,
    long_edge: Option<usize>,
    analysis: Option<Arc<Analysis>>,
) -> Result<RgbImage> {
    let path = Path::new(&photo.path);
    if !path.exists() {
        return Err(Error(format!(
            "the original of Photo {} is not reachable at {} (drive offline?)",
            photo.id, photo.path
        )));
    }
    match photo.kind.as_str() {
        "raw" => {
            // AI Denoise: blend the denoised base in once it exists (crate::denoise).
            let amount = crate::denoise::amount(recipe);
            let base = crate::denoise::ready(photo).filter(|_| amount > 0.0);
            match long_edge
                .map(|e| demosaiced(path, e))
                .transpose()?
                .flatten()
            {
                Some(d) => {
                    let d = match &base {
                        Some(b) => {
                            let den = denoised(b, d.width.max(d.height))?;
                            Arc::new(blended(&d, &den.rgb, amount)?)
                        }
                        None => d,
                    };
                    Ok(d.finish(&crate::pipeline::Look::DEFAULT, recipe, analysis.as_deref()))
                }
                None => match &base {
                    Some(b) => {
                        let raw = decode(path)?;
                        let (w, h) = raw.crop_size();
                        let d = Demosaiced {
                            width: w,
                            height: h,
                            rgb: crate::pipeline::demosaic_bilinear(
                                &crate::pipeline::normalize_white_balance(&raw),
                                w,
                                h,
                                raw.cfa,
                            ),
                            camera_to_srgb: crate::pipeline::camera_to_srgb_matrix(raw.xyz_to_cam),
                            orientation: raw.orientation,
                            scale: 1.0,
                            lens: crate::lens::Lens::new(raw.lens.clone()),
                        };
                        drop(raw);
                        let (den, ..) = crate::denoise::base_at(b, None)?;
                        let d = blended(&d, &den, amount)?;
                        Ok(d.finish(&crate::pipeline::Look::DEFAULT, recipe, analysis.as_deref()))
                    }
                    None => full_size(path, recipe, analysis),
                },
            }
        }
        "jpeg" => {
            let mut img = read_jpeg_upright(path)?;
            let full = img.width().max(img.height()) as f32;
            if let Some(e) = long_edge.map(|e| e as u32) {
                if img.width().max(img.height()) > e {
                    img = img.resize(e, e, image::imageops::FilterType::Triangle);
                }
            }
            let rgb = img.to_rgb8();
            let src = RgbImage {
                width: rgb.width() as usize,
                height: rgb.height() as usize,
                data: rgb.into_raw(),
            };
            let scale = src.width.max(src.height) as f32 / full;
            Ok(develop_rgb8_scaled(
                &src,
                recipe,
                analysis.as_deref(),
                scale,
            ))
        }
        other => Err(Error(format!("can't render a {other} Photo yet"))),
    }
}

fn full_size(path: &Path, recipe: &Recipe, analysis: Option<Arc<Analysis>>) -> Result<RgbImage> {
    Ok(develop(
        &decode(path)?,
        &DevelopOptions {
            recipe: recipe.clone(),
            analysis,
            ..Default::default()
        },
    ))
}

/// `d` with the denoised base `den` (same size) blended over it at `amount`.
fn blended(d: &Demosaiced, den: &[f32], amount: f32) -> Result<Demosaiced> {
    if den.len() != d.rgb.len() {
        return Err(Error(format!(
            "denoised base is {} values, the image {}",
            den.len(),
            d.rgb.len()
        )));
    }
    let mut rgb = d.rgb.clone();
    crate::denoise::blend(&mut rgb, den, amount);
    Ok(Demosaiced {
        rgb,
        width: d.width,
        height: d.height,
        camera_to_srgb: d.camera_to_srgb,
        orientation: d.orientation,
        scale: d.scale,
        lens: d.lens.fresh(),
    })
}

/// Which RAW (or denoised base) at which size: the file's length and modified time stand in
/// for its content.
#[derive(PartialEq, Eq, Clone)]
struct Key {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
    long_edge: usize,
}

/// Recent RAWs taken through stages 1-3, newest first. Decoding a 61 MP RAW costs ~0.2 s
/// and demosaicing another ~0.1 s, while colour and tone at 1536 px take ~10 ms, so the
/// Edit screen's sliders re-render from here. A 1536 px entry is ~19 MB.
static DEMOSAICED: Mutex<VecDeque<(Key, Arc<Demosaiced>)>> = Mutex::new(VecDeque::new());
const CACHE_BYTES: usize = 160 << 20;

/// Stages 1-3 for `path` at `long_edge`, from the cache when it has them. None when the
/// RAW is no bigger than `long_edge`.
fn demosaiced(path: &Path, long_edge: usize) -> Result<Option<Arc<Demosaiced>>> {
    // Decode outside the lock, so other Photos render meanwhile.
    cached(path, long_edge, || {
        Ok(Demosaiced::new(&decode(path)?, long_edge))
    })
}

/// The denoised base `file` at the size of a [`demosaiced`] image with long edge
/// `long_edge` (only `rgb`, `width` and `height` are meaningful).
fn denoised(file: &Path, long_edge: usize) -> Result<Arc<Demosaiced>> {
    cached(file, long_edge, || {
        let (rgb, width, height) = crate::denoise::base_at(file, Some(long_edge))?;
        Ok(Some(Demosaiced {
            width,
            height,
            rgb,
            camera_to_srgb: [[0.0; 3]; 3],
            orientation: crate::Orientation::Normal,
            scale: 1.0,
            lens: Default::default(),
        }))
    })?
    .ok_or_else(|| Error("denoised base is empty".into()))
}

/// `make()` for `path` at `long_edge`, through the cache of recent images.
fn cached(
    path: &Path,
    long_edge: usize,
    make: impl FnOnce() -> Result<Option<Demosaiced>>,
) -> Result<Option<Arc<Demosaiced>>> {
    let meta = std::fs::metadata(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
    let key = Key {
        path: path.to_path_buf(),
        len: meta.len(),
        modified: meta.modified().ok(),
        long_edge,
    };
    {
        let mut cache = DEMOSAICED.lock().unwrap();
        if let Some(i) = cache.iter().position(|(k, _)| *k == key) {
            let hit = cache.remove(i).unwrap();
            let d = hit.1.clone();
            cache.push_front(hit);
            return Ok(Some(d));
        }
    }
    let Some(d) = make()?.map(Arc::new) else {
        return Ok(None);
    };
    let mut cache = DEMOSAICED.lock().unwrap();
    cache.retain(|(k, _)| *k != key);
    cache.push_front((key, d.clone()));
    let mut total = 0;
    cache.retain(|(_, v)| {
        total += v.bytes();
        total <= CACHE_BYTES || Arc::ptr_eq(v, &d)
    });
    Ok(Some(d))
}

/// Encode as JPEG (4:2:0, as cameras do).
pub fn encode_jpeg(img: &RgbImage, quality: u8) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut enc = jpeg_encoder::Encoder::new(&mut out, quality);
    enc.set_sampling_factor(jpeg_encoder::SamplingFactor::F_2_2);
    enc.encode(
        &img.data,
        img.width as u16,
        img.height as u16,
        jpeg_encoder::ColorType::Rgb,
    )
    .map_err(|e| Error(format!("jpeg encode: {e}")))?;
    Ok(out)
}
