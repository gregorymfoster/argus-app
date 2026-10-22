//! The Recipe: a Photo's edit settings as one versioned JSON document (ADR 0002).
//!
//! The shape matches the prototype's `Recipe` (prototype/src/domain/types.ts) so the UI,
//! the Agent and the CLI read and write the same document:
//!
//! ```json
//! { "schema": 1,
//!   "global": { "exposure": 0.3, "temp": -10, "contrast": 15, "hsl": { "blue": { "sat": -20 } } },
//!   "look": null,
//!   "correction": { "exposure": 0, "wbK": 0, "face": 0 },
//!   "local": [] }
//! ```
//!
//! Slider units follow Lightroom's Basic panel: `exposure` in stops (-5..5), every other
//! slider -100..100 with 0 = unchanged. `temp` and `tint` are relative (+ is warmer / more
//! magenta), not Kelvin. `skin`, `eyes` and `teeth` are retouching amounts, 0..100. Each
//! entry of `local` is a [`Local`]: a Mask ([`crate::masks`]) and slider deltas.
//!
//! Beyond the Basic panel, `global` carries the rest of Lightroom's Edit panel: [`Curve`],
//! [`Grading`], [`Calibration`] and [`PointColor`] as nested objects, and Effects, Detail and
//! Optics as flat keys ([`MORE_SLIDERS`]: `clarity`, `vignetteMidpoint`, `sharpen`, ...).
//! All are optional and additive, so older Recipes read unchanged and `schema` stays 1.
//! Fields the renderer doesn't know (`fade`, ...) are kept as they are, so a round trip
//! never loses anything.

use crate::geometry::Crop;
use crate::heal::Spot;
use crate::lens_blur::LensBlur;
use crate::masks::Mask;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const SCHEMA: u32 = 1;

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// Per-colour Hue, Saturation and Luminance shift, each -100..100.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HslShift {
    #[serde(skip_serializing_if = "is_zero")]
    pub hue: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub sat: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub lum: f64,
}

impl HslShift {
    pub fn is_neutral(&self) -> bool {
        *self == HslShift::default()
    }
}

macro_rules! hsl_bands {
    ($($name:ident = $deg:literal),*) => {
        /// Lightroom's eight HSL colour bands.
        #[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Hsl {
            $(#[serde(skip_serializing_if = "HslShift::is_neutral")] pub $name: HslShift,)*
        }
        impl Hsl {
            pub const NAMES: &'static [&'static str] = &[$(stringify!($name)),*];
            /// Band centres in degrees of hue, in the same order as [`Hsl::bands`].
            pub const CENTRES: &'static [f64] = &[$($deg as f64),*];
            pub fn bands(&self) -> [HslShift; 8] {
                [$(self.$name),*]
            }
        }
    };
}
hsl_bands!(
    red = 0,
    orange = 30,
    yellow = 60,
    green = 120,
    aqua = 180,
    blue = 240,
    purple = 270,
    magenta = 300
);

impl Hsl {
    pub fn is_neutral(&self) -> bool {
        self.bands().iter().all(HslShift::is_neutral)
    }
}

/// Global settings: the sliders of Lightroom's Basic panel plus HSL.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Adjust {
    /// Stops, -5..5.
    #[serde(skip_serializing_if = "is_zero")]
    pub exposure: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub contrast: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub highlights: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub shadows: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub whites: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub blacks: f64,
    /// White balance, relative: + warmer, - cooler.
    #[serde(skip_serializing_if = "is_zero")]
    pub temp: f64,
    /// White balance, relative: + magenta, - green.
    #[serde(skip_serializing_if = "is_zero")]
    pub tint: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub vibrance: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub saturation: f64,
    #[serde(skip_serializing_if = "Hsl::is_neutral")]
    pub hsl: Hsl,
    /// Tone Curve: Lightroom's parametric regions and point curves.
    #[serde(skip_serializing_if = "Curve::is_neutral")]
    pub curve: Curve,
    /// Color Grading: shadows, midtones, highlights and global wheels.
    #[serde(skip_serializing_if = "Grading::is_neutral")]
    pub grading: Grading,
    /// Calibration: shadow tint and the red, green and blue primaries.
    #[serde(skip_serializing_if = "Calibration::is_neutral")]
    pub calibration: Calibration,
    /// Point Color: shifts for colours sampled from the Photo, applied in order.
    #[serde(rename = "pointColor", skip_serializing_if = "Vec::is_empty")]
    pub point_color: Vec<PointColor>,
    /// Natural skin smoothing, 0..100. Globally it evens every person's skin; in a local
    /// adjustment, whatever its Mask covers.
    #[serde(skip_serializing_if = "is_zero")]
    pub skin: f64,
    /// Brighter, crisper eyes, 0..100 (found by the face detector).
    #[serde(skip_serializing_if = "is_zero")]
    pub eyes: f64,
    /// Whiter teeth, 0..100 (found by the face detector).
    #[serde(skip_serializing_if = "is_zero")]
    pub teeth: f64,
    /// Effects, Detail and Optics sliders ([`MORE_SLIDERS`], read with [`Adjust::more`]),
    /// and settings the renderer doesn't apply yet, kept verbatim.
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// One slider of Effects, Detail or Optics: JSON key, range and the value it has when
/// absent. Flat keys in the Recipe's `global`, like Lightroom's own settings.
pub struct More {
    pub key: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
}

const fn more(key: &'static str, min: f64, max: f64, default: f64) -> More {
    More {
        key,
        min,
        max,
        default,
    }
}

/// Lightroom's Effects, Detail and Optics sliders, in its panel order and units.
pub const MORE_SLIDERS: &[More] = &[
    // Effects
    more("texture", -100.0, 100.0, 0.0),
    more("clarity", -100.0, 100.0, 0.0),
    more("dehaze", -100.0, 100.0, 0.0),
    more("vignette", -100.0, 100.0, 0.0),
    more("vignetteMidpoint", 0.0, 100.0, 50.0),
    more("vignetteRoundness", -100.0, 100.0, 0.0),
    more("vignetteFeather", 0.0, 100.0, 50.0),
    more("vignetteHighlights", 0.0, 100.0, 0.0),
    more("grain", 0.0, 100.0, 0.0),
    more("grainSize", 0.0, 100.0, 25.0),
    more("grainRoughness", 0.0, 100.0, 50.0),
    // Detail
    more("denoise", 0.0, 100.0, 0.0),
    more("sharpen", 0.0, 150.0, 0.0),
    more("sharpenRadius", 0.5, 3.0, 1.0),
    more("sharpenDetail", 0.0, 100.0, 25.0),
    more("sharpenMasking", 0.0, 100.0, 0.0),
    more("noise", 0.0, 100.0, 0.0),
    more("noiseDetail", 0.0, 100.0, 50.0),
    more("noiseContrast", 0.0, 100.0, 0.0),
    more("colorNoise", 0.0, 100.0, 0.0),
    more("colorNoiseDetail", 0.0, 100.0, 50.0),
    more("colorNoiseSmoothness", 0.0, 100.0, 50.0),
    // Optics
    more("removeCA", 0.0, 1.0, 0.0),
    more("defringePurple", 0.0, 20.0, 0.0),
    more("defringeGreen", 0.0, 20.0, 0.0),
    more("distortion", -100.0, 100.0, 0.0),
    more("lensVignette", -100.0, 100.0, 0.0),
    more("lensVignetteMidpoint", 0.0, 100.0, 50.0),
];

fn more_slider(key: &str) -> Option<&'static More> {
    MORE_SLIDERS.iter().find(|m| m.key == key)
}

/// A point curve: (input, output) points on 0..255, as in Lightroom. Empty = straight.
pub type Points = Vec<[f64; 2]>;

fn no_points(p: &Points) -> bool {
    p.is_empty()
}

/// Lightroom's Tone Curve: four parametric regions (-100..100) split at three points
/// (0..100, default 25/50/75), a point curve on all channels and one per channel, and
/// Refine Saturation (0..100, default 100: how much of the curves' saturation change stays).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Curve {
    #[serde(skip_serializing_if = "is_zero")]
    pub highlights: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub lights: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub darks: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub shadows: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_shadows: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_midtones: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_highlights: Option<f64>,
    #[serde(skip_serializing_if = "no_points")]
    pub rgb: Points,
    #[serde(skip_serializing_if = "no_points")]
    pub red: Points,
    #[serde(skip_serializing_if = "no_points")]
    pub green: Points,
    #[serde(skip_serializing_if = "no_points")]
    pub blue: Points,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refine_sat: Option<f64>,
}

impl Curve {
    pub fn is_neutral(&self) -> bool {
        *self == Curve::default()
    }

    /// The three split points as fractions 0..1, kept in order.
    pub fn splits(&self) -> [f64; 3] {
        let s = self.split_shadows.unwrap_or(25.0).clamp(5.0, 95.0);
        let m = self.split_midtones.unwrap_or(50.0).clamp(s, 95.0);
        let h = self.split_highlights.unwrap_or(75.0).clamp(m, 95.0);
        [s / 100.0, m / 100.0, h / 100.0]
    }

    fn clamped(mut self) -> Self {
        for v in [
            &mut self.highlights,
            &mut self.lights,
            &mut self.darks,
            &mut self.shadows,
        ] {
            *v = clamp(*v, -100.0, 100.0);
        }
        for v in [
            &mut self.split_shadows,
            &mut self.split_midtones,
            &mut self.split_highlights,
            &mut self.refine_sat,
        ]
        .into_iter()
        .flatten()
        {
            *v = clamp(*v, 0.0, 100.0);
        }
        for pts in [
            &mut self.rgb,
            &mut self.red,
            &mut self.green,
            &mut self.blue,
        ] {
            pts.retain(|p| p[0].is_finite() && p[1].is_finite());
            for p in pts.iter_mut() {
                *p = [p[0].clamp(0.0, 255.0), p[1].clamp(0.0, 255.0)];
            }
            pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
            pts.dedup_by(|a, b| (a[0] - b[0]).abs() < 1.0);
        }
        self
    }
}

/// One Color Grading wheel: hue in degrees (0..360), saturation 0..100, luminance -100..100.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Wheel {
    #[serde(skip_serializing_if = "is_zero")]
    pub hue: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub sat: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub lum: f64,
}

impl Wheel {
    pub fn is_neutral(&self) -> bool {
        self.sat == 0.0 && self.lum == 0.0
    }
}

fn wheel_unset(w: &Wheel) -> bool {
    *w == Wheel::default()
}

/// Lightroom's Color Grading: three tonal wheels and a global one, Blending (0..100,
/// default 50: how much the ranges overlap) and Balance (-100..100: + favours highlights).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grading {
    #[serde(skip_serializing_if = "wheel_unset")]
    pub shadows: Wheel,
    #[serde(skip_serializing_if = "wheel_unset")]
    pub midtones: Wheel,
    #[serde(skip_serializing_if = "wheel_unset")]
    pub highlights: Wheel,
    #[serde(skip_serializing_if = "wheel_unset")]
    pub global: Wheel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blending: Option<f64>,
    #[serde(skip_serializing_if = "is_zero")]
    pub balance: f64,
}

impl Grading {
    pub fn is_neutral(&self) -> bool {
        *self == Grading::default()
    }

    /// True when it changes no pixel (every wheel without saturation or luminance).
    pub fn is_identity(&self) -> bool {
        [self.shadows, self.midtones, self.highlights, self.global]
            .iter()
            .all(Wheel::is_neutral)
    }

    fn clamped(mut self) -> Self {
        for w in [
            &mut self.shadows,
            &mut self.midtones,
            &mut self.highlights,
            &mut self.global,
        ] {
            w.hue = if w.hue.is_finite() {
                w.hue.rem_euclid(360.0)
            } else {
                0.0
            };
            w.sat = clamp(w.sat, 0.0, 100.0);
            w.lum = clamp(w.lum, -100.0, 100.0);
        }
        self.blending = self.blending.map(|b| clamp(b, 0.0, 100.0));
        self.balance = clamp(self.balance, -100.0, 100.0);
        self
    }
}

/// Hue and saturation of one Calibration primary, each -100..100.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Primary {
    #[serde(skip_serializing_if = "is_zero")]
    pub hue: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub sat: f64,
}

fn primary_unset(p: &Primary) -> bool {
    *p == Primary::default()
}

/// Lightroom's Calibration: Shadows tint (-100 green .. +100 magenta) and the hue and
/// saturation of the red, green and blue primaries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Calibration {
    #[serde(skip_serializing_if = "is_zero")]
    pub shadows_tint: f64,
    #[serde(skip_serializing_if = "primary_unset")]
    pub red: Primary,
    #[serde(skip_serializing_if = "primary_unset")]
    pub green: Primary,
    #[serde(skip_serializing_if = "primary_unset")]
    pub blue: Primary,
}

impl Calibration {
    pub fn is_neutral(&self) -> bool {
        *self == Calibration::default()
    }

    fn clamped(mut self) -> Self {
        self.shadows_tint = clamp(self.shadows_tint, -100.0, 100.0);
        for p in [&mut self.red, &mut self.green, &mut self.blue] {
            p.hue = clamp(p.hue, -100.0, 100.0);
            p.sat = clamp(p.sat, -100.0, 100.0);
        }
        self
    }
}

/// One Point Color: a colour picked from the Photo (hue 0..360, sat and lum 0..100) and how
/// to shift colours near it (hueShift, satShift, lumShift -100..100). `range` (0..100,
/// default 50) widens or narrows which colours count as near.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PointColor {
    pub hue: f64,
    pub sat: f64,
    pub lum: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub hue_shift: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub sat_shift: f64,
    #[serde(skip_serializing_if = "is_zero")]
    pub lum_shift: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<f64>,
}

impl PointColor {
    fn clamped(mut self) -> Self {
        self.hue = if self.hue.is_finite() {
            self.hue.rem_euclid(360.0)
        } else {
            0.0
        };
        self.sat = clamp(self.sat, 0.0, 100.0);
        self.lum = clamp(self.lum, 0.0, 100.0);
        self.hue_shift = clamp(self.hue_shift, -100.0, 100.0);
        self.sat_shift = clamp(self.sat_shift, -100.0, 100.0);
        self.lum_shift = clamp(self.lum_shift, -100.0, 100.0);
        self.range = self.range.map(|r| clamp(r, 0.0, 100.0));
        self
    }
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_nan() {
        0.0_f64.clamp(lo, hi)
    } else {
        v.clamp(lo, hi)
    }
}

impl Adjust {
    /// An Effects, Detail or Optics slider ([`MORE_SLIDERS`]): its value, clamped, or its
    /// default when the Recipe doesn't set it.
    pub fn more(&self, key: &str) -> f64 {
        let m = more_slider(key).unwrap_or_else(|| panic!("{key} is not in MORE_SLIDERS"));
        match self.other.get(key).and_then(Value::as_f64) {
            Some(v) => clamp(v, m.min, m.max),
            None => m.default,
        }
    }

    /// Clamp every slider to its range (NaN becomes 0).
    pub fn clamped(mut self) -> Self {
        let c = |v: &mut f64, lim: f64| *v = if v.is_nan() { 0.0 } else { v.clamp(-lim, lim) };
        c(&mut self.exposure, 5.0);
        for v in [
            &mut self.contrast,
            &mut self.highlights,
            &mut self.shadows,
            &mut self.whites,
            &mut self.blacks,
            &mut self.temp,
            &mut self.tint,
            &mut self.vibrance,
            &mut self.saturation,
        ] {
            c(v, 100.0);
        }
        for v in [&mut self.skin, &mut self.eyes, &mut self.teeth] {
            *v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 100.0) };
        }
        let mut b = self.hsl.bands();
        for s in b.iter_mut() {
            c(&mut s.hue, 100.0);
            c(&mut s.sat, 100.0);
            c(&mut s.lum, 100.0);
        }
        self.hsl = Hsl::from_bands(b);
        self.curve = self.curve.clamped();
        self.grading = self.grading.clamped();
        self.calibration = self.calibration.clamped();
        self.point_color = self
            .point_color
            .into_iter()
            .map(PointColor::clamped)
            .collect();
        for m in MORE_SLIDERS {
            if let Some(v) = self.other.get(m.key).and_then(Value::as_f64) {
                self.other
                    .insert(m.key.into(), serde_json::json!(clamp(v, m.min, m.max)));
            }
        }
        self
    }

    /// True when only retouch settings (skin, eyes, teeth) or nothing at all is set.
    pub fn is_tonally_neutral(&self) -> bool {
        let d = Adjust {
            skin: 0.0,
            eyes: 0.0,
            teeth: 0.0,
            other: Map::new(),
            ..self.clone()
        };
        d == Adjust::default()
    }
}

impl Hsl {
    pub fn from_bands(b: [HslShift; 8]) -> Self {
        let [red, orange, yellow, green, aqua, blue, purple, magenta] = b;
        Hsl {
            red,
            orange,
            yellow,
            green,
            aqua,
            blue,
            purple,
            magenta,
        }
    }
}

impl Adjust {
    /// `self` with `other` added on top, slider by slider (a Look plus a Photo's own
    /// sliders). Unknown numeric settings add too; other unknown values take `other`'s.
    pub fn plus(&self, other: &Adjust) -> Adjust {
        let mut a = self.clone();
        for (v, o) in [
            (&mut a.exposure, other.exposure),
            (&mut a.contrast, other.contrast),
            (&mut a.highlights, other.highlights),
            (&mut a.shadows, other.shadows),
            (&mut a.whites, other.whites),
            (&mut a.blacks, other.blacks),
            (&mut a.temp, other.temp),
            (&mut a.tint, other.tint),
            (&mut a.vibrance, other.vibrance),
            (&mut a.saturation, other.saturation),
            (&mut a.skin, other.skin),
            (&mut a.eyes, other.eyes),
            (&mut a.teeth, other.teeth),
        ] {
            *v += o;
        }
        let mut bands = a.hsl.bands();
        for (b, o) in bands.iter_mut().zip(other.hsl.bands()) {
            b.hue += o.hue;
            b.sat += o.sat;
            b.lum += o.lum;
        }
        let [red, orange, yellow, green, aqua, blue, purple, magenta] = bands;
        a.hsl = Hsl {
            red,
            orange,
            yellow,
            green,
            aqua,
            blue,
            purple,
            magenta,
        };
        let (c, o) = (&mut a.curve, &other.curve);
        c.highlights += o.highlights;
        c.lights += o.lights;
        c.darks += o.darks;
        c.shadows += o.shadows;
        for (mine, theirs) in [
            (&mut c.split_shadows, o.split_shadows),
            (&mut c.split_midtones, o.split_midtones),
            (&mut c.split_highlights, o.split_highlights),
            (&mut c.refine_sat, o.refine_sat),
        ] {
            *mine = theirs.or(*mine);
        }
        for (mine, theirs) in [
            (&mut c.rgb, &o.rgb),
            (&mut c.red, &o.red),
            (&mut c.green, &o.green),
            (&mut c.blue, &o.blue),
        ] {
            if !theirs.is_empty() {
                *mine = theirs.clone();
            }
        }
        let (g, o) = (&mut a.grading, &other.grading);
        for (w, ow) in [
            (&mut g.shadows, o.shadows),
            (&mut g.midtones, o.midtones),
            (&mut g.highlights, o.highlights),
            (&mut g.global, o.global),
        ] {
            if ow.sat > 0.0 {
                w.hue = ow.hue;
            }
            w.sat += ow.sat;
            w.lum += ow.lum;
        }
        g.blending = o.blending.or(g.blending);
        g.balance += o.balance;
        let (k, o) = (&mut a.calibration, &other.calibration);
        k.shadows_tint += o.shadows_tint;
        for (p, op) in [
            (&mut k.red, o.red),
            (&mut k.green, o.green),
            (&mut k.blue, o.blue),
        ] {
            p.hue += op.hue;
            p.sat += op.sat;
        }
        a.point_color.extend(other.point_color.iter().copied());
        for (k, v) in &other.other {
            // A slider whose neutral value isn't 0 (a midpoint, a size) takes the
            // Photo's own value; the rest add.
            let replaces = more_slider(k).is_some_and(|m| m.default != 0.0);
            let sum = match (a.other.get(k).and_then(Value::as_f64), v.as_f64()) {
                (Some(x), Some(y)) if !replaces => serde_json::json!(x + y),
                _ => v.clone(),
            };
            a.other.insert(k.clone(), sum);
        }
        a
    }
}

fn is_default_adjust(a: &Adjust) -> bool {
    *a == Adjust::default()
}

fn is_empty(s: &str) -> bool {
    s.is_empty()
}

/// One local adjustment: slider deltas (same units as the global ones, added to them)
/// where its Mask applies. Matches the prototype's `LocalAdjust`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Local {
    pub id: String,
    #[serde(default, skip_serializing_if = "is_empty")]
    pub name: String,
    pub mask: Mask,
    #[serde(default, skip_serializing_if = "is_default_adjust")]
    pub adjust: Adjust,
    /// Lightroom's Mask Amount, 0..200 percent: how strongly the whole adjustment applies.
    /// Absent is 100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    /// Who made it: "ai" or "you" (the prototype's Actor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// Per-Photo baseline under the Look (prototype: `Correction`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Correction {
    /// Stops, added to the global exposure.
    pub exposure: f64,
    /// White balance shift in Kelvin (- cooler); 25 K = 1 step of `temp`, as in the prototype.
    #[serde(rename = "wbK")]
    pub wb_k: f64,
    /// Exposure on faces in stops, through a `faces` Mask.
    pub face: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recipe {
    pub schema: u32,
    pub global: Adjust,
    pub look: Option<String>,
    pub correction: Correction,
    /// Local adjustments, each tied to a Mask, applied in order.
    pub local: Vec<Local>,
    /// Crop and straighten, applied after colour and tone.
    #[serde(skip_serializing_if = "Crop::is_identity")]
    pub crop: Crop,
    /// Heal and Clone Spots, applied in order after the Masks and before the crop.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spots: Vec<Spot>,
    /// Lens Blur: a fake shallow depth of field from a depth map ([`crate::lens_blur`]).
    #[serde(rename = "lensBlur", skip_serializing_if = "LensBlur::is_default")]
    pub lens_blur: LensBlur,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl Default for Recipe {
    fn default() -> Self {
        Recipe {
            schema: SCHEMA,
            global: Adjust::default(),
            look: None,
            correction: Correction::default(),
            local: Vec::new(),
            crop: Crop::default(),
            spots: Vec::new(),
            lens_blur: LensBlur::default(),
            other: Map::new(),
        }
    }
}

impl Recipe {
    pub fn from_value(v: &Value) -> Result<Self> {
        let r: Recipe =
            serde_json::from_value(v.clone()).map_err(|e| Error(format!("recipe: {e}")))?;
        if r.schema > SCHEMA {
            return Err(Error(format!(
                "recipe schema {} is newer than this Argus ({SCHEMA})",
                r.schema
            )));
        }
        Ok(r)
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("Recipe serialises")
    }

    /// What the renderer applies: the global sliders with the Correction folded in, clamped.
    pub fn render_adjust(&self) -> Adjust {
        let mut a = self.global.clone();
        a.exposure += self.correction.exposure;
        a.temp += self.correction.wb_k / 25.0;
        a.clamped()
    }

    /// True when rendering needs the Mask path: local adjustments, Lens Blur, light on faces
    /// or retouching.
    pub fn needs_masks(&self) -> bool {
        let g = &self.global;
        !self.local.is_empty()
            || self.lens_blur.active()
            || self.correction.face != 0.0
            || g.skin > 0.0
            || g.eyes > 0.0
            || g.teeth > 0.0
    }

    /// The local adjustment with this id.
    pub fn local_mut(&mut self, id: &str) -> Result<&mut Local> {
        let ids: Vec<String> = self.local.iter().map(|l| l.id.clone()).collect();
        self.local.iter_mut().find(|l| l.id == id).ok_or_else(|| {
            Error(format!(
                "no local adjustment {id:?}; this Recipe has {ids:?}"
            ))
        })
    }

    /// Set one global slider by path, e.g. `exposure`, `temp` or `hsl.blue.sat`. Paths
    /// starting with `correction.` set the Correction instead. With `relative`, the value
    /// is added to the current one (`exposure=+0.3` on the CLI).
    pub fn set(&mut self, path: &str, value: f64, relative: bool) -> Result<()> {
        if !value.is_finite() {
            return Err(Error(format!("{path}: not a number")));
        }
        let mut doc = self.to_value();
        if let Some(k) = path.strip_prefix("lensBlur.") {
            let mut b = self.lens_blur.clone();
            let slot = match k {
                "amount" => &mut b.amount,
                "boost" => &mut b.boost,
                _ => {
                    return Err(Error(format!(
                        "unknown setting {path:?}; lensBlur has amount, boost (numbers), bokeh, focus, focusBrush, blurBrush"
                    )))
                }
            };
            *slot = if relative { *slot + value } else { value };
            self.lens_blur = b.clamped();
            return Ok(());
        }
        if let Some(k) = path.strip_prefix("crop.") {
            let mut c = self.crop;
            let slot = match k {
                "angle" => &mut c.angle,
                "x" => &mut c.x,
                "y" => &mut c.y,
                "w" => &mut c.w,
                "h" => &mut c.h,
                _ => {
                    return Err(Error(format!(
                        "unknown setting {path:?}; crop has angle, x, y, w, h"
                    )))
                }
            };
            *slot = if relative { *slot + value } else { value };
            self.crop = c.clamped();
            return Ok(());
        }
        let (root, rest) = match path.strip_prefix("correction.") {
            Some(rest) => ("correction", rest),
            None => ("global", path),
        };
        let keys: Vec<&str> = rest.split('.').collect();
        let known = if root == "correction" {
            matches!(keys[..], ["exposure"] | ["wbK"] | ["face"])
        } else {
            known_global(&keys)
        };
        if !known {
            return Err(Error(format!(
                "unknown setting {path:?}; known: {}, {}, hsl.<{}>.<hue|sat|lum>, {}, correction.<exposure|wbK|face>, crop.<angle|x|y|w|h>",
                KNOWN_SLIDERS.join(", "),
                MORE_SLIDERS.iter().map(|m| m.key).collect::<Vec<_>>().join(", "),
                Hsl::NAMES.join("|"),
                NESTED_DOC,
            )));
        }
        let mut node = &mut doc[root];
        for k in &keys {
            if !node.is_object() {
                *node = Value::Object(Map::new());
            }
            node = node
                .as_object_mut()
                .unwrap()
                .entry(k.to_string())
                .or_insert(Value::Null);
        }
        let cur = node.as_f64().unwrap_or_else(|| {
            if root == "global" {
                neutral(&keys)
            } else {
                0.0
            }
        });
        *node = serde_json::json!(if relative { cur + value } else { value });
        let mut next = Recipe::from_value(&doc)?;
        next.global = next.global.clamped();
        *self = next;
        Ok(())
    }
}

impl Recipe {
    /// Like [`Recipe::set`], but also takes the settings that aren't one number: a point
    /// curve (`curve.rgb`, `curve.red`, `curve.green`, `curve.blue` as `[[x, y], ...]` on
    /// 0..255, `[]` for straight) and the Point Color list (`pointColor`, see
    /// [`PointColor`]). Those always replace.
    pub fn set_value(&mut self, path: &str, value: &Value, relative: bool) -> Result<()> {
        if let Some(v) = value.as_f64() {
            return self.set(path, v, relative);
        }
        let bad = |e: serde_json::Error| Error(format!("{path}: {e}"));
        if path == "lensBlur" || path.starts_with("lensBlur.") {
            let mut doc = serde_json::to_value(&self.lens_blur).map_err(bad)?;
            match path.strip_prefix("lensBlur.") {
                None => doc = value.clone(),
                Some(k @ ("bokeh" | "focus" | "focusBrush" | "blurBrush")) => {
                    doc[k] = value.clone();
                }
                Some(_) => {
                    return Err(Error(format!(
                        "{path}: lensBlur takes amount, boost, bokeh, focus, focusBrush, blurBrush"
                    )))
                }
            }
            let b: LensBlur = serde_json::from_value(doc).map_err(bad)?;
            self.lens_blur = b.clamped();
            return Ok(());
        }
        let g = &mut self.global;
        match path {
            "curve.rgb" | "curve.red" | "curve.green" | "curve.blue" => {
                let pts: Points = serde_json::from_value(value.clone()).map_err(bad)?;
                let c = &mut g.curve;
                *match path {
                    "curve.rgb" => &mut c.rgb,
                    "curve.red" => &mut c.red,
                    "curve.green" => &mut c.green,
                    _ => &mut c.blue,
                } = pts;
            }
            "pointColor" => {
                g.point_color = serde_json::from_value(value.clone()).map_err(bad)?;
            }
            // Lens corrections from the camera's data (crate::lens); "camera" is the default.
            "lensCorrections" => match value.as_str() {
                Some("camera") => {
                    g.other.remove(path);
                }
                Some(v @ ("on" | "off")) => {
                    g.other.insert(path.into(), v.into());
                }
                _ => return Err(Error(format!("{path}: one of \"camera\", \"on\", \"off\""))),
            },
            _ => {
                return Err(Error(format!(
                    "{path}: expected a number (only curve.<rgb|red|green|blue> and pointColor take lists, lensCorrections a word)"
                )))
            }
        }
        self.global = self.global.clone().clamped();
        Ok(())
    }
}

/// The nested settings, for error messages.
const NESTED_DOC: &str = "curve.<highlights|lights|darks|shadows|splitShadows|splitMidtones|splitHighlights|refineSat>, \
grading.<shadows|midtones|highlights|global>.<hue|sat|lum>, grading.<blending|balance>, \
calibration.shadowsTint, calibration.<red|green|blue>.<hue|sat>";

/// Whether `keys` (a path under `global`) names a single-number setting.
fn known_global(keys: &[&str]) -> bool {
    match keys {
        [k] => KNOWN_SLIDERS.contains(k) || more_slider(k).is_some(),
        ["hsl", band, part] => Hsl::NAMES.contains(band) && matches!(*part, "hue" | "sat" | "lum"),
        ["curve", k] => matches!(
            *k,
            "highlights"
                | "lights"
                | "darks"
                | "shadows"
                | "splitShadows"
                | "splitMidtones"
                | "splitHighlights"
                | "refineSat"
        ),
        ["grading", "blending" | "balance"] => true,
        ["grading", "shadows" | "midtones" | "highlights" | "global", part] => {
            matches!(*part, "hue" | "sat" | "lum")
        }
        ["calibration", "shadowsTint"] => true,
        ["calibration", "red" | "green" | "blue", part] => matches!(*part, "hue" | "sat"),
        _ => false,
    }
}

/// The value a setting has when the Recipe leaves it out.
fn neutral(keys: &[&str]) -> f64 {
    match keys {
        [k] => more_slider(k).map_or(0.0, |m| m.default),
        ["curve", "splitShadows"] => 25.0,
        ["curve", "splitMidtones"] | ["grading", "blending"] => 50.0,
        ["curve", "splitHighlights"] => 75.0,
        ["curve", "refineSat"] => 100.0,
        _ => 0.0,
    }
}

impl Recipe {
    /// `to` at `amount` of the way from `from`, like the Amount slider on a Lightroom preset:
    /// 0 is `from`, 1 is `to`, above 1 pushes past it (clamped to each slider's range). Every
    /// number moves in proportion, local adjustments matched by id (a new one grows from
    /// nothing); the crop, the Look and point curves are `to`'s once `amount` is above 0.
    pub fn blend(from: &Recipe, to: &Recipe, amount: f64) -> Recipe {
        if amount <= 0.0 {
            return from.clone();
        }
        if amount == 1.0 {
            return to.clone();
        }
        let adjust = |a: &Adjust, b: &Adjust| -> Adjust {
            let v = lerp(
                &serde_json::to_value(a).unwrap_or_default(),
                &serde_json::to_value(b).unwrap_or_default(),
                amount,
                &mut Vec::new(),
            );
            serde_json::from_value::<Adjust>(v)
                .unwrap_or_else(|_| b.clone())
                .clamped()
        };
        let mut r = to.clone();
        r.global = adjust(&from.global, &to.global);
        let c = |a: f64, b: f64| a + (b - a) * amount;
        r.correction = Correction {
            exposure: c(from.correction.exposure, to.correction.exposure),
            wb_k: c(from.correction.wb_k, to.correction.wb_k),
            face: c(from.correction.face, to.correction.face),
        };
        for l in &mut r.local {
            let was = from.local.iter().find(|f| f.id == l.id);
            l.adjust = adjust(
                &was.map_or_else(Adjust::default, |f| f.adjust.clone()),
                &l.adjust,
            );
        }
        r
    }
}

/// Numbers in `b` moved `t` of the way from `a`'s, recursively; a number missing on one
/// side counts as its neutral value (`path` is the key path, for [`neutral`]). Anything
/// that isn't a number or an object is `b`'s.
fn lerp(a: &Value, b: &Value, t: f64, path: &mut Vec<String>) -> Value {
    match (a, b) {
        (Value::Object(ma), Value::Object(mb)) => {
            let mut out = Map::new();
            for k in ma.keys().chain(mb.keys()) {
                if out.contains_key(k) {
                    continue;
                }
                path.push(k.clone());
                let keys: Vec<&str> = path.iter().map(String::as_str).collect();
                let n = neutral(&keys);
                let side = |m: &Map<String, Value>| match m.get(k) {
                    Some(v) => v.clone(),
                    // A missing nested object is all neutral; a missing number is its neutral.
                    None => match ma.get(k).or(mb.get(k)) {
                        Some(Value::Object(_)) => Value::Object(Map::new()),
                        // A hue only means something with saturation: keep the one given,
                        // so a grade fades in on its own hue.
                        Some(h) if k == "hue" => h.clone(),
                        _ => serde_json::json!(n),
                    },
                };
                let v = lerp(&side(ma), &side(mb), t, path);
                path.pop();
                out.insert(k.clone(), v);
            }
            Value::Object(out)
        }
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap_or(0.0), y.as_f64().unwrap_or(0.0));
            serde_json::json!(x + (y - x) * t)
        }
        _ => b.clone(),
    }
}

pub const KNOWN_SLIDERS: &[&str] = &[
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "whites",
    "blacks",
    "temp",
    "tint",
    "vibrance",
    "saturation",
    "skin",
    "eyes",
    "teeth",
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prototype_recipe_round_trips_unknown_fields() {
        let v = json!({
            "schema": 1,
            "global": { "exposure": 0.4, "temp": -12.0, "grain": 20, "hsl": { "blue": { "sat": -30.0 } } },
            "look": "warm-film",
            "correction": { "exposure": 0.1, "wbK": -50.0, "face": 0.2 },
            "local": [{ "id": "a", "mask": { "kind": "sky" } }]
        });
        let r = Recipe::from_value(&v).unwrap();
        assert_eq!(r.global.exposure, 0.4);
        assert_eq!(r.global.hsl.blue.sat, -30.0);
        assert_eq!(r.to_value(), v);
        let a = r.render_adjust();
        assert!((a.exposure - 0.5).abs() < 1e-6);
        assert!((a.temp - -14.0).abs() < 1e-6);
    }

    #[test]
    fn empty_document_is_neutral() {
        let r = Recipe::from_value(&json!({})).unwrap();
        assert_eq!(r, Recipe::default());
        assert_eq!(r.render_adjust(), Adjust::default());
    }

    #[test]
    fn set_by_path_absolute_relative_and_clamped() {
        let mut r = Recipe::default();
        r.set("exposure", 0.3, false).unwrap();
        r.set("exposure", 0.2, true).unwrap();
        assert!((r.global.exposure - 0.5).abs() < 1e-6);
        r.set("hsl.aqua.hue", 250.0, false).unwrap();
        assert_eq!(r.global.hsl.aqua.hue, 100.0);
        r.set("correction.wbK", -100.0, false).unwrap();
        assert_eq!(r.correction.wb_k, -100.0);
        assert!(r.set("clarty", 1.0, false).is_err());
        assert!(r.set("hsl.teal.sat", 1.0, false).is_err());
    }

    #[test]
    fn look_plus_photo_adds_slider_by_slider() {
        let look: Adjust = serde_json::from_value(
            json!({ "temp": 30, "contrast": -12, "grain": 20, "hsl": { "blue": { "sat": -10 } } }),
        )
        .unwrap();
        let own: Adjust = serde_json::from_value(
            json!({ "temp": -10, "exposure": 0.3, "hsl": { "blue": { "sat": -5 } } }),
        )
        .unwrap();
        let sum = look.plus(&own);
        assert_eq!(sum.temp, 20.0);
        assert_eq!(sum.contrast, -12.0);
        assert_eq!(sum.exposure, 0.3);
        assert_eq!(sum.hsl.blue.sat, -15.0);
        assert_eq!(sum.other["grain"], json!(20));
    }

    #[test]
    fn lightroom_panels_set_by_path_clamp_and_round_trip() {
        let mut r = Recipe::default();
        r.set("clarity", 30.0, false).unwrap();
        r.set("vignetteMidpoint", 10.0, true).unwrap(); // relative to its default, 50
        r.set("sharpen", 400.0, false).unwrap();
        r.set("curve.lights", 20.0, false).unwrap();
        r.set("grading.shadows.hue", 200.0, false).unwrap();
        r.set("grading.shadows.sat", 25.0, false).unwrap();
        r.set("calibration.blue.sat", 40.0, false).unwrap();
        r.set_value(
            "curve.rgb",
            &json!([[0, 10], [128, 140], [255, 250]]),
            false,
        )
        .unwrap();
        r.set_value(
            "pointColor",
            &json!([{ "hue": 30, "sat": 50, "lum": 60, "satShift": -20 }]),
            false,
        )
        .unwrap();
        let g = &r.global;
        assert_eq!(g.more("clarity"), 30.0);
        assert_eq!(g.more("vignetteMidpoint"), 60.0);
        assert_eq!(g.more("sharpen"), 150.0);
        assert_eq!(g.more("grainSize"), 25.0);
        assert_eq!(g.curve.lights, 20.0);
        assert_eq!(g.curve.rgb.len(), 3);
        assert_eq!(g.grading.shadows.hue, 200.0);
        assert_eq!(g.calibration.blue.sat, 40.0);
        assert_eq!(g.point_color[0].sat_shift, -20.0);
        let back = Recipe::from_value(&r.to_value()).unwrap();
        assert_eq!(back, r);
        assert!(r.set("grading.shadows.tint", 1.0, false).is_err());
        assert!(r.set_value("clarity", &json!([1]), false).is_err());
    }

    #[test]
    fn look_midpoints_replace_and_amounts_add() {
        let look: Adjust =
            serde_json::from_value(json!({ "vignette": -20, "vignetteMidpoint": 30 })).unwrap();
        let own: Adjust =
            serde_json::from_value(json!({ "vignette": -10, "vignetteMidpoint": 40 })).unwrap();
        let sum = look.plus(&own);
        assert_eq!(sum.more("vignette"), -30.0);
        assert_eq!(sum.more("vignetteMidpoint"), 40.0);
    }

    #[test]
    fn newer_schema_is_refused() {
        assert!(Recipe::from_value(&json!({ "schema": 99 })).is_err());
    }

    #[test]
    fn blend_moves_every_setting_in_proportion() {
        let from =
            Recipe::from_value(&json!({ "global": { "exposure": 0.2, "contrast": 10 } })).unwrap();
        let to = Recipe::from_value(&json!({
            "global": { "exposure": 0.6, "clarity": 20, "vignetteMidpoint": 70,
                        "grading": { "shadows": { "hue": 200, "sat": 20 } } },
            "local": [{ "id": "ai-sky", "mask": { "kind": "sky" }, "adjust": { "exposure": -0.4 } }],
            "crop": { "angle": 2.0 }
        }))
        .unwrap();
        assert_eq!(Recipe::blend(&from, &to, 0.0), from);
        let full = Recipe::blend(&from, &to, 1.0);
        assert_eq!(full.global, to.global);
        let half = Recipe::blend(&from, &to, 0.5);
        let g = &half.global;
        assert!((g.exposure - 0.4).abs() < 1e-9);
        assert!((g.contrast - 5.0).abs() < 1e-9);
        assert!((g.more("clarity") - 10.0).abs() < 1e-9);
        // A slider whose neutral isn't 0 moves from its neutral.
        assert!((g.more("vignetteMidpoint") - 60.0).abs() < 1e-9);
        // A grade fades in on its own hue.
        assert_eq!(g.grading.shadows.hue, 200.0);
        assert!((g.grading.shadows.sat - 10.0).abs() < 1e-9);
        // A new local adjustment grows from nothing; the crop is the take's.
        assert!((half.local[0].adjust.exposure + 0.2).abs() < 1e-9);
        assert_eq!(half.crop.angle, 2.0);
        // Past 100% pushes further, within each slider's range.
        let more = Recipe::blend(&from, &to, 1.5);
        assert!((more.global.exposure - 0.8).abs() < 1e-9);
    }
}
