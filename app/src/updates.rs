//! Updates like Cursor and Claude do them (Greg, 2026-10-18): a release downloads and
//! installs quietly in the background, then the UI shows a small "Restart to update"
//! button; one click restarts into the new version. "Check for Updates…" (app menu, ⌘K,
//! Settings › Updates) runs the same check by hand.
//!
//! The installed bundle only takes over on the next launch, so nothing interrupts the
//! session. The UI reads `update_status` and listens for the `update` event.

use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

/// How often a running app looks for a release.
const EVERY: Duration = Duration::from_secs(6 * 3600);

#[derive(Clone, Serialize, Default)]
pub struct Status {
    /// `off` (a dev or local build with no update endpoint), `idle`, `checking`,
    /// `downloading`, `ready` (restart to finish), `current` (up to date) or `error`.
    pub state: &'static str,
    /// The running version.
    pub current: String,
    /// The release found, while downloading or ready.
    pub version: Option<String>,
    /// Its release notes.
    pub notes: Option<String>,
    /// Bytes so far and in all, while downloading.
    pub done: u64,
    pub total: Option<u64>,
    /// When the last check finished, in Unix seconds.
    pub checked_at: Option<u64>,
    pub error: Option<String>,
}

pub struct Updates(Mutex<Status>);

impl Updates {
    pub fn new(current: String, enabled: bool) -> Self {
        Self(Mutex::new(Status {
            state: if enabled { "idle" } else { "off" },
            current,
            ..Default::default()
        }))
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn set(app: &AppHandle, f: impl FnOnce(&mut Status)) {
    let s = {
        let updates = app.state::<Updates>();
        let mut s = updates.0.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    let _ = app.emit("update", s);
}

/// Look for a release and, if there is one, download and install it for the next launch.
/// Does nothing while a check or download runs, once one is ready, or in a build with no
/// update endpoint.
pub async fn check(app: AppHandle) {
    {
        let updates = app.state::<Updates>();
        let mut s = updates.0.lock().unwrap();
        if matches!(s.state, "off" | "checking" | "downloading" | "ready") {
            return;
        }
        s.state = "checking";
        s.error = None;
    }
    set(&app, |_| {});
    if let Err(e) = run(&app).await {
        eprintln!("update check failed: {e}");
        set(&app, |s| {
            s.state = "error";
            s.error = Some(e.to_string());
            s.checked_at = Some(now());
        });
    }
}

async fn run(app: &AppHandle) -> tauri_plugin_updater::Result<()> {
    let Some(update) = app.updater()?.check().await? else {
        set(app, |s| {
            s.state = "current";
            s.checked_at = Some(now());
        });
        return Ok(());
    };
    eprintln!("installing Argus {} for the next launch", update.version);
    set(app, |s| {
        s.state = "downloading";
        s.version = Some(update.version.clone());
        s.notes = update.body.clone();
        s.done = 0;
        s.total = None;
        s.checked_at = Some(now());
    });
    let mut done = 0u64;
    let mut last = 0u64;
    update
        .download_and_install(
            |chunk, total| {
                done += chunk as u64;
                // A progress event per ~1 MB is plenty for a bar.
                if done - last > 1 << 20 {
                    last = done;
                    set(app, |s| {
                        s.done = done;
                        s.total = total;
                    });
                }
            },
            || {},
        )
        .await?;
    set(app, |s| s.state = "ready");
    Ok(())
}

/// The first check soon after launch, then one every few hours.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tauri::async_runtime::spawn_blocking(|| std::thread::sleep(Duration::from_secs(5)))
            .await
            .ok();
        loop {
            check(app.clone()).await;
            tauri::async_runtime::spawn_blocking(|| std::thread::sleep(EVERY))
                .await
                .ok();
        }
    });
}

#[tauri::command]
pub fn update_status(updates: tauri::State<'_, Updates>) -> Status {
    updates.0.lock().unwrap().clone()
}

/// "Check for Updates…": returns at once; the outcome arrives as `update` events.
#[tauri::command]
pub fn check_for_updates(app: AppHandle) -> Status {
    let s = app.state::<Updates>().0.lock().unwrap().clone();
    tauri::async_runtime::spawn(check(app));
    s
}

/// "Restart to update": the new version is already installed, so a restart runs it.
#[tauri::command]
pub fn restart_to_update(app: AppHandle) {
    app.restart();
}
