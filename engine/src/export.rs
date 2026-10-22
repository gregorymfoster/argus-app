//! Export: Photos out of Argus as files, laid out like Lightroom's Export dialog (file
//! settings, image sizing, file naming, metadata, location), simplified.
//!
//! - **Format**: JPEG (quality 50..100), HEIC (macOS only: the system's own encoder, through
//!   `sips`, at the same quality), TIFF (8-bit, uncompressed), or Original: a copy of
//!   the untouched original with an XMP sidecar holding its Recipe and Verdict, so the work
//!   travels to Lightroom (Lightroom reads `<name>.xmp` next to a RAW when it imports it).
//! - **Size**: full, or a long edge, a short edge (Instagram's 1080) or megapixels, measured
//!   on the cropped Photo. **Output sharpening** for screen (low, standard, high), as in
//!   Lightroom's Export, sharpens the resized pixels a little so they don't look soft.
//! - **Naming** follows docs/naming.md (no spaces, ever): `shoot` (the default) gives
//!   `2026-10-18_Boat-Trip_DSC08005.jpg`, `sequence` gives `2026-10-18_Boat-Trip_001.jpg`,
//!   `original` keeps `DSC08005.jpg`. Files go in a folder named after the Shoot under
//!   the chosen location unless `shoot_folder` is off.
//! - **Metadata**: a JPEG keeps the original's capture metadata (camera, lens, date,
//!   exposure, copyright) and never its location, serial numbers or maker notes.
//!
//! Nothing is ever overwritten: a name that exists gets `-2`, `-3`. Each file is written
//! under a temporary name and renamed into place, so an interrupted export never leaves a
//! half-written file under a real name.

use crate::library::{Photo, Verdict};
use crate::recipe::{Adjust, Recipe};
use crate::{naming, render, Error, Result};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Jpeg,
    /// HEIC through macOS's own encoder; a JPEG elsewhere is the alternative.
    Heic,
    Tiff,
    /// The original file, untouched, plus an XMP sidecar.
    Original,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Naming {
    /// `<Shoot>_<original name>`.
    #[default]
    Shoot,
    /// `<Shoot>_<001>`, numbered from `sequence_start`.
    Sequence,
    /// The original's own name.
    Original,
}

/// Lightroom's Output sharpening for screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sharpen {
    Low,
    Standard,
    High,
}

impl Sharpen {
    fn amount(self) -> f32 {
        match self {
            Sharpen::Low => 0.35,
            Sharpen::Standard => 0.7,
            Sharpen::High => 1.1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    /// The export location the person chose.
    pub folder: PathBuf,
    /// Put each Photo in a folder named after its Shoot under `folder` (Lightroom's "Put
    /// in Subfolder"; docs/naming.md).
    pub shoot_folder: bool,
    pub format: Format,
    /// Long edge in pixels; None is full size. Ignored for Original.
    pub long_edge: Option<usize>,
    /// Short edge in pixels, in place of `long_edge` (Instagram's 1080).
    pub short_edge: Option<usize>,
    /// Megapixels, in place of `long_edge`.
    pub megapixels: Option<f64>,
    /// Output sharpening for screen; None is off.
    pub sharpen: Option<Sharpen>,
    /// JPEG quality, 50..100.
    pub quality: u8,
    pub naming: Naming,
    /// The number of the first Photo in a `sequence` (callers exporting one Photo per call
    /// pass its position).
    pub sequence_start: usize,
}

/// One Photo to export, with what Export needs to know about it.
pub struct Item<'a> {
    pub photo: &'a Photo,
    /// The Recipe as rendered: the Photo's own, with its Look folded in.
    pub recipe: &'a Recipe,
    pub verdict: Verdict,
    pub shoot_name: &'a str,
    /// The Photo's Mask analysis, when the Recipe has AI Masks ([`render::masks_for`]).
    pub masks: Option<std::sync::Arc<crate::masks::Analysis>>,
    /// The Keywords the Photo keeps, written as `dc:subject` and `lr:hierarchicalSubject`.
    pub keywords: Vec<crate::naming::Keyword>,
    /// The caption the Photo keeps, written as `dc:description` (IPTC Description).
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Exported {
    pub photo_id: i64,
    pub path: PathBuf,
    /// The XMP sidecar, for the Original format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidecar: Option<PathBuf>,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

/// `~/x` -> `$HOME/x`, so callers can pass the folder as the UI shows it.
pub fn expand_home(folder: &str) -> PathBuf {
    match (folder.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(folder),
    }
}

/// A Shoot's name as a file-name part (docs/naming.md): `2026-10-18_Boat-Trip`. Shoots
/// named before those rules (`Boat trip`) get their first frame's date and lose spaces.
pub fn shoot_part(shoot_name: &str, captured_at: &str) -> String {
    let parts = naming::parse_shoot_name(shoot_name);
    match parts.date.as_deref().or(naming::capture_date(captured_at)) {
        Some(date) => naming::shoot_name(date, &parts.title),
        None => match naming::slug(&parts.title) {
            s if s.is_empty() => "Argus".into(),
            s => s,
        },
    }
}

/// The file name for the `index`th Photo of an export, with its extension.
pub fn file_name(item: &Item, naming: Naming, index: usize, total: usize, ext: &str) -> String {
    let shoot = || shoot_part(item.shoot_name, &item.photo.captured_at);
    match naming {
        Naming::Original => naming::export_file_name("", &item.photo.file_name, ext),
        Naming::Shoot => naming::export_file_name(&shoot(), &item.photo.file_name, ext),
        Naming::Sequence => {
            let width = total.max(index).to_string().len().max(3);
            format!("{}_{index:0width$}.{ext}", shoot())
        }
    }
}

fn io(path: &Path, e: std::io::Error) -> Error {
    Error(format!("{}: {e}", path.display()))
}

/// Claim free names `<stem>.<ext>` (or `<stem>-N.<ext>`, as `naming::unique_name` counts)
/// for every extension at once, by creating them empty, which is atomic: two exports
/// running side by side never get the same name. Returns the paths in `exts` order.
fn claim(folder: &Path, stem: &str, exts: &[&str]) -> Result<Vec<PathBuf>> {
    'next: for n in 1.. {
        let base = if n == 1 {
            stem.to_string()
        } else {
            format!("{stem}-{n}")
        };
        let mut claimed: Vec<PathBuf> = Vec::new();
        for ext in exts {
            let path = folder.join(format!("{base}.{ext}"));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => claimed.push(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    for p in claimed {
                        let _ = std::fs::remove_file(p);
                    }
                    continue 'next;
                }
                Err(e) => return Err(io(&path, e)),
            }
        }
        return Ok(claimed);
    }
    unreachable!()
}

/// Write `bytes` to the claimed `path` through a temporary file and a rename.
fn put(path: &Path, write: impl FnOnce(&mut File) -> std::io::Result<()>) -> Result<u64> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!(".{name}.argus-tmp"));
    let res = (|| {
        let mut f = File::create(&tmp)?;
        write(&mut f)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)?;
        std::fs::metadata(path).map(|m| m.len())
    })();
    res.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(path);
        io(path, e)
    })
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Export one Photo. `index` and `total` place it in the export, for sequence names.
pub fn export(item: &Item, opts: &Options, index: usize, total: usize) -> Result<Exported> {
    let folder = &if opts.shoot_folder {
        opts.folder.join(naming::export_folder_name(&shoot_part(
            item.shoot_name,
            &item.photo.captured_at,
        )))
    } else {
        opts.folder.clone()
    };
    std::fs::create_dir_all(folder).map_err(|e| io(folder, e))?;
    let original = Path::new(&item.photo.path);
    let stem_of = |ext: &str| {
        let name = file_name(item, opts.naming, index, total, ext);
        name.rsplit_once('.')
            .map_or(name.as_str(), |(stem, _)| stem)
            .to_string()
    };
    match opts.format {
        Format::Original => {
            if !original.exists() {
                return Err(Error(format!(
                    "the original of Photo {} is not reachable at {} (drive offline?)",
                    item.photo.id, item.photo.path
                )));
            }
            let ext = original
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_else(|| "raw".into());
            let paths = claim(folder, &stem_of(&ext), &[&ext, "xmp"])?;
            let (copy, xmp) = (&paths[0], &paths[1]);
            if same_file(copy, original) {
                return Err(Error("refusing to write over an original".into()));
            }
            let bytes = put(copy, |f| {
                std::io::copy(&mut File::open(original)?, f).map(|_| ())
            })?;
            put(xmp, |f| f.write_all(sidecar(item).as_bytes()))?;
            Ok(Exported {
                photo_id: item.photo.id,
                path: copy.clone(),
                sidecar: Some(xmp.clone()),
                width: item.photo.width,
                height: item.photo.height,
                bytes,
            })
        }
        Format::Jpeg | Format::Heic | Format::Tiff => {
            // AI Denoise needs its base; an export waits for it rather than leaving it out.
            if crate::denoise::amount(item.recipe) > 0.0
                && crate::denoise::base_file(item.photo).is_some()
            {
                crate::denoise::compute(item.photo, &|_, _| {})?;
            }
            let mut img = render::render_photo(
                item.photo,
                item.recipe,
                long_edge(item, opts),
                item.masks.clone(),
            )?;
            if let Some(s) = opts.sharpen {
                sharpen(&mut img, s.amount());
            }
            let ext = match opts.format {
                Format::Jpeg => "jpg",
                Format::Heic => "heic",
                _ => "tif",
            };
            let path = claim(folder, &stem_of(ext), &[ext])?.remove(0);
            if same_file(&path, original) {
                return Err(Error("refusing to write over an original".into()));
            }
            let data = match opts.format {
                Format::Tiff => encode_tiff(&img)?,
                _ => {
                    let jpeg = render::encode_jpeg(&img, opts.quality.clamp(50, 100))?;
                    let jpeg = with_xmp(&with_exif(&jpeg, &exif(item)), item);
                    if opts.format == Format::Heic {
                        heic(&jpeg, opts.quality.clamp(50, 100), &path)?
                    } else {
                        jpeg
                    }
                }
            };
            let bytes = put(&path, |f| f.write_all(&data))?;
            Ok(Exported {
                photo_id: item.photo.id,
                path,
                sidecar: None,
                width: img.width as u32,
                height: img.height as u32,
                bytes,
            })
        }
    }
}

/// The long edge to render at for the chosen size, from the cropped Photo's shape.
pub fn long_edge(item: &Item, opts: &Options) -> Option<usize> {
    let (w, h) = (item.photo.width as f64, item.photo.height as f64);
    if w <= 0.0 || h <= 0.0 {
        return opts.long_edge;
    }
    let c = &item.recipe.crop;
    let (w, h) = (w * c.w.clamp(0.01, 1.0), h * c.h.clamp(0.01, 1.0));
    let ratio = w.max(h) / w.min(h);
    let full = w.max(h);
    let want = match (opts.short_edge, opts.megapixels) {
        (Some(s), _) => s as f64 * ratio,
        (None, Some(mp)) if mp > 0.0 => (mp * 1e6 * ratio).sqrt(),
        _ => return opts.long_edge,
    };
    // Never upscale: a size past the Photo's own is full size.
    (want < full).then(|| want.round().max(64.0) as usize)
}

/// Unsharp mask on luminance (a 3x3 blur, so a fine radius at output size), the way
/// Lightroom sharpens for screen: detail gets crisper without colour fringes.
pub fn sharpen(img: &mut crate::RgbImage, amount: f32) {
    let (w, h) = (img.width, img.height);
    if w < 3 || h < 3 {
        return;
    }
    let luma: Vec<f32> = img
        .data
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32)
        .collect();
    const K: [[f32; 3]; 3] = [[1.0, 2.0, 1.0], [2.0, 4.0, 2.0], [1.0, 2.0, 1.0]];
    for y in 0..h {
        for x in 0..w {
            let mut blur = 0.0;
            for (dy, row) in K.iter().enumerate() {
                let yy = (y + dy).saturating_sub(1).min(h - 1);
                for (dx, k) in row.iter().enumerate() {
                    let xx = (x + dx).saturating_sub(1).min(w - 1);
                    blur += k * luma[yy * w + xx];
                }
            }
            let detail = (luma[y * w + x] - blur / 16.0) * amount;
            let i = (y * w + x) * 3;
            for c in &mut img.data[i..i + 3] {
                *c = (*c as f32 + detail).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// A JPEG as HEIC, through macOS's own encoder (`sips`, part of every Mac).
fn heic(jpeg: &[u8], quality: u8, near: &Path) -> Result<Vec<u8>> {
    if !cfg!(target_os = "macos") {
        return Err(Error(
            "HEIC export needs macOS (it uses the Mac's own encoder); choose JPEG here".into(),
        ));
    }
    let dir = near.parent().unwrap_or(Path::new("."));
    let name = near.file_name().unwrap_or_default().to_string_lossy();
    let src = dir.join(format!(".{name}.argus-src.jpg"));
    let out = dir.join(format!(".{name}.argus-heic.heic"));
    let res = (|| {
        std::fs::write(&src, jpeg).map_err(|e| io(&src, e))?;
        let run = std::process::Command::new("/usr/bin/sips")
            .args(["-s", "format", "heic", "-s", "formatOptions"])
            .arg(quality.to_string())
            .arg(&src)
            .arg("--out")
            .arg(&out)
            .output()
            .map_err(|e| Error(format!("HEIC: can't run sips: {e}")))?;
        if !run.status.success() {
            return Err(Error(format!(
                "HEIC: sips failed: {}",
                String::from_utf8_lossy(&run.stderr).trim()
            )));
        }
        std::fs::read(&out).map_err(|e| io(&out, e))
    })();
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&out);
    res
}

fn encode_tiff(img: &crate::RgbImage) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    image::RgbImage::from_raw(img.width as u32, img.height as u32, img.data.clone())
        .ok_or_else(|| Error("tiff: bad image size".into()))?
        .write_to(&mut out, image::ImageFormat::Tiff)
        .map_err(|e| Error(format!("tiff encode: {e}")))?;
    Ok(out.into_inner())
}

/// Capture metadata worth keeping. Location (the GPS IFD), serial numbers and maker
/// notes are never copied.
const KEEP_TAGS: &[exif::Tag] = &[
    exif::Tag::Make,
    exif::Tag::Model,
    exif::Tag::Artist,
    exif::Tag::Copyright,
    exif::Tag::DateTimeOriginal,
    exif::Tag::DateTimeDigitized,
    exif::Tag::OffsetTimeOriginal,
    exif::Tag::OffsetTimeDigitized,
    exif::Tag::ExposureTime,
    exif::Tag::FNumber,
    exif::Tag::ExposureProgram,
    exif::Tag::PhotographicSensitivity,
    exif::Tag::ExposureBiasValue,
    exif::Tag::MeteringMode,
    exif::Tag::Flash,
    exif::Tag::FocalLength,
    exif::Tag::FocalLengthIn35mmFilm,
    exif::Tag::LensMake,
    exif::Tag::LensModel,
];

fn ascii(s: &str) -> exif::Value {
    exif::Value::Ascii(vec![s.as_bytes().to_vec()])
}

/// The EXIF block (TIFF bytes) for an exported JPEG: the kept tags from the original,
/// upright orientation (renders are already upright), and Argus as the software. A
/// Photo whose original has no EXIF still gets its capture time and camera.
fn exif(item: &Item) -> Vec<u8> {
    let mut fields: Vec<exif::Field> = File::open(&item.photo.path)
        .ok()
        .and_then(|f| {
            exif::Reader::new()
                .read_from_container(&mut std::io::BufReader::new(f))
                .ok()
        })
        .map(|e| {
            e.fields()
                .filter(|f| f.ifd_num == exif::In::PRIMARY && KEEP_TAGS.contains(&f.tag))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let has = |fields: &[exif::Field], t| fields.iter().any(|f| f.tag == t);
    if !has(&fields, exif::Tag::DateTimeOriginal) && item.photo.captured_at.len() >= 19 {
        let t = &item.photo.captured_at[..19];
        let exif_time = format!("{} {}", t[..10].replace('-', ":"), &t[11..]);
        fields.push(exif::Field {
            tag: exif::Tag::DateTimeOriginal,
            ifd_num: exif::In::PRIMARY,
            value: ascii(&exif_time),
        });
    }
    if !has(&fields, exif::Tag::Model) && !item.photo.camera.is_empty() {
        fields.push(exif::Field {
            tag: exif::Tag::Model,
            ifd_num: exif::In::PRIMARY,
            value: ascii(&item.photo.camera),
        });
    }
    fields.push(exif::Field {
        tag: exif::Tag::Orientation,
        ifd_num: exif::In::PRIMARY,
        value: exif::Value::Short(vec![1]),
    });
    fields.push(exif::Field {
        tag: exif::Tag::Software,
        ifd_num: exif::In::PRIMARY,
        value: ascii("Argus"),
    });
    let mut w = exif::experimental::Writer::new();
    for f in &fields {
        w.push_field(f);
    }
    let mut out = Cursor::new(Vec::new());
    match w.write(&mut out, false) {
        Ok(()) => out.into_inner(),
        Err(_) => Vec::new(),
    }
}

/// Insert an EXIF APP1 segment right after a JPEG's SOI marker.
fn with_exif(jpeg: &[u8], tiff: &[u8]) -> Vec<u8> {
    let len = tiff.len() + 8;
    if tiff.is_empty() || len > u16::MAX as usize || !jpeg.starts_with(&[0xFF, 0xD8]) {
        return jpeg.to_vec();
    }
    let mut out = Vec::with_capacity(jpeg.len() + len + 2);
    out.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xE1]);
    out.extend_from_slice(&(len as u16).to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(tiff);
    out.extend_from_slice(&jpeg[2..]);
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn signed(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    if v > 0.0 {
        format!("+{s}")
    } else {
        s
    }
}

/// An XMP sidecar Lightroom reads at import: the Basic panel and HSL as Camera Raw
/// settings, and Rejects as rating -1, as Bridge writes it (docs/naming.md; Lightroom
/// keeps Picks in its catalog only, so a Pick carries nothing). Lightroom renders with its
/// own engine, so the result is close to Argus's, not identical. White balance moves only
/// for JPEG originals: Argus's temp and tint are relative, and a RAW's Kelvin needs its
/// as-shot value.
pub fn sidecar(item: &Item) -> String {
    let a: Adjust = item.recipe.render_adjust();
    let mut crs: Vec<(String, String)> = vec![
        ("ProcessVersion".into(), "11.0".into()),
        ("HasSettings".into(), "True".into()),
        ("Exposure2012".into(), signed(a.exposure, 2)),
    ];
    for (name, v) in [
        ("Contrast2012", a.contrast),
        ("Highlights2012", a.highlights),
        ("Shadows2012", a.shadows),
        ("Whites2012", a.whites),
        ("Blacks2012", a.blacks),
        ("Vibrance", a.vibrance),
        ("Saturation", a.saturation),
    ] {
        crs.push((name.into(), signed(v.round(), 0)));
    }
    if item.photo.kind == "jpeg" && (a.temp != 0.0 || a.tint != 0.0) {
        crs.push(("WhiteBalance".into(), "Custom".into()));
        crs.push(("IncrementalTemperature".into(), signed(a.temp.round(), 0)));
        crs.push(("IncrementalTint".into(), signed(a.tint.round(), 0)));
    }
    let lr_bands = [
        "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
    ];
    for (band, s) in lr_bands.iter().zip(a.hsl.bands()) {
        crs.push((format!("HueAdjustment{band}"), signed(s.hue.round(), 0)));
        crs.push((
            format!("SaturationAdjustment{band}"),
            signed(s.sat.round(), 0),
        ));
        crs.push((
            format!("LuminanceAdjustment{band}"),
            signed(s.lum.round(), 0),
        ));
    }
    let attrs: String = crs
        .iter()
        .map(|(k, v)| format!("\n    crs:{k}=\"{}\"", xml_escape(v)))
        .collect();
    let rating = match item.verdict {
        Verdict::Reject => "\n    xmp:Rating=\"-1\"",
        _ => "",
    };
    let tags = xmp_tags(item);
    let close = if tags.is_empty() {
        "/>".to_string()
    } else {
        format!(">{tags}\n  </rdf:Description>")
    };
    format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Argus">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:lr="http://ns.adobe.com/lightroom/1.0/"
    xmp:CreatorTool="Argus"{rating}{attrs}{close}
 </rdf:RDF>
</x:xmpmeta>
"#
    )
}

/// The Keywords and caption as XMP elements (empty when there are none): `dc:subject`
/// with every level (IPTC Keywords), `lr:hierarchicalSubject` with full paths, and
/// `dc:description` (IPTC Description).
fn xmp_tags(item: &Item) -> String {
    let bag = |name: &str, items: &[String]| -> String {
        if items.is_empty() {
            return String::new();
        }
        let li: String = items
            .iter()
            .map(|k| format!("\n     <rdf:li>{}</rdf:li>", xml_escape(k)))
            .collect();
        format!("\n   <{name}>\n    <rdf:Bag>{li}\n    </rdf:Bag>\n   </{name}>")
    };
    let mut out = bag("dc:subject", &crate::naming::flat_keywords(&item.keywords));
    out += &bag(
        "lr:hierarchicalSubject",
        &crate::naming::hierarchical_subject(&item.keywords),
    );
    if let Some(c) = item
        .caption
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        out += &format!(
            "\n   <dc:description>\n    <rdf:Alt>\n     <rdf:li xml:lang=\"x-default\">{}</rdf:li>\n    </rdf:Alt>\n   </dc:description>",
            xml_escape(c)
        );
    }
    out
}

/// The JPEG with an XMP packet of its Keywords and caption after its EXIF, as Lightroom
/// embeds them, so Photos, Finder and stock sites see them. Nothing to say, no packet.
fn with_xmp(jpeg: &[u8], item: &Item) -> Vec<u8> {
    let tags = xmp_tags(item);
    if tags.is_empty() || !jpeg.starts_with(&[0xFF, 0xD8]) {
        return jpeg.to_vec();
    }
    let packet = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Argus\">
 <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">
  <rdf:Description rdf:about=\"\"
    xmlns:dc=\"http://purl.org/dc/elements/1.1/\"
    xmlns:lr=\"http://ns.adobe.com/lightroom/1.0/\">{tags}
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end=\"w\"?>"
    );
    const NS: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
    let len = NS.len() + packet.len() + 2;
    if len > u16::MAX as usize {
        return jpeg.to_vec();
    }
    // After SOI, and after the EXIF APP1 when there is one: EXIF must come first.
    let mut at = 2;
    if jpeg.get(2..4) == Some(&[0xFF, 0xE1][..]) && jpeg.get(6..10) == Some(&b"Exif"[..]) {
        at = 4 + u16::from_be_bytes([jpeg[4], jpeg[5]]) as usize;
    }
    let mut out = Vec::with_capacity(jpeg.len() + len + 2);
    out.extend_from_slice(&jpeg[..at]);
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&(len as u16).to_be_bytes());
    out.extend_from_slice(NS);
    out.extend_from_slice(packet.as_bytes());
    out.extend_from_slice(&jpeg[at..]);
    out
}
