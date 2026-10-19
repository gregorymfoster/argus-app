//! Lightroom's white balance menu and eyedropper, as values of the relative Temp and Tint
//! sliders (see [`crate::pipeline::wb_gains`]).
//!
//! Named presets (Daylight, Cloudy, ...) are colour temperatures in Kelvin, so they need
//! the temperature the camera chose: the camera's own neutral (the inverse of its as-shot
//! white balance multipliers) taken through its colour matrix to CIE xy, then McCamy's
//! (1992) formula for the correlated colour temperature. A preset then moves Temp by the
//! difference in mireds, the unit in which equal steps look equally warmer.

use crate::pipeline::{invert33, srgb_decode};
use crate::{Error, Result, RgbImage};

/// The menu, in Lightroom's order, with each preset's Kelvin and Tint.
pub const PRESETS: &[(&str, f64, f64)] = &[
    ("daylight", 5500.0, 0.0),
    ("cloudy", 6500.0, 0.0),
    ("shade", 7500.0, 0.0),
    ("tungsten", 2850.0, 0.0),
    ("fluorescent", 3800.0, 15.0),
    ("flash", 5500.0, 0.0),
];

/// Temp steps per mired: +1 Temp moves red against blue by 1/100 stop, and a mired
/// between 2850 K and 6500 K moves them by about 0.0075 stop in sRGB.
const TEMP_PER_MIRED: f64 = 0.75;

/// The colour temperature the camera balanced for, in Kelvin, from its as-shot white
/// balance multipliers (R, G, B) and XYZ -> camera matrix. None if it doesn't make sense.
pub fn as_shot_kelvin(wb: [f32; 3], xyz_to_cam: [[f32; 3]; 3]) -> Option<f64> {
    if wb.iter().any(|&v| v <= 0.0 || !v.is_finite()) {
        return None;
    }
    let neutral = wb.map(|v| wb[1] / v);
    let m = invert33(&xyz_to_cam);
    let xyz: [f64; 3] =
        std::array::from_fn(|i| (0..3).map(|j| (m[i][j] * neutral[j]) as f64).sum());
    let s: f64 = xyz.iter().sum();
    if s <= 0.0 {
        return None;
    }
    let (x, y) = (xyz[0] / s, xyz[1] / s);
    let n = (x - 0.3320) / (0.1858 - y);
    let k = 449.0 * n.powi(3) + 3525.0 * n.powi(2) + 6823.3 * n + 5520.33;
    (1500.0..=25000.0).contains(&k).then_some(k)
}

/// Temp and Tint for a named preset on a Photo the camera balanced at `as_shot` Kelvin.
pub fn preset(name: &str, as_shot: f64) -> Result<(f64, f64)> {
    let &(_, k, tint) = PRESETS.iter().find(|p| p.0 == name).ok_or_else(|| {
        Error(format!(
            "unknown white balance {name:?}; use as_shot, auto, {}",
            PRESETS.iter().map(|p| p.0).collect::<Vec<_>>().join(", ")
        ))
    })?;
    let temp = TEMP_PER_MIRED * (1e6 / as_shot - 1e6 / k);
    Ok((temp.clamp(-100.0, 100.0).round(), tint))
}

/// The eyedropper: Temp and Tint that make the area around (x, y) (0..1 of the frame)
/// grey. `render` renders the Photo with the given Temp and Tint; a few renders refine it,
/// because the tone curve bends each channel a little differently.
pub fn neutral_at(
    x: f64,
    y: f64,
    start: (f64, f64),
    render: impl Fn(f64, f64) -> Result<RgbImage>,
) -> Result<(f64, f64)> {
    let (mut temp, mut tint) = start;
    for _ in 0..4 {
        let [r, g, b] = mean_around(&render(temp, tint)?, x, y);
        if r <= 1e-4 || g <= 1e-4 || b <= 1e-4 {
            return Err(Error(
                "that spot is black; pick something grey or white".into(),
            ));
        }
        // wb_gains: Temp t/100 moves R/B by t/100 stops, Tint moves G by half that.
        let dt = 100.0 * (b / r).log2();
        let dn = 200.0 * (g / (r * b).sqrt()).log2();
        temp = (temp + dt).clamp(-100.0, 100.0);
        tint = (tint + dn).clamp(-100.0, 100.0);
        if dt.abs() < 0.5 && dn.abs() < 0.5 {
            break;
        }
    }
    Ok((temp.round(), tint.round()))
}

/// Linear RGB mean of a small patch (1% of the long edge) around (x, y).
fn mean_around(img: &RgbImage, x: f64, y: f64) -> [f64; 3] {
    let (w, h) = (img.width, img.height);
    let cx = (x.clamp(0.0, 1.0) * (w - 1) as f64).round() as usize;
    let cy = (y.clamp(0.0, 1.0) * (h - 1) as f64).round() as usize;
    let r = (w.max(h) / 100).max(1);
    let mut sum = [0f64; 3];
    let mut n = 0.0;
    for yy in cy.saturating_sub(r)..(cy + r + 1).min(h) {
        for xx in cx.saturating_sub(r)..(cx + r + 1).min(w) {
            let i = (yy * w + xx) * 3;
            for (c, v) in sum.iter_mut().enumerate() {
                *v += srgb_decode(img.data[i + c] as f32 / 255.0) as f64;
            }
            n += 1.0;
        }
    }
    sum.map(|v| v / n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{srgb_encode, to8, wb_gains};

    #[test]
    fn presets_warm_with_kelvin_and_daylight_on_daylight_is_as_shot() {
        assert_eq!(preset("daylight", 5500.0).unwrap(), (0.0, 0.0));
        assert!(preset("shade", 5500.0).unwrap().0 > preset("cloudy", 5500.0).unwrap().0);
        // A tungsten-lit frame shot on daylight balance cools a lot under Tungsten.
        assert!(preset("tungsten", 5500.0).unwrap().0 < -60.0);
        assert!(preset("candle", 5500.0).is_err());
    }

    #[test]
    fn as_shot_kelvin_reads_a_daylight_camera_balance() {
        // A camera whose matrix is the identity (camera = XYZ) balanced for D65's xy.
        let id = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let d65 = [0.9505f32, 1.0, 1.089];
        let k = as_shot_kelvin(d65.map(|v| 1.0 / v), id).unwrap();
        assert!((k - 6504.0).abs() < 150.0, "{k}");
    }

    #[test]
    fn eyedropper_makes_a_tinted_grey_grey() {
        // A flat grey card lit warm and a little green, rendered through wb_gains.
        let lit = [0.30f32, 0.24, 0.16];
        let render = |t: f64, n: f64| {
            let g = wb_gains((t / 100.0) as f32, (n / 100.0) as f32);
            let px: Vec<u8> = (0..3).map(|c| to8(srgb_encode(lit[c] * g[c]))).collect();
            Ok(RgbImage {
                width: 8,
                height: 8,
                data: px.repeat(64),
            })
        };
        let (t, n) = neutral_at(0.5, 0.5, (0.0, 0.0), render).unwrap();
        assert!(t < -50.0, "cools the warm light: {t}");
        let [r, g, b] = mean_around(&render(t, n).unwrap(), 0.5, 0.5);
        assert!(
            (r / b - 1.0).abs() < 0.03 && (g / r - 1.0).abs() < 0.03,
            "{r} {g} {b}"
        );
    }
}
