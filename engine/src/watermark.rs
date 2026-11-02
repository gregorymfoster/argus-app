//! Text watermarks for rendered exports (Lightroom's Watermark: a line such as "© Greg
//! Foster" in a corner, sized to the image, partly see-through). Drawn with the Mac's own
//! system font, so Argus ships no font files.

use crate::{Error, Result, RgbImage};
use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
    Center,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Watermark {
    pub text: String,
    #[serde(default)]
    pub corner: Corner,
    /// Text height as a percentage of the image's short edge (default 3).
    #[serde(default = "default_size")]
    pub size: f32,
    /// 0..1 (default 0.6).
    #[serde(default = "default_opacity")]
    pub opacity: f32,
}

fn default_size() -> f32 {
    3.0
}
fn default_opacity() -> f32 {
    0.6
}

/// System fonts to draw with, the Mac's first.
const FONTS: &[(&str, u32)] = &[
    ("/System/Library/Fonts/SFNS.ttf", 0),
    ("/System/Library/Fonts/Helvetica.ttc", 0),
    ("/System/Library/Fonts/Supplemental/Arial.ttf", 0),
    ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0),
    (
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        0,
    ),
];

fn font() -> Result<FontVec> {
    for (path, index) in FONTS {
        if let Ok(data) = std::fs::read(path) {
            if let Ok(f) = FontVec::try_from_vec_and_index(data, *index) {
                return Ok(f);
            }
        }
    }
    Err(Error("watermark: no system font found to draw with".into()))
}

/// Draw `w` onto `img` in white, with a soft dark edge so it reads on skies and snow.
pub fn apply(img: &mut RgbImage, w: &Watermark) -> Result<()> {
    let text = w.text.trim();
    if text.is_empty() || img.width < 16 || img.height < 16 {
        return Ok(());
    }
    let font = font()?;
    let short = img.width.min(img.height) as f32;
    let px = (short * w.size.clamp(0.5, 20.0) / 100.0).max(8.0);
    let scaled = font.as_scaled(PxScale::from(px));
    // Lay the line out.
    let mut glyphs = Vec::new();
    let mut x = 0.0f32;
    let mut prev = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(p) = prev {
            x += scaled.kern(p, id);
        }
        glyphs.push(id.with_scale_and_position(px, ab_glyph::point(x, scaled.ascent())));
        x += scaled.h_advance(id);
        prev = Some(id);
    }
    let (tw, th) = (
        x.ceil() as usize,
        (scaled.ascent() - scaled.descent()).ceil() as usize,
    );
    let mut cover = vec![0f32; tw.max(1) * th.max(1)];
    for g in glyphs {
        if let Some(o) = font.outline_glyph(g) {
            let b = o.px_bounds();
            o.draw(|gx, gy, c| {
                let (x, y) = (b.min.x as i64 + gx as i64, b.min.y as i64 + gy as i64);
                if x >= 0 && y >= 0 && (x as usize) < tw && (y as usize) < th {
                    let v = &mut cover[y as usize * tw + x as usize];
                    *v = v.max(c);
                }
            });
        }
    }
    let margin = (short * 0.025).round() as i64;
    let (iw, ih) = (img.width as i64, img.height as i64);
    let (tw, th) = (tw as i64, th as i64);
    let (ox, oy) = match w.corner {
        Corner::TopLeft => (margin, margin),
        Corner::TopRight => (iw - margin - tw, margin),
        Corner::BottomLeft => (margin, ih - margin - th),
        Corner::BottomRight => (iw - margin - tw, ih - margin - th),
        Corner::Center => ((iw - tw) / 2, (ih - th) / 2),
    };
    let alpha = w.opacity.clamp(0.0, 1.0);
    let shadow = (px / 24.0).round().max(1.0) as i64;
    // A dark copy offset by `shadow`, then the white text over it.
    for (dx, dy, value, a) in [(shadow, shadow, 0.0f32, alpha * 0.35), (0, 0, 255.0, alpha)] {
        for ty in 0..th {
            for tx in 0..tw {
                let c = cover[(ty * tw + tx) as usize] * a;
                let (x, y) = (ox + tx + dx, oy + ty + dy);
                if c <= 0.0 || x < 0 || y < 0 || x >= iw || y >= ih {
                    continue;
                }
                let i = (y as usize * img.width + x as usize) * 3;
                for v in &mut img.data[i..i + 3] {
                    *v = (*v as f32 * (1.0 - c) + value * c).round() as u8;
                }
            }
        }
    }
    Ok(())
}
