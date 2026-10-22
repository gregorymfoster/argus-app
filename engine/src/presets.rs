//! Presets (GLOSSARY: Preset), after Lightroom's Presets panel: built-in starting points
//! grouped as Lightroom groups them (Portrait, Film, Black and white, Adaptive: Sky...),
//! applied at an Amount from 0 to 200%.
//!
//! The library is data, `engine/presets/builtin.json`, read by the engine and the
//! prototype alike. A Preset's numbers are added to the Photo's Recipe (so it stacks on Auto
//! and on the person's own edit), except keys listed in `absolute` (B&W's saturation -100),
//! hues, which are set, and point curves, which replace the Photo's. Its local adjustments
//! add Masks the engine finds by itself. Amount scales all of it, and 2.0 pushes past.
//!
//! What a Preset changed is kept in the Recipe under `presets` (one entry per applied
//! Preset: id, amount, and each setting's value before and after), so a later Amount or
//! removal can take it back off exactly, leaving the person's own changes since. Applying
//! a Look-style Preset replaces the one before it; Adaptive ones stack, one of each.
//!
//! The person's own Presets (group Yours) are saved from a Photo, as Lightroom's Create
//! Preset does: the Settings groups they tick ([`crate::paste`]) are kept in the Library's
//! settings, so they travel with it. Theirs set values rather than add them, and a setting
//! in a ticked group the Preset leaves out goes back to neutral, as a paste does; Amount
//! moves each setting from the Photo's own value toward the Preset's.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::library::Library;
use crate::recipe::Recipe;
use crate::{Error, Result};

const BUILTIN: &str = include_str!("../presets/builtin.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub group: String,
    /// One line on what it does and when it suits, for the person and for the model.
    pub why: String,
    /// Global settings, as in a Recipe's `global` (nested `hsl`, `curve`, `grading`...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global: Option<Value>,
    /// Top-level `global` keys that are set rather than added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub absolute: Vec<String>,
    /// Local adjustments it adds, each on a Mask the engine finds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local: Vec<PresetLocal>,
    /// Set on the person's own Presets only: the Settings groups it covers. Its settings are
    /// set, not added, and the rest of these groups goes back to neutral.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetLocal {
    pub name: String,
    pub mask: Value,
    pub adjust: Value,
}

#[derive(Deserialize)]
struct File {
    groups: Vec<String>,
    presets: Vec<Preset>,
}

fn file() -> &'static File {
    static F: OnceLock<File> = OnceLock::new();
    F.get_or_init(|| serde_json::from_str(BUILTIN).expect("presets/builtin.json parses"))
}

/// Every built-in Preset, in panel order.
pub fn builtin() -> &'static [Preset] {
    &file().presets
}

/// The groups, in panel order.
pub fn groups() -> &'static [String] {
    &file().groups
}

pub fn find(id: &str) -> Result<&'static Preset> {
    builtin().iter().find(|p| p.id == id).ok_or_else(|| {
        Error(format!(
            "no Preset {id:?}; list_presets names them (e.g. natural, film-portrait, bw-soft)"
        ))
    })
}

/// A short hash of the library, so cached rankings know when the Presets changed.
pub fn library_hash() -> String {
    blake3::hash(BUILTIN.as_bytes()).to_hex()[..12].to_string()
}

impl Preset {
    /// Adaptive Presets stack (one of each); every other kind replaces the last.
    pub fn slot(&self) -> &str {
        if self.group.starts_with("Adaptive") {
            &self.id
        } else {
            "look"
        }
    }

    fn local_id(&self, i: usize) -> String {
        format!("preset-{}-{i}", self.id)
    }

    /// One of the person's own, saved from a Photo.
    pub fn is_yours(&self) -> bool {
        !self.groups.is_empty()
    }
}

/// The slot of an applied Preset by id, among the built-ins and `yours`. One of the
/// person's that has since been deleted still holds the Look slot, so it can be replaced.
fn slot_of(id: &str, yours: &[Preset]) -> Option<String> {
    builtin()
        .iter()
        .chain(yours)
        .find(|p| p.id == id)
        .map(|p| p.slot().to_string())
        .or_else(|| id.starts_with(YOURS_PREFIX).then(|| "look".to_string()))
}

/// `recipe` with `p` applied at `amount` (1.0 = as designed, 0..2), replacing a Preset in
/// the same slot (and this one, if it was on already). Amount 0 just takes it off.
pub fn apply(recipe: &Recipe, p: &Preset, amount: f64) -> Result<Recipe> {
    apply_among(recipe, p, amount, &[])
}

/// [`apply`], where the Recipe may also carry the person's own Presets, `yours`.
pub fn apply_among(recipe: &Recipe, p: &Preset, amount: f64, yours: &[Preset]) -> Result<Recipe> {
    let amount = amount.clamp(0.0, 2.0);
    let mut v = recipe.to_value();
    for e in applied(&v) {
        if slot_of(&e.id, yours).as_deref() == Some(p.slot()) {
            take_off(&mut v, &e.id);
        }
    }
    if amount > 0.0 {
        put_on(&mut v, p, amount);
    }
    let mut r = Recipe::from_value(&v)?;
    r.global = r.global.clamped();
    // Remember what clamping left, so taking it off later sees it as untouched.
    let mut v = r.to_value();
    let mut list = applied(&v);
    if let Some(e) = list.iter_mut().find(|e| e.id == p.id) {
        for (key, after) in e.after.iter_mut() {
            let path: Vec<&str> = key.split('.').collect();
            *after = get(&v["global"], &path)
                .cloned()
                .unwrap_or(json!(neutral(&path)));
        }
    }
    set_applied(&mut v, &list);
    Recipe::from_value(&v)
}

/// `recipe` without `p` (unchanged if it isn't on).
pub fn remove(recipe: &Recipe, p: &Preset) -> Result<Recipe> {
    let mut v = recipe.to_value();
    take_off(&mut v, &p.id);
    Recipe::from_value(&v)
}

/// The Presets on a Recipe, in the order they were applied.
pub fn on(recipe: &Recipe) -> Vec<Applied> {
    applied(&recipe.to_value())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Applied {
    pub id: String,
    pub amount: f64,
    /// Dotted path in `global` to its value before (null = unset) and after.
    #[serde(default)]
    pub before: Map<String, Value>,
    #[serde(default)]
    pub after: Map<String, Value>,
}

fn applied(v: &Value) -> Vec<Applied> {
    v.get("presets")
        .and_then(|a| serde_json::from_value(a.clone()).ok())
        .unwrap_or_default()
}

fn set_applied(v: &mut Value, list: &[Applied]) {
    let o = v.as_object_mut().expect("a Recipe is an object");
    if list.is_empty() {
        o.remove("presets");
    } else {
        o.insert("presets".into(), json!(list));
    }
}

/// The neutral value of a `global` setting (what it is when absent).
fn neutral(path: &[&str]) -> f64 {
    match path {
        [k] => crate::recipe::MORE_SLIDERS
            .iter()
            .find(|m| m.key == *k)
            .map_or(0.0, |m| m.default),
        ["curve", "splitShadows"] => 25.0,
        ["curve", "splitMidtones"] | ["grading", "blending"] => 50.0,
        ["curve", "splitHighlights"] => 75.0,
        ["curve", "refineSat"] => 100.0,
        _ => 0.0,
    }
}

const POINT_CURVES: [&str; 4] = ["rgb", "red", "green", "blue"];

fn is_points(path: &[&str]) -> bool {
    matches!(path, ["curve", c] if POINT_CURVES.contains(c))
}

/// Every setting the Preset names, as (path, value): numbers, and point curves whole.
fn leaves<'a>(v: &'a Value, path: &mut Vec<&'a str>, out: &mut Vec<(Vec<&'a str>, &'a Value)>) {
    if let Value::Object(m) = v {
        for (k, x) in m {
            path.push(k);
            if x.is_object() && !is_points(path) {
                leaves(x, path, out);
            } else {
                out.push((path.clone(), x));
            }
            path.pop();
        }
    }
}

fn get<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(v, |v, k| v.get(*k))
}

fn set(v: &mut Value, path: &[&str], x: Option<Value>) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut cur = v;
    for k in parents {
        if !cur.get(*k).is_some_and(Value::is_object) {
            cur[*k] = json!({});
        }
        cur = cur.get_mut(*k).expect("just made");
    }
    let o = cur.as_object_mut().expect("an object");
    match x {
        Some(x) => {
            o.insert((*last).to_string(), x);
        }
        None => {
            o.remove(*last);
        }
    }
}

fn round(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// A point curve `amount` of the way from straight to `pts`.
fn scale_points(pts: &Value, amount: f64) -> Value {
    let out: Vec<Value> = pts
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let x = p.get(0)?.as_f64()?;
            let y = p.get(1)?.as_f64()?;
            Some(json!([x, round((x + (y - x) * amount).clamp(0.0, 255.0))]))
        })
        .collect();
    Value::Array(out)
}

fn put_on(v: &mut Value, p: &Preset, amount: f64) {
    let mut entry = Applied {
        id: p.id.clone(),
        amount,
        before: Map::new(),
        after: Map::new(),
    };
    let empty = json!({});
    let mut ls: Vec<(Vec<&str>, Option<&Value>)> = Vec::new();
    let mut named = Vec::new();
    leaves(
        p.global.as_ref().unwrap_or(&empty),
        &mut Vec::new(),
        &mut named,
    );
    let current = v["global"].clone();
    if p.is_yours() {
        // A setting in its groups that it leaves out goes back to neutral, as a paste does.
        let mut have = Vec::new();
        leaves(&current, &mut Vec::new(), &mut have);
        for (path, x) in have {
            let covered =
                crate::paste::group_of(path[0]).is_some_and(|g| p.groups.iter().any(|x| x == g));
            if covered
                && (x.is_number() || is_points(&path))
                && !named.iter().any(|(n, _)| *n == path)
            {
                ls.push((path, None));
            }
        }
    }
    ls.extend(named.into_iter().map(|(path, x)| (path, Some(x))));
    let global = v.get_mut("global").expect("a Recipe has global");
    if !global.is_object() {
        *global = json!({});
    }
    for (path, x) in ls {
        let old = get(global, &path).cloned();
        let new = if is_points(&path) {
            match x {
                Some(x) if p.is_yours() => x.clone(),
                Some(x) => scale_points(x, amount),
                None => json!([[0, 0], [255, 255]]),
            }
        } else if let Some(x) = x.filter(|x| !x.is_number()) {
            // A choice rather than a number (lens corrections As camera): set.
            x.clone()
        } else {
            let x = x.and_then(Value::as_f64).unwrap_or(neutral(&path));
            let cur = old
                .as_ref()
                .and_then(Value::as_f64)
                .unwrap_or(neutral(&path));
            let last = *path.last().expect("non-empty");
            let to = if last == "hue" {
                x
            } else if p.is_yours()
                || neutral(&path) != 0.0
                || path.len() == 1 && p.absolute.iter().any(|a| a == last)
            {
                // Shape settings (grain size, vignette feather) are set, not added.
                cur + (x - cur) * amount
            } else {
                cur + x * amount
            };
            json!(round(to))
        };
        let key = path.join(".");
        entry.before.insert(key.clone(), old.unwrap_or(Value::Null));
        entry.after.insert(key, new.clone());
        set(global, &path, Some(new));
    }
    let local = v["local"].as_array_mut().expect("a Recipe has local");
    for (i, l) in p.local.iter().enumerate() {
        let mut adjust = Vec::new();
        leaves(&l.adjust, &mut Vec::new(), &mut adjust);
        let mut a = json!({});
        for (path, x) in adjust {
            let x = x.as_f64().unwrap_or(0.0);
            let last = *path.last().expect("non-empty");
            let y = if last == "hue" { x } else { x * amount };
            set(&mut a, &path, Some(json!(round(y))));
        }
        let id = p.local_id(i);
        local.retain(|x| x["id"] != json!(id));
        local.push(
            json!({ "id": id, "name": l.name, "mask": l.mask, "adjust": a, "source": "you" }),
        );
    }
    let mut list = applied(v);
    list.retain(|e| e.id != p.id);
    list.push(entry);
    set_applied(v, &list);
}

/// Take `p` back off: each setting it changed goes back to before, unless the person moved
/// it since, in which case only what the Preset added comes off. Its Masks go.
fn take_off(v: &mut Value, id: &str) {
    let mut list = applied(v);
    let Some(i) = list.iter().position(|e| e.id == id) else {
        return;
    };
    let e = list.remove(i);
    let global = v.get_mut("global").expect("a Recipe has global");
    for (key, after) in &e.after {
        let path: Vec<&str> = key.split('.').collect();
        let before = e.before.get(key).cloned().unwrap_or(Value::Null);
        // A Recipe leaves out settings at neutral, so a missing one reads as neutral.
        let cur = get(global, &path)
            .cloned()
            .or_else(|| after.is_number().then(|| json!(neutral(&path))));
        let untouched = cur.as_ref().is_some_and(|c| same(c, after));
        let back = if untouched {
            (!before.is_null()).then_some(before)
        } else {
            // Moved since: keep the person's change, minus what the Preset added.
            match (
                &cur,
                after.as_f64(),
                before.as_f64().or(Some(neutral(&path))),
            ) {
                (Some(Value::Number(c)), Some(a), Some(b)) if !is_points(&path) => {
                    Some(json!(round(c.as_f64().unwrap_or(0.0) - (a - b))))
                }
                _ => cur,
            }
        };
        set(global, &path, back);
    }
    prune(global);
    let prefix = format!("preset-{id}-");
    if let Some(local) = v["local"].as_array_mut() {
        local.retain(|l| !l["id"].as_str().is_some_and(|id| id.starts_with(&prefix)));
    }
    set_applied(v, &list);
}

fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-6,
        _ => a == b,
    }
}

/// Drop objects left empty (an `hsl.green` with nothing in it).
fn prune(v: &mut Value) {
    if let Value::Object(m) = v {
        for x in m.values_mut() {
            prune(x);
        }
        m.retain(|_, x| !x.as_object().is_some_and(Map::is_empty));
    }
}

// --- the person's own Presets ---

/// The panel group of the person's own Presets, shown above the built-ins.
pub const YOURS: &str = "Yours";
const YOURS_PREFIX: &str = "yours-";
/// Where they live: the Library's settings table, so they move with the Library.
const YOURS_KEY: &str = "presets.yours";

/// Settings groups a Preset can hold. Spots and crop belong to one frame, so, as in
/// Lightroom, a Preset leaves them out.
pub const PRESET_GROUPS: &[&str] = &[
    "wb",
    "light",
    "color",
    "effects",
    "detail",
    "optics",
    "calibration",
    "retouch",
    "masks",
];

/// The groups ticked when the person doesn't choose: all but White balance and Masks,
/// which belong to the light and the people of one Photo more than to a look.
pub fn default_groups() -> Vec<&'static str> {
    PRESET_GROUPS
        .iter()
        .copied()
        .filter(|g| !matches!(*g, "wb" | "masks"))
        .collect()
}

/// The person's own Presets, in the order they made them.
pub fn yours(lib: &Library) -> Result<Vec<Preset>> {
    Ok(lib
        .setting(YOURS_KEY)?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

fn put_yours(lib: &Library, list: &[Preset]) -> Result<()> {
    lib.set_setting(YOURS_KEY, &json!(list))
}

/// A built-in Preset or one of the person's, by id.
pub fn find_in(lib: &Library, id: &str) -> Result<Preset> {
    if let Some(p) = yours(lib)?.into_iter().find(|p| p.id == id) {
        return Ok(p);
    }
    find(id).cloned()
}

/// A Preset named `name` from `recipe`'s settings in `groups` (as it renders: the caller
/// folds in its Look). Not saved; see [`save`].
pub fn from_recipe(name: &str, recipe: &Recipe, groups: &[&str]) -> Result<Preset> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error("a Preset needs a name".into()));
    }
    if groups.is_empty() {
        return Err(Error("choose at least one settings group".into()));
    }
    if let Some(g) = groups.iter().find(|g| !PRESET_GROUPS.contains(g)) {
        return Err(Error(format!(
            "a Preset can't hold {g:?}; its groups are {}",
            PRESET_GROUPS.join(", ")
        )));
    }
    let v = recipe.to_value();
    let global: Map<String, Value> = v["global"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(k, _)| crate::paste::group_of(k).is_some_and(|g| groups.contains(&g)))
        .map(|(k, x)| (k.clone(), x.clone()))
        .collect();
    let local = if groups.contains(&"masks") {
        recipe
            .local
            .iter()
            .map(|l| -> Result<PresetLocal> {
                Ok(PresetLocal {
                    name: if l.name.is_empty() {
                        "Mask".into()
                    } else {
                        l.name.clone()
                    },
                    mask: serde_json::to_value(&l.mask).map_err(|e| Error(e.to_string()))?,
                    adjust: serde_json::to_value(&l.adjust).map_err(|e| Error(e.to_string()))?,
                })
            })
            .collect::<Result<_>>()?
    } else {
        Vec::new()
    };
    Ok(Preset {
        id: String::new(),
        name: name.to_string(),
        group: YOURS.into(),
        why: String::new(),
        global: (!global.is_empty()).then_some(Value::Object(global)),
        absolute: Vec::new(),
        local,
        groups: groups.iter().map(|g| g.to_string()).collect(),
    })
}

/// Save `p` among the person's Presets: as a new one, or in place of `replace` (Lightroom's
/// Update with current settings). Names are unique among theirs.
pub fn save(lib: &Library, mut p: Preset, replace: Option<&str>) -> Result<Preset> {
    let mut list = yours(lib)?;
    let same_name =
        |x: &Preset| x.name.eq_ignore_ascii_case(&p.name) && Some(x.id.as_str()) != replace;
    if list.iter().any(same_name) {
        return Err(Error(format!(
            "you already have a Preset named {:?}; pass its id to update it",
            p.name
        )));
    }
    match replace {
        Some(id) => {
            let i = list
                .iter()
                .position(|x| x.id == id)
                .ok_or_else(|| Error(format!("no Preset of yours {id:?}")))?;
            p.id = id.to_string();
            list[i] = p.clone();
        }
        None => {
            let base = format!(
                "{YOURS_PREFIX}{}",
                crate::naming::id_slug(&p.name, "preset")
            );
            let mut id = base.clone();
            let mut n = 2;
            while list.iter().any(|x| x.id == id) {
                id = format!("{base}-{n}");
                n += 1;
            }
            p.id = id;
            list.push(p.clone());
        }
    }
    put_yours(lib, &list)?;
    Ok(p)
}

/// Rename one of the person's Presets.
pub fn rename(lib: &Library, id: &str, name: &str) -> Result<Preset> {
    let mut p = yours(lib)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| Error(format!("no Preset of yours {id:?}")))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(Error("a Preset needs a name".into()));
    }
    p.name = name.to_string();
    save(lib, p, Some(id))
}

/// Delete one of the person's Presets. Photos it was applied to keep their settings.
pub fn delete(lib: &Library, id: &str) -> Result<()> {
    let mut list = yours(lib)?;
    let before = list.len();
    list.retain(|p| p.id != id);
    if list.len() == before {
        return Err(Error(format!("no Preset of yours {id:?}")));
    }
    put_yours(lib, &list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(global: Value) -> Recipe {
        Recipe {
            global: serde_json::from_value(global).unwrap(),
            ..Recipe::default()
        }
    }

    #[test]
    fn the_library_parses_and_every_preset_applies_and_comes_off() {
        assert!(builtin().len() >= 20);
        let mut ids = std::collections::HashSet::new();
        for p in builtin() {
            assert!(ids.insert(&p.id), "duplicate id {}", p.id);
            assert!(
                groups().contains(&p.group),
                "{} has unknown group {}",
                p.id,
                p.group
            );
            assert!(!p.why.is_empty());
            let base = r(json!({ "exposure": 0.4, "shadows": 30 }));
            let on = apply(&base, p, 1.0).unwrap();
            assert_ne!(on, base, "{} changes nothing", p.id);
            // Every key it names must be one the Recipe knows (no typos that land in `other`).
            assert!(
                on.global
                    .other
                    .keys()
                    .all(|k| crate::recipe::MORE_SLIDERS.iter().any(|m| m.key == k)),
                "{}: unknown keys {:?}",
                p.id,
                on.global.other.keys().collect::<Vec<_>>()
            );
            let off = remove(&on, p).unwrap();
            assert_eq!(off, base, "{} doesn't come off cleanly", p.id);
        }
    }

    #[test]
    fn a_preset_adds_to_auto_and_amount_scales_it() {
        let base = r(json!({ "shadows": 30, "temp": 2 }));
        let p = find("warm-film").unwrap();
        let full = apply(&base, p, 1.0).unwrap();
        assert_eq!(full.global.shadows, 42.0);
        assert_eq!(full.global.temp, 14.0);
        let half = apply(&full, p, 0.5).unwrap();
        assert_eq!(half.global.shadows, 36.0);
        assert_eq!(half.global.curve.rgb[0], [0.0, 7.0]);
        let double = apply(&half, p, 2.0).unwrap();
        assert_eq!(double.global.shadows, 54.0);
        assert_eq!(on(&double).len(), 1);
    }

    #[test]
    fn black_and_white_sets_saturation_and_a_look_replaces_the_last() {
        let base = r(json!({ "saturation": 10 }));
        let bw = apply(&base, find("bw-soft").unwrap(), 1.0).unwrap();
        assert_eq!(bw.global.saturation, -100.0);
        let film = apply(&bw, find("film-portrait").unwrap(), 1.0).unwrap();
        assert_eq!(film.global.saturation, 4.0, "B&W came off, then -6");
        assert_eq!(
            on(&film).iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            ["film-portrait"]
        );
    }

    #[test]
    fn adaptive_presets_stack_and_scale_their_masks() {
        let base = r(json!({}));
        let a = apply(&base, find("sky-blue").unwrap(), 0.5).unwrap();
        let b = apply(&a, find("subject-spotlight").unwrap(), 1.0).unwrap();
        let c = apply(&b, find("natural").unwrap(), 1.0).unwrap();
        assert_eq!(on(&c).len(), 3);
        assert_eq!(c.local.len(), 3);
        assert_eq!(c.local[0].adjust.exposure, -0.1);
        let off = remove(&c, find("subject-spotlight").unwrap()).unwrap();
        assert_eq!(off.local.len(), 1);
        assert_eq!(off.global.more("vignette"), 0.0);
    }

    #[test]
    fn taking_a_preset_off_keeps_what_the_person_moved_since() {
        let base = r(json!({ "shadows": 10 }));
        let p = find("natural").unwrap();
        let mut on = apply(&base, p, 1.0).unwrap();
        assert_eq!(on.global.shadows, 30.0);
        on.global.shadows = 50.0;
        let off = remove(&on, p).unwrap();
        assert_eq!(off.global.shadows, 30.0, "their +20 stays");
        assert_eq!(off.global.contrast, 0.0);
    }
}
