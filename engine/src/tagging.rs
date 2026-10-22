//! The AI's captions and Keywords: one model call per Photo, on its preview with what
//! Argus already knows (Shoot name, date, camera, faces, the Keywords the person already
//! uses). Everything it says lands as a suggestion ([`Library::suggest_tags`]) that the
//! person keeps or dismisses in the loupe's Info panel. It never names People.

use crate::agent::{Chat, Msg, Role};
use crate::analyzers;
use crate::library::{Library, PhotoFilter, PhotoId, ShootId, Verdict};
use crate::naming::{Keyword, TAGGING_GUIDE};
use crate::{Error, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

/// Long edge of the preview the model sees: enough to read a sign or a dish.
pub const EDGE: u32 = 768;
/// Most Keywords kept from one reply.
const MAX_KEYWORDS: usize = 15;

const SYSTEM: &str = "You tag photos for a photographer's library, like a careful \
assistant in Lightroom. Look at the photo and reply with a caption and keywords as JSON. \
Only say what is clearly visible or given to you; when unsure, leave it out. Never name \
or guess who a person is, and never describe their attractiveness, ethnicity, religion \
or health. Do not add keywords under People.";

/// The reply's JSON Schema: a caption and a list of Keyword paths.
pub fn schema() -> Value {
    json!({
        "type": "object",
        "required": ["caption", "keywords"],
        "properties": {
            "caption": { "type": "string" },
            "keywords": { "type": "array", "items": { "type": "string" } }
        }
    })
}

#[derive(Deserialize)]
struct Reply {
    #[serde(
        default,
        alias = "description",
        alias = "Caption",
        alias = "Description"
    )]
    caption: String,
    #[serde(default)]
    keywords: Vec<String>,
}

/// The Photos a `tag_photos` run covers: `photo_ids`, or a Shoot's (the newest when none)
/// without its rejects; Photos the AI tagged before are left out unless `redo`.
pub fn photos(
    lib: &Library,
    shoot: Option<ShootId>,
    photo_ids: Option<&[PhotoId]>,
    redo: bool,
) -> Result<Vec<PhotoId>> {
    let ids: Vec<PhotoId> = match photo_ids {
        Some(ids) => ids.to_vec(),
        None => {
            let shoot = match shoot {
                Some(s) => s,
                None => lib
                    .shoots()?
                    .iter()
                    .map(|s| s.id)
                    .max()
                    .ok_or_else(|| Error("the Library has no Shoots".into()))?,
            };
            lib.list_photos(&PhotoFilter {
                shoot: Some(shoot),
                verdict: None,
            })?
            .into_iter()
            .filter(|p| p.verdict != Verdict::Reject)
            .map(|p| p.id)
            .collect()
        }
    };
    let mut out = Vec::new();
    for id in ids {
        if redo || !lib.has_ai_tags(id)? {
            out.push(id);
        }
    }
    Ok(out)
}

/// Ask the model about each Photo and store its suggestions, calling `progress(done,
/// total)` after each. A Photo whose reply can't be read is skipped, not fatal; returns
/// how many Photos got suggestions.
pub fn tag(
    library: &Path,
    chat: &dyn Chat,
    photos: &[PhotoId],
    progress: &dyn Fn(usize, usize),
) -> Result<usize> {
    let mut tagged = 0;
    progress(0, photos.len());
    for (i, &id) in photos.iter().enumerate() {
        // Opened per Photo so the person's edits in between are never held up.
        let lib = Library::open(library)?;
        let photo = crate::tools::find_photo(&lib, id)?;
        let img = crate::tools::open_preview(&photo)?
            .thumbnail(EDGE, EDGE)
            .to_rgb8();
        let jpeg = crate::tools::encode_jpeg(&img)?;
        let text = ask(&lib, &photo)?;
        drop(lib);
        let reply = chat.next(
            SYSTEM,
            &[Msg {
                role: Role::User,
                text,
                images: vec![jpeg],
            }],
        )?;
        if let Some((caption, keywords)) = parse(&reply) {
            let mut lib = Library::open(library)?;
            lib.suggest_tags(id, Some(&caption), &keywords)?;
            tagged += 1;
        }
        progress(i + 1, photos.len());
    }
    Ok(tagged)
}

fn ask(lib: &Library, photo: &crate::library::Photo) -> Result<String> {
    let shoot = lib
        .shoots()?
        .into_iter()
        .find(|s| s.id == photo.shoot_id)
        .map(|s| crate::naming::parse_shoot_name(&s.name).title)
        .unwrap_or_default();
    let faces = analyzers::facts(lib, photo, &["faces"])
        .map(|f| analyzers::caption(&f))
        .unwrap_or_default();
    let known: Vec<String> = lib
        .keyword_counts(None, 40)?
        .into_iter()
        .filter(|k| !k.keyword.starts_with("People|"))
        .map(|k| k.keyword)
        .collect();
    let mut text = format!(
        "{TAGGING_GUIDE}\n\nThis photo: taken {} on a {}",
        photo.captured_at, photo.camera
    );
    if !shoot.is_empty() {
        text += &format!(", in a shoot the photographer named \"{shoot}\"");
    }
    if !faces.is_empty() {
        text += &format!(". Argus sees: {faces}");
    }
    text += ".";
    if !known.is_empty() {
        text += &format!(
            "\nKeywords already in this library (reuse their exact names when they fit): {}.",
            known.join(", ")
        );
    }
    text += "\nReply with a caption (one or two plain sentences: what is happening, and \
where when it is evident) and five to fifteen keywords someone would search for: the \
scene, a named place, the event, the activity and the main subjects. Skip clothing and \
small objects unless they are the point of the photo. Places are named places only \
(country, city, landmark), never words like indoor or beach. Give one path per idea: \
not both a parent and its child.";
    Ok(text)
}

/// The caption and Keywords from a reply, or None when it isn't the JSON asked for.
/// People Keywords are dropped here as well as in the Library.
fn parse(reply: &str) -> Option<(String, Vec<Keyword>)> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    let r: Reply = serde_json::from_str(&reply[start..=end]).ok()?;
    let mut keywords: Vec<Keyword> = Vec::new();
    for k in r.keywords.iter().filter_map(|k| Keyword::parse(k)) {
        // A Place is a proper noun; "indoor" or "beach" isn't one.
        let unnamed_place = k.facet == crate::naming::Facet::Places
            && !k.levels[0].starts_with(|c: char| c.is_uppercase());
        if k.facet != crate::naming::Facet::People
            && !unnamed_place
            && !keywords
                .iter()
                .any(|x| x.path().eq_ignore_ascii_case(&k.path()))
        {
            keywords.push(k);
        }
    }
    // A parent says nothing its child doesn't: `Subjects|boat` beside `Subjects|boat|bow`.
    let paths: Vec<String> = keywords
        .iter()
        .map(|k| k.path().to_lowercase() + "|")
        .collect();
    keywords.retain(|k| {
        let me = k.path().to_lowercase() + "|";
        !paths
            .iter()
            .any(|p| p.len() > me.len() && p.starts_with(&me))
    });
    keywords.truncate(MAX_KEYWORDS);
    Some((r.caption.trim().to_string(), keywords))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_becomes_keywords_without_people_or_repeats() {
        let (caption, ks) = parse(
            r#"Sure: {"caption":" Two friends on a ferry. ","keywords":["Places|Portugal|Lisbon","People|Anna","ferry","Subjects|ferry","Activities|travel","Places|indoor","Subjects|boat","Subjects|boat|bow"]}"#,
        )
        .unwrap();
        assert_eq!(caption, "Two friends on a ferry.");
        let paths: Vec<String> = ks.iter().map(Keyword::path).collect();
        assert_eq!(
            paths,
            [
                "Places|Portugal|Lisbon",
                "Subjects|ferry",
                "Activities|travel",
                "Subjects|boat|bow"
            ]
        );
        assert!(parse("no json here").is_none());
        let (caption, _) = parse(r#"{"description":"A dog.","keywords":[]}"#).unwrap();
        assert_eq!(caption, "A dog.");
    }
}
