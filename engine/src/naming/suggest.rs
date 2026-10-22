//! Before an Import: small thumbnails of files on a card, and a suggested Shoot name.
//!
//! The name comes from the local vision model (Ollama, the default Provider) looking at a
//! few thumbnails. It is one short request, so it isn't throttled like a cull; when there
//! is no local model, or it fails, the suggestion falls back to the Shoot's date.

use crate::cull::Ollama;
use crate::{Error, Result};
use base64::Engine as _;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// Thumbnails the model sees when naming a Shoot.
pub const NAME_SAMPLE: usize = 4;
const NAME_EDGE: u32 = 384;

/// A small upright JPEG of a file that isn't imported yet: a RAW's embedded preview, or
/// the JPEG itself, scaled so its long edge is `long_edge`.
pub fn thumbnail(path: &Path, long_edge: u32) -> Result<Vec<u8>> {
    let is_raw = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| !matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg"));
    let jpeg = if is_raw {
        crate::embedded_preview_quick(path)?
    } else {
        std::fs::read(path).map_err(|e| Error(format!("read {}: {e}", path.display())))?
    };
    let orientation = if is_raw {
        crate::preview::file_orientation(path).unwrap_or(1) as u32
    } else {
        exif::Reader::new()
            .read_from_container(&mut std::io::Cursor::new(&jpeg))
            .ok()
            .and_then(|ex| {
                ex.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                    .and_then(|f| f.value.get_uint(0))
            })
            .unwrap_or(1)
    };
    let img = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)
        .map_err(|e| Error(format!("{}: {e}", path.display())))?
        .thumbnail(long_edge, long_edge);
    let img = match orientation {
        6 => img.rotate90(),
        3 => img.rotate180(),
        8 => img.rotate270(),
        _ => img,
    }
    .to_rgb8();
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, 80)
        .encode(
            img.as_raw(),
            img.width() as u16,
            img.height() as u16,
            jpeg_encoder::ColorType::Rgb,
        )
        .map_err(|e| Error(format!("jpeg encode: {e}")))?;
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
pub struct NameSuggestion {
    pub name: String,
    /// "ai" when the model named it, "date" for the fallback.
    pub by: &'static str,
    /// Why the model wasn't used, when it wasn't.
    pub note: Option<String>,
}

/// A Shoot name by the rules in docs/naming.md: the Shoot's date first, then its words
/// joined by hyphens (`2026-10-18_Boat-Trip`). A name that already starts with a date
/// keeps it; with no capture time, today's date stands in.
pub fn shoot_slug(name: &str, captured_at: Option<&str>) -> String {
    let name = name.trim();
    let date = super::capture_date(name)
        .or_else(|| captured_at.and_then(super::capture_date))
        .map(str::to_string)
        .unwrap_or_else(|| crate::library::time::now()[..10].to_string());
    super::shoot_name(&date, name)
}

/// Keep a model's answer to a short, plain name.
pub fn clean_name(raw: &str) -> Option<String> {
    let s = raw
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '.' || c.is_whitespace());
    let words: Vec<&str> = s.split_whitespace().take(5).collect();
    let name = words.join(" ");
    (!name.is_empty() && name.len() <= 48).then_some(name)
}

/// Suggest a name for the Shoot these files belong to. `sample` is a few of its files
/// (spread across the Shoot), `captured_at` its first capture time for the fallback.
pub fn suggest_shoot_name(
    sample: &[&Path],
    captured_at: Option<&str>,
    provider: Option<&str>,
) -> NameSuggestion {
    let fallback = |note: String| NameSuggestion {
        name: shoot_slug("", captured_at),
        by: "date",
        note: Some(note),
    };
    let env = std::env::var("ARGUS_PROVIDER").ok();
    let provider = provider.or(env.as_deref()).unwrap_or("ollama");
    if provider != "ollama" {
        return fallback(format!(
            "names come from the local model; {provider} isn't used for this"
        ));
    }
    match ask_ollama(&Ollama::from_env(), sample) {
        Ok(name) => NameSuggestion {
            name: shoot_slug(&name, captured_at),
            by: "ai",
            note: None,
        },
        Err(e) => fallback(e.0),
    }
}

fn ask_ollama(o: &Ollama, sample: &[&Path]) -> Result<String> {
    let images: Vec<Value> = sample
        .iter()
        .take(NAME_SAMPLE)
        .filter_map(|p| thumbnail(p, NAME_EDGE).ok())
        .map(|j| Value::String(base64::engine::general_purpose::STANDARD.encode(j)))
        .collect();
    if images.is_empty() {
        return Err(Error("no thumbnails to show the model".into()));
    }
    let body = json!({
        "model": o.model,
        "stream": false,
        "format": { "type": "object", "required": ["name"],
                    "properties": { "name": { "type": "string" } } },
        // The cull's own options, so a loaded model answers at once: any difference
        // (a smaller num_ctx) made Ollama reload all 17 GB of it, and the name timed out.
        "keep_alive": "30m",
        "options": o.options(),
        "messages": [{
            "role": "user",
            "content": "These photos are from one photo shoot. Suggest a short, specific name for \
                        the shoot: 1 to 4 words in Title Case, naming the activity, event or place \
                        (for example \"Pickleball\", \"Lisbon Old Town\", \"Maya's Birthday\"). \
                        No date, no quotes. Reply as JSON: {\"name\": \"...\"}",
            "images": images,
        }],
    });
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(300))
        .build();
    let resp: Value = agent
        .post(&format!("{}/api/chat", o.base_url))
        .send_json(body)
        .map_err(|e| Error(format!("local model unavailable ({e})")))?
        .into_json()
        .map_err(|e| Error(format!("local model reply: {e}")))?;
    let text = resp["message"]["content"].as_str().unwrap_or_default();
    let name = serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v["name"].as_str().map(str::to_string))
        .unwrap_or_else(|| text.to_string());
    clean_name(&name).ok_or_else(|| Error(format!("local model gave no usable name: {text:.80}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shoot_slugs_and_clean_names() {
        assert_eq!(
            shoot_slug("Maya's Birthday", Some("2026-10-12T09:00:00")),
            "2026-10-12_Mayas-Birthday"
        );
        assert_eq!(
            shoot_slug("2026-10-11_Boat Trip", Some("2026-10-12T09:00:00")),
            "2026-10-11_Boat-Trip"
        );
        assert!(shoot_slug("Lisbon, Old Town", None).ends_with("_Lisbon-Old-Town"));
        assert_eq!(
            clean_name("  \"Pickleball.\" ").as_deref(),
            Some("Pickleball")
        );
        assert_eq!(
            clean_name("one two three four five six").as_deref(),
            Some("one two three four five")
        );
        assert_eq!(clean_name("   "), None);
    }

    #[test]
    fn falls_back_to_the_date_without_a_model() {
        let s = suggest_shoot_name(&[], Some("2026-10-12T09:00:00"), Some("ollama"));
        assert_eq!((s.name.as_str(), s.by), ("2026-10-12", "date"));
        let s = suggest_shoot_name(&[], None, Some("claude-code"));
        assert_eq!(s.by, "date");
        assert!(super::super::capture_date(&s.name).is_some(), "{}", s.name);
    }
}
