//! The Agent: a tool-use loop over the tool layer ([`crate::tools`]), so "fix the light
//! on this one", "warmer, less contrast" or "make the whole Shoot consistent" become the
//! same edit_recipe, set_look and render_photo calls the UI and MCP make.
//!
//! The Agent always starts with Shoot context: the Shoot's Contact sheet, its Looks, and
//! the Photo in focus rendered with its current Recipe. Every change in one request goes
//! into one batch, so the person undoes a whole request in one step.
//!
//! The seam is [`Chat`]: one model turn in, one JSON step out. Every Provider speaks the
//! same small protocol (`{"say": "...", "calls": [{"tool": "...", "args": {...}}]}`) rather
//! than its own native tool calling, so a local model under Ollama's structured output,
//! Claude Code on the person's plan and a test stub are interchangeable.

use crate::compute::{Compute, OllamaProcesses, Throttle};
use crate::cull::{Claude, ClaudeCode, Ollama};
use crate::library::{Author, BatchId, Library, PhotoFilter, PhotoId, ShootId};
use crate::tools::{Part, ToolSpec, Tools};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::Duration;

/// Model turns before the Agent must answer. Each turn can make several tool calls.
pub const MAX_TURNS: usize = 8;
/// Long edge of the renders the Agent looks at.
const LOOK_EDGE: u32 = 768;
/// Longest tool result (as JSON text) put back in front of the model.
const MAX_RESULT_CHARS: usize = 6000;

/// Tools the Agent may call. Importing, exporting and a full AI cull are the person's to
/// start; the cull has its own entry point (`cull_shoot`).
pub const TOOLS: &[&str] = &[
    "list_shoots",
    "list_photos",
    "get_preview",
    "get_contact_sheet",
    "set_verdict",
    "set_rating",
    "best_photos",
    "list_stacks",
    "stack_bursts",
    "read_recipe",
    "edit_recipe",
    "white_balance",
    "write_recipe",
    "render_photo",
    "list_versions",
    "list_looks",
    "set_look",
    "sync_to_look",
    "paste_settings",
    "suggest_edits",
    "match_to_photo",
    "list_suggestions",
    "queue_status",
    "analyze_photo",
    "add_mask",
    "edit_mask",
    "remove_mask",
    "view_mask",
    "view_depth",
    "add_spot",
    "edit_spot",
    "remove_spot",
];

/// One message of the conversation as the model sees it.
#[derive(Debug, Clone)]
pub struct Msg {
    pub role: Role,
    pub text: String,
    /// JPEGs that go with the text.
    pub images: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The person, or tool results handed back.
    User,
    /// The Agent's own earlier replies.
    Agent,
}

/// A Provider that can take one Agent turn: the conversation so far in, the model's raw
/// reply (a JSON step) out. `system` is the same on every turn.
pub trait Chat {
    fn next(&self, system: &str, convo: &[Msg]) -> Result<String>;
}

/// What the person asked, with where they asked it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    pub prompt: String,
    /// The Shoot on screen; defaults to the focused Photo's Shoot, else the newest.
    pub shoot_id: Option<ShootId>,
    /// The Photo open in Edit (or selected in the grid), if any.
    pub photo_id: Option<PhotoId>,
    /// Earlier turns of this conversation, oldest first, text only.
    #[serde(default)]
    pub history: Vec<Turn>,
    pub provider: Option<String>,
    pub compute: Option<Compute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub role: Role,
    pub text: String,
}

/// What the Agent did, for the chat and for undo.
#[derive(Debug, Clone, Serialize)]
pub struct Reply {
    /// What the Agent says to the person.
    pub text: String,
    /// The batch holding every change it made, None if it changed nothing.
    pub batch_id: Option<BatchId>,
    /// Photos whose Recipe or Verdict changed, and whether any Look did.
    pub recipes: Vec<PhotoId>,
    pub verdicts: Vec<PhotoId>,
    pub looks: bool,
    pub steps: Vec<Step>,
}

/// One tool call, as the chat shows it ("Rendered DSC08005 · 0.9s").
#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub tool: String,
    pub args: Value,
    pub ok: bool,
    /// The error the model saw, if the call failed.
    pub error: Option<String>,
    pub ms: u64,
}

/// The JSON each model turn must return.
#[derive(Debug, Deserialize)]
struct ModelStep {
    #[serde(default)]
    say: String,
    #[serde(default)]
    calls: Vec<Call>,
}

#[derive(Debug, Deserialize)]
struct Call {
    tool: String,
    #[serde(default)]
    args: Value,
}

/// JSON Schema for one model turn (Ollama's `format`).
pub fn step_schema() -> Value {
    json!({
        "type": "object",
        "required": ["say", "calls"],
        "properties": {
            "say": { "type": "string" },
            "calls": { "type": "array", "items": {
                "type": "object",
                "required": ["tool", "args"],
                "properties": {
                    "tool": { "type": "string", "enum": TOOLS },
                    "args": { "type": "object" }
                }
            } }
        }
    })
}

const GUIDE: &str = "You are the Agent inside Argus, a photo editor for a photographer's own shoots. \
You edit by calling tools. Originals are never touched: every change is a new Recipe Version the \
photographer sees on ordinary sliders and can undo in one step.

How to reply: every reply is ONLY a JSON object, no prose around it:
{\"say\": \"...\", \"calls\": [{\"tool\": \"<name>\", \"args\": {...}}]}
- To act, list one or more calls. You get every call's result back (JSON, and images for \
render_photo, get_preview and get_contact_sheet), then reply again.
- When you are done, reply with \"calls\": [] and put your answer to the photographer in \"say\".
- \"say\" while calls run is a few words on what you're doing. The final \"say\" is at most two \
plain sentences: what changed with the main values (\"Exposure +0.4, Shadows +30, Temperature +8\"), \
or the answer to the question. No markdown, no lists of every number.

How to edit well:
- Natural, honest results, the way a careful photographer edits in Lightroom: modest moves, no \
crushed blacks or blown highlights, skin that still looks like skin. Never reshape anyone.
- Scope: when a Photo is in focus, change only that Photo unless the photographer says the shoot, \
all, every, these or the Picks.
- Relative requests (\"warmer\", \"less contrast\", \"a bit brighter\") build on the current values: \
edit_recipe with \"relative\": true. Typical steps: exposure 0.2-0.5, temp 8-15, contrast 10-20, \
shadows or highlights 15-40. \"A bit\" is about half that; \"a lot\" about double.
- \"Fix the light\" or \"make it look good\" is a loop: look, edit, check, fix, stop.
  1. Look first: say to yourself what the photo needs (subject too dark? highlights blown? a colour \
cast on skin?) before choosing any number.
  2. Start from Auto's values (given with the Photo in focus): it is usually close on exposure and \
tone. Change only what you can see is wrong; Auto can miss white balance on skin, mood, and scenes \
meant to be dark or bright.
  3. Try it without saving: render_photo with \"recipe\" (the Photo's Recipe with your values in \
\"global\"), and compare the result with the before image and its light numbers.
  4. Fix the one biggest problem left and try again, at most three tries. Stop as soon as it looks \
right; don't keep nudging a good edit.
  5. Save the best try with edit_recipe. If the light was already good, change little or nothing and \
say so.
- Trust the light numbers that come with every render over your eye for brightness and clipping. \
This photographer's finished edits average luma mean about 48 (35 for a moody scene, up to 65 for a \
bright beach), with p99 around 90-98 and almost nothing clipped.
- Whole-shoot consistency: look at the Contact sheet, pick a reference Photo with the best light, \
then give every other Photo per-Photo Corrections (correction.exposure in stops, correction.wbK in \
Kelvin) through edit_recipe so exposure and white balance match the reference. A shared style \
(\"warm and film-like\", \"cool and moody\") is a Look: set_look on the Shoot (it goes on the Picks \
by default), and tune it with set_look again. Say which Photo you used as the reference.
- Only use setting names and photo ids you were given. If a tool returns an error, fix the call \
and try again.";

/// The system prompt: the guide plus every tool the Agent may call, with its arguments.
pub fn system_prompt() -> String {
    let mut s = String::from(GUIDE);
    s.push_str("\n\nTools:\n");
    for t in agent_specs() {
        s.push_str(&format!(
            "- {}: {}\n  args: {}\n",
            t.name,
            t.description,
            compact_schema(&t.input_schema)
        ));
    }
    s
}

fn agent_specs() -> Vec<ToolSpec> {
    Tools::specs()
        .into_iter()
        .filter(|t| TOOLS.contains(&t.name))
        .collect()
}

/// A tool's arguments in one line: `{photo_ids*: [integer], set*: object, relative: boolean}`.
fn compact_schema(schema: &Value) -> String {
    let required: Vec<&str> = schema["required"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let props = schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let fields: Vec<String> = props
        .iter()
        .map(|(k, v)| {
            let ty = match v["type"].as_str() {
                Some("array") => format!("[{}]", v["items"]["type"].as_str().unwrap_or("any")),
                Some(t) => t.to_string(),
                None => "any".into(),
            };
            let ty = match v["enum"].as_array() {
                Some(e) => e
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("|"),
                None => ty,
            };
            let star = if required.contains(&k.as_str()) {
                "*"
            } else {
                ""
            };
            format!("{k}{star}: {ty}")
        })
        .collect();
    format!("{{{}}} (* = required)", fields.join(", "))
}

/// Answer one request with the Provider `ask.provider` names (Ollama by default).
pub fn run(tools: &Tools, ask: &Ask) -> Result<Reply> {
    // The person is waiting: run flat out, and background work makes way (see Interactive).
    let _waiting = crate::compute::Interactive::begin();
    let chat = chat(
        ask.provider.as_deref(),
        Some(ask.compute.unwrap_or(Compute::Full)),
    )?;
    let started = std::time::Instant::now();
    let reply = run_with(tools, ask, chat.as_ref())?;
    log_usage(tools, ask, &reply, started.elapsed().as_secs_f64());
    Ok(reply)
}

/// Log the request as an AI take on each Photo whose Recipe it changed; the Recipe
/// itself is in the log under the same batch.
fn log_usage(tools: &Tools, ask: &Ask, reply: &Reply, seconds: f64) {
    if reply.recipes.is_empty() {
        return;
    }
    let Ok(lib) = Library::open(tools.library()) else {
        return;
    };
    let (provider, model) = crate::usage::provider_model(ask.provider.as_deref());
    for &id in &reply.recipes {
        lib.log_usage(
            id,
            "agent",
            json!({ "prompt": ask.prompt, "answer": reply.text, "batch_id": reply.batch_id,
                    "provider": provider, "model": model, "argus": env!("CARGO_PKG_VERSION"),
                    "seconds": (seconds * 10.0).round() / 10.0 }),
        );
    }
}

/// Answer one request with a given model (tests use a script).
pub fn run_with(tools: &Tools, ask: &Ask, chat: &dyn Chat) -> Result<Reply> {
    if ask.prompt.trim().is_empty() {
        return Err(Error("ask the Agent something".into()));
    }
    let system = system_prompt();
    let mut convo = context(tools, ask)?;
    for t in &ask.history {
        convo.push(Msg {
            role: t.role,
            text: t.text.clone(),
            images: vec![],
        });
    }
    convo.push(Msg {
        role: Role::User,
        text: ask.prompt.clone(),
        images: vec![],
    });

    let mut batch: Option<BatchId> = None;
    let mut steps = Vec::new();
    let mut looks = false;
    let mut said = String::new();
    let mut retried = false;
    let mut turns = 0;
    let text = loop {
        if turns == MAX_TURNS {
            break format!(
                "{} I stopped after {MAX_TURNS} rounds of tool calls; ask again to keep going.",
                said.trim()
            )
            .trim()
            .to_string();
        }
        turns += 1;
        let raw = chat.next(&system, &convo)?;
        let step = match parse_step(&raw) {
            Ok(s) => s,
            Err(e) if !retried => {
                retried = true;
                convo.push(Msg {
                    role: Role::Agent,
                    text: raw,
                    images: vec![],
                });
                convo.push(Msg {
                    role: Role::User,
                    text: format!(
                        "{e}. Reply with only the JSON object {{\"say\": ..., \"calls\": [...]}}."
                    ),
                    images: vec![],
                });
                continue;
            }
            Err(e) => return Err(e),
        };
        convo.push(Msg {
            role: Role::Agent,
            text: raw,
            images: vec![],
        });
        if step.calls.is_empty() {
            break step.say;
        }
        said = step.say;
        let mut results = String::new();
        let mut images = Vec::new();
        for (i, c) in step.calls.iter().enumerate() {
            let t0 = std::time::Instant::now();
            let out = call_tool(tools, c, &mut batch, &ask.prompt);
            if c.tool == "set_look" || c.tool == "sync_to_look" {
                looks |= out.is_ok();
            }
            let ms = t0.elapsed().as_millis() as u64;
            match out {
                Ok(parts) => {
                    let mut json = String::new();
                    for p in parts {
                        match p {
                            Part::Json(v) => json = v.to_string(),
                            Part::Jpeg(b) => images.push(b),
                        }
                    }
                    if json.len() > MAX_RESULT_CHARS {
                        let cut = (0..=MAX_RESULT_CHARS)
                            .rev()
                            .find(|&i| json.is_char_boundary(i))
                            .unwrap_or(0);
                        json = format!("{}… (cut)", &json[..cut]);
                    }
                    results += &format!("[{}] {} -> {json}\n", i + 1, c.tool);
                    steps.push(Step {
                        tool: c.tool.clone(),
                        args: c.args.clone(),
                        ok: true,
                        error: None,
                        ms,
                    });
                }
                Err(e) => {
                    results += &format!("[{}] {} failed: {}\n", i + 1, c.tool, e.0);
                    steps.push(Step {
                        tool: c.tool.clone(),
                        args: c.args.clone(),
                        ok: false,
                        error: Some(e.0),
                        ms,
                    });
                }
            }
        }
        if !images.is_empty() {
            results += &format!("{} image(s) follow, in call order.\n", images.len());
        }
        convo.push(Msg {
            role: Role::User,
            text: results,
            images,
        });
    };

    let (recipes, verdicts) = match batch {
        Some(b) => Library::open(tools.library())?.batch_photos(b)?,
        None => (vec![], vec![]),
    };
    Ok(Reply {
        text,
        batch_id: batch,
        recipes,
        verdicts,
        looks,
        steps,
    })
}

/// Run one call, opening the request's batch on its first change.
fn call_tool(
    tools: &Tools,
    c: &Call,
    batch: &mut Option<BatchId>,
    prompt: &str,
) -> Result<Vec<Part>> {
    if !TOOLS.contains(&c.tool.as_str()) {
        return Err(Error(format!(
            "{:?} is not one of your tools: {}",
            c.tool,
            TOOLS.join(", ")
        )));
    }
    let writes = Tools::specs()
        .iter()
        .any(|t| t.name == c.tool && !t.read_only);
    if writes && batch.is_none() {
        let mut lib = Library::open(tools.library())?;
        *batch = Some(lib.begin_batch(Author::Ai, &batch_label(prompt))?);
    }
    let args = if c.args.is_null() {
        json!({})
    } else {
        c.args.clone()
    };
    tools.call_in(&c.tool, args, *batch)
}

/// "Agent: warmer, less contrast", short enough for a History row.
fn batch_label(prompt: &str) -> String {
    let p = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = p.chars().take(60).collect();
    format!(
        "Agent: {short}{}",
        if short.len() < p.len() { "…" } else { "" }
    )
}

fn parse_step(text: &str) -> Result<ModelStep> {
    let (Some(a), Some(b)) = (text.find('{'), text.rfind('}')) else {
        return Err(Error(format!(
            "your reply had no JSON object: {}",
            text.chars().take(200).collect::<String>()
        )));
    };
    serde_json::from_str(&text[a..=b])
        .map_err(|e| Error(format!("your reply was not valid JSON ({e})")))
}

/// The first message: what's on screen, the Shoot's Contact sheet and the Photo in focus.
fn context(tools: &Tools, ask: &Ask) -> Result<Vec<Msg>> {
    let lib = Library::open(tools.library())?;
    let all = lib.list_photos(&PhotoFilter::default())?;
    let focus = match ask.photo_id {
        Some(id) => Some(
            all.iter()
                .find(|p| p.id == id)
                .ok_or_else(|| Error(format!("no Photo with id {id}")))?
                .clone(),
        ),
        None => None,
    };
    let shoots = lib.shoots()?;
    let shoot = ask
        .shoot_id
        .or(focus.as_ref().map(|p| p.shoot_id))
        .or(shoots.last().map(|s| s.id))
        .ok_or_else(|| Error("the Library has no Shoots yet; import a folder first".into()))?;
    let s = shoots
        .iter()
        .find(|s| s.id == shoot)
        .ok_or_else(|| Error(format!("no Shoot with id {shoot}")))?;
    let photos: Vec<_> = all.iter().filter(|p| p.shoot_id == shoot).collect();
    let picks: Vec<PhotoId> = photos
        .iter()
        .filter(|p| p.verdict == crate::library::Verdict::Pick)
        .map(|p| p.id)
        .collect();
    let looks = lib.looks(shoot)?;
    drop(lib);

    let mut text = format!(
        "Shoot \"{}\" (shoot_id {shoot}): {} Photos, {} Picks (ids {:?}).\nLooks: {}\n",
        s.name,
        photos.len(),
        picks.len(),
        picks,
        if looks.is_empty() {
            "none yet".into()
        } else {
            serde_json::to_string(&looks).unwrap_or_default()
        }
    );
    let mut images = Vec::new();
    let sheet = tools.call("get_contact_sheet", json!({ "shoot_id": shoot }))?;
    for p in sheet {
        match p {
            Part::Json(v) => {
                let legend: Vec<String> = v["photos"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|p| {
                        format!(
                            "{}=id {} {} {}",
                            p["n"],
                            p["photo_id"],
                            p["file_name"].as_str().unwrap_or(""),
                            p["verdict"].as_str().unwrap_or("")
                        )
                    })
                    .collect();
                text += &format!(
                    "Contact sheet page 1 of {} (cell number = position in capture order): {}\n",
                    v["pages"],
                    legend.join("; ")
                );
            }
            Part::Jpeg(b) => images.push(b),
        }
    }
    match &focus {
        Some(p) => {
            let recipe = tools.call("read_recipe", json!({ "photo_id": p.id }))?;
            let recipe = match recipe.first() {
                Some(Part::Json(v)) if !v.is_null() => v["recipe"].to_string(),
                _ => "never edited (all sliders 0)".into(),
            };
            text += &format!(
                "In focus: Photo {} ({}, {}x{}, {}). Its Recipe: {recipe}\nThe second image is it, rendered with that Recipe.\n",
                p.id, p.file_name, p.width, p.height, p.verdict.as_str()
            );
            for part in tools.call(
                "render_photo",
                json!({ "photo_id": p.id, "long_edge": LOOK_EDGE }),
            )? {
                match part {
                    Part::Jpeg(b) => images.push(b),
                    Part::Json(v) => {
                        text += &format!("Its light: {}\n", v["light"].as_str().unwrap_or(""))
                    }
                }
            }
            // Auto is a strong deterministic first guess at exposure and tone; the Agent
            // starts from it rather than picking numbers cold.
            let own = match tools
                .call("read_recipe", json!({ "photo_id": p.id }))?
                .first()
            {
                Some(Part::Json(v)) if !v.is_null() => {
                    crate::recipe::Recipe::from_value(&v["recipe"]).unwrap_or_default()
                }
                _ => Default::default(),
            };
            if let Ok(auto) = tools.auto_recipe(p.id, &own) {
                let g = serde_json::to_value(&auto.global).unwrap_or_default();
                let picked: serde_json::Map<String, Value> = crate::auto::AUTO_SLIDERS
                    .iter()
                    .filter_map(|k| g.get(*k).map(|v| (k.to_string(), v.clone())))
                    .collect();
                text += &format!(
                    "Auto (Lightroom-style, deterministic) would set: {}\n",
                    Value::Object(picked)
                );
            }
        }
        None => {
            text += "No single Photo is in focus; the photographer is looking at the whole Shoot.\n"
        }
    }
    Ok(vec![Msg {
        role: Role::User,
        text,
        images,
    }])
}

/// The Agent's model by Provider name, as for [`crate::cull::provider`].
pub fn chat(name: Option<&str>, compute: Option<Compute>) -> Result<Box<dyn Chat>> {
    chat_with_format(name, compute, step_schema())
}

/// A model by Provider name whose replies follow `format` (a JSON Schema). Ollama enforces
/// it as structured output; the others are told the shape in the prompt.
pub fn chat_with_format(
    name: Option<&str>,
    compute: Option<Compute>,
    format: Value,
) -> Result<Box<dyn Chat>> {
    let env = std::env::var("ARGUS_PROVIDER").ok();
    match name.or(env.as_deref()).unwrap_or("ollama") {
        "ollama" => {
            let mut o = Ollama::from_env();
            if let Some(c) = compute {
                o.compute = c;
            }
            Ok(Box::new(OllamaChat {
                o,
                format,
                think: std::env::var("ARGUS_OLLAMA_THINK").is_ok_and(|v| v == "1")
                    || crate::variant::current().is_ok_and(|v| v.think == Some(true)),
                throttle: Mutex::new(None),
            }))
        }
        "claude-code" => Ok(Box::new(ClaudeCodeChat::new(ClaudeCode::from_env())?)),
        "claude-api" | "claude" => Ok(Box::new(Claude::from_env()?)),
        other => Err(Error(format!(
            "unknown provider {other:?}; use ollama, claude-code or claude-api"
        ))),
    }
}

fn b64(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// A local model through Ollama, kept to the step schema by structured output. Loads the
/// model at full speed, then throttles it to the Compute level until the request is done.
struct OllamaChat {
    o: Ollama,
    format: Value,
    /// Let a thinking model (Qwen) reason before it answers. Off unless
    /// ARGUS_OLLAMA_THINK=1: an edit or an Agent step is short JSON, and a thinking trace
    /// is thousands of tokens that multiplied Suggestions' time per Photo.
    think: bool,
    throttle: Mutex<Option<Throttle>>,
}

impl OllamaChat {
    /// The /api/chat request body for one turn. Public to the crate for tests.
    fn body(&self, system: &str, convo: &[Msg]) -> Value {
        let mut messages = vec![json!({ "role": "system", "content": system })];
        for m in convo {
            let mut msg = json!({
                "role": if m.role == Role::User { "user" } else { "assistant" },
                "content": m.text,
            });
            if !m.images.is_empty() {
                msg["images"] = m.images.iter().map(|i| Value::String(b64(i))).collect();
            }
            messages.push(msg);
        }
        let mut options = self.o.options();
        options["num_predict"] = json!(max_output_tokens());
        json!({
            "model": self.o.model,
            "stream": false,
            "format": self.format,
            "keep_alive": "30m",
            "think": self.think,
            "options": options,
            "messages": messages
        })
    }
}

/// Most output tokens one Ollama turn may produce. A step is a few hundred tokens of JSON,
/// but Qwen sometimes loops: one Suggestions turn ran 63k tokens for 40 minutes holding the
/// model lock (2026-10-19). `num_predict` is a per-request option, so it doesn't reload
/// the model. ARGUS_OLLAMA_MAX_TOKENS overrides.
fn max_output_tokens() -> u32 {
    std::env::var("ARGUS_OLLAMA_MAX_TOKENS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8192)
}

/// One line on stderr per model request: where its time went. Ollama reports durations in ns.
pub(crate) fn log_timing(model: &str, resp: &Value, wall: Duration) {
    let s = |k: &str| resp[k].as_f64().unwrap_or(0.0) / 1e9;
    let n = |k: &str| resp[k].as_u64().unwrap_or(0);
    let gen = s("eval_duration");
    eprintln!(
        "argus ollama: {model} {:.1}s wall, load {:.1}s, prompt {} tok in {:.1}s, \
         output {} tok in {:.1}s ({:.0} tok/s), thinking {} chars",
        wall.as_secs_f64(),
        s("load_duration"),
        n("prompt_eval_count"),
        s("prompt_eval_duration"),
        n("eval_count"),
        gen,
        if gen > 0.0 {
            n("eval_count") as f64 / gen
        } else {
            0.0
        },
        resp["message"]["thinking"].as_str().map_or(0, str::len),
    );
    let prompt_s = s("prompt_eval_duration");
    crate::debug_log::event(
        "ollama",
        serde_json::json!({
            "model": model,
            "who": crate::model_lock::who(),
            "wall_s": wall.as_secs_f64(),
            "load_s": s("load_duration"),
            "prompt_tokens": n("prompt_eval_count"),
            "prompt_s": prompt_s,
            "prompt_tok_s": if prompt_s > 0.0 { n("prompt_eval_count") as f64 / prompt_s } else { 0.0 },
            "output_tokens": n("eval_count"),
            "output_s": gen,
            "output_tok_s": if gen > 0.0 { n("eval_count") as f64 / gen } else { 0.0 },
            "thinking_chars": resp["message"]["thinking"].as_str().map_or(0, str::len),
            "done_reason": resp["done_reason"],
        }),
    );
}

/// One `claude -p` run in the Debug log: what it took and cost, from its JSON output.
pub(crate) fn log_claude(model: &str, out: &std::process::Output, wall: Duration) {
    let v: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    crate::debug_log::event(
        "claude",
        serde_json::json!({
            "model": model,
            "who": crate::model_lock::who(),
            "wall_s": wall.as_secs_f64(),
            "exit": out.status.code(),
            "ok": out.status.success() && v["is_error"] != true,
            "duration_api_ms": v["duration_api_ms"],
            "turns": v["num_turns"],
            "cost_usd": v["total_cost_usd"],
            "usage": v["usage"],
            "stderr": String::from_utf8_lossy(&out.stderr).chars().take(500).collect::<String>(),
        }),
    );
}

impl Chat for OllamaChat {
    fn next(&self, system: &str, convo: &[Msg]) -> Result<String> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(3 * 3600))
            .build();
        let mut throttle = self.throttle.lock().unwrap();
        if throttle.is_none() {
            self.o.post(&agent, self.o.load_body())?;
            let local = ["127.0.0.1", "localhost", "[::1]", "0.0.0.0"]
                .iter()
                .any(|h| self.o.base_url.contains(h));
            if local {
                *throttle = Some(Throttle::start(self.o.compute, OllamaProcesses));
            }
        }
        drop(throttle);
        let start = std::time::Instant::now();
        let mut body = self.body(system, convo);
        let resp = match self.o.post(&agent, body.clone()) {
            // A model that can't think rejects even think: false on some Ollama versions.
            Err(e) if e.0.contains("does not support thinking") => {
                body.as_object_mut().unwrap().remove("think");
                self.o.post(&agent, body)?
            }
            r => r?,
        };
        log_timing(&self.o.model, &resp, start.elapsed());
        if resp["done_reason"] == "length" {
            return Err(Error(format!(
                "{} ran past {} output tokens without finishing its answer",
                self.o.model,
                max_output_tokens()
            )));
        }
        Ok(resp["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }
}

impl Chat for Claude {
    fn next(&self, system: &str, convo: &[Msg]) -> Result<String> {
        let messages: Vec<Value> = convo
            .iter()
            .map(|m| {
                let mut content = vec![json!({ "type": "text", "text": m.text })];
                for i in &m.images {
                    content.push(json!({ "type": "image", "source": { "type": "base64", "media_type": "image/jpeg", "data": b64(i) } }));
                }
                json!({ "role": if m.role == Role::User { "user" } else { "assistant" }, "content": content })
            })
            .collect();
        let body = json!({ "model": self.model, "max_tokens": 4000, "system": system, "messages": messages });
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(600))
            .build();
        let resp = agent
            .post(&format!("{}/v1/messages", self.base_url))
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .send_json(body);
        let resp: Value = match resp {
            Ok(r) => r
                .into_json()
                .map_err(|e| Error(format!("Claude response: {e}")))?,
            Err(ureq::Error::Status(code, r)) => {
                return Err(Error(format!(
                    "Claude API {code}: {}",
                    r.into_string().unwrap_or_default()
                )))
            }
            Err(e) => return Err(Error(format!("Claude API: {e}"))),
        };
        if resp["stop_reason"] == "refusal" {
            return Err(Error("Claude declined this request".into()));
        }
        Ok(resp["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect())
    }
}

/// Claude Code on the person's plan (`claude -p`, ADR 0006). The first turn starts a
/// session and later turns resume it, so only new messages are sent. Images go to a temp
/// folder that Claude Code reads with its Read tool.
struct ClaudeCodeChat {
    cc: ClaudeCode,
    dir: tempfile::TempDir,
    /// The session id, how many messages it has seen, images written, and its system prompt.
    session: Mutex<(Option<String>, usize, usize, Option<String>)>,
}

impl ClaudeCodeChat {
    fn new(cc: ClaudeCode) -> Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("argus-agent-")
            .tempdir()
            .map_err(|e| Error(format!("agent temp dir: {e}")))?;
        Ok(Self {
            cc,
            dir,
            session: Mutex::new((None, 0, 0, None)),
        })
    }
}

impl Chat for ClaudeCodeChat {
    fn next(&self, system: &str, convo: &[Msg]) -> Result<String> {
        let mut s = self.session.lock().unwrap();
        let (session, seen, images, last_system) = &mut *s;
        // A conversation that didn't grow from the last one (Suggestions' review rounds
        // reuse one Chat) starts a fresh session rather than resuming the old one.
        if convo.len() <= *seen || last_system.as_deref() != Some(system) {
            *session = None;
            *seen = 0;
            *last_system = Some(system.to_string());
        }
        let mut prompt = String::new();
        // The model's own replies are already in the session; send only the rest.
        for m in &convo[*seen..] {
            if m.role == Role::Agent && session.is_some() {
                continue;
            }
            prompt += if m.role == Role::Agent {
                "[Your earlier reply]\n"
            } else {
                ""
            };
            prompt += &m.text;
            prompt.push('\n');
            for img in &m.images {
                *images += 1;
                let path = self.dir.path().join(format!("{:03}.jpg", *images));
                std::fs::write(&path, img).map_err(|e| Error(format!("agent image: {e}")))?;
                prompt += &format!(
                    "Image: {} (look at it with the Read tool)\n",
                    path.display()
                );
            }
            prompt.push('\n');
        }
        prompt += "Reply with only the JSON object.";
        let mut cmd = std::process::Command::new(&self.cc.bin);
        cmd.args([
            "-p",
            &prompt,
            "--output-format",
            "json",
            "--allowedTools",
            "Read",
            "--model",
            &self.cc.model,
        ])
        .args(["--system-prompt", system])
        .current_dir(self.dir.path());
        if let Some(id) = session.as_deref() {
            cmd.args(["--resume", id]);
        }
        let t = std::time::Instant::now();
        let out = cmd.output().map_err(|e| {
            Error(format!(
                "can't run `{}` (is Claude Code installed?): {e}",
                self.cc.bin
            ))
        })?;
        log_claude(&self.cc.model, &out, t.elapsed());
        let stdout = String::from_utf8_lossy(&out.stdout);
        let v: Value = serde_json::from_str(&stdout).map_err(|_| {
            Error(format!(
                "Claude Code failed: {}{}",
                String::from_utf8_lossy(&out.stderr).trim(),
                stdout.trim()
            ))
        })?;
        if v["is_error"] == true {
            return Err(Error(format!(
                "Claude Code: {} (run `claude auth login` if you aren't signed in)",
                v["result"].as_str().unwrap_or("error")
            )));
        }
        *session = v["session_id"]
            .as_str()
            .map(String::from)
            .or(session.take());
        *seen = convo.len();
        Ok(v["result"].as_str().unwrap_or_default().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_parse_through_prose_and_fences() {
        let s = parse_step("Sure:\n```json\n{\"say\": \"ok\", \"calls\": [{\"tool\": \"read_recipe\", \"args\": {\"photo_id\": 1}}]}\n```").unwrap();
        assert_eq!(s.calls[0].tool, "read_recipe");
        assert!(parse_step("no json here").is_err());
    }

    #[test]
    fn system_prompt_lists_only_agent_tools_with_their_args() {
        let s = system_prompt();
        assert!(s.contains("- edit_recipe:"));
        assert!(s.contains("photo_ids*: [integer]"));
        assert!(!s.contains("- import_folder:"));
        assert!(!s.contains("- export_photos:"));
    }

    #[test]
    fn ollama_turn_sends_images_and_the_step_schema() {
        let c = OllamaChat {
            format: step_schema(),
            o: Ollama::from_env(),
            think: false,
            throttle: Mutex::new(None),
        };
        let b = c.body(
            "sys",
            &[
                Msg {
                    role: Role::User,
                    text: "hi".into(),
                    images: vec![vec![1, 2]],
                },
                Msg {
                    role: Role::Agent,
                    text: "{}".into(),
                    images: vec![],
                },
            ],
        );
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["images"][0], "AQI=");
        assert_eq!(b["messages"][2]["role"], "assistant");
        assert_eq!(b["format"]["required"], json!(["say", "calls"]));
        assert_eq!(
            b["think"], false,
            "no thinking trace before a short JSON step"
        );
        assert!(
            b["options"]["num_predict"].as_u64().is_some_and(|n| n > 0),
            "a looping model can't hold the model lock for long"
        );
        assert!(
            Ollama::from_env().load_body()["options"]
                .get("num_predict")
                .is_none(),
            "the cap is per request, not part of the model load"
        );
    }
}
