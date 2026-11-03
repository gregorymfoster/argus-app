//! When the queue may run what: its settings, the idle and overnight boosts, battery
//! and heat holds, and the [`gate`] the Worker and `argus work` both go by.

use super::*;

/// The queue's own settings, kept in the Library.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QueueConfig {
    pub compute: Compute,
    /// Stop taking new jobs (the one running finishes).
    pub paused: bool,
    /// Go to Full while the Mac has been idle this many minutes (0 = never).
    pub idle_boost_min: u32,
    /// Go to Full overnight, between [`OVERNIGHT`] local hours.
    pub overnight: bool,
    /// The Photo open in Loupe or Edit jumps the queue (`focus_photo`).
    pub focus_first: bool,
    /// Suggestions per Photo when a job doesn't say.
    pub takes: u8,
    /// Run background work on battery too. Off: on battery only the open Photo's work runs
    /// (Greg, 2026-10-18).
    pub run_on_battery: bool,
    /// "Run anyway" from the Background work panel: run on battery until the next time the
    /// Mac is plugged in.
    pub battery_override: bool,
    /// Background model work (Suggestions, culls) waits until the Mac is idle and plugged
    /// in, or overnight (Greg, 2026-10-19, after an overnight cull fought the CLI). The
    /// open Photo's work still runs.
    pub idle_only: bool,
    /// "Run now" from the Background work panel: run model work while the person works,
    /// until the queue is empty.
    pub go_ahead: bool,
    /// The Shoot match reads the Shoot's hand edits as a Gaussian process and leans on the
    /// Edit head for frames unlike them (lab notebook E10). Off: the first shipped rule
    /// (Greg, 2026-10-24: "turn it on, with an off switch").
    pub shoot_match_smart: bool,
    /// The Shoot being prepared ([`crate::prepare`]): its jobs go first and skip the idle
    /// and overnight holds.
    pub preparing: bool,
    /// "Cull now": Prepare carries on at the queue's own level instead of Full.
    pub prepare_gentle: bool,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            compute: Compute::from_env(),
            paused: false,
            idle_boost_min: 3,
            overnight: true,
            focus_first: true,
            takes: 3,
            run_on_battery: false,
            battery_override: false,
            idle_only: true,
            go_ahead: false,
            shoot_match_smart: true,
            preparing: false,
            prepare_gentle: false,
        }
    }
}

/// The overnight window, in local hours: from 1:00 up to 7:00.
pub const OVERNIGHT: (u32, u32) = (1, 7);

pub(super) const CONFIG_KEY: &str = "queue";

pub fn config(lib: &Library) -> Result<QueueConfig> {
    Ok(lib
        .setting(CONFIG_KEY)?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

pub fn set_config(lib: &Library, c: &QueueConfig) -> Result<()> {
    lib.set_setting(
        CONFIG_KEY,
        &serde_json::to_value(c).expect("config serializes"),
    )
}

/// Seconds the person hasn't touched the keyboard or mouse (macOS), or None.
pub fn idle_seconds() -> Option<f64> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let out = std::process::Command::new("ioreg")
        .args(["-c", "IOHIDSystem", "-d", "4"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains("\"HIDIdleTime\""))?;
    let ns: f64 = line.rsplit('=').next()?.trim().parse().ok()?;
    Some(ns / 1e9)
}

/// The local hour of day, 0-23.
pub(super) fn local_hour() -> u32 {
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&now, &mut tm) };
    tm.tm_hour as u32
}

/// Why the queue runs at Full beyond its own level right now: "idle" or "overnight".
/// Never on battery.
pub fn boost(c: &QueueConfig) -> Option<&'static str> {
    if power::on_battery() {
        return None;
    }
    boost_at(c, idle_seconds(), local_hour())
}

/// The kinds that run a big model (the local LLM, or AI Denoise's minutes per Photo), which
/// wait for an idle Mac ([`QueueConfig::idle_only`]). The open Photo's always run.
pub const MODEL_KINDS: &[&str] = &[
    "suggest",
    "suggest_shoot",
    "match_hero",
    "cull_shoot",
    "rank_presets",
    "tag_photos",
    "denoise",
];

/// Long model jobs: a whole-Shoot cull, a Shoot's Suggestions fanning out, or its tagging.
pub(super) const LONG_KINDS: &[&str] = &["suggest_shoot", "cull_shoot", "tag_photos"];

/// The most per-Photo model jobs (a few minutes each on a local model) the idle boost runs
/// back to back; a bigger backlog is a long run and waits for overnight, like a cull.
pub const IDLE_BATCH: usize = 3;

/// The job kinds the queue passes over now (the open Photo's always run). With idle_only,
/// model work waits for the person to step away, and long runs (a cull, a Shoot's
/// Suggestions, more than [`IDLE_BATCH`] Photos' worth) wait for overnight or Run now: a
/// few idle minutes are no promise of hours (Greg, 2026-10-19: heavy runs overnight or on
/// his go). `backlog` is how many per-Photo model jobs are queued.
pub fn held_kinds(c: &QueueConfig, boost: Option<&str>, backlog: usize) -> &'static [&'static str] {
    if !c.idle_only || c.go_ahead || boost == Some("overnight") {
        &[]
    } else if boost == Some("idle") && backlog <= IDLE_BATCH {
        LONG_KINDS
    } else {
        MODEL_KINDS
    }
}

/// Per-Photo model jobs queued below the open Photo's priority.
pub fn model_backlog(lib: &Library) -> Result<usize> {
    let mut n = 0;
    for kind in ["suggest", "match_hero"] {
        n += lib
            .jobs(Some(crate::library::JobStatus::Queued), Some(kind), 10_000)?
            .iter()
            .filter(|j| j.priority < PRIORITY_FOCUS)
            .count();
    }
    Ok(n)
}

/// Background work waits for the charger: on battery, unless the person said otherwise,
/// only the open Photo's jobs run.
pub fn held_on_battery(c: &QueueConfig, on_battery: bool) -> bool {
    on_battery && !c.run_on_battery && !c.battery_override
}

pub(super) fn boost_at(c: &QueueConfig, idle_s: Option<f64>, hour: u32) -> Option<&'static str> {
    if c.overnight && (OVERNIGHT.0..OVERNIGHT.1).contains(&hour) {
        return Some("overnight");
    }
    match idle_s {
        Some(s) if c.idle_boost_min > 0 && s >= c.idle_boost_min as f64 * 60.0 => Some("idle"),
        _ => None,
    }
}

/// The Compute level to run the next job at: the queue's, or Full while the Mac is idle
/// or overnight.
pub fn effective_compute(c: &QueueConfig) -> Compute {
    match boost(c) {
        Some(_) => Compute::Full,
        None => c.compute,
    }
}

/// Rest between jobs, by Compute level: a Gentle queue leaves gaps for the person.
pub(super) fn rest(c: Compute) -> Duration {
    match c {
        Compute::Gentle => Duration::from_secs(5),
        Compute::Balanced => Duration::from_secs(2),
        Compute::Full => Duration::ZERO,
    }
}

/// Whether the queue may start a job now and, if so, how it picks one. The app's worker and
/// `argus work` both go by it, so a shell run keeps the app's holds (battery, heat, idle and
/// overnight waits, pause).
pub enum Gate {
    /// Start nothing: why.
    Wait(&'static str),
    Run {
        compute: Compute,
        /// On battery or with the Mac running hot, only the open Photo's work.
        min_priority: i64,
        /// Kinds passed over now ([`held_kinds`]).
        held: &'static [&'static str],
        /// The Shoots being prepared: their jobs first, free of `held`.
        first: Vec<ShootId>,
    },
}

pub fn gate(lib: &Library, cfg: &QueueConfig) -> Gate {
    if cfg.paused {
        return Gate::Wait("the queue is paused");
    }
    // An Agent request or a cull (here or in another Argus process, such as `argus cull`)
    // has the model; start nothing new until it's done.
    if crate::compute::interactive() || crate::compute::foreground() {
        return Gate::Wait("the model is busy");
    }
    if crate::compute::exporting(lib.root()) {
        return Gate::Wait("an export is running");
    }
    let min_priority = if held_on_battery(cfg, power::on_battery()) || power::thermal_serious() {
        PRIORITY_FOCUS
    } else {
        i64::MIN
    };
    let why_full = boost(cfg);
    let backlog = model_backlog(lib).unwrap_or(usize::MAX);
    // Run it all, and Prepare unless the person said Cull now, go at Full: the person has
    // stepped away and asked for the hard work now (Greg, 2026-11-02).
    let asked = cfg.go_ahead || (cfg.preparing && !cfg.prepare_gentle);
    Gate::Run {
        compute: if asked {
            Compute::Full
        } else {
            why_full.map_or(cfg.compute, |_| Compute::Full)
        },
        min_priority,
        held: held_kinds(cfg, why_full, backlog),
        first: if cfg.preparing {
            crate::prepare::shoots(lib).unwrap_or_default()
        } else {
            Vec::new()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_model_runs_wait_for_overnight_even_when_idle() {
        let c = QueueConfig::default();
        assert_eq!(
            held_kinds(&c, None, 0),
            MODEL_KINDS,
            "nothing while the person works"
        );
        assert_eq!(
            held_kinds(&c, Some("idle"), IDLE_BATCH),
            LONG_KINDS,
            "a few Photos at idle"
        );
        assert_eq!(
            held_kinds(&c, Some("idle"), 95),
            MODEL_KINDS,
            "a big batch waits"
        );
        assert!(held_kinds(&c, Some("overnight"), 95).is_empty());
        assert!(held_kinds(
            &QueueConfig {
                go_ahead: true,
                ..c
            },
            None,
            95
        )
        .is_empty());
        assert!(held_kinds(
            &QueueConfig {
                idle_only: false,
                ..c
            },
            None,
            95
        )
        .is_empty());
    }

    #[test]
    fn full_overnight_and_when_idle() {
        let c = QueueConfig {
            compute: Compute::Gentle,
            ..QueueConfig::default()
        };
        assert_eq!(boost_at(&c, None, 3), Some("overnight"));
        assert_eq!(boost_at(&c, None, 7), None);
        assert_eq!(boost_at(&c, Some(600.0), 14), Some("idle"));
        assert_eq!(boost_at(&c, Some(30.0), 14), None);
        let off = QueueConfig {
            overnight: false,
            idle_boost_min: 0,
            ..c
        };
        assert_eq!(boost_at(&off, Some(1e6), 3), None);
    }

    #[test]
    fn an_old_config_reads_with_the_new_defaults() {
        let c: QueueConfig = serde_json::from_value(
            json!({ "compute": "full", "paused": true, "idle_boost_min": 5 }),
        )
        .unwrap();
        assert!(c.overnight && c.focus_first && c.paused);
        assert!(!c.run_on_battery && !c.battery_override);
    }

    #[test]
    fn battery_holds_background_work_unless_told() {
        let c = QueueConfig::default();
        assert!(held_on_battery(&c, true));
        assert!(!held_on_battery(&c, false));
        let anyway = QueueConfig {
            battery_override: true,
            ..c
        };
        assert!(!held_on_battery(&anyway, true));
        let always = QueueConfig {
            run_on_battery: true,
            ..c
        };
        assert!(!held_on_battery(&always, true));
        assert_eq!(c.takes, 3);
    }
}
