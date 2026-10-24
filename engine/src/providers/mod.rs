//! The model Providers every AI feature shares (the cull, the Agent, Suggestions and Shoot
//! names): [`Ollama`] (a local model, the default), [`ClaudeCode`] (the person's Claude plan
//! through `claude -p`) and [`Claude`] (the Claude API on their own key). [`provider`] picks
//! one by name; each one answers the cull through [`crate::cull::Provider`], and the Agent
//! drives the same clients through its own chat seam ([`crate::agent`]).

mod claude;
mod claude_code;
mod ollama;

pub use claude::{check_api_key, parse_response, store_api_key, Claude};
pub use claude_code::ClaudeCode;
pub use ollama::{
    choose_ollama_model, count_photo_keys, InstalledModel, Ollama, Stall, DEFAULT_OLLAMA_MODEL,
};

use crate::compute::Compute;
use crate::cull::{CullResult, Provider};
use crate::{Error, Result};

/// Parse a CullResult from model text, tolerating prose or a code fence around the JSON.
pub fn parse_json_reply(text: &str) -> Result<CullResult> {
    let (Some(a), Some(b)) = (text.find('{'), text.rfind('}')) else {
        return Err(Error(format!(
            "model returned no JSON: {}",
            text.chars().take(200).collect::<String>()
        )));
    };
    serde_json::from_str(&text[a..=b])
        .map_err(|e| Error(format!("model returned unexpected JSON: {e}")))
}

/// Pick a Provider by name: `ollama` (default, local), `claude-code` (your Claude plan via
/// Claude Code), `claude-api` (an Anthropic API key), or `rules` (no model: the Technical
/// check from the Facts alone, [`crate::technical::Rules`]). ARGUS_PROVIDER sets the default.
pub fn provider(name: Option<&str>, compute: Option<Compute>) -> Result<Box<dyn Provider>> {
    let env = std::env::var("ARGUS_PROVIDER").ok();
    match name.or(env.as_deref()).unwrap_or("ollama") {
        "ollama" => {
            let mut o = Ollama::from_env();
            if let Some(c) = compute {
                o.compute = c;
            }
            Ok(Box::new(o))
        }
        "claude-code" => Ok(Box::new(ClaudeCode::from_env())),
        "claude-api" | "claude" => Ok(Box::new(Claude::from_env()?)),
        "rules" => Ok(Box::new(crate::technical::Rules)),
        other => Err(Error(format!(
            "unknown provider {other:?}; use ollama, claude-code, claude-api or rules"
        ))),
    }
}
