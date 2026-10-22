//! Names Argus gives things: Shoots and their folders, copied originals, exports,
//! Sidecars, and Keywords. Every feature that names a file or a tag calls this module, so
//! the rules live in one place. They are written up, with where they come from, in
//! docs/naming.md and ADR 0010.
//!
//! - Shoot: `2026-10-18_Boat-Trip` ([`shoot_name`]), which is also its folder name
//! - Copied original: the camera's own file name, `originals/2026-10-18_Boat-Trip/DSC08005.ARW`
//! - Export: `2026-10-18_Boat-Trip_DSC08005.jpg` ([`export_file_name`])
//! - Sidecar: `DSC08005.xmp` beside the original ([`sidecar_file_name`])
//! - Keyword: `Places|Portugal|Lisbon` ([`Keyword`]), written to XMP as Lightroom does
//!
//! The suggestion half (a local model naming a Shoot from a few thumbnails) is in
//! [`suggest`].

mod suggest;

pub use suggest::*;

use serde::Serialize;
use std::path::PathBuf;

/// Longest slug Argus makes from someone's words, so paths stay readable.
pub const MAX_SLUG: usize = 48;

/// Words to a path-safe slug: ASCII letters and digits joined by hyphens, case kept.
/// `"Maya's Birthday!"` -> `Mayas-Birthday`, `"São Paulo & Rio"` -> `Sao-Paulo-and-Rio`.
/// Characters with no ASCII spelling are dropped, so the result can be empty.
pub fn slug(words: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in words.chars() {
        let word = match c {
            '\'' | '’' | '‘' | '`' => continue,
            '&' => {
                gap = true;
                Some("and")
            }
            c if c.is_ascii_alphanumeric() => {
                if gap && !out.is_empty() {
                    out.push('-');
                }
                gap = false;
                out.push(c);
                continue;
            }
            c => fold(c),
        };
        match word {
            Some(s) => {
                if gap && !out.is_empty() {
                    out.push('-');
                }
                gap = c == '&';
                out.push_str(s);
            }
            None => gap = true,
        }
    }
    cut_at_word(&out, MAX_SLUG)
}

/// A lowercase id from a display name, for a Look or a Preset: ASCII letters and digits
/// joined by hyphens (`"Warm Film #2"` -> `warm-film-2`), or `fallback` when none are left.
/// Ids never reach the disk, so unlike [`slug`] nothing is spelled out or kept in case.
pub fn id_slug(name: &str, fallback: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let parts: Vec<&str> = s.split('-').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        fallback.into()
    } else {
        parts.join("-")
    }
}

/// The date part of an ISO 8601 capture time: `2026-10-18T09:12:00` -> `2026-10-18`.
pub fn capture_date(captured_at: &str) -> Option<&str> {
    let d = captured_at.get(..10)?;
    let b = d.as_bytes();
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    let ok = digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && (1..=12).contains(&d[5..7].parse::<u32>().ok()?)
        && (1..=31).contains(&d[8..10].parse::<u32>().ok()?);
    ok.then_some(d)
}

/// A Shoot's name, which is also its folder name: the ISO date of its first frame, an
/// underscore, then the slugged title. `("2026-10-18", "Boat Trip")` -> `2026-10-18_Boat-Trip`.
/// A title that already starts with that date isn't dated twice; with no usable title the
/// name is the bare date.
pub fn shoot_name(date: &str, title: &str) -> String {
    let title = title.trim_start();
    let title = match capture_date(title) {
        Some(d) => title[d.len()..].trim_start_matches(|c: char| !c.is_alphanumeric()),
        None => title,
    };
    match slug(title) {
        s if s.is_empty() => date.to_string(),
        s => format!("{date}_{s}"),
    }
}

/// A Shoot name read back into its parts, for showing it to people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShootNameParts {
    pub date: Option<String>,
    /// The title as words: `Boat-Trip` -> `Boat Trip`.
    pub title: String,
}

/// `2026-10-18_Boat-Trip` -> date `2026-10-18`, title `Boat Trip`. Names made before these
/// rules (`Oct 12, 2026`, a folder's name) come back whole as the title.
pub fn parse_shoot_name(name: &str) -> ShootNameParts {
    match capture_date(name) {
        Some(d) if name.len() == 10 || name.as_bytes()[10] == b'_' => ShootNameParts {
            date: Some(d.to_string()),
            title: name.get(11..).unwrap_or_default().replace('-', " "),
        },
        _ => ShootNameParts {
            date: None,
            title: name.to_string(),
        },
    }
}

/// Where a Shoot's copied originals live, relative to the Library folder:
/// `originals/2026-10-18_Boat-Trip`.
pub fn originals_folder(shoot_name: &str) -> PathBuf {
    PathBuf::from("originals").join(shoot_name)
}

/// A file name Argus may create: the given name with spaces and path-hostile characters
/// turned into hyphens, accents folded, extension kept as is. Camera names
/// (`DSC08005.ARW`, `IMG_1234.HEIC`) come through unchanged.
pub fn safe_file_name(name: &str) -> String {
    let (stem, ext) = split_ext(name);
    let stem: String = stem
        .split('_')
        .map(slug)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    let stem = if stem.is_empty() {
        "photo".into()
    } else {
        stem
    };
    match ext {
        Some(e) if !slug(e).is_empty() => format!("{stem}.{}", slug(e)),
        _ => stem,
    }
}

/// `name`, or the first of `stem-2.ext`, `stem-3.ext`, ... that isn't `taken`, the way
/// Finder and Lightroom avoid overwriting.
pub fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    (2..)
        .map(|n| match ext {
            Some(e) => format!("{stem}-{n}.{e}"),
            None => format!("{stem}-{n}"),
        })
        .find(|n| !taken(n))
        .expect("some counter is free")
}

/// An exported file's name: Shoot name, underscore, the original's stem, and `ext`:
/// `2026-10-18_Boat-Trip_DSC08005.jpg`. It sorts by date, says which Shoot it came from,
/// and leads back to its original.
pub fn export_file_name(shoot_name: &str, original_file_name: &str, ext: &str) -> String {
    let stem = safe_file_name(split_ext(original_file_name).0);
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    match shoot_name {
        "" => format!("{stem}.{ext}"),
        s => format!("{s}_{stem}.{ext}"),
    }
}

/// The folder an export goes into under the chosen export location: the Shoot name.
pub fn export_folder_name(shoot_name: &str) -> String {
    shoot_name.to_string()
}

/// A Sidecar's file name, Adobe style: the original's stem with `.xmp`, so Lightroom,
/// Bridge and Capture One find it. `DSC08005.ARW` -> `DSC08005.xmp`.
pub fn sidecar_file_name(original_file_name: &str) -> String {
    format!("{}.xmp", split_ext(original_file_name).0)
}

/// The top level of every Keyword: who, where, what happened, what they were doing, and
/// what's in the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Facet {
    People,
    Places,
    Events,
    Activities,
    Subjects,
}

impl Facet {
    pub const ALL: [Facet; 5] = [
        Facet::People,
        Facet::Places,
        Facet::Events,
        Facet::Activities,
        Facet::Subjects,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Facet::People => "People",
            Facet::Places => "Places",
            Facet::Events => "Events",
            Facet::Activities => "Activities",
            Facet::Subjects => "Subjects",
        }
    }

    fn parse(s: &str) -> Option<Facet> {
        Some(match s.to_ascii_lowercase().as_str() {
            "people" | "person" | "who" => Facet::People,
            "places" | "place" | "location" | "locations" | "where" => Facet::Places,
            "events" | "event" | "occasion" => Facet::Events,
            "activities" | "activity" | "doing" => Facet::Activities,
            "subjects" | "subject" | "things" | "what" | "topics" => Facet::Subjects,
            _ => return None,
        })
    }
}

/// Longest Keyword part: IPTC's legacy keyword field holds 64 bytes.
pub const MAX_KEYWORD: usize = 64;

/// A hierarchical Keyword, Lightroom style: a Facet, then levels from broad to narrow.
/// `Places|Portugal|Lisbon`, `Subjects|Animals|dog`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Keyword {
    pub facet: Facet,
    /// Below the Facet, broad to narrow. Never empty.
    pub levels: Vec<String>,
}

impl Keyword {
    /// Read a Keyword from a person or a model. Levels split on `|`, `>` or `/`; a path
    /// that doesn't start with a Facet goes under Subjects. Each level is trimmed to plain
    /// words (no list separators or quotes) and case is kept.
    pub fn parse(text: &str) -> Option<Keyword> {
        let mut parts: Vec<String> = text
            .split(['|', '>', '/'])
            .map(clean_level)
            .filter(|p| !p.is_empty())
            .collect();
        let facet = match parts.first().and_then(|p| Facet::parse(p)) {
            Some(f) => {
                parts.remove(0);
                f
            }
            None => Facet::Subjects,
        };
        (!parts.is_empty()).then_some(Keyword {
            facet,
            levels: parts,
        })
    }

    /// The Keyword a named person gets on their Photos, `People|Maya Lee`: one level, so a
    /// slash or bar in the name is a space. None for a name with no words.
    pub fn person(name: &str) -> Option<Keyword> {
        let level = clean_level(&name.replace(['|', '>', '/'], " "));
        (!level.is_empty()).then(|| Keyword {
            facet: Facet::People,
            levels: vec![level],
        })
    }

    /// The narrowest level: `Lisbon`.
    pub fn leaf(&self) -> &str {
        self.levels.last().expect("a Keyword has a level")
    }

    /// The full path as Lightroom writes it to `lr:hierarchicalSubject`.
    pub fn path(&self) -> String {
        std::iter::once(self.facet.name())
            .chain(self.levels.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("|")
    }
}

fn clean_level(s: &str) -> String {
    let words: Vec<&str> = s
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '"' | '“' | '”'))
        .filter(|w| !w.is_empty())
        .collect();
    cut_at_word(&words.join(" "), MAX_KEYWORD)
}

/// The values for `lr:hierarchicalSubject`: each Keyword's full path, once.
pub fn hierarchical_subject(keywords: &[Keyword]) -> Vec<String> {
    dedup(keywords.iter().map(Keyword::path))
}

/// The values for `dc:subject` (IPTC Core Keywords), which most apps and stock sites read:
/// every level of every Keyword, once, without the Facets themselves. Lightroom does the
/// same when it exports a Keyword with its parents.
pub fn flat_keywords(keywords: &[Keyword]) -> Vec<String> {
    dedup(keywords.iter().flat_map(|k| k.levels.iter().cloned()))
}

/// How the Agent should word Keywords, a Title and a Description. Prompts include it so
/// every Provider tags the same way.
pub const TAGGING_GUIDE: &str = "Keywords are hierarchical paths from broad to narrow, \
joined with |, under one of these tops: People, Places, Events, Activities, Subjects. \
Examples: Places|Portugal|Lisbon|Alfama, Events|Birthday, Activities|Sports|pickleball, \
Subjects|Animals|dog, Subjects|Nature|sunset. Use singular nouns. Lowercase common nouns \
and capitalise proper nouns as they are spelled. Name a person under People only when \
the user has named them; never guess who someone is. Places go country, then region or \
city, then neighbourhood or landmark. Five to fifteen keywords per photo, only for what \
is clearly visible or known. A Title is a few words (IPTC Title). A Description is one \
or two plain sentences saying who, what, where (IPTC Description).";

fn dedup(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items.filter(|s| seen.insert(s.to_lowercase())).collect()
}

fn split_ext(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => (&name[..i], Some(&name[i + 1..])),
        _ => (name, None),
    }
}

/// At most `max` bytes, cut back to the last word break (`-` or space) when it's long.
fn cut_at_word(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let cut = &s[..max];
    match cut.rfind(['-', ' ']) {
        Some(i) if i > max / 2 => cut[..i].to_string(),
        _ => cut.to_string(),
    }
}

/// An ASCII spelling for the accented Latin letters people put in shoot names.
fn fold(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' => "A",
        'ç' | 'ć' | 'č' => "c",
        'Ç' | 'Ć' | 'Č' => "C",
        'ď' | 'đ' | 'ð' => "d",
        'Ď' | 'Đ' | 'Ð' => "D",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
        'È' | 'É' | 'Ê' | 'Ë' | 'Ē' | 'Ė' | 'Ę' | 'Ě' => "E",
        'ğ' => "g",
        'Ğ' => "G",
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i",
        'Ì' | 'Í' | 'Î' | 'Ï' | 'Ī' | 'Į' | 'İ' => "I",
        'ł' | 'ľ' => "l",
        'Ł' | 'Ľ' => "L",
        'ñ' | 'ń' | 'ň' => "n",
        'Ñ' | 'Ń' | 'Ň' => "N",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'Ō' | 'Ő' => "O",
        'ř' => "r",
        'Ř' => "R",
        'ś' | 'š' | 'ş' => "s",
        'Ś' | 'Š' | 'Ş' => "S",
        'ß' => "ss",
        'ť' | 'ţ' => "t",
        'Ť' | 'Ţ' => "T",
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'ų' => "u",
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ū' | 'Ů' | 'Ű' | 'Ų' => "U",
        'ý' | 'ÿ' => "y",
        'Ý' | 'Ÿ' => "Y",
        'ź' | 'ż' | 'ž' => "z",
        'Ź' | 'Ż' | 'Ž' => "Z",
        'æ' => "ae",
        'Æ' => "AE",
        'œ' => "oe",
        'Œ' => "OE",
        'þ' => "th",
        'Þ' => "Th",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_ascii_words_joined_by_hyphens() {
        assert_eq!(slug("Boat Trip"), "Boat-Trip");
        assert_eq!(slug("Maya's Birthday!"), "Mayas-Birthday");
        assert_eq!(slug("São Paulo & Rio"), "Sao-Paulo-and-Rio");
        assert_eq!(slug("Tom&Jerry"), "Tom-and-Jerry");
        assert_eq!(slug("  Lake   Tahoe -- 2  "), "Lake-Tahoe-2");
        assert_eq!(slug("Smørrebrød in Ærø"), "Smorrebrod-in-AEro");
        assert_eq!(slug("東京"), "");
        let long = slug(&"Golden Gate Bridge ".repeat(5));
        assert!(long.len() <= MAX_SLUG && !long.ends_with('-'), "{long}");
    }

    #[test]
    fn shoot_names_lead_with_the_iso_date() {
        assert_eq!(
            shoot_name("2026-10-18", "Boat Trip"),
            "2026-10-18_Boat-Trip"
        );
        assert_eq!(shoot_name("2026-10-18", ""), "2026-10-18");
        assert_eq!(shoot_name("2026-10-18", "!!!"), "2026-10-18");
        assert_eq!(
            shoot_name("2026-10-18", "2026-10-18 Boat trip"),
            "2026-10-18_Boat-trip"
        );
        assert_eq!(
            shoot_name("2026-10-18", "2026-10-18_Boat-Trip"),
            "2026-10-18_Boat-Trip"
        );
        assert!(!shoot_name("2026-10-18", "Maya's Party").contains(' '));
    }

    #[test]
    fn ids_are_lowercase_words_joined_by_hyphens() {
        assert_eq!(id_slug("Warm Film #2", "look"), "warm-film-2");
        assert_eq!(id_slug("  ", "preset"), "preset");
    }

    #[test]
    fn capture_dates_are_checked() {
        assert_eq!(capture_date("2026-10-18T09:12:00"), Some("2026-10-18"));
        assert_eq!(capture_date("2026-10-18"), Some("2026-10-18"));
        assert_eq!(capture_date("2026-13-01"), None);
        assert_eq!(capture_date("2026:10:18 09:12:00"), None);
        assert_eq!(capture_date("Oct 12"), None);
    }

    #[test]
    fn shoot_names_read_back_into_parts() {
        assert_eq!(
            parse_shoot_name("2026-10-18_Boat-Trip"),
            ShootNameParts {
                date: Some("2026-10-18".into()),
                title: "Boat Trip".into()
            }
        );
        assert_eq!(
            parse_shoot_name("2026-10-18").date.as_deref(),
            Some("2026-10-18")
        );
        assert_eq!(parse_shoot_name("Oct 12, 2026").title, "Oct 12, 2026");
        assert_eq!(parse_shoot_name("2026-10-18 party").date, None);
    }

    #[test]
    fn originals_keep_camera_names_in_a_dated_folder() {
        assert_eq!(
            originals_folder("2026-10-18_Boat-Trip").join(safe_file_name("DSC08005.ARW")),
            PathBuf::from("originals/2026-10-18_Boat-Trip/DSC08005.ARW")
        );
        assert_eq!(safe_file_name("IMG_1234.HEIC"), "IMG_1234.HEIC");
        assert_eq!(safe_file_name("IMG 1234 (1).jpg"), "IMG-1234-1.jpg");
        assert_eq!(safe_file_name("Café shot.JPG"), "Cafe-shot.JPG");
        assert_eq!(safe_file_name(".jpg"), "jpg");
        assert_eq!(safe_file_name("???.jpg"), "photo.jpg");
    }

    #[test]
    fn unique_names_count_from_two() {
        let taken = ["DSC00001.ARW", "DSC00001-2.ARW"];
        assert_eq!(
            unique_name("DSC00001.ARW", |n| taken.contains(&n)),
            "DSC00001-3.ARW"
        );
        assert_eq!(
            unique_name("DSC00002.ARW", |n| taken.contains(&n)),
            "DSC00002.ARW"
        );
        assert_eq!(unique_name("notes", |n| n == "notes"), "notes-2");
    }

    #[test]
    fn exports_carry_the_shoot_and_the_original() {
        assert_eq!(
            export_file_name("2026-10-18_Boat-Trip", "DSC08005.ARW", "jpg"),
            "2026-10-18_Boat-Trip_DSC08005.jpg"
        );
        assert_eq!(
            export_file_name("2026-10-18_Boat-Trip", "IMG 0042.HEIC", ".JPG"),
            "2026-10-18_Boat-Trip_IMG-0042.jpg"
        );
        assert_eq!(export_file_name("", "DSC08005.ARW", "jpg"), "DSC08005.jpg");
        assert_eq!(
            export_folder_name("2026-10-18_Boat-Trip"),
            "2026-10-18_Boat-Trip"
        );
        assert_eq!(sidecar_file_name("DSC08005.ARW"), "DSC08005.xmp");
    }

    #[test]
    fn keywords_are_hierarchical_under_a_facet() {
        let k = Keyword::parse("Places|Portugal|Lisbon").unwrap();
        assert_eq!((k.facet, k.leaf()), (Facet::Places, "Lisbon"));
        assert_eq!(k.path(), "Places|Portugal|Lisbon");
        assert_eq!(
            Keyword::parse(" location > Portugal >  Lisbon ")
                .unwrap()
                .path(),
            "Places|Portugal|Lisbon"
        );
        assert_eq!(Keyword::parse("dog").unwrap().path(), "Subjects|dog");
        assert_eq!(
            Keyword::parse("Animals/\"dog\", ").unwrap().path(),
            "Subjects|Animals|dog"
        );
        assert_eq!(Keyword::parse("People"), None);
        assert_eq!(Keyword::parse(" | | "), None);
        let long = Keyword::parse(&"word ".repeat(30)).unwrap();
        assert!(long.leaf().len() <= MAX_KEYWORD);
    }

    #[test]
    fn keywords_write_out_as_lightroom_does() {
        let ks: Vec<Keyword> = [
            "Places|Portugal|Lisbon",
            "Places|Portugal|Porto",
            "Subjects|Animals|dog",
            "subjects|animals|Dog",
            "Events|Birthday",
        ]
        .iter()
        .filter_map(|s| Keyword::parse(s))
        .collect();
        assert_eq!(
            hierarchical_subject(&ks),
            [
                "Places|Portugal|Lisbon",
                "Places|Portugal|Porto",
                "Subjects|Animals|dog",
                "Events|Birthday"
            ]
        );
        assert_eq!(
            flat_keywords(&ks),
            ["Portugal", "Lisbon", "Porto", "Animals", "dog", "Birthday"]
        );
    }
}
