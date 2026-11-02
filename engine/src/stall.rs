//! Per-Photo work that never stalls the caller silently (Greg, 2026-11-02: Apply look sat at
//! "90 of 101" with nothing saying why). [`par_within`] works on Photos in parallel on a
//! thread of its own and stops waiting once none has finished for a while: the Photos not
//! done come back as the caller's failure value, and each is named in the Debug log with how
//! long it has run, while its work runs on unseen.

use crate::library::{Library, PhotoId};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A Photo slower than this is named in the Debug log, so a stuck one can be traced.
const SLOW: Duration = Duration::from_secs(10);

/// One Photo's work: its id, its file name (for the log) and what the work needs.
pub struct Item<T> {
    pub photo_id: PhotoId,
    pub file: String,
    pub data: T,
}

/// `work` on every item, in order, each with a Library handle of its worker's own (opened
/// from `root`). If no item finishes for `stall`, the call returns: items not done come back
/// as `gave_up(index, why)`. `what` names the work in the logs ("Shoot look", "Auto").
pub fn par_within<T, R, F, G>(
    root: &Path,
    what: &'static str,
    items: Vec<Item<T>>,
    stall: Duration,
    work: F,
    gave_up: G,
) -> Vec<R>
where
    T: Send + 'static,
    R: Send + 'static,
    F: Fn(&Library, PhotoId, T) -> R + Send + Sync + 'static,
    G: Fn(usize, String) -> R,
{
    let n = items.len();
    let order: Vec<(PhotoId, String)> =
        items.iter().map(|i| (i.photo_id, i.file.clone())).collect();
    let started: Arc<Mutex<HashMap<PhotoId, Instant>>> = Default::default();
    let (tx, rx) = std::sync::mpsc::channel::<(usize, R)>();
    let root = root.to_path_buf();
    let running = started.clone();
    let work = Arc::new(work);
    std::thread::spawn(move || {
        items.into_par_iter().enumerate().for_each_init(
            || Library::open(&root),
            |lib, (i, it)| {
                let t = Instant::now();
                running.lock().unwrap().insert(it.photo_id, t);
                let r = match lib {
                    Ok(l) => Some(work(l, it.photo_id, it.data)),
                    Err(_) => None,
                };
                running.lock().unwrap().remove(&it.photo_id);
                if t.elapsed() > SLOW {
                    crate::debug_log::event(
                        "slow_frame",
                        serde_json::json!({ "what": what, "photo_id": it.photo_id, "file": it.file, "ms": t.elapsed().as_millis() as u64 }),
                    );
                }
                if let Some(r) = r {
                    let _ = tx.send((i, r));
                }
            },
        );
    });
    let mut got: Vec<Option<R>> = (0..n).map(|_| None).collect();
    let mut left = n;
    let mut why = format!(
        "took over {:.0} s, so it was left as it is",
        stall.as_secs_f64()
    );
    while left > 0 {
        match rx.recv_timeout(stall) {
            Ok((i, r)) => {
                got[i] = Some(r);
                left -= 1;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let running = started.lock().unwrap();
                let stuck: Vec<_> = order
                    .iter()
                    .filter(|(id, _)| running.contains_key(id))
                    .map(|(id, file)| serde_json::json!({ "photo_id": id, "file": file, "ms": running[id].elapsed().as_millis() as u64 }))
                    .collect();
                crate::debug_log::event(
                    "frames_stalled",
                    serde_json::json!({ "what": what, "left": left, "of": n, "running": stuck }),
                );
                crate::logs::note(&format!(
                    "{what}: no Photo finished in {} s, {left} of {n} left undone",
                    stall.as_secs()
                ));
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                why = "couldn't open the Library".into();
                break;
            }
        }
    }
    got.into_iter()
        .enumerate()
        .map(|(i, r)| r.unwrap_or_else(|| gave_up(i, why.clone())))
        .collect()
}
