//! Suggestions: the AI proposes a few whole edits for a Photo (sliders, crop and
//! straighten), each saved apart from the Photo's Recipe until the person accepts one.
//!
//! Two flavours, one model request each:
//! - fresh: a few distinct edits, from the safest to the boldest
//! - matched to a reference Photo the person already edited ("apply to the rest"): the
//!   reference's style is a Look, and each Suggestion puts this Photo under that Look with
//!   its own Corrections and crop, so frames shot in different light still match.

use crate::agent::{Chat, Msg, Role};
use crate::compute::Compute;
use crate::library::{JobStatus, Library, PhotoId};
use crate::masks::Coverage;
use crate::recipe::{Adjust, Local, Recipe};
use crate::signals::Light;
use crate::style::Profile;
use crate::tools::{Part, Tools};
use crate::variant::TakeSpec;
use crate::{Error, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Long edge of what the model sees, and of the saved Suggestion previews.
const SEE_EDGE: u32 = 768;
const PREVIEW_EDGE: u32 = 1024;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestArgs {
    pub photo_id: PhotoId,
    /// How many Suggestions (default 3 fresh, 2 matched).
    pub count: Option<usize>,
    /// Match this reference Photo's edit, through the Look `look_id` made from it.
    pub reference_id: Option<PhotoId>,
    pub look_id: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Proposals {
    suggestions: Vec<Proposal>,
}

#[derive(Debug, Deserialize)]
struct Proposal {
    /// What the model sees the Photo (or this take) still needs, written before the values
    /// so it looks before it picks numbers.
    #[serde(default)]
    see: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    why: String,
    #[serde(default)]
    set: BTreeMap<String, f64>,
    /// In a review round: nothing left to fix.
    #[serde(default)]
    done: bool,
}

/// JSON Schema of the model's reply.
pub fn schema() -> Value {
    json!({
        "type": "object",
        "required": ["suggestions"],
        "properties": { "suggestions": { "type": "array", "items": {
            "type": "object",
            "required": ["see", "title", "why", "set", "done"],
            "properties": {
                "see": { "type": "string" },
                "title": { "type": "string" },
                "why": { "type": "string" },
                "set": { "type": "object", "additionalProperties": { "type": "number" } },
                "done": { "type": "boolean" }
            }
        } } }
    })
}

const SETTINGS: &str = "Settings (Lightroom Basic panel units, 0 = unchanged): exposure in stops (-5..5); \
contrast, highlights, shadows, whites, blacks, vibrance, saturation -100..100; temp (+ warmer) and tint \
(+ magenta) -100..100 relative to as shot; hsl.<red|orange|yellow|green|aqua|blue|purple|magenta>.<hue|sat|lum> \
-100..100; correction.exposure (stops) and correction.wbK (Kelvin, - cooler) for this Photo's baseline under a \
Look; crop.angle straightens in degrees (-45..45, + turns the picture clockwise); crop.x, crop.y, crop.w, crop.h \
crop as fractions 0..1 of the straightened frame (x, y = top left). Leave the crop out when the framing is fine. \
Presence: clarity, texture, dehaze -100..100. vignette -100..100 (- darkens the edges). Tone curve regions \
curve.<highlights|lights|darks|shadows> -100..100. Color grading grading.<shadows|midtones|highlights>.hue 0..360 \
and .sat 0..100 (cool shadows and warm highlights: shadows hue ~210, highlights hue ~45). With faces only: skin \
0..100 (natural smoothing, keep it under 25), eyes 0..100, teeth 0..100. Local adjustments through the AI Masks: \
local.<subject|background|sky|faces|skin|hair>.<any setting above> in the same units, added to the global value \
where that Mask covers, e.g. local.subject.exposure 0.25 lifts the people, local.background.exposure -0.15 lets the \
background fall off, local.sky.exposure -0.4 with local.sky.highlights -30 holds a bright sky, local.faces.shadows \
20 opens shadowed faces. Use only Masks for what the photo has.";

const FRESH: &str = "You edit photos for a photographer the way a skilled photographer edits in Lightroom: \
finished photos that look clearly better than the camera made them, with light shaped where it matters (the subject \
lifted off the background, a bright sky held, shadows on faces opened), presence, and colour with intent. Natural \
skin, nobody reshaped, no blown highlights or crushed blacks unless the mood asks for it. You see the Photo as it is \
now (first image) and as Lightroom-style Auto sets it (second image), each with its light measured in numbers. Trust \
the numbers over your eye for brightness and clipping. This photographer's finished edits average luma mean about 48 \
(35 for a moody scene, up to 65 for a bright beach), with p99 around 90-98.

You make {count} takes, each with its own character, listed below with the settings it starts from: Auto, plus an \
Adaptive layer of presence and Masks for what this photo has, plus the character's own starting look. Look at the \
photo and make each take the best version of its character for THIS photo: push further where the photo can take \
it, pull back what doesn't suit it (set it to 0), and add Masks, grading or curves it needs. The takes must look \
clearly different from each other. In the first take keep Auto's exposure and highlights. Straighten tilted \
horizons and verticals, and crop when the composition clearly gains (tighter on the subject, cut a distracting edge).

Reply with only JSON: {\"suggestions\": [{\"see\": \"what this take needs on this photo, one sentence\", \"title\": \
\"the character's name\", \"why\": \"one short sentence for the photographer\", \"set\": {\"local.sky.exposure\": -0.5, \
\"clarity\": 15}, \"done\": false}]}, one per take in the order given. \"set\" holds only the settings you change \
from that take's starting values, as absolute values.

";

const MATCHED: &str = "You edit photos for a photographer. They edited a reference Photo by hand (the first \
image) and want the rest of the Shoot to match it. Its style is already the Look \"{look}\" and goes under every \
Photo automatically, so do not repeat its sliders. Your job is this Photo (the second image, shown with the Look \
already applied): propose {count} ways to make it match the reference, with this Photo's own light and \
composition. Use correction.exposure and correction.wbK so its brightness and white balance match the reference, \
small slider changes only where its light differs (a backlit face needs shadows), and crop or straighten if the \
reference is framed that way or a horizon is tilted. The first proposal is the closest match; the second can \
crop differently or treat the light a little differently.

Reply with only JSON: {\"suggestions\": [{\"see\": \"how this Photo differs from the reference, one sentence\", \
\"title\": \"2-4 words\", \"why\": \"one short sentence\", \"set\": {\"correction.exposure\": 0.3, \
\"correction.wbK\": -200}, \"done\": false}]}.

";

const REVIEW: &str = "You are checking edits you proposed for a photographer, the way a careful photographer \
checks in Lightroom before calling a photo done. You see {before}, then each take rendered as it stands, with \
its light measured in numbers. Trust the numbers over your eye for brightness and clipping. This photographer's \
finished edits average luma mean about 48 (35 for a moody scene, up to 65 for a bright beach), with p99 around \
90-98 and almost nothing clipped.

For every take, in order: compare its render with the before image and its numbers, and name in \"see\" the one \
biggest problem left (the subject too dark or too bright, blown highlights, crushed or muddy shadows, a colour \
cast on skin or on things that should be neutral, too much or too little contrast or saturation, a tilted \
horizon, a crop that cuts the subject). Then return the take's \"set\" (as given, plus that one problem fixed), \
changing as little as possible and keeping the take's character (a moody take stays moody, black and white \
stays black and white). If nothing is clearly wrong, say so in \"see\", set \"done\": true and return the same \
\"set\". Do not keep nudging a good edit. Keep each take's title and why unless they no longer fit.

Reply with only JSON: {\"suggestions\": [{\"see\": \"...\", \"title\": \"...\", \"why\": \"...\", \"set\": \
{...}, \"done\": false}]}, one per take, in the same order.

";

/// Review rounds after the first proposal: render, look, critique, adjust. On the edit
/// bench (2026-10-18, claude-code) two rounds left the first take's dE unchanged (7.57) and
/// the best take a touch further from Greg (6.90 -> 6.99) while doubling the time per Photo,
/// so only Full (overnight, idle) spends one. `ARGUS_REFINE_ROUNDS` overrides.
pub fn rounds(compute: Compute) -> usize {
    if let Some(n) = std::env::var("ARGUS_REFINE_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        return n;
    }
    match compute {
        Compute::Gentle | Compute::Balanced => 0,
        Compute::Full => 1,
    }
}

/// One take as the loop carries it: the model's words, its sliders and how it renders.
struct Take {
    /// Where the take starts; its `set` goes on top.
    start: Recipe,
    title: String,
    why: String,
    set: BTreeMap<String, f64>,
    /// The person's usual corrections (`style::Profile`), added after `set`.
    style: BTreeMap<String, f64>,
    recipe: Recipe,
    light: Light,
    jpeg: Vec<u8>,
    done: bool,
}

/// Ask the model for Suggestions on one Photo, refine them for up to `rounds` review
/// rounds, and save them (replacing its open ones). Returns the new Suggestion ids.
pub fn suggest_photo(
    tools: &Tools,
    chat: &dyn Chat,
    a: &SuggestArgs,
    job: Option<i64>,
    rounds: usize,
) -> Result<Vec<i64>> {
    let v = tools.variant()?;
    // A variant's rounds, unless ARGUS_REFINE_ROUNDS pins them.
    let rounds = match v.rounds {
        Some(n) if std::env::var("ARGUS_REFINE_ROUNDS").is_err() => n,
        _ => rounds,
    };
    let characters = v.takes.clone().unwrap_or_else(builtin_takes);
    let settings = v.settings.as_deref().unwrap_or(SETTINGS);
    let keep = v.keep_brightness.unwrap_or(true);
    let steps = Steps::new(tools, job, rounds)?;
    steps.at(0)?;
    let lib = Library::open(tools.library())?;
    let current = match lib.read_recipe(a.photo_id)? {
        Some(v) => Recipe::from_value(&v.recipe)?,
        None => Recipe::default(),
    };
    let matched = a.reference_id.is_some();
    // How the person corrects takes, from the usage log (a variant can pin it). Matched
    // takes copy their hand-edited reference, which is already their style.
    let style = match (&v.style, matched) {
        (_, true) => Profile::default(),
        (Some(moves), _) => Profile {
            moves: moves.clone(),
            ..Default::default()
        },
        (None, _) => Profile::of_library(&lib).unwrap_or_default(),
    };
    drop(lib);
    let count = a.count.unwrap_or(if matched { 2 } else { 3 }).clamp(1, 5);

    // Each Suggestion is a whole edit, so it starts from cleared sliders and crop. Matched
    // ones also put the Photo under the reference's Look with its own Corrections cleared,
    // as Lightroom's Sync replaces settings.
    // The AI's own Masks from earlier takes go too; the person's stay.
    let mut base = current.clone();
    base.global = Default::default();
    base.crop = Default::default();
    base.local.retain(|l| !l.id.starts_with(AI_LOCAL));
    let scene = scene(tools, a.photo_id);
    if matched {
        base.look = a.look_id.clone();
        base.correction = Default::default();
    }

    // What the model sees before it edits: the reference (when matching), the Photo, and
    // for a fresh edit the deterministic Auto anchor.
    let mut before = Vec::new();
    let mut text = String::new();
    if let Some(r) = a.reference_id {
        let (img, info) = render(tools, r, None)?;
        before.push(img);
        text += &format!(
            "Reference Photo {r}, as the photographer edited it. Its Recipe: {}\nIts light: {}\n",
            info["recipe"],
            info["light"].as_str().unwrap_or("")
        );
    }
    let shown = if matched { &base } else { &current };
    let (img, info) = render(tools, a.photo_id, Some(shown))?;
    before.push(img);
    text += &format!(
        "Photo {} as it is now{}. Its Recipe: {}\nIts light: {}\n",
        a.photo_id,
        if matched {
            ", under the reference's Look"
        } else {
            ""
        },
        shown.to_value(),
        info["light"].as_str().unwrap_or("")
    );
    let mut images = before.clone();
    let mut anchor = None;
    if let Some(c) = scene {
        text += &format!("{}\n", scene_line(c));
    }
    // Where each take starts. Matched takes start from the Photo under the Look; fresh ones
    // from Auto, the Adaptive layer and their character's look, so even a timid model
    // hands back a finished edit, and the takes differ.
    let mut starts = vec![base.clone(); count];
    if !matched {
        // Auto failing (an odd file) costs the anchor, not the Suggestions.
        let mut from = base.clone();
        if let Ok(auto) = tools.auto_recipe(a.photo_id, &base) {
            let (img, info) = render(tools, a.photo_id, Some(&auto))?;
            images.push(img);
            text += &format!(
                "Auto's edit (second image): {}\nIts light: {}\n",
                auto_values(&auto),
                info["light"].as_str().unwrap_or("")
            );
            from = auto.clone();
            anchor = Some(auto);
        }
        let adaptive = if v.adaptive.unwrap_or(true) {
            adaptive(scene)
        } else {
            vec![]
        };
        if !style.is_empty() {
            text += &format!(
                "\nThis photographer's own habit, learned from how they finished earlier takes, goes on top of each \
                 take after your settings: {}. Don't add it yourself.\n",
                Profile::line(&style.moves)
            );
        }
        text += "\nThe takes:\n";
        for (i, start) in starts.iter_mut().enumerate() {
            let c = &characters[i % characters.len()];
            *start = from.clone();
            let moves: Vec<(&str, f64)> = adaptive
                .iter()
                .copied()
                .chain(c.start.iter().map(|(k, v)| (k.as_str(), *v)))
                .collect();
            for &(path, v) in &moves {
                if allowed(path, scene) {
                    let _ = apply_setting(start, path, v, true);
                }
            }
            let mut shown: BTreeMap<String, f64> = BTreeMap::new();
            for k in crate::auto::AUTO_SLIDERS
                .iter()
                .copied()
                .chain(moves.iter().map(|m| m.0))
            {
                if let Some(v) = setting(start, k).filter(|v| v.abs() > 1e-6) {
                    shown.insert(k.into(), (v * 100.0).round() / 100.0);
                }
            }
            text += &format!(
                "Take {} \"{}\": {} Starts at {}\n",
                i + 1,
                c.name,
                c.brief,
                serde_json::to_string(&shown).unwrap_or_default()
            );
        }
    }
    text += &format!("Propose {count} Suggestions.");
    let system = if matched {
        MATCHED.replace(
            "{look}",
            a.look_id.as_deref().unwrap_or("the reference's Look"),
        )
    } else {
        v.fresh.clone().unwrap_or_else(|| FRESH.into())
    }
    .replace("{count}", &count.to_string())
        + settings;

    steps.at(1)?;
    let proposals = ask(
        chat,
        &system,
        Msg {
            role: Role::User,
            text,
            images,
        },
    )?;
    steps.at(2)?;
    let mut takes = Vec::new();
    for (i, mut p) in proposals.suggestions.into_iter().take(count).enumerate() {
        if i == 0 && keep {
            keep_brightness(&mut p, anchor.as_ref());
        }
        if !matched {
            // The character names stay put, so benches and the usage log can tell takes apart.
            p.title = characters[i % characters.len()].name.clone();
        }
        let own = style.for_take(&p.title).clone();
        takes.push(take(tools, a.photo_id, &starts[i], p, &own, scene)?);
    }
    if takes.is_empty() {
        return Err(Error("the model proposed no Suggestions".into()));
    }
    trace(a.photo_id, 0, &takes);

    // Render, look, critique, adjust. The model judges its own work unreliably, so a
    // revision that clips or crushes clearly more than the take it replaces is refused,
    // and a take the model calls done, or barely changes, stops moving.
    let review = v.review.as_deref().unwrap_or(REVIEW).replace(
        "{before}",
        if matched {
            "the reference Photo (first image), this Photo before your edits under the reference's Look"
        } else {
            "the Photo before your edits (first image)"
        },
    ) + settings;
    for round in 0..rounds {
        if takes.iter().all(|t| t.done) {
            break;
        }
        steps.at(3 + round)?;
        let mut text = String::new();
        let mut images = before.clone();
        for (i, t) in takes.iter().enumerate() {
            images.push(t.jpeg.clone());
            text += &format!(
                "Take {} \"{}\" (image {}): set {}\n  light: {}\n",
                i + 1,
                t.title,
                images.len(),
                serde_json::to_string(&t.set).unwrap_or_default(),
                t.light.line()
            );
        }
        text += &format!(
            "Review round {} of {rounds}: check each take and fix its biggest problem.",
            round + 1
        );
        let Ok(revised) = ask(
            chat,
            &review,
            Msg {
                role: Role::User,
                text,
                images,
            },
        ) else {
            break; // keep the takes we have
        };
        for (i, (t, mut p)) in takes.iter_mut().zip(revised.suggestions).enumerate() {
            if t.done {
                continue;
            }
            if i == 0 && keep {
                keep_brightness(&mut p, anchor.as_ref());
            }
            eprintln!(
                "argus refine: Photo {} round {} \"{}\": {}{}",
                a.photo_id,
                round + 1,
                t.title,
                match p.see.trim() {
                    "" => "(no critique, stopping)",
                    see => see,
                },
                if p.done { " (done)" } else { "" }
            );
            // No named problem means nothing to fix: an empty critique ends the take
            // rather than applying an unexplained change.
            if p.done || p.see.trim().is_empty() || barely_changed(&t.set, &p.set) {
                t.done = true;
                continue;
            }
            let (title, why) = (t.title.clone(), t.why.clone());
            if !matched {
                p.title = title.clone();
            }
            let next = take(tools, a.photo_id, &t.start, p, &t.style, scene)?;
            if worse_light(&t.light, &next.light) {
                t.done = true;
                continue;
            }
            *t = Take {
                title: if next.title.is_empty() {
                    title
                } else {
                    next.title
                },
                why: if next.why.is_empty() { why } else { next.why },
                ..next
            };
        }
        trace(a.photo_id, round + 1, &takes);
    }

    steps.at(3 + rounds)?;
    let mut list = Vec::new();
    for (i, t) in takes.into_iter().enumerate() {
        let preview = save_preview(tools, a.photo_id, &t.recipe, job, i)?;
        list.push((t.title, t.why, t.recipe.to_value(), Some(preview)));
    }
    // Cancelled while the takes rendered: nothing lands.
    steps.at(3 + rounds)?;
    let mut lib = Library::open(tools.library())?;
    lib.replace_suggestions(a.photo_id, job, a.reference_id, &list)
}

/// A Suggestions job's progress as steps (`done` of `total` on the job), so the Edit strip
/// can say what it's doing, and its cancel checkpoints: a cancelled job stops at its next
/// step. Without a job it does nothing.
struct Steps {
    lib: Option<(Library, i64)>,
    total: usize,
}

impl Steps {
    fn new(tools: &Tools, job: Option<i64>, rounds: usize) -> Result<Self> {
        let lib = match job {
            Some(id) => Some((Library::open(tools.library())?, id)),
            None => None,
        };
        Ok(Steps {
            lib,
            total: 4 + rounds,
        })
    }

    fn at(&self, step: usize) -> Result<()> {
        let Some((lib, id)) = &self.lib else {
            return Ok(());
        };
        if lib.job(*id)?.status == JobStatus::Cancelled {
            return Err(Error("cancelled".into()));
        }
        lib.job_progress(*id, step as i64, self.total as i64)
    }
}

/// What step `done` of a Suggestions job of `total` steps is (see [`Steps`]).
pub fn stage(done: i64, total: i64) -> String {
    match done {
        _ if total <= 0 => "Starting".into(),
        0 => "Looking at the photo".into(),
        1 => "Model thinking".into(),
        2 => "Rendering takes".into(),
        d if d >= total - 1 => "Saving takes".into(),
        d => format!("Review round {} of {}", d - 2, total - 4),
    }
}

/// One model request, with one retry when the reply isn't the JSON asked for.
fn ask(chat: &dyn Chat, system: &str, msg: Msg) -> Result<Proposals> {
    let mut convo = vec![msg];
    for _ in 0..2 {
        let raw = chat.next(system, &convo)?;
        match parse(&raw) {
            Ok(p) => return Ok(p),
            Err(e) => {
                convo.push(Msg {
                    role: Role::Agent,
                    text: raw,
                    images: vec![],
                });
                convo.push(Msg {
                    role: Role::User,
                    text: format!("{e}. Reply with only the JSON object."),
                    images: vec![],
                });
            }
        }
    }
    Err(Error("the model didn't return Suggestions as JSON".into()))
}

/// The natural (first) take of a fresh edit keeps Auto's Exposure and Highlights. On the
/// edit bench (2026-10-19, claude-code, 14 cases, dE to Greg's edits) the model's first take
/// was further from Greg than Auto (7.82 against 6.76; no edit 8.31), and putting back Auto's
/// value one slider at a time showed nearly all of that gap was brightness: Exposure (it
/// pulled bright scenes like a beach down by up to half a stop toward the "mean 48" it was
/// told about) and Highlights (16 lower than Auto on average). With both kept from Auto the
/// first take scored 6.79. Brightness is what the model judges worst (see
/// docs/research/ai-retouching-loops.md); the bolder takes still choose their own.
fn keep_brightness(p: &mut Proposal, auto: Option<&Recipe>) {
    if let Some(auto) = auto {
        p.set.insert("exposure".into(), auto.global.exposure);
        p.set.insert("highlights".into(), auto.global.highlights);
    }
}

/// A proposal applied to its take's `start` and rendered for the model to look at.
fn take(
    tools: &Tools,
    id: PhotoId,
    start: &Recipe,
    p: Proposal,
    style: &BTreeMap<String, f64>,
    scene: Option<Coverage>,
) -> Result<Take> {
    let mut r = start.clone();
    for (path, &v) in &p.set {
        // A setting the model made up, or a Mask for something the photo doesn't have, is
        // skipped, not fatal: the rest is still a good edit.
        if allowed(path, scene) {
            let _ = apply_setting(&mut r, path, v, false);
        }
    }
    for (path, &v) in style {
        let _ = apply_setting(&mut r, path, v, true);
    }
    r.global = r.global.clamped();
    let (jpeg, info) = render(tools, id, Some(&r))?;
    let light = serde_json::from_value(info["light_stats"].clone()).unwrap_or_default();
    Ok(Take {
        start: start.clone(),
        title: p.title.trim().to_string(),
        why: p.why.trim().to_string(),
        set: p.set,
        style: style.clone(),
        recipe: r,
        light,
        jpeg,
        done: false,
    })
}

/// With `ARGUS_REFINE_TRACE=<file>`, append every round's takes as JSON lines, so the
/// bench can score each round and see whether reviewing helps or drifts.
fn trace(photo: PhotoId, round: usize, takes: &[Take]) {
    let Ok(path) = std::env::var("ARGUS_REFINE_TRACE") else {
        return;
    };
    let lines: String = takes
        .iter()
        .enumerate()
        .map(|(i, t)| {
            json!({ "photo_id": photo, "round": round, "take": i, "title": t.title,
                "recipe": t.recipe.to_value() })
            .to_string()
                + "\n"
        })
        .collect();
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(lines.as_bytes());
    }
}

/// One character of a fresh take: its name (the take's title, the same every time so
/// benches and the usage log can tell takes apart), what it is for, and how its start moves
/// off Auto and the Adaptive layer (added to them).
struct Character {
    name: &'static str,
    brief: &'static str,
    start: &'static [(&'static str, f64)],
}

/// The takes of a fresh edit, in order. Starting values are deliberately visible: on the
/// edit bench the model nudged Auto when left to itself, and Greg found the takes too timid
/// (2026-10-19).
const CHARACTERS: &[Character] = &[
    Character {
        name: "Natural",
        brief: "The finished, honest version: light where it belongs, the subject lifted, the sky held, a little presence. What a careful photographer delivers.",
        start: &[],
    },
    Character {
        name: "Punchy",
        brief: "Vivid and crisp: more contrast and local contrast, richer colour, the subject popping, a deeper sky. Bold but believable; skin stays natural.",
        start: &[
            ("contrast", 15.0),
            ("clarity", 10.0),
            ("texture", 6.0),
            ("dehaze", 6.0),
            ("vibrance", 15.0),
            ("local.subject.exposure", 0.1),
            ("local.sky.saturation", 12.0),
        ],
    },
    Character {
        name: "Moody",
        brief: "A deliberate mood: darker and denser, colour graded (cool shadows and warm highlights, or a faded film look), softer highlights, edges falling off. Faces stay readable.",
        start: &[
            ("exposure", -0.3),
            ("contrast", 12.0),
            ("highlights", -20.0),
            ("vibrance", -10.0),
            ("curve.shadows", -10.0),
            ("grading.shadows.hue", 210.0),
            ("grading.shadows.sat", 15.0),
            ("grading.highlights.hue", 45.0),
            ("grading.highlights.sat", 12.0),
            ("vignette", -18.0),
        ],
    },
    Character {
        name: "Black and white",
        brief: "Black and white with strong tonal separation: deep blacks, bright whites, presence.",
        start: &[("saturation", -100.0), ("contrast", 20.0), ("clarity", 10.0)],
    },
    Character {
        name: "Airy",
        brief: "Bright and soft: lifted exposure and shadows, gentle contrast, clean whites, pastel colour.",
        start: &[
            ("exposure", 0.3),
            ("shadows", 15.0),
            ("contrast", -10.0),
            ("highlights", -10.0),
            ("vibrance", -5.0),
        ],
    },
];

/// The built-in takes as a variant file would give them.
fn builtin_takes() -> Vec<TakeSpec> {
    CHARACTERS
        .iter()
        .map(|c| TakeSpec {
            name: c.name.into(),
            brief: c.brief.into(),
            start: c.start.iter().map(|&(k, v)| (k.to_string(), v)).collect(),
        })
        .collect()
}

/// Local adjustments a take adds have ids starting with this, one per Mask kind.
const AI_LOCAL: &str = "ai-";

/// The Mask kinds a take may use, by `local.<kind>.<setting>`.
const LOCAL_KINDS: &[&str] = &["subject", "background", "sky", "faces", "skin", "hair"];

/// Lightroom's Adaptive presets, built from what the Mask models found: presence on the
/// whole frame, the people lifted with the background falling off a little, a bright sky
/// held. Added to Auto. A fixed pass like this looked clearly better than Auto on the
/// sample RAWs (/mnt/project-files/argus-ai-edits, 2026-10-19).
fn adaptive(scene: Option<Coverage>) -> Vec<(&'static str, f64)> {
    let mut v = vec![
        ("clarity", 10.0),
        ("texture", 6.0),
        ("dehaze", 4.0),
        ("contrast", 8.0),
        ("vibrance", 8.0),
    ];
    let Some(c) = scene else {
        return v;
    };
    if c.people >= PEOPLE {
        v.push(("local.subject.exposure", 0.2));
        if c.people < 0.7 {
            v.push(("local.background.exposure", -0.12));
        }
    }
    if c.sky >= SKY {
        v.push(("local.sky.exposure", -0.3));
        v.push(("local.sky.highlights", -20.0));
    }
    v
}

/// Shares of the frame below which people or sky aren't worth a Mask.
const PEOPLE: f32 = 0.02;
const SKY: f32 = 0.04;

/// What the Mask models see in the Photo, if it can be analysed.
fn scene(tools: &Tools, id: PhotoId) -> Option<Coverage> {
    let lib = Library::open(tools.library()).ok()?;
    let photo = lib.photo(id).ok()??;
    crate::render::analysis(&lib, &photo)
        .ok()
        .map(|a| a.coverage())
}

fn scene_line(c: Coverage) -> String {
    let pct = |v: f32| (v * 100.0).round();
    let mut kinds: Vec<&str> = Vec::new();
    if c.people >= PEOPLE {
        kinds.extend(["subject", "background", "skin", "hair"]);
        if c.faces > 0 {
            kinds.push("faces");
        }
    }
    if c.sky >= SKY {
        kinds.push("sky");
    }
    format!(
        "What the Mask models found: people cover {}% of the frame ({} faces), sky {}%. Masks you can use: {}.",
        pct(c.people),
        c.faces,
        pct(c.sky),
        if kinds.is_empty() { "none".to_string() } else { kinds.join(", ") }
    )
}

/// Whether a setting fits the photo: a local one only through a Mask for something it has
/// (unknown when it couldn't be analysed, so allowed).
fn allowed(path: &str, scene: Option<Coverage>) -> bool {
    let Some(kind) = path
        .strip_prefix("local.")
        .and_then(|r| r.split('.').next())
    else {
        return true;
    };
    let Some(c) = scene else {
        return LOCAL_KINDS.contains(&kind);
    };
    match kind {
        "sky" => c.sky >= SKY,
        "faces" => c.people >= PEOPLE && c.faces > 0,
        "subject" | "background" | "skin" | "hair" => c.people >= PEOPLE,
        _ => false,
    }
}

/// [`Recipe::set`], plus `local.<kind>.<setting>`: that setting on the take's local
/// adjustment through an AI Mask of that kind, made the first time it's needed.
fn apply_setting(r: &mut Recipe, path: &str, v: f64, relative: bool) -> Result<()> {
    let Some(rest) = path.strip_prefix("local.") else {
        return r.set(path, v, relative);
    };
    let (kind, slider) = rest
        .split_once('.')
        .filter(|(k, s)| {
            LOCAL_KINDS.contains(k) && !s.contains("crop.") && !s.starts_with("correction.")
        })
        .ok_or_else(|| Error(format!("unknown setting {path:?}")))?;
    let id = format!("{AI_LOCAL}{kind}");
    if !r.local.iter().any(|l| l.id == id) {
        let mask = serde_json::from_value(json!({ "kind": kind }))
            .map_err(|e| Error(format!("{path}: {e}")))?;
        let mut name = kind.to_string();
        name[..1].make_ascii_uppercase();
        r.local.push(Local {
            id: id.clone(),
            name: format!("{name} (AI)"),
            mask,
            adjust: Adjust::default(),
            source: Some("ai".into()),
            other: Default::default(),
        });
    }
    let l = r.local_mut(&id)?;
    let mut one = Recipe {
        global: l.adjust.clone(),
        ..Recipe::default()
    };
    one.set(slider, v, relative)?;
    l.adjust = one.global;
    Ok(())
}

/// A setting's value in a Recipe by the same paths as [`apply_setting`] (None when the
/// path names nothing there).
fn setting(r: &Recipe, path: &str) -> Option<f64> {
    let (root, rest) = match path.strip_prefix("local.") {
        Some(rest) => {
            let (kind, slider) = rest.split_once('.')?;
            let l = r
                .local
                .iter()
                .find(|l| l.id == format!("{AI_LOCAL}{kind}"))?;
            (serde_json::to_value(&l.adjust).ok()?, slider)
        }
        None => (serde_json::to_value(&r.global).ok()?, path),
    };
    let mut node = &root;
    for k in rest.split('.') {
        match node.get(k) {
            Some(n) => node = n,
            None => return Some(0.0),
        }
    }
    node.as_f64()
}

/// Auto's sliders as a compact `{"exposure":0.4,...}`.
fn auto_values(auto: &Recipe) -> String {
    let g = serde_json::to_value(&auto.global).unwrap_or_default();
    let picked: serde_json::Map<String, Value> = crate::auto::AUTO_SLIDERS
        .iter()
        .filter_map(|k| g.get(*k).map(|v| (k.to_string(), v.clone())))
        .collect();
    Value::Object(picked).to_string()
}

/// True when a revision moves no setting by a visible amount.
fn barely_changed(a: &BTreeMap<String, f64>, b: &BTreeMap<String, f64>) -> bool {
    a.keys().chain(b.keys()).all(|k| {
        let d = (a.get(k).copied().unwrap_or(0.0) - b.get(k).copied().unwrap_or(0.0)).abs();
        let step = match k.as_str() {
            "exposure" | "correction.exposure" => 0.05,
            "correction.wbK" => 50.0,
            "crop.angle" => 0.3,
            k if k.starts_with("crop.") => 0.01,
            _ => 2.0,
        };
        d < step
    })
}

/// A revision that blows or crushes clearly more of the frame than the take it replaces.
fn worse_light(was: &Light, now: &Light) -> bool {
    let worse = |a: f32, b: f32| b > 1.0 && b > a + (0.5f32).max(a * 0.5);
    worse(was.clipped_pct, now.clipped_pct) || worse(was.crushed_pct, now.crushed_pct)
}

fn parse(text: &str) -> Result<Proposals> {
    let (Some(a), Some(b)) = (text.find('{'), text.rfind('}')) else {
        return Err(Error("your reply had no JSON object".into()));
    };
    serde_json::from_str(&text[a..=b])
        .map_err(|e| Error(format!("your reply was not valid JSON ({e})")))
}

/// Render a Photo (with `recipe`, else its own) for the model: the JPEG and the tool's JSON.
fn render(tools: &Tools, id: PhotoId, recipe: Option<&Recipe>) -> Result<(Vec<u8>, Value)> {
    let mut args = json!({ "photo_id": id, "long_edge": SEE_EDGE });
    if let Some(r) = recipe {
        args["recipe"] = r.to_value();
    }
    let mut info = Value::Null;
    let mut jpeg = None;
    for p in tools.call("render_photo", args)? {
        match p {
            Part::Json(v) => info = v,
            Part::Jpeg(b) => jpeg = Some(b),
        }
    }
    Ok((
        jpeg.ok_or_else(|| Error("render_photo returned no image".into()))?,
        info,
    ))
}

/// Render a Suggestion into the Library's previews folder; returns its relative path.
fn save_preview(
    tools: &Tools,
    id: PhotoId,
    recipe: &Recipe,
    job: Option<i64>,
    i: usize,
) -> Result<String> {
    let parts = tools.call(
        "render_photo",
        json!({ "photo_id": id, "long_edge": PREVIEW_EDGE, "recipe": recipe.to_value() }),
    )?;
    let Some(Part::Jpeg(jpeg)) = parts.into_iter().find(|p| matches!(p, Part::Jpeg(_))) else {
        return Err(Error("render_photo returned no image".into()));
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let rel = format!(
        "{}/suggestions/{id}-{}-{i}-{stamp}.jpg",
        crate::library::CACHE_DIR,
        job.unwrap_or(0)
    );
    let path = tools.library().join(&rel);
    std::fs::create_dir_all(path.parent().unwrap())
        .map_err(|e| Error(format!("suggestion preview: {e}")))?;
    std::fs::write(&path, jpeg).map_err(|e| Error(format!("suggestion preview: {e}")))?;
    Ok(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_settings_make_one_ai_mask_per_kind() {
        let mut r = Recipe::default();
        apply_setting(&mut r, "local.sky.exposure", -0.4, false).unwrap();
        apply_setting(&mut r, "local.sky.highlights", -20.0, true).unwrap();
        apply_setting(&mut r, "local.sky.exposure", -0.1, true).unwrap();
        apply_setting(&mut r, "clarity", 12.0, false).unwrap();
        assert_eq!(r.local.len(), 1);
        let sky = &r.local[0];
        assert_eq!((sky.id.as_str(), sky.mask.kind()), ("ai-sky", "sky"));
        assert!((sky.adjust.exposure + 0.5).abs() < 1e-9);
        assert_eq!(sky.adjust.highlights, -20.0);
        assert_eq!(setting(&r, "local.sky.highlights"), Some(-20.0));
        assert_eq!(setting(&r, "clarity"), Some(12.0));
        assert!(apply_setting(&mut r, "local.moon.exposure", 1.0, false).is_err());
        assert!(apply_setting(&mut r, "local.sky.crop.angle", 1.0, false).is_err());
    }

    #[test]
    fn masks_only_for_what_the_photo_has() {
        let beach = Coverage {
            people: 0.0,
            sky: 0.4,
            faces: 0,
        };
        assert!(allowed("local.sky.exposure", Some(beach)));
        assert!(!allowed("local.subject.exposure", Some(beach)));
        assert!(allowed("exposure", Some(beach)));
        assert!(allowed("local.faces.shadows", None));
        let kinds: Vec<&str> = adaptive(Some(beach)).iter().map(|m| m.0).collect();
        assert!(
            kinds.contains(&"local.sky.exposure") && !kinds.contains(&"local.subject.exposure")
        );
    }
}
