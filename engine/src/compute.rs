//! Compute level: how much of the Mac a local model may take while it works.
//!
//! On Apple Silicon Ollama runs on the GPU through Metal, and there is no knob to cap a
//! process's GPU share. So a [`Throttle`] does what the OS allows from outside: it drops
//! Ollama's processes to background QoS (macOS then favours the person's apps for CPU and
//! I/O) and, below Full, duty-cycles them with SIGSTOP/SIGCONT so the model only computes
//! part of the time. Slower culls in exchange for a usable Mac.

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Compute {
    /// For when the person is working: background QoS, model runs half the time.
    #[default]
    Gentle,
    /// Background QoS and three quarters of the time.
    Balanced,
    /// Everything, for overnight runs.
    Full,
}

/// What a Compute level means in practice. Public for tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// Ollama `num_batch`: smaller batches mean shorter GPU bursts, so pauses take effect
    /// sooner and the person's apps get the GPU between them. None leaves the default.
    pub num_batch: Option<u32>,
    pub background: bool,
    /// (run, pause) duty cycle; None runs flat out.
    pub duty: Option<(Duration, Duration)>,
}

impl Compute {
    /// ARGUS_COMPUTE (gentle | balanced | full), default Gentle.
    pub fn from_env() -> Self {
        std::env::var("ARGUS_COMPUTE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_default()
    }

    pub fn plan(self) -> Plan {
        let ms = Duration::from_millis;
        match self {
            // Measured on an M4 Max with Ollama 0.40.6 (2026-10-18). A 1 s / 1 s cycle only
            // cut GPU use from 73% to 61%, because queued GPU work runs through short pauses.
            // 1.5 s / 3 s with num_batch 128 halved GPU use (93% to 48%), but culls took 7x
            // as long: each restart costs time. So cycles are long and batches moderate.
            Compute::Gentle => Plan {
                num_batch: Some(256),
                background: true,
                duty: Some((ms(4000), ms(4000))),
            },
            Compute::Balanced => Plan {
                num_batch: None,
                background: true,
                duty: Some((ms(6000), ms(2000))),
            },
            Compute::Full => Plan {
                num_batch: None,
                background: false,
                duty: None,
            },
        }
    }
}

impl std::str::FromStr for Compute {
    type Err = crate::Error;
    fn from_str(s: &str) -> crate::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "gentle" => Ok(Compute::Gentle),
            "balanced" => Ok(Compute::Balanced),
            "full" => Ok(Compute::Full),
            other => Err(crate::Error(format!(
                "unknown compute level {other:?}; use gentle, balanced or full"
            ))),
        }
    }
}

/// How Argus's own CPU work (building previews) shares the Mac at a Compute level: a
/// rayon pool with fewer threads below Full, at a lower QoS so macOS keeps them off the
/// person's way (Background runs on the efficiency cores only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuPlan {
    pub threads: usize,
    pub priority: CpuPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuPriority {
    Background,
    Utility,
    Normal,
}

impl Compute {
    /// The CPU share for `cores` logical cores.
    pub fn cpu_plan(self, cores: usize) -> CpuPlan {
        let cores = cores.max(1);
        match self {
            Compute::Gentle => CpuPlan {
                threads: (cores / 4).max(2).min(cores),
                priority: CpuPriority::Background,
            },
            Compute::Balanced => CpuPlan {
                threads: (cores / 2).max(2).min(cores),
                priority: CpuPriority::Utility,
            },
            Compute::Full => CpuPlan {
                threads: cores,
                priority: CpuPriority::Normal,
            },
        }
    }

    /// A thread pool for CPU work at this level; run work inside it with `install`.
    pub fn cpu_pool(self) -> crate::Result<rayon::ThreadPool> {
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        let plan = self.cpu_plan(cores);
        rayon::ThreadPoolBuilder::new()
            .num_threads(plan.threads)
            .thread_name(|i| format!("argus-cpu-{i}"))
            .start_handler(move |_| lower_thread_priority(plan.priority))
            .build()
            .map_err(|e| crate::Error(format!("thread pool: {e}")))
    }
}

/// Lower the calling thread's priority (macOS QoS class, Linux nice).
fn lower_thread_priority(p: CpuPriority) {
    #[cfg(target_os = "macos")]
    {
        let class = match p {
            CpuPriority::Background => libc::qos_class_t::QOS_CLASS_BACKGROUND,
            CpuPriority::Utility => libc::qos_class_t::QOS_CLASS_UTILITY,
            CpuPriority::Normal => return,
        };
        // SAFETY: sets the QoS of the calling thread only; no pointers involved.
        unsafe {
            libc::pthread_set_qos_class_self_np(class, 0);
        }
    }
    #[cfg(target_os = "linux")]
    {
        let nice = match p {
            CpuPriority::Background => 19,
            CpuPriority::Utility => 10,
            CpuPriority::Normal => return,
        };
        // SAFETY: on Linux nice is per thread; who = 0 is the calling thread.
        unsafe {
            libc::setpriority(libc::PRIO_PROCESS, 0, nice);
        }
    }
}

static INTERACTIVE: AtomicUsize = AtomicUsize::new(0);

/// Marks AI work the person is waiting on (an Agent request, Suggestions for the Photo
/// open in Edit) while it lives. Background work makes way for it: a [`Throttle`] stops
/// pausing Ollama and lifts its background QoS, and the queue starts no new job.
pub struct Interactive(());

impl Interactive {
    pub fn begin() -> Self {
        INTERACTIVE.fetch_add(1, Ordering::SeqCst);
        Interactive(())
    }
}

impl Drop for Interactive {
    fn drop(&mut self) {
        INTERACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Whether any [`Interactive`] work is running in this process.
pub fn interactive() -> bool {
    INTERACTIVE.load(Ordering::SeqCst) > 0
}

static FOREGROUND: AtomicUsize = AtomicUsize::new(0);
static MARKERS: AtomicUsize = AtomicUsize::new(0);

/// A long model run in the foreground (a cull) while it lives, in this process or, through
/// a marker file, in another Argus process on this Mac (the CLI beside the app). A Queue
/// starts no new job while one runs, so it doesn't share the model with it. Unlike
/// [`Interactive`] it leaves the [`Throttle`] alone: a Gentle cull stays gentle.
pub struct Foreground {
    marker: Option<std::path::PathBuf>,
}

impl Foreground {
    pub fn begin() -> Self {
        FOREGROUND.fetch_add(1, Ordering::SeqCst);
        let dir = foreground_dir();
        let path = dir.join(format!(
            "{}-{}",
            std::process::id(),
            MARKERS.fetch_add(1, Ordering::SeqCst)
        ));
        let marker = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(&path, b""))
            .ok()
            .map(|_| path);
        Foreground { marker }
    }
}

impl Drop for Foreground {
    fn drop(&mut self) {
        FOREGROUND.fetch_sub(1, Ordering::SeqCst);
        if let Some(m) = &self.marker {
            let _ = std::fs::remove_file(m);
        }
    }
}

/// Where processes leave their [`Foreground`] markers: ARGUS_FOREGROUND_DIR, else
/// `argus-foreground` in the per-user temp folder.
fn foreground_dir() -> std::path::PathBuf {
    std::env::var_os("ARGUS_FOREGROUND_DIR")
        .map(Into::into)
        .unwrap_or_else(|| std::env::temp_dir().join("argus-foreground"))
}

/// Whether a [`Foreground`] run is going on in this or another live Argus process. Markers
/// a killed process left behind are removed.
pub fn foreground() -> bool {
    if FOREGROUND.load(Ordering::SeqCst) > 0 {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(foreground_dir()) else {
        return false;
    };
    let me = std::process::id();
    let mut any = false;
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|n| n.split('-').next())
            .and_then(|p| p.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == me {
            continue; // this process's own are counted above; a stale one is ours to drop
        }
        if alive(pid) {
            any = true;
        } else {
            let _ = std::fs::remove_file(e.path());
        }
    }
    any
}

pub(crate) fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: signal 0 only checks that the process exists; nothing is sent.
        let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
        r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

/// The processes a Throttle acts on. The real one shells out to pgrep, taskpolicy and kill;
/// tests swap in a recorder.
pub trait Processes: Send + Sync + 'static {
    fn pids(&self) -> Vec<u32>;
    fn background(&self, pid: u32, on: bool);
    fn pause(&self, pid: u32);
    fn resume(&self, pid: u32);
    /// A separate process that thaws Ollama if Argus dies while it's paused (a kill
    /// skips Drop). The Throttle stops it once it has restored Ollama itself.
    fn watchdog(&self) -> Option<std::process::Child> {
        None
    }
}

/// Ollama's own processes on this Mac (the server and any model runners it spawned).
pub struct OllamaProcesses;

impl Processes for OllamaProcesses {
    fn pids(&self) -> Vec<u32> {
        let listing = Command::new("pgrep")
            .args(["-fl", PATTERN])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        ollama_pids(&listing, std::process::id())
    }
    fn background(&self, pid: u32, on: bool) {
        let flag = if on { "-b" } else { "-B" };
        let _ = Command::new("taskpolicy")
            .args([flag, "-p", &pid.to_string()])
            .output();
    }
    fn pause(&self, pid: u32) {
        let _ = Command::new("kill")
            .args(["-STOP", &pid.to_string()])
            .output();
    }
    fn resume(&self, pid: u32) {
        let _ = Command::new("kill")
            .args(["-CONT", &pid.to_string()])
            .output();
    }
    fn watchdog(&self) -> Option<std::process::Child> {
        use std::os::unix::process::CommandExt;
        let script = format!(
            "while kill -0 {} 2>/dev/null; do sleep 1; done; \
             for p in $(pgrep -f '{PATTERN}'); do kill -CONT $p; taskpolicy -B -p $p; done",
            std::process::id()
        );
        Command::new("sh")
            .args(["-c", &script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0) // survives a Ctrl-C sent to Argus's process group
            .spawn()
            .ok()
    }
}

const PATTERN: &str = "ollama (serve|runner)|llama-server";

/// Pids from `pgrep -fl` output: the model runners when there are any (they do the work,
/// and the server stays responsive), otherwise the server itself. Ollama 0.35 runs models
/// in a child `llama-server`; older versions in `ollama runner`. Public for tests.
pub fn ollama_pids(pgrep_listing: &str, me: u32) -> Vec<u32> {
    let rows: Vec<(u32, bool)> = pgrep_listing
        .lines()
        .filter_map(|l| {
            let (pid, cmd) = l.trim().split_once(' ')?;
            // Judge by the executable, so a shell whose script merely mentions Ollama
            // (like our own watchdog) is never mistaken for it.
            let mut words = cmd.split_whitespace();
            let exe = words.next()?.rsplit('/').next()?;
            let arg = words.next().unwrap_or_default();
            let runner = exe == "llama-server" || (exe == "ollama" && arg == "runner");
            let server = exe == "ollama" && arg == "serve";
            (runner || server).then_some((pid.parse().ok()?, runner))
        })
        .filter(|&(p, _)| p != me)
        .collect();
    let runners = rows.iter().any(|&(_, r)| r);
    rows.into_iter()
        .filter(|&(_, r)| r == runners)
        .map(|(p, _)| p)
        .collect()
}

/// Holds Ollama to a Compute level while it lives. Dropping it lets Ollama run normally again.
pub struct Throttle {
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    watchdog: Option<std::process::Child>,
}

impl Throttle {
    pub fn start(level: Compute, procs: impl Processes) -> Self {
        let plan = level.plan();
        // A previous Argus that died mid-pause would have left Ollama frozen or in the
        // background; thaw it first.
        for pid in procs.pids() {
            procs.resume(pid);
            procs.background(pid, false);
        }
        let stop = Arc::new(AtomicBool::new(false));
        if !plan.background && plan.duty.is_none() {
            return Self {
                stop,
                worker: None,
                watchdog: None,
            };
        }
        let watchdog = procs.watchdog();
        let flag = stop.clone();
        let worker = std::thread::spawn(move || {
            let tick = Duration::from_millis(100);
            let mut seen: Vec<u32> = Vec::new();
            let mut lifted = false;
            while !flag.load(Ordering::Relaxed) {
                // Runners come and go as models load, so look again every cycle.
                let pids = procs.pids();
                // The person is waiting on the model: let it run flat out until they aren't.
                if interactive() {
                    for &p in &pids {
                        if !lifted && plan.background {
                            procs.background(p, false);
                        }
                        if !seen.contains(&p) {
                            seen.push(p);
                        }
                    }
                    lifted = true;
                    sleep_unless(&flag, Duration::from_millis(500), tick);
                    continue;
                }
                for &p in &pids {
                    if !seen.contains(&p) || lifted {
                        if plan.background {
                            procs.background(p, true);
                        }
                        if !seen.contains(&p) {
                            seen.push(p);
                        }
                    }
                }
                lifted = false;
                let Some((run, pause)) = plan.duty else {
                    sleep_unless(&flag, Duration::from_secs(2), tick);
                    continue;
                };
                sleep_unless(&flag, run, tick);
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                if interactive() {
                    continue;
                }
                for &p in &pids {
                    procs.pause(p);
                }
                // A pause ends early when the person starts waiting on the model.
                let end = std::time::Instant::now() + pause;
                while std::time::Instant::now() < end
                    && !flag.load(Ordering::Relaxed)
                    && !interactive()
                {
                    std::thread::sleep(tick);
                }
                for &p in &pids {
                    procs.resume(p);
                }
            }
            // Restore everything this Throttle touched, plus anything that inherited
            // background QoS from it (a runner spawned by a backgrounded server).
            for p in seen.into_iter().chain(procs.pids()) {
                procs.resume(p);
                if plan.background {
                    procs.background(p, false);
                }
            }
        });
        Self {
            stop,
            worker: Some(worker),
            watchdog,
        }
    }
}

fn sleep_unless(flag: &AtomicBool, total: Duration, tick: Duration) {
    let mut left = total;
    while !left.is_zero() && !flag.load(Ordering::Relaxed) {
        let step = left.min(tick);
        std::thread::sleep(step);
        left -= step;
    }
}

impl Drop for Throttle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
        if let Some(mut w) = self.watchdog.take() {
            let _ = w.kill();
            let _ = w.wait();
        }
    }
}
