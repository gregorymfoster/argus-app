//! The Resource meter: how much CPU, memory, energy and GPU the AI is taking on this
//! Mac, for the meter in the app's sidebar and for the Debug log (Greg, 2026-10-20).
//!
//! Three groups of processes, read every few seconds by one sampler thread:
//! - Local model: Ollama and its model runner (`ollama`, `llama-server`), whoever started
//!   them. A Mac's Ollama is shared, so another app's use counts here too.
//! - Claude: processes this one started (`claude -p` and its children).
//! - Argus: this process (decode, renders, Masks, the UI's engine).
//!
//! On macOS each process's CPU time, memory footprint and billed energy come from
//! `proc_pid_rusage` (the numbers Activity Monitor shows) and its GPU time from the
//! Metal clients `ioreg` lists. No root, no `powermetrics`. On Linux (cloud tests) CPU
//! and memory come from `/proc`; energy and GPU are absent. `ARGUS_METER_FAKE=1` adds a
//! busy fake model, for the UI's checks.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    LocalModel,
    Claude,
    Argus,
}

impl Group {
    pub const ALL: [Group; 3] = [Group::LocalModel, Group::Claude, Group::Argus];

    pub fn key(self) -> &'static str {
        match self {
            Group::LocalModel => "local_model",
            Group::Claude => "claude",
            Group::Argus => "argus",
        }
    }
}

/// One process at one moment. Times and energy are totals since the process started.
#[derive(Debug, Clone, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub cpu_s: f64,
    pub mem_bytes: u64,
    pub energy_j: Option<f64>,
    pub gpu_s: Option<f64>,
}

/// Which group a process is in, if any: Ollama by name, Claude when this process started
/// it (`descends` says so), Argus when it is this process.
pub fn classify(name: &str, pid: u32, me: u32, descends: bool) -> Option<Group> {
    let n = name.to_ascii_lowercase();
    if n.starts_with("ollama") || n.contains("llama-server") || n.contains("llama_server") {
        Some(Group::LocalModel)
    } else if pid == me {
        Some(Group::Argus)
    } else if descends {
        Some(Group::Claude)
    } else {
        None
    }
}

/// What one group is using now: CPU in % of one core (Activity Monitor's convention, so
/// 400% is four cores), memory, power in watts and GPU in % busy.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rates {
    pub cpu_pct: f64,
    pub mem_bytes: u64,
    pub watts: Option<f64>,
    pub gpu_pct: Option<f64>,
    pub procs: usize,
}

/// What a group used in total since the meter started: CPU seconds, joules, GPU seconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Used {
    pub cpu_s: f64,
    pub energy_j: f64,
    pub gpu_s: f64,
}

/// Every group's [`Used`], for spans like one Queue job ([`Totals::since`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Totals(pub HashMap<Group, Used>);

impl Totals {
    pub fn since(&self, before: &Totals) -> Totals {
        Totals(
            self.0
                .iter()
                .map(|(g, u)| {
                    let b = before.0.get(g).copied().unwrap_or_default();
                    (
                        *g,
                        Used {
                            cpu_s: u.cpu_s - b.cpu_s,
                            energy_j: u.energy_j - b.energy_j,
                            gpu_s: u.gpu_s - b.gpu_s,
                        },
                    )
                })
                .collect(),
        )
    }

    pub fn to_json(&self) -> Value {
        let mut m = serde_json::Map::new();
        for g in Group::ALL {
            let u = self.0.get(&g).copied().unwrap_or_default();
            m.insert(
                g.key().into(),
                json!({
                    "cpu_s": round(u.cpu_s, 2),
                    "energy_j": round(u.energy_j, 1),
                    "gpu_s": round(u.gpu_s, 2),
                }),
            );
        }
        Value::Object(m)
    }
}

fn round(x: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (x * f).round() / f
}

/// Rates between two readings of the same processes, by group, and what was used in
/// between. A process in only one reading (it started or ended) adds its memory but no
/// rate. Public for tests.
pub fn rates(
    before: &[(Group, Proc)],
    after: &[(Group, Proc)],
    secs: f64,
) -> (HashMap<Group, Rates>, HashMap<Group, Used>) {
    let mut r: HashMap<Group, Rates> = Group::ALL.iter().map(|g| (*g, Rates::default())).collect();
    let mut used: HashMap<Group, Used> = HashMap::new();
    let prev: HashMap<u32, &Proc> = before.iter().map(|(_, p)| (p.pid, p)).collect();
    for (g, p) in after {
        let e = r.get_mut(g).expect("every group");
        e.mem_bytes += p.mem_bytes;
        e.procs += 1;
        if p.energy_j.is_some() && e.watts.is_none() {
            e.watts = Some(0.0);
        }
        if p.gpu_s.is_some() && e.gpu_pct.is_none() {
            e.gpu_pct = Some(0.0);
        }
        let Some(b) = prev.get(&p.pid).filter(|b| b.name == p.name) else {
            continue;
        };
        let u = used.entry(*g).or_default();
        let cpu = (p.cpu_s - b.cpu_s).max(0.0);
        u.cpu_s += cpu;
        e.cpu_pct += 100.0 * cpu / secs;
        if let (Some(a), Some(b)) = (p.energy_j, b.energy_j) {
            let j = (a - b).max(0.0);
            u.energy_j += j;
            e.watts = Some(e.watts.unwrap_or(0.0) + j / secs);
        }
        if let (Some(a), Some(b)) = (p.gpu_s, b.gpu_s) {
            let s = (a - b).max(0.0);
            u.gpu_s += s;
            e.gpu_pct = Some(e.gpu_pct.unwrap_or(0.0) + 100.0 * s / secs);
        }
    }
    (r, used)
}

/// One point of the meter's history: seconds since 1970 and each group's rates.
#[derive(Debug, Clone)]
struct Point {
    at: f64,
    rates: HashMap<Group, Rates>,
}

struct State {
    last: Option<(Instant, Vec<(Group, Proc)>)>,
    history: std::collections::VecDeque<Point>,
    totals: Totals,
    /// When the UI last asked: the meter reads every 2 s while watched, else every 10 s.
    watched: Option<Instant>,
    logged: Option<Instant>,
    started: bool,
}

static STATE: std::sync::LazyLock<Mutex<State>> = std::sync::LazyLock::new(|| {
    Mutex::new(State {
        last: None,
        history: std::collections::VecDeque::new(),
        totals: Totals::default(),
        watched: None,
        logged: None,
        started: false,
    })
});

/// History kept for the sparklines.
const HISTORY: Duration = Duration::from_secs(10 * 60);
const FAST: Duration = Duration::from_secs(2);
const SLOW: Duration = Duration::from_secs(10);

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Start the sampler thread (once per process).
pub fn start() {
    {
        let mut s = state();
        if s.started {
            return;
        }
        s.started = true;
    }
    let _ = std::thread::Builder::new()
        .name("argus-meter".into())
        .spawn(|| loop {
            sample();
            let watched = state()
                .watched
                .is_some_and(|w| w.elapsed() < Duration::from_secs(15));
            std::thread::sleep(if watched { FAST } else { SLOW });
        });
}

/// Read every process once and fold it into the history, the totals and the Debug log.
pub fn sample() {
    let now = Instant::now();
    let procs = grouped();
    let mut s = state();
    let Some((then, before)) = s.last.take() else {
        s.last = Some((now, procs));
        return;
    };
    let secs = now.duration_since(then).as_secs_f64().max(0.001);
    let (rates, used) = rates(&before, &procs, secs);
    for (g, u) in used {
        let t = s.totals.0.entry(g).or_default();
        t.cpu_s += u.cpu_s;
        t.energy_j += u.energy_j;
        t.gpu_s += u.gpu_s;
    }
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    s.history.push_back(Point {
        at,
        rates: rates.clone(),
    });
    while s
        .history
        .front()
        .is_some_and(|p| at - p.at > HISTORY.as_secs_f64())
    {
        s.history.pop_front();
    }
    s.last = Some((now, procs));
    // Every 30 s while a model works, every 5 min otherwise.
    let busy = rates
        .iter()
        .any(|(g, r)| *g != Group::Argus && (r.cpu_pct > 10.0 || r.gpu_pct.unwrap_or(0.0) > 5.0));
    let every = Duration::from_secs(if busy { 30 } else { 300 });
    if s.logged.is_none_or(|l| l.elapsed() >= every) {
        s.logged = Some(now);
        let totals = s.totals.to_json();
        drop(s);
        crate::debug_log::event(
            "resources",
            json!({ "busy": busy, "now": rates_json(&rates), "totals": totals }),
        );
    }
}

fn rates_json(r: &HashMap<Group, Rates>) -> Value {
    let mut m = serde_json::Map::new();
    for g in Group::ALL {
        let x = r.get(&g).copied().unwrap_or_default();
        m.insert(
            g.key().into(),
            json!({
                "cpu_pct": round(x.cpu_pct, 1),
                "mem_bytes": x.mem_bytes,
                "watts": x.watts.map(|w| round(w, 2)),
                "gpu_pct": x.gpu_pct.map(|p| round(p, 1)),
                "procs": x.procs,
            }),
        );
    }
    Value::Object(m)
}

/// What every group has used since the meter started (for one job's share: read it
/// before and after, then [`Totals::since`]). Takes a fresh reading first so a short job
/// isn't missed between samples.
pub fn totals() -> Totals {
    if state().started {
        sample();
    }
    state().totals.clone()
}

/// The meter for the UI (`resource_usage`): each group now, the last 10 minutes (or its
/// last `points` samples), totals since launch, the Mac's memory, and where the Debug log
/// is. Starts the sampler. `watch`: someone is looking at the panel, so sample every 2 s for
/// a while; the sidebar's quiet line asks without it and the sampler stays at 10 s.
pub fn snapshot(watch: bool, points: Option<usize>) -> Value {
    start();
    let first = state().history.is_empty();
    if first {
        // The very first ask: read twice so there is a rate to show.
        sample();
        std::thread::sleep(Duration::from_millis(300));
        sample();
    }
    let mut s = state();
    if watch {
        s.watched = Some(Instant::now());
    }
    let now = s
        .history
        .back()
        .map(|p| rates_json(&p.rates))
        .unwrap_or(json!({}));
    let skip = points.map_or(0, |n| s.history.len().saturating_sub(n));
    let history: Vec<Value> = s
        .history
        .iter()
        .skip(skip)
        .map(|p| {
            let mut v = rates_json(&p.rates);
            v["at"] = json!(round(p.at, 1));
            v
        })
        .collect();
    json!({
        "now": now,
        "history": history,
        "totals": s.totals.to_json(),
        "mem_total_bytes": mem_total(),
        "interval_s": FAST.as_secs(),
        "energy": cfg!(target_os = "macos") || fake(),
        "log": crate::debug_log::path(),
    })
}

fn fake() -> bool {
    std::env::var("ARGUS_METER_FAKE").is_ok_and(|v| v == "1")
}

/// Every process in a group, measured. A process is this one's descendant when its parent
/// chain reaches this pid. Only those are measured, so the rest of the Mac costs one cheap
/// listing.
fn grouped() -> Vec<(Group, Proc)> {
    let me = std::process::id();
    let all = list_processes();
    let parent: HashMap<u32, u32> = all.iter().map(|(pid, ppid, _)| (*pid, *ppid)).collect();
    let descends = |mut pid: u32| {
        for _ in 0..64 {
            match parent.get(&pid) {
                Some(&pp) if pp == me => return true,
                Some(&pp) if pp > 1 && pp != pid => pid = pp,
                _ => return false,
            }
        }
        false
    };
    let picked: Vec<(Group, u32, u32, String)> = all
        .into_iter()
        .filter_map(|(pid, ppid, name)| {
            classify(&name, pid, me, descends(pid)).map(|g| (g, pid, ppid, name))
        })
        .collect();
    // `ioreg` is a process spawn: only when something besides Argus is running.
    let gpu = if picked.iter().any(|(g, ..)| *g != Group::Argus) {
        gpu_seconds()
    } else {
        None
    };
    let mut out: Vec<(Group, Proc)> = picked
        .into_iter()
        .filter_map(|(g, pid, ppid, name)| {
            let (cpu_s, mem_bytes, energy_j) = measure(pid)?;
            let gpu_s = gpu.as_ref().map(|m| m.get(&pid).copied().unwrap_or(0.0));
            Some((
                g,
                Proc {
                    pid,
                    ppid,
                    name,
                    cpu_s,
                    mem_bytes,
                    energy_j,
                    gpu_s,
                },
            ))
        })
        .collect();
    if fake() {
        out.push((Group::LocalModel, fake_model()));
    }
    out
}

/// A model runner busy on ~6 cores and the GPU at ~35 W, holding 21 GB.
fn fake_model() -> Proc {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    // Rates of base x (1 + 0.15 sin(t/7)), integrated so readings a moment apart differ
    // by that rate times the moment.
    let total = |base: f64| base * (t - 0.15 * 7.0 * (t / 7.0).cos());
    Proc {
        pid: u32::MAX - 1,
        ppid: 1,
        name: "llama-server".into(),
        cpu_s: total(1.2),
        mem_bytes: 21_400_000_000,
        energy_j: Some(total(34.0)),
        gpu_s: Some(total(0.82)),
    }
}

/// The Mac's physical memory in bytes (0 if unknown).
pub fn mem_total() -> u64 {
    #[cfg(target_os = "macos")]
    {
        let mut v: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        // SAFETY: hw.memsize is a u64; `v` and `len` describe that buffer.
        let ok = unsafe {
            libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                (&mut v as *mut u64).cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        } == 0;
        if ok {
            return v;
        }
        0
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|m| {
                m.lines()
                    .find(|l| l.starts_with("MemTotal:"))
                    .and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
            })
            .map_or(0, |kb| kb * 1024)
    }
}

/// Every process: pid, parent pid and name.
#[cfg(target_os = "macos")]
fn list_processes() -> Vec<(u32, u32, String)> {
    let mut pids = vec![0i32; 8192];
    // SAFETY: the buffer and its size in bytes match.
    let n = unsafe {
        libc::proc_listallpids(
            pids.as_mut_ptr().cast(),
            (pids.len() * std::mem::size_of::<i32>()) as i32,
        )
    };
    pids.truncate(n.max(0) as usize);
    let mut out = Vec::new();
    for pid in pids.into_iter().filter(|p| *p > 0) {
        // SAFETY: a zeroed proc_bsdinfo is a valid value of this plain C struct.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
        // SAFETY: PROC_PIDTBSDINFO fills a proc_bsdinfo of exactly `size` bytes.
        let got = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        if got != size {
            continue;
        }
        // pbi_name holds up to 32 bytes, pbi_comm 16; prefer the longer.
        let raw: &[libc::c_char] = if info.pbi_name[0] != 0 {
            &info.pbi_name
        } else {
            &info.pbi_comm
        };
        let bytes: Vec<u8> = raw
            .iter()
            .take_while(|c| **c != 0)
            .map(|c| *c as u8)
            .collect();
        out.push((
            pid as u32,
            info.pbi_ppid,
            String::from_utf8_lossy(&bytes).into_owned(),
        ));
    }
    out
}

/// CPU seconds, memory footprint and billed energy (J) of one process, from the same
/// `proc_pid_rusage` Activity Monitor reads.
#[cfg(target_os = "macos")]
fn measure(pid: u32) -> Option<(f64, u64, Option<f64>)> {
    static TICK_S: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    // CPU times in rusage are Mach ticks (not ns on Apple silicon).
    let tick_s = *TICK_S.get_or_init(|| {
        // libc's binding is deprecated (in favour of the mach2 crate), so declare it here.
        #[repr(C)]
        struct Timebase {
            numer: u32,
            denom: u32,
        }
        extern "C" {
            fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
        }
        let mut tb = Timebase { numer: 0, denom: 0 };
        // SAFETY: fills the struct it is given.
        unsafe { mach_timebase_info(&mut tb) };
        if tb.denom == 0 {
            1e-9
        } else {
            tb.numer as f64 / tb.denom as f64 * 1e-9
        }
    });
    // rusage_info_v6 as u64 slots (libc has no v6 yet): the 16-byte uuid is slots 0-1,
    // then ri_user_time 2, ri_system_time 3, ri_phys_footprint 9, ri_billed_energy 33 and
    // ri_energy_nj 42. On Greg's Mac (macOS 27, 2026-10-20) billed energy sat at 25 mJ
    // while the model ran, so energy comes from ri_energy_nj.
    let mut ru = [0u64; 58];
    // SAFETY: flavor 6 fills a rusage_info_v6, which is 58 u64s.
    let ok = unsafe { libc::proc_pid_rusage(pid as i32, 6, ru.as_mut_ptr().cast()) } == 0;
    ok.then(|| {
        (
            (ru[2] + ru[3]) as f64 * tick_s,
            ru[9],
            Some(ru[42] as f64 * 1e-9),
        )
    })
}

/// GPU seconds by pid from the Metal clients `ioreg` lists (what Activity Monitor's GPU
/// column reads), or None if `ioreg` fails.
#[cfg(target_os = "macos")]
fn gpu_seconds() -> Option<HashMap<u32, f64>> {
    std::process::Command::new("/usr/sbin/ioreg")
        .args(["-r", "-d", "1", "-w", "0", "-c", "AGXDeviceUserClient"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_ioreg_gpu(&String::from_utf8_lossy(&o.stdout)))
}

#[cfg(not(target_os = "macos"))]
fn gpu_seconds() -> Option<HashMap<u32, f64>> {
    None
}

/// Sum `accumulatedGPUTime` (ns) per `IOUserClientCreator` pid. A process can have
/// several clients and each client several AppUsage entries; macOS lists AppUsage before
/// the creator, so each `+-o` block is read whole. Public for tests.
pub fn parse_ioreg_gpu(text: &str) -> HashMap<u32, f64> {
    let mut out: HashMap<u32, f64> = HashMap::new();
    for block in text.split("+-o ") {
        let Some(pid) = block
            .split("\"IOUserClientCreator\" = \"pid ")
            .nth(1)
            .and_then(|rest| rest.split(',').next()?.trim().parse::<u32>().ok())
        else {
            continue;
        };
        for part in block.split("\"accumulatedGPUTime\"=").skip(1) {
            let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(ns) = digits.parse::<f64>() {
                *out.entry(pid).or_default() += ns * 1e-9;
            }
        }
    }
    out
}

#[cfg(not(target_os = "macos"))]
fn list_processes() -> Vec<(u32, u32, String)> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|e| {
            let pid = e.file_name().to_str()?.parse::<u32>().ok()?;
            let p = parse_proc_stat(
                pid,
                &std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?,
                100.0,
                4096,
            )?;
            Some((pid, p.ppid, p.name))
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn measure(pid: u32) -> Option<(f64, u64, Option<f64>)> {
    // USER_HZ is 100 and pages are 4 KiB on the Linux the cloud checks run on.
    let p = parse_proc_stat(
        pid,
        &std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?,
        100.0,
        4096,
    )?;
    Some((p.cpu_s, p.mem_bytes, None))
}

/// One `/proc/<pid>/stat` line as a [`Proc`]. Public for tests.
pub fn parse_proc_stat(pid: u32, stat: &str, tick: f64, page: u64) -> Option<Proc> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let name = stat[open + 1..close].to_owned();
    let f: Vec<&str> = stat[close + 2..].split_whitespace().collect();
    // After the name: state(0) ppid(1) ... utime(11) stime(12) ... rss(21).
    let num = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok());
    Some(Proc {
        pid,
        ppid: num(1)? as u32,
        name,
        cpu_s: (num(11)? + num(12)?) as f64 / tick,
        mem_bytes: num(21)? * page,
        energy_j: None,
        gpu_s: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, name: &str, cpu: f64, j: Option<f64>) -> Proc {
        Proc {
            pid,
            ppid: 1,
            name: name.into(),
            cpu_s: cpu,
            mem_bytes: 1_000,
            energy_j: j,
            gpu_s: j.map(|x| x / 10.0),
        }
    }

    #[test]
    fn groups_ollama_by_name_and_claude_by_parentage() {
        assert_eq!(classify("ollama", 5, 1, false), Some(Group::LocalModel));
        assert_eq!(
            classify("llama-server", 5, 1, false),
            Some(Group::LocalModel)
        );
        assert_eq!(classify("Argus", 1, 1, false), Some(Group::Argus));
        assert_eq!(classify("claude", 5, 1, true), Some(Group::Claude));
        assert_eq!(classify("node", 5, 1, true), Some(Group::Claude));
        // Greg's own Claude Code isn't Argus's.
        assert_eq!(classify("claude", 5, 1, false), None);
    }

    #[test]
    fn rates_come_from_the_same_process_in_both_readings() {
        let g = Group::LocalModel;
        let before = vec![(g, p(10, "llama-server", 100.0, Some(500.0)))];
        let after = vec![
            (g, p(10, "llama-server", 104.0, Some(560.0))),
            // Started between readings: memory counts, no rate yet.
            (g, p(11, "ollama", 50.0, Some(9.0))),
        ];
        let (r, used) = rates(&before, &after, 2.0);
        let m = r[&g];
        assert_eq!(m.procs, 2);
        assert_eq!(m.mem_bytes, 2_000);
        assert!((m.cpu_pct - 200.0).abs() < 1e-9, "{m:?}");
        assert!((m.watts.unwrap() - 30.0).abs() < 1e-9);
        assert!((m.gpu_pct.unwrap() - 300.0).abs() < 1e-9);
        assert!((used[&g].energy_j - 60.0).abs() < 1e-9);
        assert_eq!(r[&Group::Claude].procs, 0);
    }

    #[test]
    fn a_job_s_share_is_the_difference_of_totals() {
        let mut a = Totals::default();
        a.0.insert(
            Group::LocalModel,
            Used {
                cpu_s: 1.0,
                energy_j: 10.0,
                gpu_s: 2.0,
            },
        );
        let mut b = a.clone();
        b.0.insert(
            Group::LocalModel,
            Used {
                cpu_s: 4.0,
                energy_j: 70.0,
                gpu_s: 5.0,
            },
        );
        b.0.insert(
            Group::Claude,
            Used {
                cpu_s: 1.0,
                ..Used::default()
            },
        );
        let d = b.since(&a);
        assert_eq!(d.0[&Group::LocalModel].energy_j, 60.0);
        assert_eq!(d.0[&Group::Claude].cpu_s, 1.0);
        assert_eq!(d.to_json()["local_model"]["gpu_s"], 3.0);
    }

    #[test]
    fn reads_gpu_time_from_ioreg() {
        let text = r#"+-o AGXDeviceUserClient  <class AGXDeviceUserClient, id 0x1, !registered>
    {
      "AppUsage" = ({"API"="Metal","lastSubmittedTime"=1,"accumulatedGPUTime"=2500000000},{"API"="Metal","accumulatedGPUTime"=500000000})
      "IOUserClientCreator" = "pid 412, llama-server"
    }
+-o AGXDeviceUserClient  <class AGXDeviceUserClient, id 0x2, !registered>
    {
      "IOUserClientCreator" = "pid 99, WindowServer"
      "AppUsage" = ({"API"="Metal","accumulatedGPUTime"=1000000000})
    }
+-o AGXDeviceUserClient  <class AGXDeviceUserClient, id 0x3, !registered>
    {
      "IOUserClientCreator" = "pid 412, llama-server"
    }
"#;
        let g = parse_ioreg_gpu(text);
        assert!((g[&412] - 3.0).abs() < 1e-9, "{g:?}");
        assert!((g[&99] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn reads_proc_stat_with_spaces_in_the_name() {
        let stat = "4242 (llama server) S 1 4242 4242 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 8 0 100 123456 2048 18446744073709551615";
        let p = parse_proc_stat(4242, stat, 100.0, 4096).unwrap();
        assert_eq!(p.name, "llama server");
        assert_eq!(p.ppid, 1);
        assert!((p.cpu_s - 3.0).abs() < 1e-9);
        assert_eq!(p.mem_bytes, 2048 * 4096);
    }

    #[test]
    fn this_process_is_measured() {
        let mine = grouped();
        assert!(mine
            .iter()
            .any(|(g, p)| *g == Group::Argus && p.pid == std::process::id() && p.mem_bytes > 0));
    }
}
