//! The Claude Code Provider: `claude -p` on this Mac, so a request runs on the person's
//! Claude plan instead of an API key (ADR 0006).

use super::parse_json_reply;
use crate::cull::{schema, system, ContactSheet, CullResult, Provider};
use crate::{Error, Result};
use serde_json::Value;
use std::time::Instant;

/// Claude Code on this Mac (`claude -p`), so a cull runs on the person's Claude plan
/// instead of an API key. See ADR 0006. The Contact sheet goes to a temp folder that
/// Claude Code reads with its Read tool.
pub struct ClaudeCode {
    pub bin: String,
    pub model: String,
}

impl ClaudeCode {
    pub fn from_env() -> Self {
        Self {
            bin: std::env::var("ARGUS_CLAUDE_BIN").unwrap_or_else(|_| "claude".into()),
            model: std::env::var("ARGUS_CLAUDE_MODEL").unwrap_or_else(|_| "opus".into()),
        }
    }
}

impl Provider for ClaudeCode {
    fn cull(&self, sheet: &ContactSheet) -> Result<CullResult> {
        let dir = std::env::temp_dir().join(format!(
            "argus-sheet-{}-{}",
            sheet.shoot,
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).map_err(|e| Error(format!("sheet dir: {e}")))?;
        let mut list = String::new();
        for p in &sheet.photos {
            let path = dir.join(format!("{:03}.jpg", p.n));
            std::fs::write(&path, &p.jpeg).map_err(|e| Error(format!("sheet: {e}")))?;
            list += &format!("{}: {}\n", p.caption(), path.display());
        }
        let prompt = format!(
            "{}\n\n{} Read every image file below \
             with the Read tool before deciding anything.\n\n{list}\nReply with only a JSON object \
             (no prose, no code fence) matching this JSON Schema, with every photo number exactly once:\n{}",
            system(),
            sheet.intro(),
            schema()
        );
        let t = Instant::now();
        let out = std::process::Command::new(&self.bin)
            .args([
                "-p",
                &prompt,
                "--output-format",
                "json",
                "--allowedTools",
                "Read",
                "--model",
                &self.model,
            ])
            .current_dir(&dir)
            .output();
        let _ = std::fs::remove_dir_all(&dir);
        let out = out.map_err(|e| {
            Error(format!(
                "can't run `{}` (is Claude Code installed?): {e}",
                self.bin
            ))
        })?;
        crate::agent::log_claude(&self.model, &out, t.elapsed());
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
        parse_json_reply(v["result"].as_str().unwrap_or_default())
    }
}
