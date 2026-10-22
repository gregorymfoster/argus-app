//! How far along the long things are (a card scan, an import), for progress bars with an
//! ETA. Work in this process registers a [`Task`] and ticks it; the `progress` command
//! reads them all. In-memory only: it describes what this process is doing right now.

use serde::Serialize;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct Entry {
    id: u64,
    kind: &'static str,
    label: String,
    /// The background job this is, for `queue_status`.
    job: Option<i64>,
    /// What it waits on instead of working ("argus cull is using the model"); no ETA then.
    waiting: Option<String>,
    total: Arc<AtomicUsize>,
    done: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    started: Instant,
}

static TASKS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// The task this thread works for ([`Task::enter`]), so code deep in a request can say
    /// what it waits on ([`waiting_here`]) without being handed the task.
    static CURRENT: Cell<Option<u64>> = const { Cell::new(None) };
}

/// A running piece of work. Tick it as items finish; it disappears when dropped.
pub struct Task {
    id: u64,
    total: Arc<AtomicUsize>,
    done: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
}

impl Task {
    pub fn start(kind: &'static str, label: impl Into<String>, total: usize) -> Task {
        Self::new(kind, label.into(), total, None)
    }

    /// The running background job `job`, so the queue can say what it is doing right now.
    pub fn for_job(job: i64, label: impl Into<String>) -> Task {
        Self::new("job", label.into(), 0, Some(job))
    }

    fn new(kind: &'static str, label: String, total: usize, job: Option<i64>) -> Task {
        let done = Arc::new(AtomicUsize::new(0));
        let total = Arc::new(AtomicUsize::new(total));
        let stop = Arc::new(AtomicBool::new(false));
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        TASKS.lock().unwrap().push(Entry {
            id,
            kind,
            label,
            job,
            waiting: None,
            total: total.clone(),
            done: done.clone(),
            stop: stop.clone(),
            started: Instant::now(),
        });
        Task {
            id,
            total,
            done,
            stop,
        }
    }

    /// The flag [`cancel`] raises: the work should stop at the next item, keeping what's done.
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }

    /// One more item done. Safe from any thread.
    pub fn tick(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }

    /// Where it stands, for work whose size firms up as it goes (an AI cull). Safe from
    /// any thread.
    pub fn set(&self, done: usize, total: usize) {
        self.total.store(total, Ordering::Relaxed);
        self.done.store(done, Ordering::Relaxed);
    }

    /// Say what it is doing now ("Model reading the Shoot").
    pub fn relabel(&self, label: impl Into<String>) {
        let label = label.into();
        edit(self.id, |e| e.label = label);
    }

    /// Say what it waits on before it can go on (None: working again).
    pub fn set_waiting(&self, what: Option<String>) {
        edit(self.id, |e| e.waiting = what);
    }

    /// Make this the task [`waiting_here`] reports to, on this thread, until the guard drops.
    pub fn enter(&self) -> Entered {
        Entered(CURRENT.with(|c| c.replace(Some(self.id))))
    }
}

fn edit(id: u64, f: impl FnOnce(&mut Entry)) {
    if let Some(e) = TASKS.lock().unwrap().iter_mut().find(|e| e.id == id) {
        f(e);
    }
}

/// While it lives, [`waiting_here`] on this thread reports to the task it was made from.
pub struct Entered(Option<u64>);

impl Drop for Entered {
    fn drop(&mut self) {
        CURRENT.with(|c| c.set(self.0));
    }
}

/// Say what the task this thread works for (if any) is doing now ("Culling").
pub fn label_here(label: &str) {
    if let Some(id) = CURRENT.with(Cell::get) {
        edit(id, |e| e.label = label.to_owned());
    }
}

/// Tell the task this thread works for (if any) what it waits on, or None once it goes on.
pub fn waiting_here(what: Option<&str>) {
    if let Some(id) = CURRENT.with(Cell::get) {
        edit(id, |e| e.waiting = what.map(str::to_owned));
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        TASKS.lock().unwrap().retain(|e| e.id != self.id);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub kind: &'static str,
    pub label: String,
    pub done: usize,
    pub total: usize,
    pub elapsed_s: f64,
    /// Seconds left at the pace so far; None until the first item is done, or while waiting.
    pub eta_s: Option<f64>,
    /// What it waits on instead of working, if it does.
    pub waiting: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<i64>,
}

/// Ask every running task of `kind` to stop after the item in hand. True if one was running.
pub fn cancel(kind: &str) -> bool {
    let tasks = TASKS.lock().unwrap();
    let mut any = false;
    for e in tasks.iter().filter(|e| e.kind == kind) {
        e.stop.store(true, Ordering::Relaxed);
        any = true;
    }
    any
}

/// What work returns when the person stopped it ([`cancel`]); the UI tells it from a failure.
pub const STOPPED: &str = "stopped";

/// True once [`cancel`] asked the task this thread works for ([`Task::enter`]) to stop, so
/// code deep in a request (a model reply streaming in) can give up between pieces.
pub fn stopped_here() -> bool {
    CURRENT.with(Cell::get).is_some_and(|id| {
        TASKS
            .lock()
            .unwrap()
            .iter()
            .any(|e| e.id == id && e.stop.load(Ordering::Relaxed))
    })
}

/// [`stopped_here`] as an error to return, for `?`.
pub fn check_stopped() -> crate::Result<()> {
    if stopped_here() {
        Err(crate::Error(STOPPED.into()))
    } else {
        Ok(())
    }
}

/// Everything running now, oldest first.
pub fn snapshot() -> Vec<Snapshot> {
    TASKS
        .lock()
        .unwrap()
        .iter()
        .map(|e| {
            let total = e.total.load(Ordering::Relaxed);
            let done = e.done.load(Ordering::Relaxed).min(total);
            let elapsed_s = e.started.elapsed().as_secs_f64();
            let eta_s = (done > 0 && e.waiting.is_none())
                .then(|| elapsed_s / done as f64 * (total - done) as f64);
            Snapshot {
                kind: e.kind,
                label: e.label.clone(),
                done,
                total,
                elapsed_s,
                eta_s,
                waiting: e.waiting.clone(),
                job_id: e.job,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_report_progress_and_vanish_when_done() {
        let t = Task::start("test", "Counting", 4);
        t.tick();
        let s = snapshot().into_iter().find(|s| s.kind == "test").unwrap();
        assert_eq!((s.done, s.total), (1, 4));
        assert!(s.eta_s.is_some());
        let stop = t.stop_flag();
        assert!(cancel("test") && stop.load(Ordering::Relaxed));
        drop(t);
        assert!(snapshot().iter().all(|s| s.kind != "test"));
        assert!(!cancel("test"));
    }

    #[test]
    fn cancel_reaches_work_on_the_task_s_thread() {
        let t = Task::start("test-stop", "Culling", 0);
        assert!(!stopped_here());
        let _in = t.enter();
        assert!(!stopped_here() && check_stopped().is_ok());
        cancel("test-stop");
        assert!(stopped_here());
        assert_eq!(check_stopped().unwrap_err().0, STOPPED);
    }

    #[test]
    fn a_waiting_task_says_on_what_and_has_no_eta() {
        let t = Task::for_job(7, "AI cull");
        t.set(1, 4);
        {
            let _in = t.enter();
            waiting_here(Some("`argus cull` is using the model"));
        }
        waiting_here(Some("not this task: the guard is gone"));
        let s = snapshot()
            .into_iter()
            .find(|s| s.job_id == Some(7))
            .unwrap();
        assert_eq!(
            s.waiting.as_deref(),
            Some("`argus cull` is using the model")
        );
        assert_eq!(s.eta_s, None);
        t.set_waiting(None);
        let s = snapshot()
            .into_iter()
            .find(|s| s.job_id == Some(7))
            .unwrap();
        assert!(s.waiting.is_none() && s.eta_s.is_some());
    }
}
