//! Copy, paste and sync settings, as in Lightroom: take some groups of one Photo's
//! settings (Light, Color, Crop, Masks, ...) and put them on other Photos, leaving the
//! rest of each target as it was. The `paste_settings` tool runs it; the app's ⌘⇧C,
//! ⌘⇧V, ⌥⌘V (paste the previous Photo's settings) and Sync settings all call that tool.
//!
//! Groups follow Lightroom's Copy Settings dialog, simplified. A group is a set of
//! Recipe keys: flat slider keys and nested objects in `global`, plus `correction`'s
//! fields, `local` (Masks), `spots` (Heal and Clone), `crop` and
//! `lensBlur`. Pasting a group replaces those keys on the
//! target, so a slider the source never moved is reset to its default there, which is
//! what Lightroom does. Keys no group knows (a setting from a newer Argus) stay as the
//! target has them. The prototype's copy of this list is `prototype/src/domain/settings.ts`.

use crate::recipe::Recipe;
use crate::{Error, Result};
use serde_json::{Map, Value};

pub struct Group {
    pub id: &'static str,
    pub label: &'static str,
    /// Ticked when the person doesn't choose (Lightroom leaves crop and Masks out).
    pub default: bool,
}

pub const GROUPS: &[Group] = &[
    Group {
        id: "wb",
        label: "White balance",
        default: true,
    },
    Group {
        id: "light",
        label: "Light",
        default: true,
    },
    Group {
        id: "color",
        label: "Color",
        default: true,
    },
    Group {
        id: "effects",
        label: "Effects",
        default: true,
    },
    Group {
        id: "detail",
        label: "Detail",
        default: true,
    },
    Group {
        id: "optics",
        label: "Optics",
        default: true,
    },
    Group {
        id: "calibration",
        label: "Calibration",
        default: true,
    },
    Group {
        id: "retouch",
        label: "Retouch",
        default: true,
    },
    Group {
        id: "masks",
        label: "Masks",
        default: false,
    },
    Group {
        id: "spots",
        label: "Spot removal",
        default: false,
    },
    Group {
        id: "crop",
        label: "Crop",
        default: false,
    },
    Group {
        id: "lensBlur",
        label: "Lens Blur",
        default: false,
    },
];

/// The group a key of `global` belongs to, or None for a key no group knows.
pub fn group_of(key: &str) -> Option<&'static str> {
    Some(match key {
        "temp" | "tint" => "wb",
        "exposure" | "contrast" | "highlights" | "shadows" | "whites" | "blacks" | "curve" => {
            "light"
        }
        "vibrance" | "saturation" | "hsl" | "grading" | "pointColor" => "color",
        "texture" | "clarity" | "dehaze" | "fade" => "effects",
        k if k.starts_with("vignette") || k.starts_with("grain") => "effects",
        k if k.starts_with("sharpen")
            || k.starts_with("noise")
            || k.starts_with("colorNoise")
            || k.starts_with("denoise") =>
        {
            "detail"
        }
        "removeCA" | "distortion" => "optics",
        k if k.starts_with("defringe") || k.starts_with("lens") => "optics",
        "calibration" => "calibration",
        "skin" | "eyes" | "teeth" => "retouch",
        _ => return None,
    })
}

/// The group a key of `correction` belongs to.
fn correction_group(key: &str) -> Option<&'static str> {
    match key {
        "exposure" | "face" => Some("light"),
        "wbK" => Some("wb"),
        _ => None,
    }
}

/// The groups to paste: the ones named (checked against [`GROUPS`]), or the defaults.
pub fn groups(named: Option<&[String]>) -> Result<Vec<&'static str>> {
    match named {
        None => Ok(GROUPS.iter().filter(|g| g.default).map(|g| g.id).collect()),
        Some(names) => names
            .iter()
            .map(|n| {
                GROUPS
                    .iter()
                    .find(|g| g.id == n)
                    .map(|g| g.id)
                    .ok_or_else(|| {
                        let ids: Vec<_> = GROUPS.iter().map(|g| g.id).collect();
                        Error(format!(
                            "unknown settings group {n:?}; groups are {}",
                            ids.join(", ")
                        ))
                    })
            })
            .collect(),
    }
}

/// Replace the keys of `into` that `group` puts in one of `groups` with `from`'s.
fn paste_keys(
    from: &Map<String, Value>,
    into: &mut Map<String, Value>,
    groups: &[&str],
    group: fn(&str) -> Option<&'static str>,
) {
    let chosen = |k: &str| group(k).is_some_and(|g| groups.contains(&g));
    into.retain(|k, _| !chosen(k));
    for (k, v) in from {
        if chosen(k) {
            into.insert(k.clone(), v.clone());
        }
    }
}

fn object(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

/// `into` with `groups` of `from`'s settings in place of its own.
pub fn paste(from: &Recipe, into: &Recipe, groups: &[&str]) -> Result<Recipe> {
    let mut out = into.to_value();
    let src = from.to_value();
    let o = out.as_object_mut().expect("a Recipe is an object");
    for (field, group) in [
        ("global", group_of as fn(&str) -> Option<&'static str>),
        ("correction", correction_group),
    ] {
        let mut mine = object(o.remove(field).unwrap_or_default());
        paste_keys(&object(src[field].clone()), &mut mine, groups, group);
        o.insert(field.into(), Value::Object(mine));
    }
    for (field, group) in [
        ("local", "masks"),
        ("spots", "spots"),
        ("crop", "crop"),
        ("lensBlur", "lensBlur"),
    ] {
        if groups.contains(&group) {
            match src.get(field) {
                Some(v) => o.insert(field.into(), v.clone()),
                None => o.remove(field),
            };
        }
    }
    Recipe::from_value(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn recipe(v: Value) -> Recipe {
        Recipe::from_value(&v).unwrap()
    }

    #[test]
    fn pastes_only_the_chosen_groups() {
        let from = recipe(json!({ "global": { "exposure": 0.5, "temp": 12, "clarity": 20 } }));
        let into = recipe(json!({ "global": { "exposure": -1.0, "temp": -5, "skin": 30 } }));
        let out = paste(&from, &into, &["light"]).unwrap();
        assert_eq!(out.global.exposure, 0.5);
        assert_eq!(out.global.temp, -5.0, "White balance wasn't chosen");
        assert_eq!(out.global.skin, 30.0);
        assert!(out.global.more("clarity") == 0.0, "Effects wasn't chosen");
    }

    #[test]
    fn a_slider_the_source_never_moved_goes_back_to_its_default() {
        let from = recipe(json!({ "global": { "exposure": 0.5 } }));
        let into = recipe(json!({ "global": { "shadows": 40, "curve": { "lights": 10 } } }));
        let out = paste(&from, &into, &["light"]).unwrap();
        assert_eq!(out.global.shadows, 0.0);
        assert!(out.global.curve.is_neutral());
    }

    #[test]
    fn masks_spots_and_crop_come_along_only_when_chosen() {
        let from = recipe(json!({
            "local": [{ "id": "m1", "mask": { "kind": "sky" }, "adjust": { "exposure": -0.3 } }],
            "spots": [{ "id": "s1", "x": 0.5, "y": 0.5, "r": 0.01, "from": { "x": 0.6, "y": 0.5 } }],
            "crop": { "angle": 2.0, "x": 0.1, "y": 0.1, "w": 0.8, "h": 0.8 } }));
        let into = Recipe::default();
        let defaults = groups(None).unwrap();
        let out = paste(&from, &into, &defaults).unwrap();
        assert!(out.local.is_empty() && out.crop.is_identity());
        assert!(out.spots.is_empty());
        let out = paste(&from, &into, &["masks", "spots", "crop"]).unwrap();
        assert_eq!(out.local.len(), 1);
        assert_eq!(out.spots, from.spots);
        assert_eq!(out.crop, from.crop);
    }

    #[test]
    fn per_photo_corrections_travel_with_their_group() {
        let from = recipe(json!({ "correction": { "exposure": 0.4, "wbK": 100, "face": 0.2 } }));
        let out = paste(&from, &Recipe::default(), &["wb"]).unwrap();
        assert_eq!((out.correction.exposure, out.correction.wb_k), (0.0, 100.0));
    }

    #[test]
    fn unknown_keys_stay_as_the_target_has_them() {
        let from = recipe(json!({ "global": { "exposure": 1.0 } }));
        let into = recipe(json!({ "global": { "futureSlider": 7 } }));
        let all: Vec<_> = GROUPS.iter().map(|g| g.id).collect();
        let out = paste(&from, &into, &all).unwrap();
        assert_eq!(out.global.other.get("futureSlider"), Some(&json!(7)));
    }

    #[test]
    fn every_known_slider_has_a_group() {
        for m in crate::recipe::MORE_SLIDERS {
            assert!(group_of(m.key).is_some(), "{} has no settings group", m.key);
        }
        assert!(groups(Some(&["nope".into()])).is_err());
    }
}
