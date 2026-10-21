//! The AI's suggested order of the Presets for a Shoot (Greg 2026-10-21: "have the presets
//! automatically rank for what would look best in the shoot"). A few representative Photos
//! (Best Photos first, else Picks, else frames spread over the Shoot) are each rendered with
//! every Preset into one numbered sheet, and the model picks the eight that suit that Photo
//! best, with a few words why. Picks add up across Photos into the Shoot's order.
//!
//! It is a suggestion order, not a verdict: model judges agree with Greg's own picks about
//! two times in three (memory argus-model-judges), and Qwen answers partly by slot, so the
//! model is one weak input, hedged three ways: each Photo's sheet is in its own shuffled
//! order (a slot bias spreads out instead of piling onto one Preset), Presets the scene can't
//! use are left off (Adaptive Sky without sky, Portrait ones without faces), and a pick
//! whose tile clips or crushes clearly more than the Photo does now is dropped, measured, not
//! judged. The UI says "suggested" and keeps every Preset one click away. One sheet per representative keeps it to a few model calls, cheap
//! enough for the background Queue (job `rank_presets`). The result is cached per Shoot in
//! the Library's settings and stays until the Presets or the representatives change.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agent::{Msg, Role};
use crate::compute::Compute;
use crate::library::{Library, Photo, PhotoFilter, PhotoId, ShootId, Verdict};
use crate::presets::{self, Preset};
use crate::recipe::Recipe;
use crate::{render, Error, Result};

/// Representative Photos per Shoot: one model call each.
pub const PHOTOS: usize = 3;
/// How many Presets the model picks per Photo.
pub const PICKS: usize = 8;
/// Tile long edge on the sheet, and its columns.
const TILE: u32 = 256;
const COLUMNS: u32 = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ranked {
    pub id: String,
    /// Sum over the representatives of (PICKS - place) / PICKS; 0 = never picked.
    pub score: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ranking {
    pub shoot_id: ShootId,
    /// Every Preset, the suggested first.
    pub ranked: Vec<Ranked>,
    /// The Photos it was judged on.
    pub photos: Vec<PhotoId>,
    pub provider: String,
    pub model: String,
    /// [`presets::library_hash`] when it was made.
    pub presets: String,
    pub at: String,
}

fn key(shoot: ShootId) -> String {
    format!("preset_rank:{shoot}")
}

/// The Shoot's cached ranking, if it is for today's Presets.
pub fn cached(lib: &Library, shoot: ShootId) -> Result<Option<Ranking>> {
    Ok(lib
        .setting(&key(shoot))?
        .and_then(|v| serde_json::from_value::<Ranking>(v).ok())
        .filter(|r| r.presets == presets::library_hash()))
}

/// The Photos a ranking is judged on: the Shoot's Best Photos, then its Picks, then frames
/// spread evenly through it; never a reject.
pub fn representatives(lib: &Library, shoot: ShootId) -> Result<Vec<Photo>> {
    let all = lib.list_photos(&PhotoFilter {
        shoot: Some(shoot),
        verdict: None,
    })?;
    let ok: Vec<&Photo> = all
        .iter()
        .filter(|p| p.verdict != Verdict::Reject)
        .collect();
    let mut out: Vec<Photo> = Vec::new();
    let add = |p: &Photo, out: &mut Vec<Photo>| {
        if out.len() < PHOTOS && !out.iter().any(|q| q.id == p.id) {
            out.push(p.clone());
        }
    };
    for b in lib.best_photos(shoot)? {
        if let Some(p) = ok.iter().find(|p| p.id == b.photo_id) {
            add(p, &mut out);
        }
    }
    for p in ok.iter().filter(|p| p.verdict == Verdict::Pick) {
        add(p, &mut out);
    }
    let n = ok.len();
    for i in 0..PHOTOS {
        if let Some(p) = ok.get((2 * i + 1) * n / (2 * PHOTOS)) {
            add(p, &mut out);
        }
    }
    Ok(out)
}

fn schema() -> Value {
    json!({ "type": "object", "required": ["best"], "properties": {
        "best": { "type": "array", "maxItems": PICKS, "items": { "type": "object", "required": ["n", "why"],
            "properties": { "n": { "type": "integer" }, "why": { "type": "string" } } } } } })
}

const SYSTEM: &str = "You are a photo editor with natural, tasteful judgement, choosing Lightroom-style presets for a photographer's shoot. \
You see one photo rendered with many presets, each tile numbered. Pick the presets that would look best on this photo and on the shoot it comes from: \
flattering, believable skin; skies and highlights held; colour that suits the scene and light. Prefer subtle over gimmicky, but pick a strong style \
(black and white, film, cinematic) when the photo would clearly carry it. Never pick a tile that clips, crushes, or makes skin grey, orange or green. \
Reply with JSON only.";

/// The sheet's legend and the request, for one Photo.
fn ask(photo: &Photo, list: &[&Preset]) -> String {
    let mut t = format!(
        "Photo {} ({}). Tile 0 is the photo as it is now. The others, by number:\n",
        photo.file_name, photo.captured_at
    );
    for (i, p) in list.iter().enumerate() {
        t += &format!("{}: {} ({}): {}\n", i + 1, p.name, p.group, p.why);
    }
    t += &format!(
        "Pick up to {PICKS} tiles, best first, as {{\"best\": [{{\"n\": 3, \"why\": \"a few words\"}}]}}. Don't pick 0."
    );
    t
}

/// Whether the scene can use `p`: Sky Presets need sky, Portrait ones faces.
pub fn fits(p: &Preset, scene: &crate::masks::Coverage) -> bool {
    match p.group.as_str() {
        "Adaptive: Sky" => scene.sky >= 0.04,
        "Adaptive: Portrait" | "Portrait" => scene.faces > 0,
        "Adaptive: Subject" => scene.people >= 0.02,
        _ => true,
    }
}

/// `list` in an order of its own for this Photo (a fixed shuffle seeded by its id), so a
/// model's slot bias lands on different Presets for each Photo.
pub fn shuffled<'a>(list: &[&'a Preset], seed: PhotoId) -> Vec<&'a Preset> {
    let mut out = list.to_vec();
    let mut x = (seed as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    for i in (1..out.len()).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.swap(i, (x % (i as u64 + 1)) as usize);
    }
    out
}

/// A tile that blows or crushes clearly more of the frame than the Photo does now.
pub fn damages(now: &crate::signals::Light, tile: &crate::signals::Light) -> bool {
    tile.clipped_pct > now.clipped_pct + 2.0 || tile.crushed_pct > now.crushed_pct + 4.0
}

struct Sheet {
    jpeg: Vec<u8>,
    /// Per Preset of the list, whether its tile [`damages`] the Photo.
    damaged: Vec<bool>,
}

/// The numbered sheet: tile 0 the Photo as edited, then each Preset on top of that.
fn sheet(
    photo: &Photo,
    base: &Recipe,
    list: &[&Preset],
    masks: Option<std::sync::Arc<crate::masks::Analysis>>,
    compute: Compute,
) -> Result<Sheet> {
    let mut recipes = vec![base.clone()];
    for p in list {
        recipes.push(presets::apply(base, p, 1.0)?);
    }
    let rendered: Vec<(image::RgbImage, crate::signals::Light)> =
        compute.cpu_pool()?.install(|| {
            recipes
                .par_iter()
                .map(|r| {
                    let img = render::render_photo(photo, r, Some(TILE as usize), masks.clone())?;
                    let light = crate::signals::light(&img);
                    let img =
                        image::RgbImage::from_raw(img.width as u32, img.height as u32, img.data)
                            .ok_or_else(|| Error("render: bad image size".into()))?;
                    Ok((img, light))
                })
                .collect::<Result<_>>()
        })?;
    let damaged = rendered[1..]
        .iter()
        .map(|(_, l)| damages(&rendered[0].1, l))
        .collect();
    let tiles: Vec<&image::RgbImage> = rendered.iter().map(|(t, _)| t).collect();
    let rows = (tiles.len() as u32).div_ceil(COLUMNS);
    let mut out =
        image::RgbImage::from_pixel(COLUMNS * TILE, rows * TILE, image::Rgb([24, 24, 24]));
    for (i, t) in tiles.iter().enumerate() {
        let (cx, cy) = ((i as u32 % COLUMNS) * TILE, (i as u32 / COLUMNS) * TILE);
        let (x, y) = (cx + (TILE - t.width()) / 2, cy + (TILE - t.height()) / 2);
        image::imageops::replace(&mut out, *t, x as i64, y as i64);
        crate::tools::draw_number(&mut out, cx + 4, cy + 4, i);
    }
    let img = crate::RgbImage {
        width: out.width() as usize,
        height: out.height() as usize,
        data: out.into_raw(),
    };
    Ok(Sheet {
        jpeg: render::encode_jpeg(&img, 85)?,
        damaged,
    })
}

/// The model's picks for one sheet as (preset index, why), in its order, unknown and
/// repeated numbers dropped.
pub fn parse_picks(reply: &str, n: usize) -> Result<Vec<(usize, String)>> {
    let start = reply
        .find('{')
        .ok_or_else(|| Error(format!("no JSON in reply: {reply}")))?;
    let end = reply.rfind('}').map_or(reply.len(), |e| e + 1);
    let v: Value = serde_json::from_str(&reply[start..end])
        .map_err(|e| Error(format!("bad JSON in reply ({e}): {reply}")))?;
    let mut out: Vec<(usize, String)> = Vec::new();
    for b in v["best"].as_array().into_iter().flatten() {
        let Some(k) = b["n"].as_u64().map(|k| k as usize) else {
            continue;
        };
        if (1..=n).contains(&k) && !out.iter().any(|(i, _)| *i == k - 1) {
            out.push((k - 1, b["why"].as_str().unwrap_or("").trim().to_string()));
        }
        if out.len() == PICKS {
            break;
        }
    }
    Ok(out)
}

/// Sum each Photo's picks into one order over every Preset: picked ones by score, then the
/// rest in panel order. `why` is the line from the Photo that ranked it highest.
pub fn combine(all: &[&Preset], picks: &[Vec<(usize, String)>]) -> Vec<Ranked> {
    let mut ranked: Vec<(usize, Ranked, f64)> = all
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                i,
                Ranked {
                    id: p.id.clone(),
                    score: 0.0,
                    why: None,
                },
                0.0,
            )
        })
        .collect();
    for photo in picks {
        for (place, (i, why)) in photo.iter().enumerate() {
            let s = (PICKS - place) as f64 / PICKS as f64;
            let r = &mut ranked[*i];
            r.1.score += s;
            if s > r.2 && !why.is_empty() {
                r.1.why = Some(why.clone());
                r.2 = s;
            }
        }
    }
    ranked.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then(a.0.cmp(&b.0)));
    ranked
        .into_iter()
        .map(|(_, mut r, _)| {
            r.score = (r.score * 1000.0).round() / 1000.0;
            r
        })
        .collect()
}

/// Rank the Presets for `shoot` with the model, store the result and return it. `progress`
/// hears (Photos done, Photos).
pub fn rank(
    lib: &Library,
    shoot: ShootId,
    provider: Option<&str>,
    compute: Compute,
    progress: &dyn Fn(usize, usize),
) -> Result<Ranking> {
    let photos = representatives(lib, shoot)?;
    if photos.is_empty() {
        return Err(Error(format!(
            "Shoot {shoot} has no Photos to rank Presets on"
        )));
    }
    let all: Vec<&Preset> = presets::builtin().iter().collect();
    let chat = crate::agent::chat_with_format(provider, Some(compute), schema())?;
    let mut picks = Vec::new();
    progress(0, photos.len());
    for (i, photo) in photos.iter().enumerate() {
        let own = match lib.read_recipe(photo.id)? {
            Some(v) => Recipe::from_value(&v.recipe)?,
            None => Recipe::default(),
        };
        let base = crate::tools::with_look(lib, photo, &own)?;
        // The Analysis finds the scene and the Adaptive Presets' Masks, once per Photo.
        let analysis = render::analysis(lib, photo).ok();
        let fit: Vec<&Preset> = match &analysis {
            Some(a) => all
                .iter()
                .copied()
                .filter(|p| fits(p, &a.coverage()))
                .collect(),
            None => all.iter().copied().filter(|p| p.local.is_empty()).collect(),
        };
        let list = shuffled(&fit, photo.id);
        let s = sheet(photo, &base, &list, analysis, compute)?;
        let msg = Msg {
            role: Role::User,
            text: ask(photo, &list),
            images: vec![s.jpeg],
        };
        let reply = chat.next(SYSTEM, &[msg])?;
        // Back to indices in `all`, without picks the light meter says damage the Photo.
        let kept = parse_picks(&reply, list.len())?
            .into_iter()
            .filter(|(k, _)| !s.damaged[*k])
            .filter_map(|(k, why)| {
                all.iter()
                    .position(|p| p.id == list[k].id)
                    .map(|i| (i, why))
            })
            .collect();
        picks.push(kept);
        progress(i + 1, photos.len());
    }
    let (provider, model) = crate::usage::provider_model(provider);
    let ranking = Ranking {
        shoot_id: shoot,
        ranked: combine(&all, &picks),
        photos: photos.iter().map(|p| p.id).collect(),
        provider,
        model,
        presets: presets::library_hash(),
        at: crate::library::time::now(),
    };
    lib.set_setting(&key(shoot), &json!(ranking))?;
    Ok(ranking)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_photo_gets_its_own_order_of_the_same_presets() {
        let all: Vec<&Preset> = presets::builtin().iter().collect();
        let (a, b) = (shuffled(&all, 1), shuffled(&all, 2));
        assert_ne!(
            a.iter().map(|p| &p.id).collect::<Vec<_>>(),
            b.iter().map(|p| &p.id).collect::<Vec<_>>()
        );
        let mut ids: Vec<&String> = a.iter().map(|p| &p.id).collect();
        ids.sort();
        let mut want: Vec<&String> = all.iter().map(|p| &p.id).collect();
        want.sort();
        assert_eq!(ids, want);
    }

    #[test]
    fn sky_and_portrait_presets_only_fit_scenes_that_have_them() {
        let landscape = crate::masks::Coverage {
            people: 0.0,
            sky: 0.3,
            faces: 0,
        };
        let indoor_portrait = crate::masks::Coverage {
            people: 0.4,
            sky: 0.0,
            faces: 2,
        };
        let sky = presets::find("sky-blue").unwrap();
        let skin = presets::find("portrait-soft").unwrap();
        assert!(fits(sky, &landscape) && !fits(sky, &indoor_portrait));
        assert!(!fits(skin, &landscape) && fits(skin, &indoor_portrait));
        assert!(fits(presets::find("film-portrait").unwrap(), &landscape));
    }

    #[test]
    fn picks_parse_from_a_chatty_reply_and_drop_bad_numbers() {
        let r = "Sure: {\"best\": [{\"n\": 3, \"why\": \"warm skin\"}, {\"n\": 0, \"why\": \"x\"}, {\"n\": 3, \"why\": \"again\"}, {\"n\": 99, \"why\": \"\"}, {\"n\": 1, \"why\": \"clean\"}]}";
        assert_eq!(
            parse_picks(r, 10).unwrap(),
            vec![(2, "warm skin".into()), (0, "clean".into())]
        );
    }

    #[test]
    fn picks_add_up_across_photos_and_the_rest_keep_panel_order() {
        let all: Vec<&Preset> = presets::builtin().iter().take(5).collect();
        let picks = vec![
            vec![(3, "a".into()), (1, "b".into())],
            vec![(1, "c".into())],
        ];
        let r = combine(&all, &picks);
        let ids: Vec<&str> = r.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(
            ids,
            [&all[1].id, &all[3].id, &all[0].id, &all[2].id, &all[4].id].map(|s| s.as_str())
        );
        assert_eq!(r[0].score, 1.875);
        assert_eq!(r[0].why.as_deref(), Some("c"));
        assert_eq!(r[2].score, 0.0);
    }
}
