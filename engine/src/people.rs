//! The People view: faces across the Library grouped by who they look like, so the person
//! names each person once (Lightroom's People view). Grouping uses the `identity` Fact
//! (SFace vectors, [`crate::analyzers::identity`]) and runs on this Mac; it only ever
//! groups and asks ("Is this Maya?"). Names come from the person, through
//! [`Library::name_faces`], which also keeps `People|<name>` on each Photo.
//!
//! - [`overview`]: everyone named, with how many Photos, and the groups of unnamed faces
//!   (biggest first), each with the named person it looks most like, if any;
//! - [`person`]: one person's Photos and the unnamed faces that might be them;
//! - [`face_crops`]: small square JPEGs of faces for the UI, cached in the Library.

use crate::analyzers::{self, identity};
use crate::library::{FaceRef, Library, Person, PersonId, Photo, PhotoFilter, PhotoId};
use crate::masks::Face;
use crate::{Error, Result};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Unnamed faces at least this alike (to a group's average) are grouped together.
pub const GROUP: f32 = 0.5;
/// Two groups whose averages are at least this alike become one.
const MERGE: f32 = 0.55;
/// An unnamed face at least this like a person's average is offered as maybe them.
pub const SUGGEST: f32 = 0.42;
/// At most this many maybe-faces per person.
const MAX_SUGGESTIONS: usize = 60;

/// One readable face in the Library.
struct Seen {
    face: FaceRef,
    vector: Vec<f32>,
    /// Share of the frame (box area), so the clearest faces lead groups and covers.
    area: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct PersonSummary {
    pub id: PersonId,
    pub name: String,
    pub photos: usize,
    pub faces: usize,
    pub cover: FaceRef,
}

#[derive(Debug, Clone, Serialize)]
pub struct Maybe {
    pub person_id: PersonId,
    pub name: String,
    pub similarity: f32,
}

/// Unnamed faces that look like one person.
#[derive(Debug, Clone, Serialize)]
pub struct Group {
    /// The cover face, as `photo-face`; stable while the group keeps its clearest face.
    pub id: String,
    pub faces: Vec<FaceRef>,
    pub photos: usize,
    pub cover: FaceRef,
    /// The named person it looks most like, if alike enough to ask.
    pub maybe: Option<Maybe>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    pub people: Vec<PersonSummary>,
    pub groups: Vec<Group>,
    /// Faces read so far, and Photos whose faces aren't read yet (the Queue reads them
    /// after Import).
    pub faces: usize,
    pub unread_photos: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    #[serde(flatten)]
    pub face: FaceRef,
    pub similarity: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct PersonView {
    pub person: Person,
    /// Their Photos, in capture order.
    pub photos: Vec<PhotoId>,
    pub faces: Vec<FaceRef>,
    /// Unnamed faces that might be them, most alike first.
    pub suggestions: Vec<Suggestion>,
}

/// Everyone named and the groups of faces still to name.
pub fn overview(lib: &Library) -> Result<Overview> {
    let state = State::load(lib)?;
    let mut people: Vec<PersonSummary> = state
        .people
        .iter()
        .filter_map(|p| {
            let faces = state.named.get(&p.id)?;
            let photos: HashSet<PhotoId> =
                faces.iter().map(|&i| state.seen[i].face.photo_id).collect();
            let cover = *faces
                .iter()
                .max_by(|&&a, &&b| state.seen[a].area.total_cmp(&state.seen[b].area))?;
            Some(PersonSummary {
                id: p.id,
                name: p.name.clone(),
                photos: photos.len(),
                faces: faces.len(),
                cover: state.seen[cover].face,
            })
        })
        .collect();
    people.sort_by(|a, b| b.photos.cmp(&a.photos).then_with(|| a.name.cmp(&b.name)));
    let centroids = state.centroids();
    let unnamed = state.unnamed();
    let vectors: Vec<&[f32]> = unnamed
        .iter()
        .map(|&i| state.seen[i].vector.as_slice())
        .collect();
    let mut groups: Vec<Group> = group(&vectors)
        .into_iter()
        .map(|members| {
            let idx: Vec<usize> = members.iter().map(|&m| unnamed[m]).collect();
            let cover = *idx
                .iter()
                .max_by(|&&a, &&b| state.seen[a].area.total_cmp(&state.seen[b].area))
                .expect("a group has a face");
            let avg = mean(idx.iter().map(|&i| state.seen[i].vector.as_slice()));
            let maybe = centroids
                .iter()
                .filter(|(id, _)| {
                    !idx.iter()
                        .all(|&i| state.not.contains(&(state.seen[i].face, *id)))
                })
                .map(|(id, c)| (*id, dot(&avg, c)))
                .filter(|(_, s)| *s >= SUGGEST)
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, s)| Maybe {
                    person_id: id,
                    name: state.name_of(id),
                    similarity: round(s),
                });
            let photos: HashSet<PhotoId> =
                idx.iter().map(|&i| state.seen[i].face.photo_id).collect();
            let mut faces: Vec<FaceRef> = idx.iter().map(|&i| state.seen[i].face).collect();
            faces.sort();
            let c = state.seen[cover].face;
            Group {
                id: format!("{}-{}", c.photo_id, c.face),
                faces,
                photos: photos.len(),
                cover: c,
                maybe,
            }
        })
        .collect();
    groups.sort_by(|a, b| {
        b.faces
            .len()
            .cmp(&a.faces.len())
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(Overview {
        people,
        groups,
        faces: state.seen.len(),
        unread_photos: state.unread_photos,
    })
}

/// Queue the Analyzers for Photos whose faces aren't read yet (Libraries from before the
/// People view, or an Import the Queue hasn't reached). Returns the job and how many Photos.
pub fn read_faces(lib: &mut Library) -> Result<(Option<crate::library::JobId>, usize)> {
    let unread = State::load(lib)?.unread;
    if unread.is_empty() {
        return Ok((None, 0));
    }
    Ok((
        Some(analyzers::enqueue(lib, None, Some(&unread))?),
        unread.len(),
    ))
}

/// One person's Photos and the unnamed faces that might be them.
pub fn person(lib: &Library, id: PersonId) -> Result<PersonView> {
    let person = lib.person(id)?;
    let state = State::load(lib)?;
    let named = state.named.get(&id).cloned().unwrap_or_default();
    let order: HashMap<PhotoId, usize> = state
        .order
        .iter()
        .enumerate()
        .map(|(i, &p)| (p, i))
        .collect();
    let mut photos: Vec<PhotoId> = named.iter().map(|&i| state.seen[i].face.photo_id).collect();
    photos.sort_by_key(|p| order.get(p).copied().unwrap_or(usize::MAX));
    photos.dedup();
    let faces = named.iter().map(|&i| state.seen[i].face).collect();
    let mut suggestions = Vec::new();
    if !named.is_empty() {
        let c = mean(named.iter().map(|&i| state.seen[i].vector.as_slice()));
        // A Photo already showing them can't show them twice.
        let theirs: HashSet<PhotoId> = photos.iter().copied().collect();
        for i in state.unnamed() {
            let s = &state.seen[i];
            if theirs.contains(&s.face.photo_id) || state.not.contains(&(s.face, id)) {
                continue;
            }
            let sim = dot(&c, &s.vector);
            if sim >= SUGGEST {
                suggestions.push(Suggestion {
                    face: s.face,
                    similarity: round(sim),
                });
            }
        }
        suggestions.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
        suggestions.truncate(MAX_SUGGESTIONS);
    }
    Ok(PersonView {
        person,
        photos,
        faces,
        suggestions,
    })
}

/// What the Library knows about faces, loaded once per call.
struct State {
    seen: Vec<Seen>,
    people: Vec<Person>,
    /// Indexes into `seen` of each person's faces.
    named: BTreeMap<PersonId, Vec<usize>>,
    hidden: HashSet<FaceRef>,
    not: HashSet<(FaceRef, PersonId)>,
    /// Every Photo id in capture order.
    order: Vec<PhotoId>,
    unread_photos: usize,
    /// Photos whose faces aren't read yet.
    unread: Vec<PhotoId>,
}

impl State {
    fn load(lib: &Library) -> Result<State> {
        let photos = lib.list_photos(&PhotoFilter::default())?;
        let order = photos.iter().map(|p| p.id).collect();
        let version = analyzers::version_of("identity")?;
        let boxes: HashMap<PhotoId, Vec<Face>> = lib
            .fact_values("faces", None)?
            .into_iter()
            .map(|(id, _, v)| (id, serde_json::from_value(v).unwrap_or_default()))
            .collect();
        let reads: HashMap<PhotoId, serde_json::Value> = lib
            .fact_values("identity", None)?
            .into_iter()
            .filter(|(_, v, _)| *v == version)
            .map(|(id, _, v)| (id, v))
            .collect();
        let mut seen = Vec::new();
        let mut read: HashSet<PhotoId> = HashSet::new();
        for (photo, _, blob) in lib.fact_blobs("identity", version, None)? {
            let Some(value) = reads.get(&photo) else {
                continue;
            };
            read.insert(photo);
            let fact = analyzers::Fact {
                value: value.clone(),
                blob: Some(blob),
            };
            for (face, v) in identity::vectors(&fact).into_iter().enumerate() {
                let Some(vector) = v else { continue };
                let area = boxes
                    .get(&photo)
                    .and_then(|b| b.get(face))
                    .map_or(0.0, |f| f.w * f.h);
                seen.push(Seen {
                    face: FaceRef {
                        photo_id: photo,
                        face,
                    },
                    vector,
                    area,
                });
            }
        }
        let at: HashMap<FaceRef, usize> =
            seen.iter().enumerate().map(|(i, s)| (s.face, i)).collect();
        let mut named: BTreeMap<PersonId, Vec<usize>> = BTreeMap::new();
        let mut hidden = HashSet::new();
        for n in lib.face_names()? {
            match n.person {
                Some(p) => {
                    if let Some(&i) = at.get(&n.face) {
                        named.entry(p).or_default().push(i);
                    }
                }
                None => {
                    hidden.insert(n.face);
                }
            }
        }
        let unread: Vec<PhotoId> = photos
            .iter()
            .map(|p| p.id)
            .filter(|id| !read.contains(id))
            .collect();
        Ok(State {
            unread_photos: unread.len(),
            unread,
            seen,
            people: lib.people()?,
            named,
            hidden,
            not: lib.faces_not()?.into_iter().collect(),
            order,
        })
    }

    /// Faces nobody named or hid.
    fn unnamed(&self) -> Vec<usize> {
        let named: HashSet<usize> = self.named.values().flatten().copied().collect();
        (0..self.seen.len())
            .filter(|i| !named.contains(i) && !self.hidden.contains(&self.seen[*i].face))
            .collect()
    }

    fn centroids(&self) -> Vec<(PersonId, Vec<f32>)> {
        self.named
            .iter()
            .map(|(id, idx)| {
                (
                    *id,
                    mean(idx.iter().map(|&i| self.seen[i].vector.as_slice())),
                )
            })
            .collect()
    }

    fn name_of(&self, id: PersonId) -> String {
        self.people
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .unwrap_or_default()
    }
}

/// Group unit vectors by who they look like: each joins the group whose average it is
/// most like (at least [`GROUP`]) or starts a new one; a second pass moves each to its
/// best group now that the averages have settled, and groups that ended up alike
/// (at least [`MERGE`]) merge. Groups are lists of indexes into `vectors`.
pub fn group(vectors: &[&[f32]]) -> Vec<Vec<usize>> {
    let mut sums: Vec<Vec<f32>> = Vec::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    let best = |sums: &[Vec<f32>], v: &[f32]| -> Option<(usize, f32)> {
        sums.iter()
            .enumerate()
            .map(|(g, s)| (g, dot(v, s) / norm(s).max(1e-9)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
    };
    for (i, v) in vectors.iter().enumerate() {
        match best(&sums, v).filter(|(_, s)| *s >= GROUP) {
            Some((g, _)) => {
                add(&mut sums[g], v);
                members[g].push(i);
            }
            None => {
                sums.push(v.to_vec());
                members.push(vec![i]);
            }
        }
    }
    // Settle: each face to its best group's average, alone if none is alike enough.
    let centres: Vec<Vec<f32>> = sums.iter().map(|s| unit(s)).collect();
    let mut again: Vec<Vec<usize>> = vec![Vec::new(); centres.len()];
    let mut alone = Vec::new();
    for (i, v) in vectors.iter().enumerate() {
        match centres
            .iter()
            .enumerate()
            .map(|(g, c)| (g, dot(v, c)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, s)| *s >= GROUP)
        {
            Some((g, _)) => again[g].push(i),
            None => alone.push(vec![i]),
        }
    }
    let mut groups: Vec<Vec<usize>> = again.into_iter().filter(|g| !g.is_empty()).collect();
    groups.extend(alone);
    // Merge groups whose averages are alike, biggest first.
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
    let mut merged: Vec<(Vec<f32>, Vec<usize>)> = Vec::new();
    for g in groups {
        let c = mean(g.iter().map(|&i| vectors[i]));
        match merged
            .iter()
            .enumerate()
            .map(|(k, (m, _))| (k, dot(&c, m)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, s)| *s >= MERGE)
        {
            Some((k, _)) => {
                merged[k].1.extend(g);
                merged[k].0 = mean(merged[k].1.iter().map(|&i| vectors[i]));
            }
            None => merged.push((c, g)),
        }
    }
    merged.into_iter().map(|(_, g)| g).collect()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn norm(a: &[f32]) -> f32 {
    dot(a, a).sqrt()
}

fn add(sum: &mut [f32], v: &[f32]) {
    for (s, x) in sum.iter_mut().zip(v) {
        *s += x;
    }
}

fn unit(v: &[f32]) -> Vec<f32> {
    let n = norm(v).max(1e-9);
    v.iter().map(|x| x / n).collect()
}

/// The unit-length average of unit vectors.
fn mean<'a>(vs: impl Iterator<Item = &'a [f32]>) -> Vec<f32> {
    let mut sum = vec![0.0; identity::DIMS];
    for v in vs {
        add(&mut sum, v);
    }
    unit(&sum)
}

fn round(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

/// Where face crops are cached, under the Library folder.
const CROPS_DIR: &str = "cache/faces";

/// A square crop around each face (the face and a margin, `edge` px), as a
/// Library-relative JPEG, keyed `photo-face`; None for a face the Library doesn't have.
pub fn face_crops(
    lib: &Library,
    faces: &[FaceRef],
    edge: u32,
) -> Result<BTreeMap<String, Option<String>>> {
    let edge = edge.clamp(32, 512);
    let mut by_photo: BTreeMap<PhotoId, Vec<usize>> = BTreeMap::new();
    for f in faces {
        by_photo.entry(f.photo_id).or_default().push(f.face);
    }
    let photos: HashMap<PhotoId, Photo> = lib
        .list_photos(&PhotoFilter::default())?
        .into_iter()
        .filter(|p| by_photo.contains_key(&p.id))
        .map(|p| (p.id, p))
        .collect();
    let mut out = BTreeMap::new();
    for (id, wanted) in by_photo {
        let Some(photo) = photos.get(&id) else {
            for f in wanted {
                out.insert(format!("{id}-{f}"), None);
            }
            continue;
        };
        let boxes =
            crate::analyzers::faces::read(&analyzers::facts(lib, photo, &["faces"])?["faces"]);
        let stamp = std::fs::metadata(&photo.preview)
            .map(|m| (m.len(), m.modified().ok()))
            .map_err(|e| Error(format!("preview {}: {e}", photo.preview.display())))?;
        let mut img: Option<image::RgbImage> = None;
        for f in wanted {
            let key = format!("{id}-{f}");
            let Some(b) = boxes.get(f) else {
                out.insert(key, None);
                continue;
            };
            let mut h = blake3::Hasher::new();
            h.update(format!("{stamp:?}{b:?}").as_bytes());
            let rel = format!(
                "{CROPS_DIR}/{:02x}/{id}-{f}-{edge}-{}.jpg",
                id % 256,
                &h.finalize().to_hex()[..12]
            );
            let path = lib.root().join(&rel);
            if !path.is_file() {
                let img = match &img {
                    Some(i) => i,
                    None => img.insert(crate::tools::open_preview(photo)?.to_rgb8()),
                };
                let crop = crop_face(img, b, edge);
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)
                        .map_err(|e| Error(format!("{}: {e}", dir.display())))?;
                }
                std::fs::write(&path, crate::tools::encode_jpeg(&crop)?)
                    .map_err(|e| Error(format!("write {}: {e}", path.display())))?;
            }
            out.insert(key, Some(rel));
        }
    }
    Ok(out)
}

/// The face and some hair and chin around it, square, `edge` px.
fn crop_face(img: &image::RgbImage, f: &Face, edge: u32) -> image::RgbImage {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let side = (f.w * w).max(f.h * h) * 1.5;
    let cx = (f.x + f.w / 2.0) * w;
    let cy = (f.y + f.h / 2.0) * h;
    let side = side.min(w).min(h).max(1.0);
    let x = (cx - side / 2.0).clamp(0.0, w - side);
    let y = (cy - side / 2.0).clamp(0.0, h - side);
    let sub =
        image::imageops::crop_imm(img, x as u32, y as u32, side as u32, side as u32).to_image();
    image::imageops::resize(&sub, edge, edge, image::imageops::FilterType::Triangle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit vector near axis `k`, nudged by `j` so no two are the same.
    fn near(k: usize, j: usize) -> Vec<f32> {
        let mut v = vec![0.0; identity::DIMS];
        v[k] = 1.0;
        v[64 + (j % 60)] = 0.3;
        unit(&v)
    }

    #[test]
    fn faces_of_one_person_group_together_and_strangers_stay_apart() {
        let vs: Vec<Vec<f32>> = vec![
            near(0, 1),
            near(1, 2),
            near(0, 3),
            near(2, 4),
            near(0, 5),
            near(1, 6),
        ];
        let refs: Vec<&[f32]> = vs.iter().map(|v| v.as_slice()).collect();
        let mut groups = group(&refs);
        for g in &mut groups {
            g.sort();
        }
        groups.sort();
        assert_eq!(groups, vec![vec![0, 2, 4], vec![1, 5], vec![3]]);
    }
}
