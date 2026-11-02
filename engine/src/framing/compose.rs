//! Crop: where the people in a frame sit loose, a punch-in on them at the frame's own shape.
//!
//! Greg's crops (docs/research/recompose.md) are nearly all punch-ins at the camera's 3:2,
//! keeping a median 62% of the frame, on frames with people. So a proposal keeps the frame's
//! shape, holds every person found whole (never a cut-off head: Greg, 2026-10-24) with room
//! around them, sets the faces a little above the middle, and is only made when it would
//! take off a real share of the frame. Everything is pure geometry over the faces and the
//! people mask, so it is tested without models.

use crate::masks::models::Face;

/// A rectangle, 0..1 of the frame's width and height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn right(&self) -> f64 {
        self.x + self.w
    }
    fn bottom(&self) -> f64 {
        self.y + self.h
    }
    fn union(self, o: Rect) -> Rect {
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        Rect {
            x,
            y,
            w: self.right().max(o.right()) - x,
            h: self.bottom().max(o.bottom()) - y,
        }
    }
    fn contains(&self, o: &Rect) -> bool {
        const E: f64 = 1e-9;
        o.x >= self.x - E
            && o.y >= self.y - E
            && o.right() <= self.right() + E
            && o.bottom() <= self.bottom() + E
    }
}

/// A face counts toward the framing at this share of the biggest face's width or more.
const MAIN: f64 = 0.5;

/// The tunables, with the defaults the bench chose.
#[derive(Debug, Clone, Copy)]
pub struct Rules {
    /// Room kept around the people, as a share of the crop's side.
    pub margin: f64,
    /// Never keep less than this share of the frame's side (0.7 keeps 49% of its area).
    pub tightest: f64,
    /// Only propose a crop that keeps at most this share of the side: a smaller trim isn't
    /// worth a decision.
    pub loosest: f64,
    /// Where the faces' middle sits, from the crop's top, as a share of its height.
    pub eyes_at: f64,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            margin: 0.1,
            tightest: 0.7,
            loosest: 0.88,
            eyes_at: 0.38,
        }
    }
}

/// Where the people are: the faces, and the box around the people mask.
pub struct Subject {
    pub faces: Vec<Rect>,
    pub people: Option<Rect>,
}

impl Subject {
    pub fn from_faces(faces: &[Face]) -> Vec<Rect> {
        faces
            .iter()
            .map(|f| Rect {
                x: f.x as f64,
                y: f.y as f64,
                w: f.w as f64,
                h: f.h as f64,
            })
            .collect()
    }

    /// The faces that matter: at least `MAIN` of the biggest one's width, so people far
    /// behind (a crowd, a street) don't decide the framing.
    fn main_faces(&self) -> Vec<Rect> {
        let big = self.faces.iter().map(|f| f.w).fold(0.0, f64::max);
        self.faces
            .iter()
            .copied()
            .filter(|f| f.w >= big * MAIN)
            .collect()
    }

    /// Everything that must stay in the frame: each main face with the body under it (about
    /// three faces wide and seven tall, the whole person standing), and anyone the people mask
    /// sees overlapping those bodies. Bodies run past the frame's edge when the person does,
    /// which rules out any crop on that side.
    fn keep(&self) -> Option<Rect> {
        let bodies: Vec<Rect> = self
            .main_faces()
            .iter()
            .map(|f| Rect {
                x: f.x - f.w,
                y: f.y - f.h * 0.4,
                w: f.w * 3.0,
                h: f.h * 7.4,
            })
            .collect();
        let mut keep = bodies.iter().copied().reduce(Rect::union)?;
        if let Some(p) = self.people {
            if p.x < keep.right()
                && keep.x < p.right()
                && p.y < keep.bottom()
                && keep.y < p.bottom()
            {
                keep = keep.union(p);
            }
        }
        // Clip to the frame, remembering where it ran off.
        let x0 = keep.x.max(0.0);
        let y0 = keep.y.max(0.0);
        Some(Rect {
            x: x0,
            y: y0,
            w: keep.right().min(1.0) - x0,
            h: keep.bottom().min(1.0) - y0,
        })
    }
}

/// The box around a person-probability plane (`n`×`n`, row by row, 1 = person), between the
/// 1st and 99th percentile of its mass on each axis so stray specks don't widen it. None when
/// there is too little person to go by.
pub fn people_box(plane: &[f32], n: usize) -> Option<Rect> {
    let mut cols = vec![0f64; n];
    let mut rows = vec![0f64; n];
    for y in 0..n {
        for x in 0..n {
            let p = plane[y * n + x] as f64;
            if p > 0.5 {
                cols[x] += p;
                rows[y] += p;
            }
        }
    }
    let total: f64 = cols.iter().sum();
    if total < (n * n) as f64 * 0.01 {
        return None;
    }
    let span = |v: &[f64]| {
        let (mut run, mut lo, mut hi) = (0.0, 0, n - 1);
        let mut lo_set = false;
        for (i, x) in v.iter().enumerate() {
            run += x;
            if !lo_set && run >= total * 0.01 {
                lo = i;
                lo_set = true;
            }
            if run <= total * 0.99 {
                hi = i + 1;
            }
        }
        (lo as f64 / n as f64, ((hi + 1).min(n)) as f64 / n as f64)
    };
    let (x0, x1) = span(&cols);
    let (y0, y1) = span(&rows);
    Some(Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

/// The crop for a frame with `subject` in it, at the frame's own shape, or None when the
/// people already fill it, aren't there, or the crop would trim too little to matter.
pub fn propose(subject: &Subject, rules: &Rules) -> Option<Rect> {
    let faces = subject.main_faces();
    if faces.is_empty() {
        return None; // People without a face found: too unsure where they are.
    }
    let keep = subject.keep()?;
    // A side share `s` keeps the frame's shape: the crop is s by s of the frame.
    let need = (keep.w.max(keep.h) / (1.0 - 2.0 * rules.margin)).max(rules.tightest);
    if need > rules.loosest {
        return None;
    }
    let s = need;
    // Across: centred on everything kept.
    let cx = keep.x + keep.w / 2.0;
    // Down: the faces' middle at `eyes_at` of the crop, as long as everything kept fits.
    let fy = {
        let top = faces.iter().map(|f| f.y).fold(f64::MAX, f64::min);
        let bottom = faces.iter().map(|f| f.bottom()).fold(f64::MIN, f64::max);
        (top + bottom) / 2.0
    };
    let m = rules.margin * s;
    let mut y = fy - rules.eyes_at * s;
    // Keep room above the top of the subject and below its bottom where it can.
    y = y.min(keep.y - m).max(keep.bottom() + m - s);
    let mut x = cx - s / 2.0;
    x = x.clamp(0.0, 1.0 - s);
    y = y.clamp(0.0, 1.0 - s);
    let crop = Rect { x, y, w: s, h: s };
    // Never cut into a person kept: if clamping to the frame did, leave the frame alone. A
    // person running off one edge pins the crop to that edge; off two facing edges, no crop.
    let touches = |a: f64, b: f64| a <= 0.01 && b >= 0.99;
    if touches(keep.x, keep.right()) || touches(keep.y, keep.bottom()) {
        return None;
    }
    crop.contains(&keep).then_some(crop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(x: f64, y: f64, w: f64) -> Rect {
        Rect {
            x,
            y,
            w,
            h: w * 1.3,
        }
    }

    #[test]
    fn a_small_person_off_centre_gets_a_punch_in_that_holds_them_whole() {
        let s = Subject {
            faces: vec![face(0.62, 0.3, 0.06)],
            people: Some(Rect {
                x: 0.55,
                y: 0.27,
                w: 0.2,
                h: 0.55,
            }),
        };
        let c = propose(&s, &Rules::default()).expect("a crop");
        assert!(c.w < 0.88 && c.w >= 0.7, "{c:?}");
        assert!(c.contains(&s.keep().unwrap()), "{c:?}");
        assert!(c.x > 0.0, "moved toward the person: {c:?}");
    }

    #[test]
    fn people_filling_the_frame_or_no_face_leave_it_alone() {
        let big = Subject {
            faces: vec![face(0.2, 0.2, 0.15), face(0.6, 0.25, 0.15)],
            people: Some(Rect {
                x: 0.05,
                y: 0.15,
                w: 0.9,
                h: 0.85,
            }),
        };
        assert_eq!(propose(&big, &Rules::default()), None);
        let faceless = Subject {
            faces: vec![],
            people: Some(Rect {
                x: 0.4,
                y: 0.4,
                w: 0.1,
                h: 0.3,
            }),
        };
        assert_eq!(propose(&faceless, &Rules::default()), None);
    }

    #[test]
    fn a_person_at_the_edge_is_never_cut() {
        // Kept box runs to the bottom edge: the crop must reach it or not happen.
        let s = Subject {
            faces: vec![face(0.45, 0.4, 0.05)],
            people: Some(Rect {
                x: 0.4,
                y: 0.38,
                w: 0.15,
                h: 0.62,
            }),
        };
        if let Some(c) = propose(&s, &Rules::default()) {
            assert!(c.contains(&s.keep().unwrap()), "{c:?}");
        }
    }

    #[test]
    fn the_people_box_ignores_specks() {
        let n = 64;
        let mut plane = vec![0f32; n * n];
        for y in 20..50 {
            for x in 30..40 {
                plane[y * n + x] = 1.0;
            }
        }
        plane[2 * n + 2] = 1.0; // a speck far away
        let b = people_box(&plane, n).unwrap();
        assert!(
            (b.x - 30.0 / 64.0).abs() < 0.02 && (b.y - 20.0 / 64.0).abs() < 0.03,
            "{b:?}"
        );
    }
}
