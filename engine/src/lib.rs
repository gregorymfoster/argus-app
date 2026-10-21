//! Argus engine: RAW decode, a small CPU develop pipeline, and the Library.
//!
//! Public surface is deliberately tiny:
//! - [`decode`] -> [`RawPhoto`]
//! - [`develop`] -> [`RgbImage`] (8-bit sRGB), with a [`recipe::Recipe`] applied
//! - [`develop_rgb8`] -> a [`recipe::Recipe`] applied to an already-rendered image (JPEG Photos)
//! - [`develop_linear`] -> [`LinearImage`] (linear sRGB f32, for fitting a Look)
//! - [`embedded_preview`] -> the camera's largest embedded JPEG, unchanged
//! - [`cull::cull_shoot`] -> AI Verdicts, star Ratings, Stacks and Best Photos for a whole Shoot via a [`cull::Provider`]
//! - [`library::Library`] -> Shoots, Photos, Verdicts and Recipes in SQLite
//! - [`tools::Tools`] -> the tool layer as data (JSON Schemas + one `call`), for the Agent and MCP
//! - [`agent::run`] -> the Agent's tool-use loop: a request in plain words, carried out with the tools
//! - [`commands::dispatch`] -> the UI's calls: every tool, plus person-made flags and edits
//!
//! Each pipeline stage is a separate pure function in [`pipeline`] so it can be
//! ported to a WGSL shader later.

pub mod agent;
pub mod analyzers;
pub mod auto;
pub mod bursts;
pub mod color;
pub mod commands;
pub mod compute;
pub mod cull;
pub mod debug_log;
pub mod denoise;
pub mod effects;
pub mod exemplars;
pub mod export;
pub mod geometry;
pub mod heal;
pub mod jobs;
pub mod lens;
pub mod libraries;
pub mod library;
mod local;
pub mod logs;
pub mod masks;
pub mod meter;
pub mod model_lock;
pub mod naming;
pub mod paste;
pub mod pipeline;
pub mod power;
pub mod preset_rank;
pub mod presets;
mod preview;
pub mod progress;
pub mod rawcheck;
pub mod recipe;
pub mod render;
pub mod signals;
pub mod style;
pub mod suggest;
pub mod taste;
pub mod tools;
pub mod usage;
pub mod variant;
pub mod white_balance;

use std::path::Path;

pub use preview::{
    embedded_preview, embedded_preview_at_least, embedded_preview_quick, jpeg_dimensions,
};

#[derive(Debug)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

/// Colour index of a CFA site: 0 = R, 1 = G, 2 = B.
pub type CfaPattern = [[u8; 2]; 2];

/// Image orientation as EXIF-style rotation applied at the end of the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Normal,
    Rotate90, // rotate 90 degrees clockwise to display upright
    Rotate180,
    Rotate270, // rotate 270 clockwise (90 counter-clockwise)
}

/// A decoded RAW: the Bayer mosaic plus everything needed to develop it.
pub struct RawPhoto {
    pub make: String,
    pub model: String,
    /// Full sensor dimensions of `mosaic`.
    pub sensor_width: usize,
    pub sensor_height: usize,
    /// Recommended crop inside the sensor area (x, y, w, h).
    pub crop: (usize, usize, usize, usize),
    /// Raw 16-bit samples, `sensor_width * sensor_height`.
    pub mosaic: Vec<u16>,
    /// Pattern at sensor origin (0,0).
    pub cfa: CfaPattern,
    /// Black level per 2x2 position at sensor origin, row-major.
    pub black: [f32; 4],
    pub white: [f32; 4],
    /// As-shot white balance multipliers (R, G, B), as stored by the camera.
    pub wb: [f32; 3],
    /// Camera matrix: XYZ(D65-ish) -> camera RGB (3x3).
    pub xyz_to_cam: [[f32; 3]; 3],
    pub orientation: Orientation,
    /// The lens corrections the camera wrote into the file ([`lens`]), if any.
    pub lens: Option<std::sync::Arc<lens::Profile>>,
}

impl RawPhoto {
    /// Dimensions of the developed (cropped) image before orientation.
    pub fn crop_size(&self) -> (usize, usize) {
        (self.crop.2, self.crop.3)
    }
}

/// 8-bit interleaved RGB.
#[derive(Clone)]
pub struct RgbImage {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct DevelopOptions {
    /// If set, output is downsized so its longest edge is at most this. When the
    /// target is <= half of the source, a fast 2x2 "binning" demosaic is used.
    pub long_edge: Option<usize>,
    /// Global tone and colour parameters (exposure, shoulder, shadow lift, S-curve, saturation).
    pub look: pipeline::Look,
    /// The Photo's edit settings, applied on top of the Look. Default: no edits.
    pub recipe: recipe::Recipe,
    /// What the Mask models found in this Photo (cached by the Library). When None and the
    /// Recipe has an AI Mask, the render analyses itself.
    pub analysis: Option<std::sync::Arc<masks::Analysis>>,
}

impl Default for DevelopOptions {
    fn default() -> Self {
        Self {
            long_edge: None,
            look: pipeline::Look::DEFAULT,
            recipe: recipe::Recipe::default(),
            analysis: None,
        }
    }
}

/// Map an EXIF Orientation value (1-8) to the rotation that displays the image upright.
/// `Rotate90` is clockwise. Mirrored values (2, 4, 5, 7) never occur in camera files, so they
/// map to the nearest pure rotation and the flip is ignored.
fn orientation_from_exif(v: u16) -> Orientation {
    match v {
        3 | 4 => Orientation::Rotate180,
        6 | 5 => Orientation::Rotate90,
        8 | 7 => Orientation::Rotate270,
        _ => Orientation::Normal,
    }
}

/// A RAW's developed size, upright (what [`develop`] would return at full size), and its
/// orientation, from the file's headers (`data`, the whole file) without decoding pixels.
pub fn raw_size(path: &Path, data: &[u8]) -> Result<(usize, usize, Orientation)> {
    let (w, h) = match preview::tiff_default_crop_size(data) {
        Some(size) => size,
        // No crop tag: ask rawler, whose dummy decode still allocates the full mosaic.
        None => {
            rawcheck::check(path)?;
            let src = rawler::rawsource::RawSource::new(path)
                .map_err(|e| Error(format!("read {}: {e}", path.display())))?;
            let img = rawler::decode_dummy(&src).map_err(|e| Error(format!("rawler: {e}")))?;
            match img.crop_area.or(img.active_area) {
                Some(r) => (r.d.w, r.d.h),
                None => (img.width, img.height),
            }
        }
    };
    let (w, h) = (w & !1, h & !1);
    let o = orientation_from_exif(preview::tiff_orientation(data).unwrap_or(1));
    Ok(match o {
        Orientation::Rotate90 | Orientation::Rotate270 => (h, w, o),
        _ => (w, h, o),
    })
}

/// Decode a RAW file with rawler.
pub fn decode(path: &Path) -> Result<RawPhoto> {
    rawcheck::check(path)?;
    let mut img = rawler::decode_file(path).map_err(|e| {
        // An empty file or one that only ends in .ARW: rawler's "No decoder found, model ''"
        // says nothing to a person.
        if e.to_string().contains("No decoder found") {
            let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            Error(format!("not a RAW file Argus can read ({bytes} bytes)"))
        } else {
            Error(format!("rawler decode failed: {e}"))
        }
    })?;
    // Take the samples rather than copy them: a 61 MP mosaic is 120 MB.
    let mosaic = match std::mem::replace(&mut img.data, rawler::RawImageData::Integer(Vec::new())) {
        rawler::RawImageData::Integer(v) => v,
        rawler::RawImageData::Float(_) => return Err(Error("float raw data not supported".into())),
    };
    if img.cpp != 1 {
        return Err(Error(format!(
            "expected 1 component per pixel, got {}",
            img.cpp
        )));
    }
    let mut cfa = [[0u8; 2]; 2];
    for (r, row) in cfa.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            *v = img.camera.cfa.color_at(r, c) as u8;
        }
    }
    let (w, h) = (img.width, img.height);
    let crop = match img.crop_area.or(img.active_area) {
        Some(r) => (r.p.x, r.p.y, r.d.w, r.d.h),
        None => (0, 0, w, h),
    };
    // Keep the crop origin even so the CFA phase at the crop origin equals the sensor phase.
    let crop = (crop.0 & !1, crop.1 & !1, crop.2 & !1, crop.3 & !1);

    let mut xyz_to_cam = [[0f32; 3]; 3];
    // rawler (0.8) fills `color_matrix` per illuminant (flat row-major XYZ->cam); the
    // legacy `xyz_to_cam` field is empty for these files. Prefer D65, then D50, then any.
    use rawler::imgop::xyz::Illuminant;
    let flat = [
        Illuminant::D65,
        Illuminant::D50,
        Illuminant::D55,
        Illuminant::D75,
    ]
    .iter()
    .find_map(|i| img.color_matrix.get(i))
    .or_else(|| img.color_matrix.values().next())
    .filter(|m| m.len() >= 9);
    if let Some(m) = flat {
        for (i, row) in xyz_to_cam.iter_mut().enumerate() {
            row.copy_from_slice(&m[i * 3..i * 3 + 3]);
        }
    } else {
        for (i, row) in xyz_to_cam.iter_mut().enumerate() {
            row.copy_from_slice(&img.xyz_to_cam[i]);
        }
    }
    if std::env::var_os("ARGUS_DEBUG").is_some() {
        eprintln!("illuminants: {:?} wb {:?} black {:?} white {:?} orient {:?} cfa {:?} crop {:?} active {:?}", img.color_matrix.keys().collect::<Vec<_>>(), img.wb_coeffs, img.blacklevel, img.whitelevel, img.orientation, cfa, img.crop_area, img.active_area);
        eprintln!("xyz_to_cam {:?}", xyz_to_cam);
        let mut hist = std::collections::BTreeMap::new();
        for v in mosaic.iter().step_by(7) {
            *hist.entry(*v).or_insert(0u32) += 1;
        }
        let first: Vec<_> = hist.iter().take(40).collect();
        eprintln!(
            "distinct raw values (1/7 sample): {} min {:?} max {:?} first {:?}",
            hist.len(),
            hist.keys().next(),
            hist.keys().last(),
            first
        );
    }
    if xyz_to_cam.iter().flatten().all(|v| *v == 0.0) {
        return Err(Error("no colour matrix for this camera".into()));
    }
    let wb = if img.wb_coeffs[0].is_nan() || img.wb_coeffs[0] <= 0.0 {
        [1.0, 1.0, 1.0]
    } else {
        [img.wb_coeffs[0], img.wb_coeffs[1], img.wb_coeffs[2]]
    };
    use rawler::Orientation as O;
    // rawler 0.8 reports Normal for every Sony ARW; the file's own TIFF tag is authoritative.
    let orientation = match preview::file_orientation(path) {
        Some(v) => orientation_from_exif(v),
        None => match img.orientation {
            O::Rotate90 => Orientation::Rotate90,
            O::Rotate180 => Orientation::Rotate180,
            O::Rotate270 => Orientation::Rotate270,
            _ => Orientation::Normal,
        },
    };
    Ok(RawPhoto {
        make: img.clean_make.clone(),
        model: img.clean_model.clone(),
        sensor_width: w,
        sensor_height: h,
        crop,
        mosaic,
        cfa,
        black: img.blacklevel.as_bayer_array(),
        white: img.whitelevel.as_bayer_array(),
        wb,
        xyz_to_cam,
        orientation,
        lens: lens::read(path).map(std::sync::Arc::new),
    })
}

/// Run the full pipeline: normalize+WB -> demosaic -> (resize) -> colour+tone+8-bit -> orient.
///
/// Per-pixel stages are fused into as few parallel passes as is reasonable; each stage is
/// still a small named function in [`pipeline`] (the future WGSL kernels).
pub fn develop(raw: &RawPhoto, opts: &DevelopOptions) -> RgbImage {
    use pipeline::*;
    use rayon::prelude::*;
    if let Some(d) = opts.long_edge.and_then(|t| Demosaiced::new(raw, t)) {
        return d.finish(&opts.look, &opts.recipe, opts.analysis.as_deref());
    }
    let (cw, ch) = raw.crop_size();
    let adjust = opts.recipe.render_adjust();
    if opts.recipe.needs_masks() || effects::active(&adjust) {
        // Masks and neighbourhood effects need the whole image at once.
        let rgb = demosaic_bilinear(&normalize_white_balance(raw), cw, ch, raw.cfa);
        let d = Demosaiced {
            width: cw,
            height: ch,
            rgb,
            camera_to_srgb: camera_to_srgb_matrix(raw.xyz_to_cam),
            orientation: raw.orientation,
            scale: 1.0,
            lens: lens::Lens::new(raw.lens.clone()),
        };
        return d.finish(&opts.look, &opts.recipe, opts.analysis.as_deref());
    }
    // Full resolution: demosaic one row at a time straight into colour+tone+quantize.
    let fin = Finish::new(camera_to_srgb_matrix(raw.xyz_to_cam), &opts.look, &adjust);
    let mosaic = normalize_white_balance(raw); // stages 1+2
    let mut data = vec![0u8; cw * ch * 3];
    let profile = raw.lens.as_ref().map(|p| (p, lens::which(p, &adjust)));
    if let Some((p, which)) = profile.filter(|(_, w)| w.any()) {
        // Lens corrections move pixels, so they need the whole demosaiced image; each
        // corrected row then goes straight into colour and tone.
        let rgb = demosaic_bilinear(&mosaic, cw, ch, raw.cfa); // stage 3
        drop(mosaic);
        let warp = lens::Warp::new(cw, ch, p, which);
        data.par_chunks_mut(cw * 3)
            .enumerate()
            .with_min_len(8)
            .for_each_init(
                || vec![0f32; cw * 3],
                |scratch, (y, orow)| {
                    warp.row(&rgb, y, scratch);
                    color_tone_row(scratch, &fin, orow); // stages 4-7
                },
            );
        return orient(data, cw, ch, raw.orientation); // stage 8
    }
    data.par_chunks_mut(cw * 3)
        .enumerate()
        .with_min_len(8)
        .for_each_init(
            || vec![0f32; cw * 3],
            |scratch, (y, orow)| {
                demosaic_bilinear_row(&mosaic, cw, ch, raw.cfa, y, scratch); // stage 3
                color_tone_row(scratch, &fin, orow); // stages 4-7
            },
        );
    orient(data, cw, ch, raw.orientation) // stage 8
}

/// A RAW developed through stages 1-3 at a reduced size: white-balanced camera RGB,
/// demosaiced and resized, not yet coloured or toned. A Recipe only touches the later
/// stages, so [`Demosaiced::finish`] can re-render an edit without decoding the RAW again
/// (the Edit screen's sliders; see [`render::render_photo`]).
pub struct Demosaiced {
    pub width: usize,
    pub height: usize,
    /// Interleaved linear camera RGB, `width * height * 3`.
    pub rgb: Vec<f32>,
    pub camera_to_srgb: [[f32; 3]; 3],
    pub orientation: Orientation,
    /// Pixels of this image per pixel of the RAW, for size-dependent effects.
    pub scale: f32,
    /// The camera's lens corrections, applied in [`Demosaiced::finish`].
    pub lens: lens::Lens,
}

impl Demosaiced {
    /// Stages 1-3 with the long edge at most `long_edge` (before orientation). None when
    /// the RAW is no bigger than that: full size goes through [`develop`]'s streaming path.
    pub fn new(raw: &RawPhoto, long_edge: usize) -> Option<Self> {
        use pipeline::*;
        let (cw, ch) = raw.crop_size();
        let t = long_edge.max(1);
        if cw.max(ch) <= t {
            return None;
        }
        let (mut rgb, mut w, mut h) = if t * 2 <= cw.max(ch) {
            normalize_demosaic_half(raw) // stages 1-3 (fast path)
        } else {
            let rgb = demosaic_bilinear(&normalize_white_balance(raw), cw, ch, raw.cfa);
            (rgb, cw, ch)
        };
        if w.max(h) > t {
            (rgb, w, h) = resize_area(&rgb, w, h, t);
        }
        Some(Self {
            width: w,
            height: h,
            rgb,
            camera_to_srgb: camera_to_srgb_matrix(raw.xyz_to_cam),
            orientation: raw.orientation,
            scale: w.max(h) as f32 / cw.max(ch) as f32,
            lens: lens::Lens::new(raw.lens.clone()),
        })
    }

    /// Stages 4-8: colour, tone, the Recipe, 8-bit, upright. A Recipe with Masks goes
    /// through [`local::finish`] on the upright image, with the Photo's `analysis` when the
    /// caller has it cached.
    pub fn finish(
        &self,
        look: &pipeline::Look,
        recipe: &recipe::Recipe,
        analysis: Option<&masks::Analysis>,
    ) -> RgbImage {
        use pipeline::*;
        let adjust = recipe.render_adjust();
        // Lens corrections first, on the frame as the sensor saw it.
        let corrected = self
            .lens
            .corrected(&self.rgb, self.width, self.height, &adjust);
        let base: &[f32] = corrected.as_deref().map_or(&self.rgb, |v| v);
        if recipe.needs_masks() {
            let (rgb, w, h) =
                orient_pixels(base.to_vec(), self.width, self.height, self.orientation);
            let data = local::finish(
                &rgb,
                (w, h, self.scale),
                self.camera_to_srgb,
                look,
                recipe,
                analysis,
            );
            return RgbImage {
                width: w,
                height: h,
                data,
            };
        }
        let fin = Finish::new(self.camera_to_srgb, look, &adjust);
        let data = finish_pixels(base, self.width, self.height, &fin, &adjust, self.scale);
        orient(data, self.width, self.height, self.orientation)
    }

    /// Heap size, for caches.
    pub fn bytes(&self) -> usize {
        self.rgb.len() * std::mem::size_of::<f32>() + self.lens.bytes()
    }
}

/// Stages 4-7 over a whole image, then the neighbourhood effects ([`effects::apply`]) when
/// the Recipe has any, to 8-bit. `scale` is pixels per pixel of the original.
fn finish_pixels(
    rgb: &[f32],
    w: usize,
    h: usize,
    fin: &pipeline::Finish,
    adjust: &recipe::Adjust,
    scale: f32,
) -> Vec<u8> {
    use rayon::prelude::*;
    if !effects::active(adjust) {
        return pipeline::color_tone(rgb, w, fin);
    }
    let mut enc: Vec<f32> = rgb
        .par_chunks(3)
        .flat_map_iter(|p| fin.px([p[0], p[1], p[2]]))
        .collect();
    effects::apply(&mut enc, w, h, adjust, scale);
    enc.into_par_iter().map(pipeline::to8).collect()
}

/// Linear sRGB f32 image (before exposure, tone and saturation), oriented upright.
pub struct LinearImage {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

/// Develop to linear sRGB (stages 1-4 plus orientation) with the long edge at most
/// `long_edge`. Used by `argus fit-look` so a [`pipeline::Look`] can be evaluated many
/// times without re-demosaicing; apply a Look with [`pipeline::Finish`] and
/// [`pipeline::color_tone`] using an identity matrix.
pub fn develop_linear(raw: &RawPhoto, long_edge: usize) -> LinearImage {
    use pipeline::*;
    let (cw, ch) = raw.crop_size();
    let m = camera_to_srgb_matrix(raw.xyz_to_cam);
    let (mut rgb, mut w, mut h) = if long_edge * 2 <= cw.max(ch) {
        normalize_demosaic_half(raw)
    } else {
        (
            demosaic_bilinear(&normalize_white_balance(raw), cw, ch, raw.cfa),
            cw,
            ch,
        )
    };
    if w.max(h) > long_edge {
        (rgb, w, h) = resize_area(&rgb, w, h, long_edge);
    }
    apply_matrix(&mut rgb, &m);
    let (data, width, height) = orient_pixels(rgb, w, h, raw.orientation);
    LinearImage {
        width,
        height,
        data,
    }
}

/// Apply a Recipe to an image that is already rendered (a JPEG Photo or a preview):
/// decode sRGB to linear, then the same stages 4-7 as [`develop`] with the neutral Look,
/// so an empty Recipe returns the image unchanged.
pub fn develop_rgb8(img: &RgbImage, recipe: &recipe::Recipe) -> RgbImage {
    develop_rgb8_with(img, recipe, None)
}

/// [`develop_rgb8`] with the Photo's cached [`masks::Analysis`] for its AI Masks.
pub fn develop_rgb8_with(
    img: &RgbImage,
    recipe: &recipe::Recipe,
    analysis: Option<&masks::Analysis>,
) -> RgbImage {
    develop_rgb8_scaled(img, recipe, analysis, 1.0)
}

/// [`develop_rgb8_with`] for an image shown at `scale` pixels per pixel of its original.
pub fn develop_rgb8_scaled(
    img: &RgbImage,
    recipe: &recipe::Recipe,
    analysis: Option<&masks::Analysis>,
    scale: f32,
) -> RgbImage {
    use pipeline::*;
    let lin: Vec<f32> = {
        let table: Vec<f32> = (0..256).map(|v| srgb_decode(v as f32 / 255.0)).collect();
        img.data.iter().map(|&v| table[v as usize]).collect()
    };
    let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    if recipe.needs_masks() {
        let data = local::finish(
            &lin,
            (img.width, img.height, scale),
            identity,
            &Look::NEUTRAL,
            recipe,
            analysis,
        );
        return RgbImage {
            width: img.width,
            height: img.height,
            data,
        };
    }
    let adjust = recipe.render_adjust();
    let fin = Finish::new(identity, &Look::NEUTRAL, &adjust);
    RgbImage {
        width: img.width,
        height: img.height,
        data: finish_pixels(&lin, img.width, img.height, &fin, &adjust, scale),
    }
}

#[cfg(test)]
mod exif_orientation_tests {
    use super::*;

    #[test]
    fn exif_values_map_to_upright_rotation() {
        assert_eq!(orientation_from_exif(1), Orientation::Normal);
        assert_eq!(orientation_from_exif(3), Orientation::Rotate180);
        assert_eq!(orientation_from_exif(6), Orientation::Rotate90);
        assert_eq!(orientation_from_exif(8), Orientation::Rotate270);
    }
}
