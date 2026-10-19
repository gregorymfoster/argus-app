use argus_engine::compute::{
    foreground, ollama_pids, Compute, Foreground, Interactive, Processes, Throttle,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<String>>>);

impl Recorder {
    fn log(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

impl Processes for Recorder {
    fn pids(&self) -> Vec<u32> {
        vec![42]
    }
    fn background(&self, pid: u32, on: bool) {
        self.0.lock().unwrap().push(format!("bg {pid} {on}"));
    }
    fn pause(&self, pid: u32) {
        self.0.lock().unwrap().push(format!("pause {pid}"));
    }
    fn resume(&self, pid: u32) {
        self.0.lock().unwrap().push(format!("resume {pid}"));
    }
}

#[test]
fn levels_parse_and_default_to_gentle() {
    assert_eq!("Balanced".parse::<Compute>().unwrap(), Compute::Balanced);
    assert!("turbo".parse::<Compute>().is_err());
    assert_eq!(Compute::default(), Compute::Gentle);
    let gentle = Compute::Gentle.plan();
    assert!(gentle.background && gentle.duty.is_some() && gentle.num_batch.is_some());
    let full = Compute::Full.plan();
    assert!(!full.background && full.duty.is_none() && full.num_batch.is_none());
}

#[test]
fn gentle_backgrounds_and_duty_cycles_ollama_then_lets_it_go() {
    let rec = Recorder::default();
    let t = Throttle::start(Compute::Gentle, rec.clone());
    std::thread::sleep(Duration::from_millis(4500)); // past one 4 s run phase
    drop(t);
    let log = rec.log();
    assert_eq!(
        log[..2],
        ["resume 42", "bg 42 false"],
        "thaws Ollama left by a crash"
    );
    assert!(log.contains(&"bg 42 true".into()));
    assert!(log.contains(&"pause 42".into()));
    // Nothing is left paused or in the background once the Throttle is gone.
    let tail: Vec<_> = log.iter().rev().take(2).cloned().collect();
    assert_eq!(tail, ["bg 42 false", "resume 42"]);

    // While the person waits on the model (an Agent request), Gentle stops pausing it and
    // lifts its background QoS. Same test, as Interactive is process-wide.
    let rec = Recorder::default();
    let t = Throttle::start(Compute::Gentle, rec.clone());
    std::thread::sleep(Duration::from_millis(300));
    let waiting = Interactive::begin();
    std::thread::sleep(Duration::from_millis(4500));
    let during = rec.log().len();
    assert!(!rec.log().contains(&"pause 42".into()), "{:?}", rec.log());
    assert_eq!(rec.log().last().unwrap(), "bg 42 false", "lifted");
    drop(waiting);
    drop(t);
    assert!(rec.log()[during..].contains(&"resume 42".into()));
}

#[test]
fn full_leaves_ollama_alone() {
    let rec = Recorder::default();
    drop(Throttle::start(Compute::Full, rec.clone()));
    assert_eq!(rec.log(), ["resume 42", "bg 42 false"]);
}

#[test]
fn throttles_runners_not_the_server_when_a_model_is_loaded() {
    let idle = "101 /Applications/Ollama.app/Contents/Resources/ollama serve\n";
    assert_eq!(ollama_pids(idle, 1), [101]);
    let busy = format!(
        "{idle}202 /Applications/Ollama.app/Contents/Resources/ollama runner --model x --port 5\n"
    );
    assert_eq!(ollama_pids(&busy, 1), [202]);
    assert_eq!(ollama_pids(&busy, 202), [101]);
    // Ollama 0.35 runs the model in a child llama-server.
    let v035 =
        format!("{idle}303 /Applications/Ollama.app/Contents/Resources/llama-server --model x\n");
    assert_eq!(ollama_pids(&v035, 1), [303]);
    // Argus's watchdog shell names the pattern in its script; it must never be paused.
    let watchdog = format!("{v035}404 sh -c while kill -0 9 2>/dev/null; do sleep 1; done; for p in $(pgrep -f 'ollama (serve|runner)|llama-server')\n");
    assert_eq!(ollama_pids(&watchdog, 1), [303]);
}

#[test]
fn cpu_share_grows_with_the_compute_level() {
    use argus_engine::compute::CpuPriority;
    let (g, b, f) = (
        Compute::Gentle.cpu_plan(16),
        Compute::Balanced.cpu_plan(16),
        Compute::Full.cpu_plan(16),
    );
    assert_eq!((g.threads, b.threads, f.threads), (4, 8, 16));
    assert_eq!(g.priority, CpuPriority::Background);
    assert_eq!(f.priority, CpuPriority::Normal);
    // Never more threads than cores, never none.
    assert_eq!(Compute::Gentle.cpu_plan(1).threads, 1);
    assert_eq!(Compute::Balanced.cpu_plan(0).threads, 1);
}

#[test]
fn a_cull_in_another_process_makes_the_queue_wait_until_it_ends() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ARGUS_FOREGROUND_DIR", dir.path());
    assert!(!foreground());
    // In this process.
    let cull = Foreground::begin();
    assert!(foreground());
    drop(cull);
    assert!(!foreground());
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        0,
        "marker removed"
    );
    // In another live process (here a child standing in for `argus cull`).
    let mut other = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    std::fs::write(dir.path().join(format!("{}-0", other.id())), b"").unwrap();
    assert!(foreground());
    // Killed without cleaning up: its marker no longer counts and is cleared away.
    other.kill().unwrap();
    other.wait().unwrap();
    assert!(!foreground());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
