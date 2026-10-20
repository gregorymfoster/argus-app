//! The Argus desktop app: a Tauri 2 window over the React UI in `prototype/`.
//!
//! It is a thin transport, like `argus serve`: one `call` command runs any engine
//! command (see `argus_engine::commands`), and the `argus://` protocol serves the
//! Library's preview JPEGs. The UI picks this transport when it finds Tauri.
//!
//! Which Library is open is the person's choice, remembered in `libraries.json` in the
//! app's config folder (`argus_engine::libraries`, ADR 0011). `$ARGUS_LIBRARY` pins one
//! (tests, dev), and `$ARGUS_CONFIG` moves the config file.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod updates;

use argus_engine::commands::library_file;
use argus_engine::jobs::Worker;
use argus_engine::libraries::{default_library, Libraries};
use argus_engine::library::Library;
use argus_engine::tools::Tools;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::http::{header, Response, StatusCode};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter, Manager};

/// The standard Mac menus (⌘Q, ⌘W, Edit, Window) plus Argus › Check for Updates… and
/// Settings… on ⌘,, which tells the UI to open its Settings sheet, and View › Zoom In / Zoom Out / Actual Size
/// (⌘+ ⌘− ⌘0), which the UI applies to the webview and remembers (prototype/src/ui/zoom.ts).
fn app_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::default(app)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
    let updates = MenuItem::with_id(
        app,
        "check-updates",
        "Check for Updates…",
        true,
        None::<&str>,
    )?;
    // On macOS the first submenu is the app menu: About, Check for Updates…, then Settings…
    match menu.items()?.first().and_then(|i| i.as_submenu().cloned()) {
        Some(first) if cfg!(target_os = "macos") => {
            first.insert(&updates, 1)?;
            first.insert(&settings, 3)?;
        }
        Some(first) => {
            first.append(&updates)?;
            first.append(&settings)?;
        }
        None => {
            menu.append(&updates)?;
            menu.append(&settings)?;
        }
    }
    let zoom = [
        MenuItem::with_id(app, "zoom:in", "Zoom In", true, Some("CmdOrCtrl+="))?,
        MenuItem::with_id(app, "zoom:out", "Zoom Out", true, Some("CmdOrCtrl+-"))?,
        MenuItem::with_id(app, "zoom:reset", "Actual Size", true, Some("CmdOrCtrl+0"))?,
    ];
    let view = menu
        .items()?
        .into_iter()
        .filter_map(|i| i.as_submenu().cloned())
        .find(|m| m.text().is_ok_and(|t| t == "View"));
    let view = match view {
        Some(v) => {
            v.prepend(&PredefinedMenuItem::separator(app)?)?;
            v
        }
        None => {
            let v = Submenu::new(app, "View", true)?;
            menu.append(&v)?;
            v
        }
    };
    for (i, item) in zoom.iter().enumerate() {
        view.insert(item, i)?;
    }
    // Edit › Undo, Redo and Select All go to the UI rather than the webview's first
    // responder, which only knows about text fields: the UI undoes Argus's own changes
    // (flags, edits, removals) and selects all Photos, and keeps the native behaviour
    // when a text field has focus (prototype/src/ui/editMenu.ts).
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &MenuItem::with_id(app, "edit:undo", "Undo", true, Some("CmdOrCtrl+Z"))?,
            &MenuItem::with_id(app, "edit:redo", "Redo", true, Some("CmdOrCtrl+Shift+Z"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &MenuItem::with_id(app, "edit:select-all", "Select All", true, Some("CmdOrCtrl+A"))?,
        ],
    )?;
    let items = menu.items()?;
    match items
        .iter()
        .position(|i| i.as_submenu().is_some_and(|m| m.text().is_ok_and(|t| t == "Edit")))
    {
        Some(at) => {
            menu.remove(&items[at])?;
            menu.insert(&edit, at)?;
        }
        None => menu.insert(&edit, 1)?,
    }
    Ok(menu)
}

struct Queue(#[allow(dead_code)] Option<Worker>);

/// Where the app remembers the open Library and recent ones.
fn config_file() -> PathBuf {
    if let Some(p) = std::env::var_os("ARGUS_CONFIG") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let dir = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Argus")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("argus")
    };
    dir.join("libraries.json")
}

/// Run one engine command (see `argus_engine::libraries` and `argus_engine::commands`).
/// The tool layer opens the Library per call (cheap; SQLite WAL), so a long import or cull
/// never blocks flagging, and it runs off the UI thread.
#[tauri::command]
async fn call(
    libs: tauri::State<'_, Arc<Libraries>>,
    command: String,
    args: Option<Value>,
) -> Result<Value, String> {
    let libs = Arc::clone(&libs);
    tauri::async_runtime::spawn_blocking(move || {
        libs.call(&command, args.unwrap_or(Value::Null))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn main() {
    // Launched from Finder, stderr goes nowhere: keep it in ~/Library/Logs/Argus so a stuck
    // or failed run can be traced. `just app` keeps it in the terminal.
    if cfg!(target_os = "macos") && !argus_engine::logs::stderr_is_terminal() {
        if let Some(dir) = argus_engine::logs::default_dir() {
            let _ = argus_engine::logs::to_file(&dir);
        }
    }
    let libs = Arc::new(Libraries::load(
        Some(config_file()),
        std::env::var_os("ARGUS_LIBRARY").map(PathBuf::from),
        default_library(),
    ));
    let backup_root = libs.current().filter(|r| Library::is_library(r));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .menu(app_menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "settings" => {
                let _ = app.emit("open-settings", ());
            }
            "check-updates" => {
                // The UI says what the check found; Settings › Updates shows the details.
                let _ = app.emit("check-updates", ());
                tauri::async_runtime::spawn(updates::check(app.clone()));
            }
            id => {
                if let Some(step) = id.strip_prefix("zoom:") {
                    let _ = app.emit("zoom", step);
                } else if let Some(action) = id.strip_prefix("edit:") {
                    let _ = app.emit("edit-menu", action);
                }
            }
        })
        .setup(move |app| {
            // The release's version (set in its Tauri config), not the crate's 0.0.1.
            if cfg!(target_os = "macos") || std::env::var_os("ARGUS_LOG_DIR").is_some() {
                if let Some(dir) = argus_engine::logs::default_dir() {
                    argus_engine::logs::start_debug(&dir, "app");
                }
            }
            argus_engine::logs::note(&format!("Argus {} started", app.package_info().version));
            // The daily Library backup (ADR 0009), off the UI thread.
            if let Some(lib) = backup_root.clone() {
                std::thread::spawn(move || {
                    match Library::open(&lib).and_then(|l| l.backup_if_due()) {
                        Ok(Some(b)) => eprintln!("backed up the Library to {}", b.display()),
                        Ok(None) => {}
                        Err(e) => eprintln!("Library backup failed: {e}"),
                    }
                });
            }
            // Release builds update themselves (ADR 0004, docs/release.md). Dev and local
            // builds have no update endpoint (CI fills it in), so they skip this.
            let has_endpoint = app
                .config()
                .plugins
                .0
                .get("updater")
                .and_then(|u| u["endpoints"].as_array())
                .is_some_and(|e| !e.is_empty());
            let enabled = !cfg!(debug_assertions) && has_endpoint;
            app.manage(updates::Updates::new(
                app.package_info().version.to_string(),
                enabled,
            ));
            if enabled {
                updates::start(app.handle().clone());
            }
            Ok(())
        })
        // The background Queue (Suggestions, "apply to the rest", culls) works on whichever
        // Library is open while the app runs. `ARGUS_QUEUE=off` leaves it idle (UI tests).
        .manage(Queue(
            (std::env::var("ARGUS_QUEUE").as_deref() != Ok("off")).then(|| {
                let libs = Arc::clone(&libs);
                Worker::start(
                    move || libs.current().filter(|r| Library::is_library(r)),
                    Tools::new,
                )
            }),
        ))
        .manage(libs)
        .register_asynchronous_uri_scheme_protocol("argus", move |ctx, request, responder| {
            let libs = Arc::clone(&ctx.app_handle().state::<Arc<Libraries>>());
            let rel = request.uri().path().to_string();
            std::thread::spawn(move || {
                let file = libs
                    .current()
                    .and_then(|root| library_file(&root, &rel))
                    .map(std::fs::read);
                let resp = match file {
                    Some(Ok(bytes)) => Response::builder()
                        .header(header::CONTENT_TYPE, "image/jpeg")
                        .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
                        .body(bytes),
                    _ => Response::builder()
                        .status(StatusCode::NOT_FOUND)
                        .body(Vec::new()),
                };
                responder.respond(resp.expect("static response"));
            });
        })
        .invoke_handler(tauri::generate_handler![
            call,
            updates::update_status,
            updates::check_for_updates,
            updates::restart_to_update
        ])
        .run(tauri::generate_context!())
        .expect("Argus failed to start");
}
