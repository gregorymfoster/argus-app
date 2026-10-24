//! The Claude API Provider: Claude over the Messages API on the person's Anthropic API key,
//! read from ANTHROPIC_API_KEY or the macOS Keychain, where [`store_api_key`] keeps it.

use crate::cull::{
    ask, schema, system, ContactSheet, CullResult, Provider, MAX_IMAGES_PER_REQUEST,
};
use crate::{Error, Result};
use serde_json::{json, Value};
use std::time::Duration;

/// Claude over the Messages API (raw HTTP; there is no official Rust SDK).
pub struct Claude {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
}

const MODEL: &str = "claude-opus-5-5";

impl Claude {
    /// Key from ANTHROPIC_API_KEY, else the macOS Keychain item `argus-anthropic`.
    /// ANTHROPIC_BASE_URL overrides the endpoint (the prototype's live test uses a mock).
    pub fn from_env() -> Result<Self> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
            .or_else(keychain_key)
            .ok_or_else(|| {
                Error(
                    "no Claude API key: set ANTHROPIC_API_KEY, or store one with \
                     `security add-generic-password -s argus-anthropic -a argus -w`"
                        .into(),
                )
            })?;
        Ok(Self {
            api_key: key,
            model: MODEL.into(),
            base_url: std::env::var("ANTHROPIC_BASE_URL")
                .unwrap_or_else(|_| "https://api.anthropic.com".into()),
        })
    }

    /// The Messages API request body for a Contact sheet. Public for tests.
    pub fn request_body(&self, sheet: &ContactSheet) -> Result<Value> {
        if sheet.photos.len() > MAX_IMAGES_PER_REQUEST {
            return Err(Error(format!(
                "Shoot has {} photos; culling more than {MAX_IMAGES_PER_REQUEST} at once isn't supported yet",
                sheet.photos.len()
            )));
        }
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut content = vec![json!({
            "type": "text",
            "text": format!("{} Each image is preceded by its number.", sheet.intro())
        })];
        for p in &sheet.photos {
            content.push(json!({
                "type": "text",
                "text": p.caption()
            }));
            content.push(json!({
                "type": "image",
                "source": { "type": "base64", "media_type": "image/jpeg", "data": b64.encode(&p.jpeg) }
            }));
        }
        content.push(json!({
            "type": "text",
            "text": format!("{} Return every photo number exactly once.", ask())
        }));
        let schema = schema();
        Ok(json!({
            "model": self.model,
            "max_tokens": 16000,
            "system": system(),
            "output_config": { "effort": "high", "format": { "type": "json_schema", "schema": schema } },
            "fallbacks": "default",
            "messages": [{ "role": "user", "content": content }]
        }))
    }
}

impl Provider for Claude {
    fn cull(&self, sheet: &ContactSheet) -> Result<CullResult> {
        let body = self.request_body(sheet)?;
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(600))
            .build();
        let resp = agent
            .post(&format!("{}/v1/messages", self.base_url))
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", "2023-06-01")
            .set("anthropic-beta", "server-side-fallback-2026-07-01")
            .set("content-type", "application/json")
            .send_json(body);
        let resp: Value = match resp {
            Ok(r) => r
                .into_json()
                .map_err(|e| Error(format!("Claude response: {e}")))?,
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                return Err(Error(format!("Claude API {code}: {text}")));
            }
            Err(e) => return Err(Error(format!("Claude API: {e}"))),
        };
        parse_response(&resp)
    }
}

/// Pull the CullResult out of a Messages API response, checking stop_reason first.
pub fn parse_response(resp: &Value) -> Result<CullResult> {
    match resp["stop_reason"].as_str() {
        Some("end_turn") => {}
        Some("refusal") => {
            return Err(Error(format!(
                "Claude declined to cull this shoot ({})",
                resp["stop_details"]["category"]
                    .as_str()
                    .unwrap_or("no category")
            )))
        }
        Some("max_tokens") => {
            return Err(Error(
                "Claude ran out of output tokens before finishing".into(),
            ))
        }
        other => return Err(Error(format!("unexpected stop_reason {other:?}"))),
    }
    let text = resp["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect::<String>();
    serde_json::from_str(&text).map_err(|e| Error(format!("Claude returned unexpected JSON: {e}")))
}

/// Check the shape of an Anthropic API key before it is stored: `sk-ant-` and URL-safe
/// characters only, so it can go to `security -i` on stdin without quoting.
pub fn check_api_key(key: &str) -> Result<&str> {
    let key = key.trim();
    let safe = key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if key.starts_with("sk-ant-") && key.len() > 20 && safe {
        Ok(key)
    } else {
        Err(Error(
            "that doesn't look like an Anthropic API key (they start with sk-ant-)".into(),
        ))
    }
}

/// Keep the Claude API key in the macOS Keychain item `argus-anthropic`, where
/// [`Claude::from_env`] finds it. The key goes over stdin, never in a process's arguments.
pub fn store_api_key(key: &str) -> Result<()> {
    use std::io::Write;
    let key = check_api_key(key)?;
    if !cfg!(target_os = "macos") {
        return Err(Error(
            "the Keychain is macOS only: set ANTHROPIC_API_KEY instead".into(),
        ));
    }
    let mut child = std::process::Command::new("security")
        .arg("-i")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| Error(format!("couldn't run security: {e}")))?;
    let line = format!("add-generic-password -U -s argus-anthropic -a argus -w {key}\n");
    child
        .stdin
        .take()
        .ok_or_else(|| Error("couldn't talk to security".into()))?
        .write_all(line.as_bytes())
        .map_err(|e| Error(format!("couldn't talk to security: {e}")))?;
    let out = child
        .wait_with_output()
        .map_err(|e| Error(format!("security failed: {e}")))?;
    if out.status.success() && keychain_key().as_deref() == Some(key) {
        Ok(())
    } else {
        Err(Error(format!(
            "couldn't save the key in the Keychain: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

fn keychain_key() -> Option<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", "argus-anthropic", "-w"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|k| !k.is_empty())
}
