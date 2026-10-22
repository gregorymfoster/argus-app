//! Finding Photos by meaning: SigLIP B/16 (Google, Apache 2.0) puts a Photo and a few
//! words in the same 768-number space, so "kids in the snow" finds the frames that look
//! like it with no Keyword typed. The image half runs as the `meaning` Analyzer (one
//! vector per Photo, stored as a Fact); the text half runs once per query.
//!
//! The model is too large to embed in the app (407 MB), so it lives in the models folder
//! ([`dir`]) and everything here is off until it's there ([`available`]); search falls back
//! to words alone. [`download`] fetches it once from the public argus-releases page (Greg,
//! 2026-10-22), resuming a cut-off file and checking each against the manifest's sha256.

use crate::analyzers::runtime::{self, ndarray, Model, Tensor};
use crate::analyzers::{Analyzer, Cost, Fact, Input};
use crate::library::{Library, PhotoId, SearchHit, ShootId};
use crate::{Error, Result};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

pub const DIMS: usize = 768;
const SIZE: usize = 224;
/// The text tower's fixed length, as SigLIP was trained.
const TOKENS: usize = 64;
const EOS: i64 = 1;
/// The trained logit scale and bias: `sigmoid(SCALE * cos + BIAS)` is the model's
/// probability that the words describe the image.
const SCALE: f32 = 117.33;
const BIAS: f32 = -12.93;

pub const IMAGE_FILE: &str = "siglip-image.onnx";
pub const TEXT_FILE: &str = "siglip-text.onnx";
pub const VOCAB_FILE: &str = "siglip-vocab.tsv";

/// The models folder: ARGUS_MODELS, else `~/Library/Application Support/Argus/models`
/// on the Mac (`~/.local/share/argus/models` elsewhere).
pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("ARGUS_MODELS") {
        return d.into();
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Argus/models")
    } else {
        home.join(".local/share/argus/models")
    }
}

/// Whether the model is in the models folder.
pub fn available() -> bool {
    let d = dir();
    [IMAGE_FILE, TEXT_FILE, VOCAB_FILE]
        .iter()
        .all(|f| d.join(f).is_file())
}

/// Where [`download`] fetches the files: ARGUS_MODEL_URL (tests), else the release that
/// holds this model on the public argus-releases page.
fn base_url() -> String {
    std::env::var("ARGUS_MODEL_URL").unwrap_or_else(|_| {
        "https://github.com/gregorymfoster/argus-releases/releases/download/siglip-b16-v1".into()
    })
}

/// One file of the model, as `engine/models/manifest.json` lists it.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ModelFile {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
}

/// The model's files, from the manifest built into the engine.
pub fn files() -> Vec<ModelFile> {
    #[derive(serde::Deserialize)]
    struct Manifest {
        models: Vec<serde_json::Value>,
    }
    let m: Manifest = serde_json::from_str(include_str!("../models/manifest.json"))
        .expect("engine/models/manifest.json");
    m.models
        .into_iter()
        .filter(|v| v["analyzer"] == "meaning")
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect()
}

/// What search by meaning needs and has: for Settings and the palette.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Status {
    pub installed: bool,
    /// The whole download, and how much of it is on disk (part files included).
    pub bytes: u64,
    pub have_bytes: u64,
    pub folder: String,
}

pub fn status() -> Status {
    let d = dir();
    let files = files();
    let have = |f: &ModelFile| {
        let done = d.join(&f.file);
        let part = d.join(format!("{}.part", f.file));
        std::fs::metadata(&done)
            .or_else(|_| std::fs::metadata(&part))
            .map(|m| m.len().min(f.bytes))
            .unwrap_or(0)
    };
    Status {
        installed: available(),
        bytes: files.iter().map(|f| f.bytes).sum(),
        have_bytes: files.iter().map(have).sum(),
        folder: d.to_string_lossy().into_owned(),
    }
}

/// Fetch the model into the models folder, calling `progress(bytes done, bytes total)` as
/// it goes. A file already there is kept; a `.part` file from a cut-off download is
/// resumed; each file is checked against its sha256 before it's renamed into place, so
/// [`available`] never sees a half-written model.
pub fn download(progress: &dyn Fn(u64, u64)) -> Result<()> {
    download_to(&dir(), &base_url(), &files(), progress)
}

/// [`download`] of `files` from `base` into `d`.
pub fn download_to(
    d: &std::path::Path,
    base: &str,
    files: &[ModelFile],
    progress: &dyn Fn(u64, u64),
) -> Result<()> {
    use sha2::Digest;
    use std::io::{Read, Write};
    std::fs::create_dir_all(d).map_err(|e| Error(format!("models folder {}: {e}", d.display())))?;
    let total: u64 = files.iter().map(|f| f.bytes).sum();
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(20))
        .timeout_read(std::time::Duration::from_secs(60))
        .build();
    let mut before = 0u64;
    for f in files {
        let done = d.join(&f.file);
        if done.is_file() {
            before += f.bytes;
            progress(before, total);
            continue;
        }
        let part = d.join(format!("{}.part", f.file));
        let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if have > f.bytes {
            have = 0;
        }
        if have < f.bytes {
            let url = format!("{base}/{}", f.file);
            let mut req = agent.get(&url);
            if have > 0 {
                req = req.set("Range", &format!("bytes={have}-"));
            }
            let resp = req
                .call()
                .map_err(|e| Error(format!("download {url}: {e}")))?;
            // A server that ignores the Range sends the whole file again.
            if resp.status() != 206 {
                have = 0;
            }
            let mut out = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(have > 0)
                .truncate(have == 0)
                .open(&part)
                .map_err(|e| Error(format!("{}: {e}", part.display())))?;
            let mut body = resp.into_reader();
            let mut buf = vec![0u8; 1 << 16];
            let mut last = std::time::Instant::now();
            loop {
                let n = body
                    .read(&mut buf)
                    .map_err(|e| Error(format!("download {url}: {e}")))?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])
                    .map_err(|e| Error(format!("{}: {e}", part.display())))?;
                have += n as u64;
                if last.elapsed().as_millis() > 250 {
                    progress(before + have.min(f.bytes), total);
                    last = std::time::Instant::now();
                }
            }
            out.flush()
                .map_err(|e| Error(format!("{}: {e}", part.display())))?;
        }
        let mut hash = sha2::Sha256::new();
        let mut file =
            std::fs::File::open(&part).map_err(|e| Error(format!("{}: {e}", part.display())))?;
        std::io::copy(&mut file, &mut hash)
            .map_err(|e| Error(format!("{}: {e}", part.display())))?;
        let got = format!("{:x}", hash.finalize());
        if got != f.sha256 {
            let _ = std::fs::remove_file(&part);
            return Err(Error(format!(
                "{} came down damaged (sha256 {got}, expected {}); it will download again",
                f.file, f.sha256
            )));
        }
        std::fs::rename(&part, &done).map_err(|e| Error(format!("{}: {e}", done.display())))?;
        before += f.bytes;
        progress(before, total);
    }
    Ok(())
}

/// The `meaning` Analyzer: a Photo's SigLIP vector. It runs only once the model is there.
pub struct Meaning;

impl Analyzer for Meaning {
    fn id(&self) -> &'static str {
        "meaning"
    }
    fn version(&self) -> u32 {
        1
    }
    fn cost(&self) -> Cost {
        Cost::Model
    }
    fn available(&self) -> bool {
        available()
    }
    fn run(&self, input: &Input) -> Result<Fact> {
        let v = embed_image(input.preview)?;
        Ok(Fact {
            value: json!({ "dims": DIMS }),
            blob: Some(v.iter().flat_map(|x| x.to_le_bytes()).collect()),
        })
    }
}

/// The unit vector stored in a `meaning` Fact's blob.
pub fn vector(blob: &[u8]) -> Option<Vec<f32>> {
    (blob.len() == DIMS * 4).then(|| {
        blob.as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect()
    })
}

/// The model's probability (0..1) that words with vector `text` describe a Photo with
/// vector `image`.
pub fn probability(text: &[f32], image: &[f32]) -> f32 {
    let cos: f32 = text.iter().zip(image).map(|(a, b)| a * b).sum();
    1.0 / (1.0 + (-(SCALE * cos + BIAS)).exp())
}

/// A Photo counts as found by meaning from this probability up (cosine 0.06). SigLIP's
/// probabilities run low for retrieval: on 200 bench photos of people, words for things
/// in them ("sunset", "orange shirt") topped out at cosine 0.07 to 0.12, words for things
/// that weren't ("dog", "beach", "a car") at 0.05 or below.
pub const FOUND: f32 = 0.0028;
/// What a sure meaning match adds to a hit's score (a kept Keyword leaf adds 3 a word).
const WEIGHT: f32 = 4.0;

/// Photos for `query`, best first: every Photo whose words match (as
/// [`Library::search_words`]) and, once the model is installed, every Photo the model
/// thinks the words describe. A Photo found both ways ranks highest; one found only by
/// meaning says "looks like" in `matched`.
pub fn search(lib: &Library, query: &str, shoot: Option<ShootId>) -> Result<Vec<SearchHit>> {
    let mut hits = lib.search_words(query, shoot)?;
    if query.trim().is_empty() || !available() {
        return Ok(hits);
    }
    let text = match embed_text(query.trim()) {
        Ok(v) => v,
        Err(e) => {
            crate::debug_log::event("meaning_failed", json!({ "error": e.0 }));
            return Ok(hits);
        }
    };
    let at: HashMap<PhotoId, usize> = hits
        .iter()
        .enumerate()
        .map(|(i, h)| (h.photo_id, i))
        .collect();
    for (photo, shoot_id, blob) in lib.fact_blobs("meaning", Meaning.version(), shoot)? {
        let Some(v) = vector(&blob) else { continue };
        let p = probability(&text, &v);
        match at.get(&photo) {
            Some(&i) => hits[i].score += WEIGHT * p,
            None if p >= FOUND => hits.push(SearchHit {
                photo_id: photo,
                shoot_id,
                score: WEIGHT * p,
                matched: vec!["looks like".into()],
            }),
            None => {}
        }
    }
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok(hits)
}

fn model(
    cell: &'static OnceLock<std::result::Result<Model, String>>,
    file: &str,
    text: bool,
) -> Result<&'static Model> {
    cell.get_or_init(|| {
        let path = dir().join(file);
        if text {
            runtime::load_file_i64(&path, &[1, TOKENS])
        } else {
            runtime::load_file(&path, &[1, 3, SIZE, SIZE])
        }
        .map_err(|e| e.0)
    })
    .as_ref()
    .map_err(|e| Error(e.clone()))
}

/// A Photo's unit vector: the whole frame stretched to 224 square, RGB scaled to -1..1.
pub fn embed_image(preview: &image::RgbImage) -> Result<Vec<f32>> {
    static IMAGE: OnceLock<std::result::Result<Model, String>> = OnceLock::new();
    let img = image::imageops::resize(
        preview,
        SIZE as u32,
        SIZE as u32,
        image::imageops::FilterType::CatmullRom,
    );
    let t: Tensor = ndarray::Array4::from_shape_fn((1, 3, SIZE, SIZE), |(_, c, y, x)| {
        img.get_pixel(x as u32, y as u32)[c] as f32 / 127.5 - 1.0
    })
    .into();
    unit(runtime::run_first(model(&IMAGE, IMAGE_FILE, false)?, t)?)
}

/// The unit vector of a query's words (the last few are remembered, since the palette asks
/// again as the person types).
pub fn embed_text(text: &str) -> Result<Vec<f32>> {
    static TEXT: OnceLock<std::result::Result<Model, String>> = OnceLock::new();
    static SEEN: OnceLock<Mutex<HashMap<String, Vec<f32>>>> = OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);
    if let Some(v) = seen.lock().unwrap().get(text) {
        return Ok(v.clone());
    }
    let ids = tokenizer()?.encode(text);
    let t: Tensor = ndarray::Array2::from_shape_vec((1, TOKENS), ids)
        .map_err(|e| Error(format!("text input: {e}")))?
        .into();
    let v = unit(runtime::run_first(model(&TEXT, TEXT_FILE, true)?, t)?)?;
    let mut seen = seen.lock().unwrap();
    if seen.len() > 64 {
        seen.clear();
    }
    seen.insert(text.to_string(), v.clone());
    Ok(v)
}

fn unit(v: Vec<f32>) -> Result<Vec<f32>> {
    if v.len() != DIMS {
        return Err(Error(format!(
            "meaning model gave {} numbers, not {DIMS}",
            v.len()
        )));
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
    Ok(v.into_iter().map(|x| x / norm).collect())
}

fn tokenizer() -> Result<&'static Tokenizer> {
    static TOK: OnceLock<std::result::Result<Tokenizer, String>> = OnceLock::new();
    TOK.get_or_init(|| {
        let path = dir().join(VOCAB_FILE);
        std::fs::read_to_string(&path)
            .map(|s| Tokenizer::new(&s))
            .map_err(|e| format!("{}: {e}", path.display()))
    })
    .as_ref()
    .map_err(|e| Error(e.clone()))
}

/// SigLIP's SentencePiece unigram tokenizer, as Hugging Face's `SiglipTokenizer` runs it:
/// lowercase, drop ASCII punctuation, collapse spaces, then the most likely split into
/// pieces (bytes for anything the vocabulary lacks), an end token, padded to 64.
pub struct Tokenizer {
    /// Piece text to (id, score).
    pieces: HashMap<String, (i64, f32)>,
    /// Longest piece, in chars.
    longest: usize,
    /// What an unknown char costs, below every real piece (SentencePiece's rule).
    unknown: f32,
    /// `<0x00>`'s id; the other bytes follow it.
    byte0: Option<i64>,
}

impl Tokenizer {
    /// From the vocabulary file: one `piece<TAB>score` line per id, in id order.
    pub fn new(vocab: &str) -> Self {
        let mut pieces = HashMap::new();
        let mut longest = 1;
        let mut min = 0f32;
        let mut byte0 = None;
        for (id, line) in vocab.lines().enumerate() {
            let Some((piece, score)) = line.split_once('\t') else {
                continue;
            };
            let score: f32 = score.parse().unwrap_or(0.0);
            let id = id as i64;
            if piece == "<0x00>" {
                byte0 = Some(id);
            }
            // Control pieces and bytes are never matched as text.
            if (piece.starts_with('<') && piece.ends_with('>')) || piece.is_empty() {
                continue;
            }
            min = min.min(score);
            longest = longest.max(piece.chars().count());
            pieces.insert(piece.to_string(), (id, score));
        }
        Self {
            pieces,
            longest,
            unknown: min - 10.0,
            byte0,
        }
    }

    /// The model's input ids for `text`.
    pub fn encode(&self, text: &str) -> Vec<i64> {
        let clean: String = text
            .to_lowercase()
            .chars()
            .filter(|c| !c.is_ascii_punctuation())
            .collect();
        let words: Vec<&str> = clean.split_whitespace().collect();
        let mut ids = Vec::new();
        if !words.is_empty() {
            let chars: Vec<char> = format!("▁{}", words.join("▁")).chars().collect();
            ids = self.split(&chars);
        }
        ids.truncate(TOKENS - 1);
        ids.push(EOS);
        ids.resize(TOKENS, EOS);
        ids
    }

    /// Viterbi over the pieces: the split whose scores sum highest.
    fn split(&self, chars: &[char]) -> Vec<i64> {
        let n = chars.len();
        // best[i]: (score of the best split of chars[..i], where its last piece starts, id or None for unknown)
        let mut best: Vec<(f32, usize, Option<i64>)> = vec![(f32::NEG_INFINITY, 0, None); n + 1];
        best[0].0 = 0.0;
        for start in 0..n {
            let base = best[start].0;
            if base == f32::NEG_INFINITY {
                continue;
            }
            let mut piece = String::new();
            let mut single = false;
            for end in start + 1..=(start + self.longest).min(n) {
                piece.push(chars[end - 1]);
                if let Some(&(id, score)) = self.pieces.get(&piece) {
                    single |= end == start + 1;
                    if base + score > best[end].0 {
                        best[end] = (base + score, start, Some(id));
                    }
                }
            }
            if !single && base + self.unknown > best[start + 1].0 {
                best[start + 1] = (base + self.unknown, start, None);
            }
        }
        let mut out = Vec::new();
        let mut end = n;
        while end > 0 {
            let (_, start, id) = best[end];
            match (id, self.byte0) {
                (Some(id), _) => out.push(vec![id]),
                (None, Some(b0)) => {
                    let mut buf = [0u8; 4];
                    let bytes = chars[start].encode_utf8(&mut buf).as_bytes();
                    out.push(bytes.iter().map(|&b| b0 + b as i64).collect());
                }
                (None, None) => out.push(vec![2]),
            }
            end = start;
        }
        out.into_iter().rev().flatten().collect()
    }
}
