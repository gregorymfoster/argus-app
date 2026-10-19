//! Whether the Mac is running on battery. Background AI waits for the charger (Greg,
//! 2026-10-18): a Gentle cull still drains a laptop battery in an hour or two.
//!
//! macOS answers through `pmset -g batt` ("Now drawing from 'Battery Power'"), read at
//! most every 30 s. `ARGUS_POWER=battery|ac` fakes it for tests and the e2e loops.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How often the real power source is read: plugging in shows up within this.
const FRESH: Duration = Duration::from_secs(30);

static LAST: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

/// True on battery, false on AC or on a Mac with no battery (or not a Mac).
pub fn on_battery() -> bool {
    if let Ok(v) = std::env::var("ARGUS_POWER") {
        return v.eq_ignore_ascii_case("battery");
    }
    if !cfg!(target_os = "macos") {
        return false;
    }
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, v)) = *last {
        if at.elapsed() < FRESH {
            return v;
        }
    }
    let v = std::process::Command::new("pmset")
        .args(["-g", "batt"])
        .output()
        .map(|o| parse_pmset(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or(false);
    *last = Some((Instant::now(), v));
    v
}

static THERMAL: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

/// True while macOS says the Mac is running hot (thermal state serious or critical):
/// background work then waits, as on battery. `ARGUS_THERMAL=serious|nominal` fakes it.
pub fn thermal_serious() -> bool {
    if let Ok(v) = std::env::var("ARGUS_THERMAL") {
        return matches!(v.to_ascii_lowercase().as_str(), "serious" | "critical");
    }
    let mut last = THERMAL.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, v)) = *last {
        if at.elapsed() < FRESH {
            return v;
        }
    }
    // NSProcessInfoThermalState: 0 nominal, 1 fair, 2 serious, 3 critical.
    let v = thermal_state() >= 2;
    *last = Some((Instant::now(), v));
    v
}

/// `[[NSProcessInfo processInfo] thermalState]` through the Objective-C runtime, so the
/// engine needs no Objective-C crate. 0 when it can't tell.
#[cfg(target_os = "macos")]
fn thermal_state() -> isize {
    use std::ffi::{c_char, c_void};
    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }
    #[link(name = "Foundation", kind = "framework")]
    extern "C" {}
    type SendObj = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
    type SendInt = unsafe extern "C" fn(*mut c_void, *mut c_void) -> isize;
    // SAFETY: objc_msgSend is called through the exact signatures of the two methods
    // (`+processInfo` returns an object, `-thermalState` an NSInteger), as Apple requires.
    unsafe {
        let class = objc_getClass(c"NSProcessInfo".as_ptr());
        if class.is_null() {
            return 0;
        }
        let send_obj: SendObj = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let send_int: SendInt = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let info = send_obj(class, sel_registerName(c"processInfo".as_ptr()));
        if info.is_null() {
            return 0;
        }
        send_int(info, sel_registerName(c"thermalState".as_ptr()))
    }
}

#[cfg(not(target_os = "macos"))]
fn thermal_state() -> isize {
    0
}

fn parse_pmset(out: &str) -> bool {
    out.lines()
        .next()
        .is_some_and(|l| l.contains("'Battery Power'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pmset() {
        assert!(parse_pmset(
            "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t81%; discharging; 4:12 remaining present: true"
        ));
        assert!(!parse_pmset(
            "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t100%; charged; 0:00 remaining present: true"
        ));
        assert!(!parse_pmset("Now drawing from 'AC Power'\n"));
        assert!(!parse_pmset(""));
    }
}
