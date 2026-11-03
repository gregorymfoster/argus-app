//! Telling the Mac's sleep from slow work (Greg, 2026-11-02: exports and background work
//! should survive closing the lid). A sleeping Mac stops the monotonic clock ([`Instant`]
//! counts only time awake, on macOS and Linux alike) while the wall clock keeps going, so
//! the gap between the two over a piece of work is how long the Mac slept during it.
//!
//! The Queue uses it so a job that fails across a sleep (a cloud model's connection cut by
//! the lid, a card reader gone for a moment) goes back in the queue, marked
//! "Paused while asleep", instead of counting as failed.

use std::time::{Duration, Instant, SystemTime};

/// Less than this between the clocks is clock drift or an NTP nudge, not a sleep.
pub const SLEPT: Duration = Duration::from_secs(30);

/// A moment on both clocks.
#[derive(Debug, Clone, Copy)]
pub struct Since {
    wall: SystemTime,
    mono: Instant,
}

impl Since {
    pub fn now() -> Self {
        Self {
            wall: SystemTime::now(),
            mono: Instant::now(),
        }
    }

    /// A moment as if the Mac had slept for `slept` since it (tests).
    pub fn slept_for(slept: Duration) -> Self {
        let now = Self::now();
        Self {
            wall: now.wall.checked_sub(slept).unwrap_or(now.wall),
            ..now
        }
    }

    /// How long the Mac slept since this moment.
    pub fn slept(&self) -> Duration {
        let wall = SystemTime::now()
            .duration_since(self.wall)
            .unwrap_or_default();
        wall.saturating_sub(self.mono.elapsed())
    }

    /// Whether the Mac slept since this moment ([`SLEPT`] or more).
    pub fn did_sleep(&self) -> bool {
        self.slept() >= SLEPT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn awake_time_is_not_sleep() {
        let s = Since::now();
        std::thread::sleep(Duration::from_millis(20));
        assert!(!s.did_sleep());
        assert!(Since::slept_for(Duration::from_secs(600)).did_sleep());
    }
}
