//! Argus's own previews, built after an Import. Import writes a RAW's embedded camera JPEG
//! as its preview so the grid fills at card-read speed; [`Library::render_previews`] then
//! replaces each with Argus's render, at the Compute level's share of the CPU.

use super::import::{camera_preview_rel, preview_rel, write_preview, ImportFailure, CAMERA_SUFFIX};
use super::{db_err, resolve_path, Library, PhotoId, ShootId};
use crate::compute::Compute;
use crate::{Error, Result};
use rusqlite::params;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone, Serialize)]
pub struct PreviewReport {
    pub rendered: usize,
    /// Photos whose original isn't reachable; they keep the camera preview until it is.
    pub offline: usize,
    pub failed: Vec<ImportFailure>,
    pub ms: f64,
}

/// One preview build per Library at a time: a second caller waits, then picks up whatever
/// the first left (Photos imported meanwhile).
static RUNNING: Mutex<Option<HashMap<PathBuf, Arc<Mutex<()>>>>> = Mutex::new(None);

impl Library {
    /// How many Photos (in `shoot`, or anywhere) still show the camera's preview.
    pub fn pending_previews(&self, shoot: Option<ShootId>) -> Result<usize> {
        Ok(self.camera_previews(shoot)?.len())
    }

    fn camera_previews(&self, shoot: Option<ShootId>) -> Result<Vec<(PhotoId, String, String)>> {
        let mut q = self
            .db
            .prepare(
                "SELECT id, path, hash FROM photos
                 WHERE preview LIKE '%' || ?1 AND (?2 IS NULL OR shoot_id = ?2) ORDER BY id",
            )
            .map_err(db_err)?;
        let rows = q
            .query_map(params![CAMERA_SUFFIX, shoot], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(db_err)?;
        rows.collect::<std::result::Result<_, _>>().map_err(db_err)
    }

    /// Replace camera previews (in `shoot`, or anywhere) with Argus's own render, using
    /// `compute`'s share of the CPU. `progress(done, total)` follows each Photo.
    pub fn render_previews(
        &self,
        shoot: Option<ShootId>,
        compute: Compute,
        progress: &(dyn Fn(usize, usize) + Sync),
    ) -> Result<PreviewReport> {
        let t0 = Instant::now();
        let lock = {
            let mut running = RUNNING.lock().unwrap();
            running
                .get_or_insert_with(HashMap::new)
                .entry(self.root.clone())
                .or_default()
                .clone()
        };
        let _one_at_a_time = lock.lock().unwrap();
        let settings = self.settings()?;
        let pool = compute.cpu_pool()?;
        // Two Photos in flight keep the pool busy between one's decode and the next;
        // more would only hold more 61 MP mosaics in memory.
        let in_flight = pool.current_num_threads().clamp(1, 2);
        let (mut rendered, mut offline, mut failed) = (0, 0, Vec::new());
        let mut tried = HashSet::new();
        loop {
            let todo: Vec<_> = self
                .camera_previews(shoot)?
                .into_iter()
                .filter(|(id, ..)| !tried.contains(id))
                .collect();
            if todo.is_empty() {
                break;
            }
            tried.extend(todo.iter().map(|(id, ..)| *id));
            let total = rendered + offline + failed.len() + todo.len();
            let next = AtomicUsize::new(0);
            let (send, recv) = mpsc::channel();
            std::thread::scope(|s| -> Result<()> {
                for _ in 0..in_flight.min(todo.len()) {
                    let (send, next, todo, pool) = (send.clone(), &next, &todo, &pool);
                    let (root, settings) = (&self.root, &settings);
                    s.spawn(move || {
                        while let Some((id, stored, hash)) =
                            todo.get(next.fetch_add(1, Ordering::Relaxed))
                        {
                            let path = resolve_path(root, stored);
                            let outcome = if !path.exists() {
                                Err(None)
                            } else {
                                pool.install(|| {
                                    write_preview(
                                        &path,
                                        &[],
                                        "raw",
                                        settings.preview_long_edge as usize,
                                        settings.preview_quality,
                                        &root.join(preview_rel(hash)),
                                    )
                                })
                                .map_err(|e: Error| Some(e.0))
                            };
                            let _ = send.send((*id, hash.clone(), path, outcome));
                        }
                    });
                }
                drop(send);
                // Record each one as it lands, so the grid swaps previews in as they come.
                for (id, hash, path, outcome) in recv {
                    match outcome {
                        Ok(_) => {
                            let camera = camera_preview_rel(&hash);
                            self.db
                                .execute(
                                    "UPDATE photos SET preview = ?1 WHERE id = ?2 AND preview = ?3",
                                    params![preview_rel(&hash), id, camera],
                                )
                                .map_err(db_err)?;
                            let _ = std::fs::remove_file(self.root.join(camera));
                            rendered += 1;
                        }
                        Err(None) => offline += 1,
                        Err(Some(error)) => failed.push(ImportFailure {
                            file: path.to_string_lossy().into_owned(),
                            error,
                        }),
                    }
                    progress(rendered + offline + failed.len(), total);
                }
                Ok(())
            })?;
        }
        Ok(PreviewReport {
            rendered,
            offline,
            failed,
            ms: (t0.elapsed().as_secs_f64() * 10_000.0).round() / 10.0,
        })
    }
}
