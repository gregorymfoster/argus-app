//! A sanity check of a TIFF-based RAW's headers (ARW, DNG, NEF, CR2) before rawler reads it.
//!
//! rawler 0.8 allocates every header entry's values straight from the count the file claims
//! (`vec![0; count]` in `Entry::parse`). One corrupt byte in a header can claim gigabytes,
//! and a failed allocation aborts the whole process, which `catch_unwind` can't stop: in the
//! app, where the engine runs in-process, one bad file on a card would take Argus down. So
//! the IFDs rawler walks are read here first, and a file with an entry larger than the file
//! itself, or an IFD running past its end, fails as that one Photo with a plain error.

use crate::{Error, Result};
use std::fs::File;
use std::path::Path;

/// IFDs walked at most, so a looping or exploding chain of offsets can't stall the check.
const MAX_IFDS: usize = 64;

/// Bytes per value of each TIFF field type (unknown types count as one byte each).
fn type_size(t: u16) -> u64 {
    match t {
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 | 16..=18 => 8,
        _ => 1,
    }
}

struct Reader {
    file: File,
    len: u64,
    le: bool,
}

impl Reader {
    fn bytes<const N: usize>(&self, at: u64) -> Option<[u8; N]> {
        use std::os::unix::fs::FileExt;
        let mut b = [0u8; N];
        (at + N as u64 <= self.len && self.file.read_exact_at(&mut b, at).is_ok()).then_some(b)
    }
    fn u16(&self, at: u64) -> Option<u16> {
        let b = self.bytes::<2>(at)?;
        Some(if self.le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    }
    fn u32(&self, at: u64) -> Option<u32> {
        let b = self.bytes::<4>(at)?;
        Some(if self.le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }
}

/// Ok when the file isn't TIFF-based (rawler's other readers are not this one's business)
/// or every IFD rawler would walk fits inside the file.
pub fn check(path: &Path) -> Result<()> {
    let file = File::open(path).map_err(|e| Error(format!("read {}: {e}", path.display())))?;
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut r = Reader {
        file,
        len,
        le: true,
    };
    let le = match r.bytes::<2>(0) {
        Some(m) if &m == b"II" => true,
        Some(m) if &m == b"MM" => false,
        _ => return Ok(()),
    };
    r.le = le;
    let corrupt = |what: String| Err(Error(format!("corrupt RAW file: {what}")));
    let mut stack = vec![r.u32(4).unwrap_or(0) as u64];
    let mut seen = Vec::new();
    while let Some(ifd) = stack.pop() {
        if ifd == 0 || seen.contains(&ifd) {
            continue;
        }
        if seen.len() >= MAX_IFDS {
            return corrupt(format!("more than {MAX_IFDS} header blocks"));
        }
        seen.push(ifd);
        let Some(n) = r.u16(ifd) else {
            return corrupt(format!(
                "a header block at {ifd} lies past the end ({len} bytes)"
            ));
        };
        if ifd + 2 + n as u64 * 12 > len {
            return corrupt(format!(
                "a header block at {ifd} runs past the end ({len} bytes)"
            ));
        }
        for i in 0..n as u64 {
            let e = ifd + 2 + i * 12;
            let (Some(tag), Some(typ), Some(count)) = (r.u16(e), r.u16(e + 2), r.u32(e + 4)) else {
                return corrupt(format!("a header entry at {e} is cut short"));
            };
            let bytes = count as u64 * type_size(typ);
            if bytes > len {
                return corrupt(format!(
                    "tag {tag:#06x} claims {bytes} bytes in a {len}-byte file"
                ));
            }
            // The IFDs rawler follows: SubIFDs, Exif, GPS and Interoperability.
            if matches!(tag, 0x014A | 0x8769 | 0x8825 | 0xA005) || typ == 13 {
                let at = if bytes <= 4 {
                    e + 8
                } else {
                    r.u32(e + 8).unwrap_or(0) as u64
                };
                for k in 0..count.min(MAX_IFDS as u32) as u64 {
                    stack.extend(r.u32(at + k * 4).map(u64::from));
                }
            }
        }
        stack.extend(r.u32(ifd + 2 + n as u64 * 12).map(u64::from));
    }
    Ok(())
}
