//! ICC colour profiles for exported files. Only Display P3 is built here (sRGB files carry
//! no profile, which every viewer reads as sRGB). The profile is ICC v4 like Apple's own:
//! D50 colorants of the P3 primaries with a D65 white, and the sRGB transfer curve.

/// s15Fixed16Number, big-endian.
fn s15(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

fn xyz_tag(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut t = b"XYZ \0\0\0\0".to_vec();
    for v in [x, y, z] {
        t.extend_from_slice(&s15(v));
    }
    t
}

/// multiLocalizedUnicodeType with one en-US string.
fn mluc(text: &str) -> Vec<u8> {
    let utf16: Vec<u8> = text.encode_utf16().flat_map(|c| c.to_be_bytes()).collect();
    let mut t = b"mluc\0\0\0\0".to_vec();
    t.extend_from_slice(&1u32.to_be_bytes()); // records
    t.extend_from_slice(&12u32.to_be_bytes()); // record size
    t.extend_from_slice(b"enUS");
    t.extend_from_slice(&(utf16.len() as u32).to_be_bytes());
    t.extend_from_slice(&28u32.to_be_bytes()); // offset of the string from the tag start
    t.extend_from_slice(&utf16);
    t
}

/// The sRGB curve as parametricCurveType function 3.
fn srgb_trc() -> Vec<u8> {
    let mut t = b"para\0\0\0\0".to_vec();
    t.extend_from_slice(&3u16.to_be_bytes());
    t.extend_from_slice(&[0, 0]);
    for v in [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045] {
        t.extend_from_slice(&s15(v));
    }
    t
}

fn chad() -> Vec<u8> {
    // Bradford D65 -> D50.
    let m = [
        [1.047_882, 0.022_918_7, -0.050_192_2],
        [0.029_581_5, 0.990_484, -0.017_073_8],
        [-0.009_234_5, 0.015_043_6, 0.752_131],
    ];
    let mut t = b"sf32\0\0\0\0".to_vec();
    for row in m {
        for v in row {
            t.extend_from_slice(&s15(v));
        }
    }
    t
}

/// The Display P3 profile, as written into P3 exports.
pub fn display_p3() -> Vec<u8> {
    let trc = srgb_trc();
    let tags: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"desc", mluc("Display P3")),
        (*b"cprt", mluc("No copyright, use freely")),
        (*b"wtpt", xyz_tag(0.964_2, 1.0, 0.824_9)),
        (*b"rXYZ", xyz_tag(0.515_1, 0.241_2, -0.001_1)),
        (*b"gXYZ", xyz_tag(0.292_0, 0.692_2, 0.041_9)),
        (*b"bXYZ", xyz_tag(0.157_1, 0.066_6, 0.784_1)),
        (*b"rTRC", trc.clone()),
        (*b"gTRC", trc.clone()),
        (*b"bTRC", trc),
        (*b"chad", chad()),
    ];
    let table = 4 + 12 * tags.len();
    let mut body = Vec::new();
    let mut entries = Vec::new();
    for (sig, data) in &tags {
        while body.len() % 4 != 0 {
            body.push(0);
        }
        entries.push((*sig, 128 + table + body.len(), data.len()));
        body.extend_from_slice(data);
    }
    while body.len() % 4 != 0 {
        body.push(0);
    }
    let size = 128 + table + body.len();
    let mut p = Vec::with_capacity(size);
    p.extend_from_slice(&(size as u32).to_be_bytes());
    p.extend_from_slice(b"\0\0\0\0"); // preferred CMM
    p.extend_from_slice(&[4, 0x40, 0, 0]); // v4.4
    p.extend_from_slice(b"mntrRGB XYZ ");
    p.extend_from_slice(&[0x07, 0xEA, 0, 11, 0, 2, 0, 0, 0, 0, 0, 0]); // 2026-11-02
    p.extend_from_slice(b"acspAPPL");
    p.extend_from_slice(&[0; 4 + 4 + 4 + 8 + 4]); // flags, maker, model, attributes, intent
    for v in [0.964_2, 1.0, 0.824_9] {
        p.extend_from_slice(&s15(v)); // PCS illuminant
    }
    p.extend_from_slice(b"ARGS"); // creator
    p.extend_from_slice(&[0; 16 + 28]); // profile ID, reserved
    debug_assert_eq!(p.len(), 128);
    p.extend_from_slice(&(tags.len() as u32).to_be_bytes());
    for (sig, off, len) in entries {
        p.extend_from_slice(&sig);
        p.extend_from_slice(&(off as u32).to_be_bytes());
        p.extend_from_slice(&(len as u32).to_be_bytes());
    }
    p.extend_from_slice(&body);
    p
}

/// Insert `profile` as an APP2 ICC_PROFILE segment right after a JPEG's SOI marker.
pub fn with_icc(jpeg: &[u8], profile: &[u8]) -> Vec<u8> {
    let len = profile.len() + 16;
    if len > u16::MAX as usize || !jpeg.starts_with(&[0xFF, 0xD8]) {
        return jpeg.to_vec();
    }
    let mut out = Vec::with_capacity(jpeg.len() + len + 2);
    out.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xE2]);
    out.extend_from_slice(&(len as u16).to_be_bytes());
    out.extend_from_slice(b"ICC_PROFILE\0");
    out.extend_from_slice(&[1, 1]);
    out.extend_from_slice(profile);
    out.extend_from_slice(&jpeg[2..]);
    out
}
