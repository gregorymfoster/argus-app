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
    type Stamp = (u64, SystemTime);
    type Mem = HashMap<PathBuf, (Stamp, Arc<Analysis>)>;
    static MEM: OnceLock<Mutex<Mem>> = OnceLock::new();
    let mem = MEM.get_or_init(Default::default);
    let stamp = std::fs::metadata(&photo.preview)
        .map(|m| (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
        .map_err(|e| Error(format!("preview of Photo {}: {e}", photo.id)))?;
    if let Some((at, a)) = mem.lock().unwrap().get(&photo.preview) {
        if *at == stamp {
            return Ok(a.clone());
        }
    }
    let facts = crate::analyzers::facts(lib, photo, crate::masks::ANALYSIS_FACTS)?;
    let a = Arc::new(Analysis::from_facts(&facts)?);
    mem.lock()
        .unwrap()
        .insert(photo.preview.clone(), (stamp, a.clone()));
    Ok(a)
}

/// The Analysis `recipe` needs, if it has AI Masks or retouching. A Photo that can't be
/// analysed renders with the analysis made from the render itself.
pub fn masks_for(lib: &Library, photo: &Photo, recipe: &Recipe) -> Option<Arc<Analysis>> {
    if !recipe.needs_masks() {
        return None;
    }
    match analysis(lib, photo) {
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
    let mut img = crate::geometry::apply(
        develop_photo(photo, recipe, long_edge, analysis)?,
        &recipe.crop,
    );
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
        "raw" => match long_edge {
            Some(edge) => match demosaiced(path, edge)? {
                Some(d) => {
                    Ok(d.finish(&crate::pipeline::Look::DEFAULT, recipe, analysis.as_deref()))
                }
                None => full_size(path, recipe, analysis),
            },
            None => full_size(path, recipe, analysis),
        },
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

/// Which RAW at which size: the file's length and modified time stand in for its content.
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
    // Decode outside the lock, so other Photos render meanwhile.
    let Some(d) = Demosaiced::new(&decode(path)?, long_edge).map(Arc::new) else {
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
