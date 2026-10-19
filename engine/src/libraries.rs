//! Which Library the app has open, and the person's other Libraries: create one where
//! they choose, open or switch to another, move the open one, forget a recent one.
//!
//! The choice lives in a small JSON config outside every Library (the app's config
//! folder), so a Library folder holds only itself and stays portable (ADR 0011). The
//! transports (the desktop app, `argus serve`) hold one [`Libraries`] and send every
//! command through [`Libraries::call`]: the commands below are handled here, the rest go
//! to [`crate::commands::dispatch`] on the open Library.
//!
//! | command          | args     | returns                                         |
//! |------------------|----------|-------------------------------------------------|
//! | `library`        | {}       | {current, exists, recent: [{path, exists}], suggested} |
//! | `create_library` | {path}   | `library`, now on the new Library               |
//! | `open_library`   | {path}   | `library`, now on that Library                  |
//! | `move_library`   | {to}     | {moved: [`MoveReport`], library}                |
//! | `forget_library` | {path}   | `library`                                       |
//!
//! `create_library` in a folder that already has other files makes an `Argus-Library`
//! folder inside it, the way Lightroom makes a catalog folder; `move_library` into a
//! non-empty folder does the same with the Library's own folder name.

use crate::commands::dispatch;
use crate::library::{move_library, Library, MoveReport};
use crate::tools::Tools;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// A new Library's folder name when the chosen folder isn't empty.
pub const NEW_LIBRARY_NAME: &str = "Argus-Library";
const RECENT: usize = 8;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct State {
    current: Option<PathBuf>,
    #[serde(default)]
    recent: Vec<PathBuf>,
}

pub struct Libraries {
    config: Option<PathBuf>,
    suggested: PathBuf,
    /// Held for reading by every command, for writing while the Library moves.
    state: RwLock<State>,
}

/// The default Library folder: `~/Pictures/Argus/library`.
pub fn default_library() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join("Pictures/Argus/library")
}

impl Libraries {
    /// Load the choice from `config` (if any). `pinned` (`ARGUS_LIBRARY`) opens that
    /// Library, creating it if needed. Otherwise the remembered Library opens; with none
    /// remembered, an existing Library at `suggested` is adopted, else there is no
    /// current Library and the UI asks where to make one.
    pub fn load(config: Option<PathBuf>, pinned: Option<PathBuf>, suggested: PathBuf) -> Self {
        let mut state: State = config
            .as_deref()
            .and_then(|c| std::fs::read(c).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if let Some(p) = pinned {
            if Library::open(&p).is_ok() {
                state.current = Some(p.canonicalize().unwrap_or(p));
            }
        } else if state.current.is_none() && Library::is_library(&suggested) {
            state.current = suggested.canonicalize().ok();
        }
        if let Some(c) = state.current.clone() {
            remember(&mut state, &c);
        }
        let libs = Self {
            config,
            suggested,
            state: RwLock::new(state),
        };
        libs.save(&libs.state.read().unwrap());
        libs
    }

    /// The open Library's folder, if there is one.
    pub fn current(&self) -> Option<PathBuf> {
        self.state.read().unwrap().current.clone()
    }

    fn save(&self, s: &State) {
        if let Some(c) = &self.config {
            if let Some(dir) = c.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(c, serde_json::to_vec_pretty(s).expect("State serializes"));
        }
    }

    fn info(&self, s: &State) -> Value {
        json!({
            "current": s.current,
            "exists": s.current.as_deref().is_some_and(Library::is_library),
            "recent": s.recent.iter()
                .map(|p| json!({ "path": p, "exists": Library::is_library(p) }))
                .collect::<Vec<_>>(),
            "suggested": self.suggested,
        })
    }

    /// Run one command: a Library command here, anything else on the open Library.
    pub fn call(&self, command: &str, args: Value) -> Result<Value> {
        let path = |key: &str| -> Result<PathBuf> {
            args.get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(expand_home)
                .ok_or_else(|| Error(format!("{command}: `{key}` (a folder path) is required")))
        };
        match command {
            "library" => Ok(self.info(&self.state.read().unwrap())),
            "create_library" => {
                let mut dir = path("path")?;
                if !Library::is_library(&dir) && !is_empty_or_missing(&dir) {
                    dir = dir.join(NEW_LIBRARY_NAME);
                    if !Library::is_library(&dir) && !is_empty_or_missing(&dir) {
                        return Err(Error(format!(
                            "{} already exists and isn't a Library; pick another folder",
                            dir.display()
                        )));
                    }
                }
                let lib = Library::open(&dir)?;
                self.switch(lib.root())
            }
            "open_library" => {
                let dir = path("path")?;
                if !Library::is_library(&dir) {
                    return Err(Error(format!(
                        "{} isn't an Argus Library (no library.db in it)",
                        dir.display()
                    )));
                }
                let root = Library::open(&dir)?.root().to_path_buf();
                self.switch(&root)
            }
            "move_library" => {
                let mut to = path("to")?;
                let mut s = self.state.write().unwrap();
                let from = s
                    .current
                    .clone()
                    .ok_or_else(|| Error("no Library is open".into()))?;
                if to.is_dir() && !is_empty_or_missing(&to) {
                    to = to.join(from.file_name().unwrap_or_default());
                }
                let moved: MoveReport = move_library(&from, &to)?;
                s.recent.retain(|p| p != &from);
                s.current = Some(moved.to.clone());
                remember(&mut s, &moved.to);
                self.save(&s);
                Ok(json!({ "moved": moved, "library": self.info(&s) }))
            }
            "forget_library" => {
                let dir = path("path")?;
                let mut s = self.state.write().unwrap();
                s.recent.retain(|p| p != &dir);
                self.save(&s);
                Ok(self.info(&s))
            }
            _ => {
                let s = self.state.read().unwrap();
                let root = s
                    .current
                    .as_deref()
                    .ok_or_else(|| Error("no Library is open; create or open one first".into()))?;
                if !Library::is_library(root) {
                    return Err(Error(format!(
                        "the Library at {} can't be found; is its disk connected?",
                        root.display()
                    )));
                }
                dispatch(&Tools::new(root), command, args)
            }
        }
    }

    fn switch(&self, root: &Path) -> Result<Value> {
        let mut s = self.state.write().unwrap();
        s.current = Some(root.to_path_buf());
        remember(&mut s, root);
        self.save(&s);
        Ok(self.info(&s))
    }
}

fn remember(s: &mut State, p: &Path) {
    s.recent.retain(|r| r != p);
    s.recent.insert(0, p.to_path_buf());
    s.recent.truncate(RECENT);
}

fn is_empty_or_missing(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        // macOS drops .DS_Store into folders the person merely opened in Finder.
        Ok(mut d) => d.all(|e| e.is_ok_and(|e| e.file_name() == ".DS_Store")),
        Err(_) => !dir.exists(),
    }
}

/// `~/x` as the person typed it, in the home folder.
fn expand_home(s: &str) -> PathBuf {
    match s.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(rest),
        None => PathBuf::from(s),
    }
}
