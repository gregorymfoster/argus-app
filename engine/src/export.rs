//! Export: Photos out of Argus as files, laid out like Lightroom's Export dialog (file
//! settings, image sizing, file naming, metadata, location), simplified.
//!
//! - **Format**: JPEG (quality 50..100), TIFF (8-bit, uncompressed), or Original: a copy of
//!   the untouched original with an XMP sidecar holding its Recipe and Verdict, so the work
//!   travels to Lightroom (Lightroom reads `<name>.xmp` next to a RAW when it imports it).
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
        Format::Jpeg | Format::Tiff => {
            let img =
                render::render_photo(item.photo, item.recipe, opts.long_edge, item.masks.clone())?;
            let ext = if opts.format == Format::Jpeg {
                "jpg"
            } else {
                "tif"
            };
            let path = claim(folder, &stem_of(ext), &[ext])?.remove(0);
            if same_file(&path, original) {
                return Err(Error("refusing to write over an original".into()));
            }
            let data = if opts.format == Format::Jpeg {
                let jpeg = render::encode_jpeg(&img, opts.quality.clamp(50, 100))?;
                with_exif(&jpeg, &exif(item))
            } else {
                encode_tiff(&img)?
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
    format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Argus">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    xmp:CreatorTool="Argus"{rating}{attrs}/>
 </rdf:RDF>
</x:xmpmeta>
"#
    )
}
