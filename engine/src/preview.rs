//! Extract the camera's largest embedded JPEG, byte-for-byte.
//!
//! rawler only exposes decoded previews, so we walk the TIFF IFD tree ourselves
//! (IFD0, chained IFDs and SubIFDs) looking for JPEGInterchangeFormat(+Length) pairs.

use crate::{Error, Result};
use std::path::Path;

struct Tiff<'a> {
    b: &'a [u8],
    le: bool,
}

impl Tiff<'_> {
    fn u16(&self, o: usize) -> Option<u16> {
        let s = self.b.get(o..o + 2)?;
        Some(if self.le {
            u16::from_le_bytes([s[0], s[1]])
        } else {
            u16::from_be_bytes([s[0], s[1]])
        })
    }
    fn u32(&self, o: usize) -> Option<u32> {
        let s = self.b.get(o..o + 4)?;
        let a = [s[0], s[1], s[2], s[3]];
        Some(if self.le {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        })
    }
}

fn collect(t: &Tiff, ifd: usize, depth: u32, out: &mut Vec<(usize, usize)>, seen: &mut Vec<usize>) {
    if depth > 4 || ifd == 0 || seen.contains(&ifd) {
        return;
    }
    seen.push(ifd);
    let Some(n) = t.u16(ifd) else { return };
    let (mut off, mut len) = (None, None);
    for i in 0..n as usize {
        let e = ifd + 2 + i * 12;
        let (Some(tag), Some(typ), Some(count)) = (t.u16(e), t.u16(e + 2), t.u32(e + 4)) else {
            return;
        };
        let val = if typ == 3 && count == 1 {
            t.u16(e + 8).map(|v| v as u32)
        } else {
            t.u32(e + 8)
        };
        match tag {
            0x0201 => off = val,
            0x0202 => len = val,
            0x014A => {
                // SubIFDs: array of u32 offsets (inline if count == 1)
                let base = if count == 1 {
                    e + 8
                } else {
                    t.u32(e + 8).unwrap_or(0) as usize
                };
                for k in 0..count as usize {
                    if let Some(o) = t.u32(base + k * 4) {
                        collect(t, o as usize, depth + 1, out, seen);
                    }
                }
            }
            _ => {}
        }
    }
    if let (Some(o), Some(l)) = (off, len) {
        out.push((o as usize, l as usize));
    }
    if let Some(next) = t.u32(ifd + 2 + n as usize * 12) {
        collect(t, next as usize, depth + 1, out, seen);
    }
}

/// DefaultCropSize (tag 0xC620) from the first IFD that has it: the image size inside the
/// sensor's margins, which rawler uses as the crop for files that carry it (A7R V ARWs).
pub fn tiff_default_crop_size(b: &[u8]) -> Option<(usize, usize)> {
    let le = match b.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let t = Tiff { b, le };
    let mut stack = vec![t.u32(4)? as usize];
    let mut seen = Vec::new();
    while let Some(ifd) = stack.pop() {
        if ifd == 0 || seen.contains(&ifd) || seen.len() > 32 {
            continue;
        }
        seen.push(ifd);
        let n = t.u16(ifd)? as usize;
        for i in 0..n {
            let e = ifd + 2 + i * 12;
            let (tag, typ, count) = (t.u16(e)?, t.u16(e + 2)?, t.u32(e + 4)?);
            match (tag, typ) {
                (0xC620, 3) if count == 2 => {
                    return Some((t.u16(e + 8)? as usize, t.u16(e + 10)? as usize))
                }
                (0xC620, 4) if count == 2 => {
                    let o = t.u32(e + 8)? as usize;
                    return Some((t.u32(o)? as usize, t.u32(o + 4)? as usize));
                }
                (0x014A, _) => {
                    let base = if count == 1 {
                        e + 8
                    } else {
                        t.u32(e + 8)? as usize
                    };
                    for k in 0..count as usize {
                        stack.extend(t.u32(base + k * 4).map(|o| o as usize));
                    }
                }
                _ => {}
            }
        }
        stack.extend(t.u32(ifd + 2 + n * 12).map(|o| o as usize));
    }
    None
}

/// Read the EXIF Orientation (IFD0 tag 0x0112) from a TIFF-based file's header bytes.
///
/// Returns 1-8, or `None` if the tag is absent or the header is not TIFF. rawler 0.8 reports
/// `Normal` for every Sony ARW, so this tag is the authority for orientation.
pub fn tiff_orientation(b: &[u8]) -> Option<u16> {
    let le = match b.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let t = Tiff { b, le };
    let ifd = t.u32(4)? as usize;
    let n = t.u16(ifd)? as usize;
    for i in 0..n {
        let e = ifd + 2 + i * 12;
        if t.u16(e)? == 0x0112 {
            // SHORT, count 1: value is stored inline in the first two bytes of the value field.
            let v = t.u16(e + 8)?;
            return (1..=8).contains(&v).then_some(v);
        }
    }
    None
}

/// Read the EXIF orientation from the start of a file without loading all of it.
pub fn file_orientation(path: &Path) -> Option<u16> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(1 << 20)
        .read_to_end(&mut buf)
        .ok()?;
    tiff_orientation(&buf)
}

/// Returns the largest embedded JPEG (by byte length) as stored in the file.
pub fn embedded_preview(path: &Path) -> Result<Vec<u8>> {
    let b = std::fs::read(path).map_err(|e| Error(format!("read {}: {e}", path.display())))?;
    largest_jpeg(&b)
}

/// Like [`embedded_preview`], but tries the first few MB of the file first: cameras store
/// their previews near the start, and a card reader is slow to read a whole RAW.
pub fn embedded_preview_quick(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut head = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(4 << 20).read_to_end(&mut head))
        .map_err(|e| Error(format!("read {}: {e}", path.display())))?;
    largest_jpeg(&head).or_else(|_| embedded_preview(path))
}

/// The smallest embedded JPEG whose long edge is at least `min_edge`, as stored: what
/// Import shows until Argus's own render is ready. An A7R V file carries 160 px, 1616 px
/// and full-size (9504 px) JPEGs; for a 1024 px floor this picks the 1616 px one, which
/// decodes in milliseconds where the full-size one takes longer than developing the RAW.
pub fn embedded_preview_at_least(b: &[u8], min_edge: usize) -> Option<&[u8]> {
    embedded_jpegs(b)
        .into_iter()
        .filter_map(|j| Some((jpeg_dimensions(j)?, j)))
        .filter(|&((w, h), _)| w.max(h) >= min_edge)
        .min_by_key(|&((w, h), _)| w * h)
        .map(|(_, j)| j)
}

/// Every embedded JPEG in a TIFF-based RAW (IFD0, chained IFDs and SubIFDs).
fn embedded_jpegs(b: &[u8]) -> Vec<&[u8]> {
    let le = match b.get(0..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return Vec::new(),
    };
    let t = Tiff { b, le };
    let mut found = Vec::new();
    collect(
        &t,
        t.u32(4).unwrap_or(0) as usize,
        0,
        &mut found,
        &mut Vec::new(),
    );
    found
        .into_iter()
        .filter(|&(o, l)| l > 4 && b.get(o..o + 2) == Some(&[0xFF, 0xD8]) && o + l <= b.len())
        .map(|(o, l)| &b[o..o + l])
        .collect()
}

fn largest_jpeg(b: &[u8]) -> Result<Vec<u8>> {
    if b.len() < 8 {
        return Err(Error("file too small".into()));
    }
    if !matches!(&b[0..2], b"II" | b"MM") {
        return Err(Error("not a TIFF-based RAW".into()));
    }
    embedded_jpegs(b)
        .into_iter()
        .max_by_key(|j| j.len())
        .map(<[u8]>::to_vec)
        .ok_or_else(|| Error("no embedded JPEG found".into()))
}

/// Parse JPEG dimensions from the first SOF marker.
pub fn jpeg_dimensions(j: &[u8]) -> Option<(usize, usize)> {
    let mut i = 2;
    while i + 9 < j.len() {
        if j[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = j[i + 1];
        if (0xC0..=0xCF).contains(&m) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
            let h = u16::from_be_bytes([j[i + 5], j[i + 6]]) as usize;
            let w = u16::from_be_bytes([j[i + 7], j[i + 8]]) as usize;
            return Some((w, h));
        }
        if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) || m == 0xFF {
            i += if m == 0xFF { 1 } else { 2 };
            continue;
        }
        i += 2 + u16::from_be_bytes([j[i + 2], j[i + 3]]) as usize;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal TIFF: header + IFD0 with one entry (Orientation) unless `value` is None,
    /// in which case the single entry is ImageWidth (0x0100).
    fn tiff(le: bool, value: Option<u16>) -> Vec<u8> {
        let w16 = |v: u16| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        let w32 = |v: u32| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        let mut b = Vec::new();
        b.extend_from_slice(if le { b"II" } else { b"MM" });
        b.extend_from_slice(&w16(42));
        b.extend_from_slice(&w32(8));
        b.extend_from_slice(&w16(1));
        b.extend_from_slice(&w16(if value.is_some() { 0x0112 } else { 0x0100 }));
        b.extend_from_slice(&w16(3));
        b.extend_from_slice(&w32(1));
        b.extend_from_slice(&w16(value.unwrap_or(7)));
        b.extend_from_slice(&[0, 0]);
        b.extend_from_slice(&w32(0));
        b
    }

    #[test]
    fn reads_orientation_in_both_byte_orders() {
        for le in [true, false] {
            for v in [1u16, 6, 8] {
                assert_eq!(
                    tiff_orientation(&tiff(le, Some(v))),
                    Some(v),
                    "le={le} v={v}"
                );
            }
            assert_eq!(tiff_orientation(&tiff(le, None)), None, "le={le} missing");
        }
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(tiff_orientation(b"nope"), None);
        assert_eq!(tiff_orientation(&tiff(true, Some(9))), None);
        assert_eq!(tiff_orientation(&tiff(true, Some(8))[..12]), None);
    }
}
