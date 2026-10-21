//! Exemplars: the person's own finished edits of scenes like this Photo, shown to the model
//! beside a fresh Suggestion so its takes lean toward their taste rather than a generic one.
//! Reference edits are the biggest single lever in the retouching papers (RetouchLLM,
//! PerTouch) and the way practitioners get an AI to edit "like me"; see
//! docs/research/ai-retouching-loops.md. Nothing is computed here: Photos without a stored
//! embedding, or that the person never edited by hand, are simply not candidates.

use crate::analyzers::{self, embedding};
use crate::library::{Author, Library, PhotoFilter, PhotoId, Verdict};
use crate::recipe::Recipe;
use crate::Result;
use serde_json::Value;

/// How many exemplars a fresh Suggestion shows unless a variant says otherwise.
pub const DEFAULT: usize = 2;
/// Below this cosine similarity a Photo is a different kind of scene, and its edit says
/// little about this one. Across 39 of Greg's favorites from different days the median pair
/// scored 0.09 and the 95th percentile 0.30, while pairs at 0.4-0.5 were the same kind of
/// scene (two people posing by water, a bright living room); same-burst frames score ~0.9.
pub const MIN_SIMILARITY: f32 = 0.35;

/// One candidate: a Photo the person edited by hand, with its embedding.
pub struct Candidate {
    pub photo_id: PhotoId,
    pub embedding: Vec<f32>,
}

/// The `n` candidates most like `target`, most similar first, above [`MIN_SIMILARITY`].
pub fn pick(target: &[f32], candidates: &[Candidate], n: usize) -> Vec<(PhotoId, f32)> {
    let mut scored: Vec<(PhotoId, f32)> = candidates
        .iter()
        .map(|c| (c.photo_id, embedding::similarity(target, &c.embedding)))
        .filter(|&(_, s)| s >= MIN_SIMILARITY)
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.truncate(n);
    scored
}

/// Up to `n` Photos the person finished by hand that look most like `photo`: their latest
/// Recipe Version is the person's own (not Auto's, not an accepted take left as it was),
/// it changes something, and the Photo isn't rejected.
pub fn find(lib: &Library, photo: PhotoId, n: usize) -> Result<Vec<(PhotoId, f32)>> {
    if n == 0 {
        return Ok(vec![]);
    }
    let Some(target) = stored_embedding(lib, photo)? else {
        return Ok(vec![]);
    };
    let blank = Recipe::default().to_value();
    let mut candidates = Vec::new();
    for p in lib.list_photos(&PhotoFilter::default())? {
        if p.id == photo || p.verdict == Verdict::Reject {
            continue;
        }
        let Some(v) = lib.read_recipe(p.id)? else {
            continue;
        };
        if v.author != Author::Person || same_edit(&v.recipe, &blank) {
            continue;
        }
        if let Some(e) = stored_embedding(lib, p.id)? {
            candidates.push(Candidate {
                photo_id: p.id,
                embedding: e,
            });
        }
    }
    Ok(pick(&target, &candidates, n))
}

/// A Recipe's visible settings in one compact line for a prompt: the Look it sits under,
/// its non-zero sliders, and how many Masks it has.
pub fn describe(r: &Recipe) -> String {
    let mut parts = Vec::new();
    if let Some(look) = &r.look {
        parts.push(format!("Look \"{look}\""));
    }
    if let Value::Object(g) = serde_json::to_value(&r.global).unwrap_or_default() {
        let moved: serde_json::Map<String, Value> = g
            .into_iter()
            .filter(|(_, v)| v.as_f64().is_some_and(|x| x.abs() > 1e-6))
            .collect();
        if !moved.is_empty() {
            parts.push(Value::Object(moved).to_string());
        }
    }
    if !r.local.is_empty() {
        parts.push(format!("{} Masks", r.local.len()));
    }
    if parts.is_empty() {
        "the default develop".into()
    } else {
        parts.join(", ")
    }
}

fn stored_embedding(lib: &Library, photo: PhotoId) -> Result<Option<Vec<f32>>> {
    Ok(lib.fact(photo, "embedding")?.and_then(|r| {
        embedding::vector(&analyzers::Fact {
            value: r.value,
            blob: r.blob,
        })
    }))
}

/// Two Recipe values that render the same (a reset to defaults isn't an edit).
fn same_edit(a: &Value, b: &Value) -> bool {
    Recipe::from_value(a)
        .map(|r| r.to_value() == *b)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(angle: f32) -> Vec<f32> {
        vec![angle.cos(), angle.sin()]
    }

    #[test]
    fn picks_the_most_alike_edits_and_skips_other_scenes() {
        // cos(0.2) = 0.98, cos(0.8) = 0.70, cos(1.5) = 0.07.
        let cands = [
            Candidate {
                photo_id: 1,
                embedding: unit(1.5),
            },
            Candidate {
                photo_id: 2,
                embedding: unit(0.8),
            },
            Candidate {
                photo_id: 3,
                embedding: unit(0.2),
            },
        ];
        let got = pick(&unit(0.0), &cands, 2);
        assert_eq!(got.iter().map(|g| g.0).collect::<Vec<_>>(), vec![3, 2]);
        let one: Vec<_> = pick(&unit(0.0), &cands, 5)
            .into_iter()
            .map(|g| g.0)
            .collect();
        assert_eq!(one, vec![3, 2], "an unrelated scene is never an exemplar");
        assert!(pick(&unit(0.0), &cands, 0).is_empty());
    }

    #[test]
    fn describes_only_what_the_edit_changes() {
        assert_eq!(describe(&Recipe::default()), "the default develop");
        let mut r = Recipe::default();
        r.global.exposure = 0.3;
        r.look = Some("shoot-look".into());
        let d = describe(&r);
        assert!(
            d.contains("Look \"shoot-look\"") && d.contains("\"exposure\":0.3"),
            "{d}"
        );
        assert!(!d.contains("contrast"), "{d}");
    }
}
