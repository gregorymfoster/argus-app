//! The background thread, one round of the queue at a time ([`Rounds::tick`]).

use super::*;

/// How often the worker looks for Backup places due to run.
pub(super) const DUE_EVERY: Duration = Duration::from_secs(5 * 60);

/// Queue the backups that are due now ([`Library::backups_due`]).
pub fn queue_due_backups(lib: &mut Library) -> Result<usize> {
    let due = lib.backups_due()?;
    for (place, verify) in &due {
        let mut args =
            json!({ "place": place, "why": if *verify { "Monthly check" } else { "Scheduled" } });
        if *verify {
            args["verify"] = json!(true);
        }
        enqueue(lib, "backup", &args, Some(PRIORITY_KEYWORDS), None)?;
    }
    Ok(due.len())
}

/// What one round of the queue did ([`Rounds::tick`]).
#[derive(Debug)]
pub enum Tick {
    /// Ran a job, at this Compute level.
    Ran { job: Box<Job>, compute: Compute },
    /// Jobs are queued but the [`gate`] starts nothing now: why.
    Wait(&'static str),
    /// Jobs are queued but each is held now (battery, heat, the idle or overnight wait).
    Held,
    /// Nothing is queued.
    Empty,
}

/// How a round picks its job.
#[derive(Debug, Clone, Copy, Default)]
pub struct TickOptions {
    /// Run whatever is queued, past the gate and its holds (`argus work --drain`, benches).
    /// Due backups aren't queued then, so a bench never copies a Library anywhere.
    pub drain: bool,
    /// Run at this Compute level instead of the queue's.
    pub compute: Option<Compute>,
}

/// The background queue, one round at a time. The app's [`Worker`] and `argus work` both
/// go round by it, so a shell run keeps the app's housekeeping: the battery override ends
/// on the charger, Run now ends when the queue empties, due backups get queued, Prepare
/// settles, and the Mac stays awake while Prepare or Run it all has work.
#[derive(Default)]
pub struct Rounds {
    awake: power::Awake,
    last_due: Option<std::time::Instant>,
}

impl Rounds {
    pub fn tick(&mut self, tools: &Tools, opts: TickOptions) -> Result<Tick> {
        let mut lib = Library::open(tools.library())?;
        let mut cfg = config(&lib).unwrap_or_default();
        if cfg.battery_override && !power::on_battery() {
            // Plugged in again: the next unplug waits for the charger as usual.
            cfg.battery_override = false;
            let _ = set_config(&lib, &cfg);
        }
        // Backup places due on their own (daily, on connect, the monthly check), looked at
        // every few minutes.
        if !opts.drain
            && self
                .last_due
                .is_none_or(|t: std::time::Instant| t.elapsed() >= DUE_EVERY)
        {
            self.last_due = Some(std::time::Instant::now());
            if let Err(e) = queue_due_backups(&mut lib) {
                crate::logs::note(&format!("argus backups: {e}"));
            }
            // A run another Argus process left part way (`argus serve` killed mid-export),
            // or an Import paused for its card that is back.
            if let Err(e) = queue_orphaned_runs(&mut lib) {
                crate::logs::note(&format!("argus resume: {e}"));
            }
        }
        // Nothing queued (the usual idle state): no power, heat or model checks.
        if !lib.has_queued_jobs().unwrap_or(true) {
            self.awake.set(false);
            if cfg.preparing {
                let _ = crate::prepare::settle(&lib);
            }
            // "Run now" lasts until the queue is empty.
            // (Saved from `cfg` as updated above, so it can't bring a cleared override back.)
            if cfg.go_ahead && lib.queue_summary().is_ok_and(|q| q.running.is_none()) {
                cfg.go_ahead = false;
                let _ = set_config(&lib, &cfg);
            }
            return Ok(Tick::Empty);
        }
        let (compute, min_priority, held, first) = if opts.drain {
            (effective_compute(&cfg), i64::MIN, &[][..], Vec::new())
        } else {
            match gate(&lib, &cfg) {
                Gate::Run {
                    compute,
                    min_priority,
                    held,
                    first,
                } => (compute, min_priority, held, first),
                Gate::Wait(why) => {
                    self.awake.set(false);
                    return Ok(Tick::Wait(why));
                }
            }
        };
        let compute = opts.compute.unwrap_or(compute);
        self.awake.set(crate::prepare::wants_awake(&cfg, true));
        // Never hold the Library open across a job (docs/agents/concurrency.md).
        drop(lib);
        let ran = run_next_claim(tools, compute, min_priority, held, &first);
        if cfg.preparing {
            if let Ok(lib) = Library::open(tools.library()) {
                if let Err(e) = crate::prepare::settle(&lib) {
                    crate::logs::note(&format!("argus prepare: {e}"));
                }
            }
        }
        Ok(match ran? {
            Some(job) => Tick::Ran {
                job: Box::new(job),
                compute,
            },
            None => Tick::Held,
        })
    }
}

/// A background thread working through the queue until dropped.
pub struct Worker {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    /// Work on whichever Library `current` names (it can change, as when the person opens
    /// another one), with tools made by `make` so tests and the app can swap Providers.
    pub fn start(
        current: impl Fn() -> Option<PathBuf> + Send + 'static,
        make: impl Fn(PathBuf) -> Tools + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = std::thread::Builder::new()
            .name("argus-queue".into())
            .spawn(move || {
                // Parked between rounds; `stop` unparks it, so an idle queue wakes once per
                // round instead of five times a second.
                let sleep = |d: Duration| {
                    let end = std::time::Instant::now() + d;
                    while !flag.load(Ordering::Relaxed) {
                        let left = end.saturating_duration_since(std::time::Instant::now());
                        if left.is_zero() {
                            break;
                        }
                        std::thread::park_timeout(left);
                    }
                };
                let mut open: Option<(PathBuf, Tools)> = None;
                let mut rounds = Rounds::default();
                while !flag.load(Ordering::Relaxed) {
                    let Some(library) = current() else {
                        sleep(Duration::from_secs(2));
                        continue;
                    };
                    if open.as_ref().map(|(p, _)| p) != Some(&library) {
                        // A job a quit left running starts again.
                        if let Ok(mut lib) = Library::open(&library) {
                            let _ = lib.requeue_stale_jobs();
                            // An Export or Import it left part way finishes now.
                            if let Err(e) = queue_orphaned_runs(&mut lib) {
                                crate::logs::note(&format!("argus resume: {e}"));
                            }
                        }
                        open = Some((library.clone(), make(library.clone())));
                    }
                    let tools = &open.as_ref().expect("set above").1;
                    match rounds.tick(tools, TickOptions::default()) {
                        Ok(Tick::Ran { compute, .. }) => sleep(rest(compute)),
                        Ok(Tick::Wait(_) | Tick::Held | Tick::Empty) => {
                            sleep(Duration::from_secs(2))
                        }
                        Err(e) => {
                            crate::logs::note(&format!("argus queue: {e}"));
                            sleep(Duration::from_secs(10));
                        }
                    }
                }
            })
            .expect("spawn queue worker");
        Worker {
            stop,
            handle: Some(handle),
        }
    }

    /// Stop after the current job.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            h.thread().unpark();
            let _ = h.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = &self.handle {
            h.thread().unpark();
        }
    }
}
