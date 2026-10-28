//! Splitting one Import into Shoots. A card often holds a whole day (a morning walk, an
//! afternoon at the beach, dinner), and Greg wants one Shoot per outing, so Import
//! proposes a Split wherever the camera sat idle for a while. The person can move any
//! Split before importing, and every Photo still lands in exactly one Shoot.
//!
//! Only capture times are used for now. Place (GPS) and a change of light are the next
//! clues to add; the Shoot's name comes from the time of day until the model names it.

use crate::library::time::capture_seconds;
use crate::naming;
use serde::Serialize;

/// When a gap in capture time starts a new Shoot.
#[derive(Debug, Clone, Copy)]
pub struct SplitRule {
    /// A gap at least this long always starts a new Shoot.
    pub gap_minutes: f64,
    /// A run of fewer Photos than this joins the neighbour it is closer to in time...
    pub min_photos: usize,
    /// ...unless both of its gaps are at least this long, so a lone frame shot hours
    /// apart from everything else keeps a Shoot of its own.
    pub alone_minutes: f64,
}

impl Default for SplitRule {
    fn default() -> Self {
        Self {
            gap_minutes: 60.0,
            min_photos: 3,
            alone_minutes: 240.0,
        }
    }
}

/// One proposed Shoot: a run of Photos in capture order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProposedShoot {
    /// Index of its first Photo in the list given to [`propose`].
    pub start: usize,
    pub count: usize,
    /// `2026-10-25_Morning`: the first frame's date and the time of day, unique within
    /// this proposal.
    pub name: String,
    pub first: String,
    pub last: String,
    /// Minutes since the previous Shoot's last frame; None for the first.
    pub gap_before_minutes: Option<f64>,
}

/// Proposes Shoots for Photos given by capture time (`YYYY-MM-DDTHH:MM:SS`), already in
/// capture order. An empty list proposes nothing.
pub fn propose(captured: &[&str], rule: &SplitRule) -> Vec<ProposedShoot> {
    if captured.is_empty() {
        return Vec::new();
    }
    let secs: Vec<i64> = captured.iter().map(|t| capture_seconds(t)).collect();
    let gap = |i: usize| (secs[i] - secs[i - 1]) as f64 / 60.0;

    // Runs split at every long gap: (start, end exclusive).
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    for i in 1..secs.len() {
        if gap(i) >= rule.gap_minutes {
            runs.push((start, i));
            start = i;
        }
    }
    runs.push((start, secs.len()));

    // Small runs join the nearer neighbour, smallest first, until none is left to join.
    loop {
        let small = (0..runs.len())
            .filter(|&r| runs[r].1 - runs[r].0 < rule.min_photos && runs.len() > 1)
            .filter_map(|r| {
                let before = (r > 0).then(|| gap(runs[r].0));
                let after = (r + 1 < runs.len()).then(|| gap(runs[r].1));
                let alone = [before, after]
                    .iter()
                    .all(|g| g.is_none_or(|g| g >= rule.alone_minutes));
                if alone {
                    return None;
                }
                let into_prev = match (before, after) {
                    (Some(b), Some(a)) => b <= a,
                    (Some(_), None) => true,
                    _ => false,
                };
                Some((runs[r].1 - runs[r].0, r, into_prev))
            })
            .min_by_key(|&(n, r, _)| (n, r));
        let Some((_, r, into_prev)) = small else {
            break;
        };
        if into_prev {
            runs[r - 1].1 = runs[r].1;
        } else {
            runs[r + 1].0 = runs[r].0;
        }
        runs.remove(r);
    }

    let mut names: Vec<String> = Vec::new();
    runs.iter()
        .map(|&(s, e)| {
            let date = naming::capture_date(captured[s]).unwrap_or("undated");
            let name =
                naming::unique_name(&naming::shoot_name(date, time_of_day(captured[s])), |n| {
                    names.iter().any(|m| m == n)
                });
            names.push(name.clone());
            ProposedShoot {
                start: s,
                count: e - s,
                name,
                first: captured[s].to_string(),
                last: captured[e - 1].to_string(),
                gap_before_minutes: (s > 0).then(|| gap(s)),
            }
        })
        .collect()
}

/// Morning, Afternoon, Evening or Night, from the hour of a capture time.
fn time_of_day(captured_at: &str) -> &'static str {
    match captured_at.get(11..13).and_then(|h| h.parse::<u32>().ok()) {
        Some(5..=11) => "Morning",
        Some(12..=16) => "Afternoon",
        Some(17..=20) => "Evening",
        _ => "Night",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(times: &[&str]) -> Vec<String> {
        times.iter().map(|t| format!("2026-10-25T{t}")).collect()
    }

    fn split(times: &[&str]) -> Vec<(usize, String)> {
        let t = at(times);
        let refs: Vec<&str> = t.iter().map(String::as_str).collect();
        propose(&refs, &SplitRule::default())
            .into_iter()
            .map(|s| (s.count, s.name))
            .collect()
    }

    #[test]
    fn nothing_in_nothing_out() {
        assert!(propose(&[], &SplitRule::default()).is_empty());
    }

    #[test]
    fn one_outing_stays_one_shoot() {
        assert_eq!(
            split(&["09:00:00", "09:10:00", "09:50:00", "10:30:00"]),
            vec![(4, "2026-10-25_Morning".to_string())]
        );
    }

    #[test]
    fn a_long_gap_starts_a_new_shoot_named_for_its_time_of_day() {
        let s = split(&[
            "09:12:00", "09:30:00", "10:48:00", "14:28:00", "15:00:00", "15:55:00", "19:02:00",
            "19:30:00", "20:31:00",
        ]);
        assert_eq!(
            s,
            vec![
                (3, "2026-10-25_Morning".to_string()),
                (3, "2026-10-25_Afternoon".to_string()),
                (3, "2026-10-25_Evening".to_string()),
            ]
        );
    }

    #[test]
    fn a_stray_frame_joins_the_nearer_shoot() {
        // 11:20 is 32 min after the walk and 3 h before the beach.
        let s = split(&[
            "09:00:00", "09:10:00", "09:20:00", "10:30:00", "14:30:00", "14:40:00", "14:50:00",
        ]);
        assert_eq!(s.iter().map(|x| x.0).collect::<Vec<_>>(), vec![4, 3]);
    }

    #[test]
    fn a_lone_frame_hours_from_anything_keeps_its_own_shoot() {
        let s = split(&[
            "09:00:00", "09:10:00", "09:20:00", "15:00:00", "21:00:00", "21:10:00", "21:20:00",
        ]);
        assert_eq!(s.iter().map(|x| x.0).collect::<Vec<_>>(), vec![3, 1, 3]);
    }

    #[test]
    fn two_shoots_in_one_part_of_the_day_get_unique_names() {
        let s = split(&[
            "13:00:00", "13:10:00", "13:20:00", "15:00:00", "15:10:00", "15:20:00",
        ]);
        assert_eq!(s[0].1, "2026-10-25_Afternoon");
        assert_eq!(s[1].1, "2026-10-25_Afternoon-2");
    }

    #[test]
    fn a_night_out_past_midnight_stays_together() {
        let t = [
            "2026-10-25T23:30:00",
            "2026-10-25T23:50:00",
            "2026-10-26T00:20:00",
            "2026-10-26T00:45:00",
        ];
        let s = propose(&t, &SplitRule::default());
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "2026-10-25_Night");
    }

    #[test]
    fn gaps_and_ranges_are_reported() {
        let t = at(&[
            "09:00:00", "09:10:00", "09:20:00", "11:20:00", "11:30:00", "11:40:00",
        ]);
        let refs: Vec<&str> = t.iter().map(String::as_str).collect();
        let s = propose(&refs, &SplitRule::default());
        assert_eq!(s[1].start, 3);
        assert_eq!(s[1].gap_before_minutes, Some(120.0));
        assert_eq!(s[0].last, "2026-10-25T09:20:00");
        assert_eq!(s[1].first, "2026-10-25T11:20:00");
    }
}
