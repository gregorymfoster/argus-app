//! Stacks found without a model: runs of frames shot seconds apart that look alike (by
//! the embedding Fact). A cull's Stacks come from the model, which sometimes leaves a
//! burst apart; these fill the gaps, so every burst can be culled as one Stack.

use crate::analyzers::{self, embedding};
use crate::cull::{seconds, NEAR_IDENTICAL, SAME_SCENE};
use crate::library::{Library, Photo, PhotoFilter, PhotoId, ShootId};
use crate::Result;
use std::collections::HashMap;

/// Longest pause between two frames of one burst, and between two near-identical frames
/// (the same shot tried again a little later).
const BURST_GAP_S: i64 = 30;
const RETRY_GAP_S: i64 = 300;

/// What grouping needs to know about one frame, in capture order.
pub struct Frame {
    pub id: PhotoId,
    /// Seconds since any fixed epoch; 0 when unknown.
    pub at: i64,
    pub embedding: Option<Vec<f32>>,
    /// Higher is the better frame of a burst (fewer closed eyes, then sharper).
    pub quality: f32,
}

/// Bursts among `frames` (in capture order), each best frame first. A frame joins the
/// one before it when they look alike and were shot close together.
pub fn group(frames: &[Frame]) -> Vec<Vec<PhotoId>> {
    let mut runs: Vec<Vec<&Frame>> = Vec::new();
    for (i, f) in frames.iter().enumerate() {
        let joins = i.checked_sub(1).map(|j| &frames[j]).is_some_and(|prev| {
            let (Some(a), Some(b)) = (&prev.embedding, &f.embedding) else {
                return false;
            };
            let gap = (f.at - prev.at).abs();
            let sim = embedding::similarity(a, b);
            (sim >= SAME_SCENE && gap <= BURST_GAP_S)
                || (sim >= NEAR_IDENTICAL && gap <= RETRY_GAP_S)
        });
        match runs.last_mut() {
            Some(run) if joins => run.push(f),
            _ => runs.push(vec![f]),
        }
    }
    runs.into_iter()
        .filter(|r| r.len() > 1)
        .map(|mut r| {
            r.sort_by(|a, b| b.quality.total_cmp(&a.quality));
            r.into_iter().map(|f| f.id).collect()
        })
        .collect()
}

/// `stacks` (best first, e.g. a model's) joined with `bursts`: Stacks that share a frame
/// become one. Frames keep their order from `stacks`; frames only a burst found follow
/// in the burst's order.
pub fn merge(stacks: &[Vec<PhotoId>], bursts: &[Vec<PhotoId>]) -> Vec<Vec<PhotoId>> {
    // Union-find over Photo ids.
    let mut parent: HashMap<PhotoId, PhotoId> = HashMap::new();
    fn root(parent: &mut HashMap<PhotoId, PhotoId>, x: PhotoId) -> PhotoId {
        let p = *parent.entry(x).or_insert(x);
        if p == x {
            return x;
        }
        let r = root(parent, p);
        parent.insert(x, r);
        r
    }
    // Every frame in order of preference: the given Stacks' frames first.
    let mut order: Vec<PhotoId> = Vec::new();
    for list in stacks.iter().chain(bursts) {
        for &id in list {
            if !parent.contains_key(&id) {
                order.push(id);
            }
            root(&mut parent, id);
        }
        for w in list.windows(2) {
            let (a, b) = (root(&mut parent, w[0]), root(&mut parent, w[1]));
            if a != b {
                parent.insert(b, a);
            }
        }
    }
    let mut groups: Vec<(PhotoId, Vec<PhotoId>)> = Vec::new();
    for id in order {
        let r = root(&mut parent, id);
        match groups.iter_mut().find(|(g, _)| *g == r) {
            Some((_, m)) => m.push(id),
            None => groups.push((r, vec![id])),
        }
    }
    groups
        .into_iter()
        .map(|(_, m)| m)
        .filter(|m| m.len() > 1)
        .collect()
}

/// A Shoot's bursts from the Facts already stored (nothing is computed here, so it's
/// cheap; Photos without an embedding yet stay out).
pub fn find(lib: &Library, photos: &[Photo]) -> Result<Vec<Vec<PhotoId>>> {
    let mut frames = Vec::with_capacity(photos.len());
    for p in photos {
        let emb = lib.fact(p.id, "embedding")?.and_then(|r| {
            embedding::vector(&analyzers::Fact {
                value: r.value,
                blob: r.blob,
            })
        });
        let sharp = lib
            .fact(p.id, "signals")?
            .map(|r| {
                analyzers::signals::read(&analyzers::Fact {
                    value: r.value,
                    blob: r.blob,
                })
                .sharpness
            })
            .unwrap_or(0.0);
        let closed = lib
            .fact(p.id, "eyes")?
            .map(|r| {
                analyzers::eyes::read(&analyzers::Fact {
                    value: r.value,
                    blob: r.blob,
                })
                .iter()
                .filter(|f| f.read && f.eyes_closed)
                .count()
            })
            .unwrap_or(0);
        frames.push(Frame {
            id: p.id,
            at: seconds(&p.captured_at),
            embedding: emb,
            // Sharpness is a few thousand at most, so a closed pair of eyes always weighs more.
            quality: sharp - closed as f32 * 1e6,
        });
    }
    Ok(group(&frames))
}

/// The Facts bursts are found from.
pub const FACTS: &[&str] = &["signals", "eyes", "embedding"];

/// Join a Shoot's bursts into its Stacks and save them, first computing any Facts still
/// missing when `compute` (else Photos without them stay out). Returns the Stacks, best first.
pub fn stack_shoot(lib: &mut Library, shoot: ShootId, compute: bool) -> Result<Vec<Vec<PhotoId>>> {
    let photos = lib.list_photos(&PhotoFilter {
        shoot: Some(shoot),
        verdict: None,
    })?;
    if compute {
        analyzers::facts_for(lib, &photos, FACTS)?;
    }
    let merged = merge(&current_stacks(&photos), &find(lib, &photos)?);
    lib.replace_stacks(shoot, &merged)?;
    Ok(merged)
}

/// The Stacks these Photos are in, each best first.
pub fn current_stacks(photos: &[Photo]) -> Vec<Vec<PhotoId>> {
    let mut by: std::collections::BTreeMap<i64, Vec<(i64, PhotoId)>> = Default::default();
    for p in photos {
        if let Some(s) = p.stack {
            by.entry(s)
                .or_default()
                .push((p.stack_rank.unwrap_or(i64::MAX), p.id));
        }
    }
    by.into_values()
        .map(|mut m| {
            m.sort();
            m.into_iter().map(|(_, id)| id).collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(angle: f32) -> Vec<f32> {
        vec![angle.cos(), angle.sin()]
    }
    fn frame(id: PhotoId, at: i64, angle: f32, quality: f32) -> Frame {
        Frame {
            id,
            at,
            embedding: Some(unit(angle)),
            quality,
        }
    }

    #[test]
    fn a_burst_is_alike_frames_seconds_apart_best_first() {
        // cos(0.1) = 0.995, cos(0.4) = 0.92, cos(1.2) = 0.36.
        let frames = [
            frame(1, 0, 0.0, 1.0),
            frame(2, 1, 0.1, 5.0),
            frame(3, 2, 0.4, 2.0),
            frame(4, 3, 1.6, 0.0), // a new scene
            frame(5, 4, 1.6, 0.0),
            frame(6, 600, 1.6, 0.0), // same scene, ten minutes later
        ];
        assert_eq!(group(&frames), vec![vec![2, 3, 1], vec![4, 5]]);
    }

    #[test]
    fn near_identical_frames_stack_minutes_apart_but_lookalikes_do_not() {
        let frames = [
            frame(1, 0, 0.0, 0.0),
            frame(2, 120, 0.1, 1.0),
            frame(3, 240, 0.5, 0.0),
        ];
        assert_eq!(group(&frames), vec![vec![2, 1]]);
    }

    #[test]
    fn frames_without_an_embedding_stay_apart() {
        let mut frames = vec![frame(1, 0, 0.0, 0.0), frame(2, 1, 0.0, 0.0)];
        frames[1].embedding = None;
        assert!(group(&frames).is_empty());
    }

    #[test]
    fn merge_joins_stacks_that_share_a_frame_and_keeps_the_model_order() {
        let model = vec![vec![3, 1], vec![7, 8]];
        let bursts = vec![vec![2, 1, 4], vec![10, 11]];
        assert_eq!(
            merge(&model, &bursts),
            vec![vec![3, 1, 2, 4], vec![7, 8], vec![10, 11]]
        );
    }
}
