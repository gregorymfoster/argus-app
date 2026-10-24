//! The Ollama Provider: a local vision model through Ollama's HTTP API, the private default.
//! It streams replies with a stall watchdog ([`Stall`]) and runs under the Compute level's
//! throttle; the model is the one chosen in Settings ([`choose_ollama_model`]) or
//! [`DEFAULT_OLLAMA_MODEL`].

use super::parse_json_reply;
use crate::compute::{Compute, OllamaProcesses, Throttle};
use crate::cull::{
    ask, schema, system, ContactSheet, CullResult, Provider, Streamed, SHEET_LONG_EDGE,
};
use crate::{Error, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// A model Ollama has pulled on this Mac.
#[derive(Debug, Clone, Serialize)]
pub struct InstalledModel {
    pub name: String,
    pub bytes: u64,
    /// In memory now, so the next request starts at once.
    pub loaded: bool,
}

/// A local vision model through Ollama's HTTP API (the private, default Provider).
pub struct Ollama {
    pub base_url: String,
    pub model: String,
    /// Context window to ask for; a Shoot's images need far more than Ollama's default.
    pub num_ctx: u32,
    /// How much of the Mac the model may take (see [`crate::compute`]).
    pub compute: Compute,
}

/// The local vision model that culled Greg's bench Shoots best (about 17 GB at Q4); it
/// beat gemma4:31b on star AUC and Best Photos (2026-10-18, see ADR 0006).
pub const DEFAULT_OLLAMA_MODEL: &str = "qwen3.8:27b";

/// The model chosen in Settings › AI and Compute (kept in the Library as `ollama_model`).
static CHOSEN_MODEL: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

/// Use `model` for Ollama from now on in this process (None: back to the default).
/// ARGUS_OLLAMA_MODEL still wins, so tests and benches can pin one.
pub fn choose_ollama_model(model: Option<String>) {
    *CHOSEN_MODEL.write().unwrap() = model.filter(|m| !m.trim().is_empty());
}

impl Ollama {
    /// OLLAMA_HOST (default 127.0.0.1:11434), ARGUS_OLLAMA_MODEL (else the model chosen in
    /// Settings, else [`DEFAULT_OLLAMA_MODEL`]), ARGUS_OLLAMA_CTX, ARGUS_COMPUTE.
    pub fn from_env() -> Self {
        let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "127.0.0.1:11434".into());
        let base_url = if host.starts_with("http") {
            host
        } else {
            format!("http://{host}")
        };
        Self {
            base_url,
            model: std::env::var("ARGUS_OLLAMA_MODEL")
                .ok()
                .or_else(|| CHOSEN_MODEL.read().unwrap().clone())
                .unwrap_or_else(|| DEFAULT_OLLAMA_MODEL.into()),
            num_ctx: std::env::var("ARGUS_OLLAMA_CTX")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(65_536),
            compute: Compute::from_env(),
        }
    }

    /// The /api/chat request body for a Contact sheet. Public for tests.
    pub fn request_body(&self, sheet: &ContactSheet) -> Value {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut messages = vec![
            json!({ "role": "system", "content": system() }),
            json!({ "role": "user", "content": format!(
                "{} Each one follows as its own message.", sheet.intro()) }),
        ];
        for p in &sheet.photos {
            messages.push(json!({
                "role": "user",
                "content": p.caption(),
                "images": [b64.encode(&p.jpeg)]
            }));
        }
        messages.push(json!({ "role": "user", "content": format!(
            "{} Return JSON with every photo number from 1 to {} exactly once.",
            ask(), sheet.photos.len()) }));
        let mut body = json!({
            "model": self.model,
            "stream": true,
            "format": schema(),
            "keep_alive": "30m",
            "options": self.options(),
            "messages": messages
        });
        // A harness variant can turn thinking on or off (bench/tune.py); unset, Ollama decides.
        if let Some(think) = crate::variant::current().ok().and_then(|v| v.think) {
            body["think"] = json!(think);
        }
        body
    }

    /// Model options. Any difference makes Ollama reload the model, so the load request
    /// and the cull must send exactly these.
    pub(crate) fn options(&self) -> Value {
        let mut options = json!({ "num_ctx": self.num_ctx, "temperature": 0 });
        if let Some(n) = self.compute.plan().num_batch {
            options["num_batch"] = json!(n);
        }
        options
    }

    /// Loads the model without asking anything. Public for tests.
    pub fn load_body(&self) -> Value {
        // Keep it loaded well past the cull; Ollama's 5-minute default can lapse while a
        // throttled cull is still sending.
        json!({ "model": self.model, "stream": false, "messages": [], "keep_alive": "30m", "options": self.options() })
    }

    /// The models Ollama has pulled, largest first, and which are loaded now (`/api/tags`,
    /// `/api/ps`). Err when Ollama isn't running.
    pub fn installed(&self) -> Result<Vec<InstalledModel>> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(3))
            .build();
        let get = |path: &str| -> Result<Value> {
            agent
                .get(&format!("{}{path}", self.base_url))
                .call()
                .map_err(|e| Error(format!("can't reach Ollama at {} ({e})", self.base_url)))?
                .into_json()
                .map_err(|e| Error(format!("Ollama {path}: {e}")))
        };
        let tags = get("/api/tags")?;
        let loaded: Vec<String> = get("/api/ps")
            .ok()
            .and_then(|v| v["models"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|m| m["name"].as_str().map(str::to_owned))
            .collect();
        let mut models: Vec<InstalledModel> = tags["models"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|m| {
                let name = m["name"].as_str()?.to_owned();
                Some(InstalledModel {
                    loaded: loaded.contains(&name),
                    bytes: m["size"].as_u64().unwrap_or(0),
                    name,
                })
            })
            .collect();
        models.sort_by_key(|m| std::cmp::Reverse(m.bytes));
        Ok(models)
    }

    /// One request, once no other Argus process is using the model (a waiting job says
    /// on whom through [`crate::progress::waiting_here`]).
    pub(crate) fn post(&self, agent: &ureq::Agent, body: Value) -> Result<Value> {
        let _turn = crate::model_lock::take(&crate::progress::waiting_here);
        send_chat(agent, &self.base_url, &self.model, body)?
            .into_json()
            .map_err(|e| Error(format!("Ollama response: {e}")))
    }

    /// Send a streaming /api/chat request and gather the reply's content, telling `seen`
    /// after every piece how far it has got. Public for tests.
    pub fn stream(
        &self,
        agent: &ureq::Agent,
        body: Value,
        seen: &dyn Fn(Streamed),
    ) -> Result<String> {
        self.stream_within(
            agent,
            body,
            Stall::from_env(Duration::from_secs(3600)),
            seen,
        )
    }

    /// [`Self::stream`], giving up with an error when the model sends nothing for longer
    /// than `stall` allows: a request Ollama dropped (its runner restarted) or never
    /// started fails where the person sees it, instead of a bar that waits all night.
    pub fn stream_within(
        &self,
        agent: &ureq::Agent,
        body: Value,
        stall: Stall,
        seen: &dyn Fn(Streamed),
    ) -> Result<String> {
        use std::io::BufRead;
        use std::sync::mpsc::RecvTimeoutError;
        let t = Instant::now();
        // The request reads on its own thread so a silent socket can't hold this one; on
        // a stall that thread is left to end at the agent's read timeout.
        let (tx, lines) = std::sync::mpsc::channel::<Result<String>>();
        let (agent2, base, model) = (agent.clone(), self.base_url.clone(), self.model.clone());
        std::thread::Builder::new()
            .name("argus-ollama-stream".into())
            .spawn(move || {
                let r = match send_chat(&agent2, &base, &model, body) {
                    Ok(r) => r,
                    Err(e) => return drop(tx.send(Err(e))),
                };
                for line in std::io::BufReader::new(r.into_reader()).lines() {
                    let line = line.map_err(|e| Error(format!("Ollama stream: {e}")));
                    let end = line.is_err();
                    if tx.send(line).is_err() || end {
                        return;
                    }
                }
            })
            .map_err(|e| Error(format!("Ollama stream thread: {e}")))?;
        let mut content = String::new();
        let mut so_far = Streamed::default();
        let mut scanned = 0;
        let mut finished = false;
        loop {
            let wait = if so_far.tokens == 0 {
                stall.first
            } else {
                stall.gap
            };
            // Wait in short slices so Cancel (`cancel_task`) lands within a moment; leaving
            // drops the reader, which closes the request and stops Ollama generating.
            let since = Instant::now();
            let got = loop {
                crate::progress::check_stopped()?;
                let left = wait.saturating_sub(since.elapsed());
                if left.is_zero() {
                    break Err(RecvTimeoutError::Timeout);
                }
                match lines.recv_timeout(left.min(Duration::from_millis(250))) {
                    Err(RecvTimeoutError::Timeout) => continue,
                    other => break other,
                }
            };
            let line = match got {
                Ok(line) => line?,
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(Error(format!(
                        "the model stopped answering: nothing from Ollama for {} ({}). Ollama \
                         may have restarted or be busy with another app; try again",
                        minutes(wait),
                        if so_far.tokens == 0 {
                            "it never started replying".to_owned()
                        } else {
                            format!("{} tokens in", so_far.tokens)
                        }
                    )))
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(&line)
                .map_err(|e| Error(format!("Ollama stream: {e} in {line:.200}")))?;
            if let Some(e) = v["error"].as_str() {
                return Err(Error(format!("Ollama: {e}")));
            }
            let piece = v["message"]["content"].as_str().unwrap_or_default();
            let thinking = v["message"]["thinking"].as_str().unwrap_or_default();
            if !piece.is_empty() || !thinking.is_empty() {
                so_far.tokens += 1;
            }
            content.push_str(piece);
            let (found, next) = count_photo_keys(content.as_bytes(), scanned);
            so_far.photos += found;
            scanned = next;
            seen(so_far);
            if v["done"].as_bool() == Some(true) {
                crate::agent::log_timing(&self.model, &v, t.elapsed());
                finished = true;
                break;
            }
        }
        if !finished {
            return Err(Error(format!(
                "Ollama's reply broke off after {} tokens (its model runner may have \
                 restarted); try again",
                so_far.tokens
            )));
        }
        Ok(content)
    }
}

fn send_chat(
    agent: &ureq::Agent,
    base_url: &str,
    model: &str,
    body: Value,
) -> Result<ureq::Response> {
    match agent.post(&format!("{base_url}/api/chat")).send_json(body) {
        Ok(r) => Ok(r),
        Err(ureq::Error::Status(code, r)) => {
            let text = r.into_string().unwrap_or_default();
            let hint = if text.contains("not found") {
                format!(" (run `ollama pull {model}`)")
            } else {
                String::new()
            };
            Err(Error(format!("Ollama {code}: {text}{hint}")))
        }
        Err(e) => Err(Error(format!(
            "can't reach Ollama at {base_url} ({e}); is it running?"
        ))),
    }
}

fn minutes(d: Duration) -> String {
    let m = d.as_secs() / 60;
    if m == 0 {
        format!("{} s", d.as_secs())
    } else {
        format!("{m} min")
    }
}

/// How long a streaming request may stay silent before it counts as dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stall {
    /// Until the first token: the model loads, reads every image and may queue in Ollama.
    pub first: Duration,
    /// Between tokens once it is replying.
    pub gap: Duration,
}

impl Stall {
    /// ARGUS_OLLAMA_FIRST_TOKEN_S and ARGUS_OLLAMA_STALL_S (default 5 min) override.
    fn from_env(first: Duration) -> Self {
        let secs = |k: &str| {
            std::env::var(k)
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .map(Duration::from_secs_f64)
        };
        Stall {
            first: secs("ARGUS_OLLAMA_FIRST_TOKEN_S").unwrap_or(first),
            gap: secs("ARGUS_OLLAMA_STALL_S").unwrap_or(Duration::from_secs(300)),
        }
    }

    /// For a cull of `sheet` at `compute`: 5 min plus the time to read the sheet's prompt
    /// at half the 63 tokens/s Qwen 3.8 27B read at Full on Greg's M4 Max (2026-10-20:
    /// 41.7k tokens in 657 s), twice that at Balanced and four times at Gentle, whose
    /// pauses slow the read. Image tokens follow pixels (one per 28x28 patch), so bigger
    /// previews and face close-ups get longer; a flat 6 s a Photo once timed out on them.
    pub fn for_cull(sheet: &ContactSheet, compute: Compute) -> Self {
        let tokens: u64 = sheet
            .photos
            .iter()
            .map(|p| {
                let (w, h) = image::ImageReader::new(std::io::Cursor::new(&p.jpeg))
                    .with_guessed_format()
                    .ok()
                    .and_then(|r| r.into_dimensions().ok())
                    .unwrap_or((SHEET_LONG_EDGE, SHEET_LONG_EDGE * 2 / 3));
                150 + u64::from(w) * u64::from(h) / (28 * 28)
            })
            .sum();
        let full = 300 + tokens / 30;
        let factor = match compute {
            Compute::Full => 1,
            Compute::Balanced => 2,
            Compute::Gentle => 4,
        };
        Self::from_env(Duration::from_secs(full * factor))
    }
}

/// Count the `"n"` keys (one per Photo answered) in `json[from..]`; returns how many and
/// where the next scan starts, so a key split across two pieces is counted once.
/// Escaped quotes inside strings (`\"n\"`) never match.
pub fn count_photo_keys(json: &[u8], from: usize) -> (usize, usize) {
    const KEY: &[u8] = b"\"n\"";
    if json.len() < KEY.len() {
        return (0, from);
    }
    let last = json.len() - KEY.len();
    let found = (from..=last)
        .filter(|&i| &json[i..i + KEY.len()] == KEY)
        .count();
    (found, (last + 1).max(from))
}

impl Provider for Ollama {
    fn cull(&self, sheet: &ContactSheet) -> Result<CullResult> {
        self.cull_streaming(sheet, &|_| {})
    }

    /// Qwen 3.8 27B read a 67-Photo sheet at about 12 s a Photo at Gentle on Greg's Mac
    /// (2026-10-20, 45 prompt tokens/s); faster levels scale by their share of the Mac.
    fn read_pace(&self) -> Option<f64> {
        Some(match self.compute {
            Compute::Full => 4.0,
            Compute::Balanced => 7.0,
            Compute::Gentle => 12.0,
        })
    }

    fn cull_streaming(&self, sheet: &ContactSheet, seen: &dyn Fn(Streamed)) -> Result<CullResult> {
        // A cull holds the model for many minutes: background Queues in this and other
        // Argus processes start nothing new meanwhile, so they don't share the model.
        let _foreground = crate::compute::Foreground::begin();
        let stall = Stall::for_cull(sheet, self.compute);
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(12 * 3600)) // slow is fine; Gentle can take hours
            // Ends a reading thread that `stream_within` gave up on.
            .timeout_read(stall.first.max(stall.gap) + Duration::from_secs(60))
            .build();
        // Load the model at full speed first: pausing a loading model makes the load
        // crawl, and the throttle is only meant for the cull itself. Loading waits behind
        // whatever else Ollama is running, so it gets the same limit as the first token.
        let loader = ureq::AgentBuilder::new().timeout(stall.first).build();
        self.post(&loader, self.load_body()).map_err(|e| {
            if e.0.contains("timed out") || e.0.contains("Timeout") {
                Error(format!(
                    "Ollama didn't load {} within {}: another app may be using it; try again",
                    self.model,
                    minutes(stall.first)
                ))
            } else {
                e
            }
        })?;
        // Only throttle an Ollama on this Mac; a remote one isn't ours to pause.
        let local = ["127.0.0.1", "localhost", "[::1]", "0.0.0.0"]
            .iter()
            .any(|h| self.base_url.contains(h));
        let _throttle = local.then(|| Throttle::start(self.compute, OllamaProcesses));
        let text = self.stream_within(&agent, self.request_body(sheet), stall, seen)?;
        parse_json_reply(&text)
    }

    fn local_model(&self) -> bool {
        true
    }
}
