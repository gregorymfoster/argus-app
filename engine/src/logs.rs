//! The app's log: what it prints to stderr (queue failures, culls, model waits) goes to
//! `~/Library/Logs/Argus/argus.log`, so a stuck or failed run can be traced afterwards
//! (Console.app reads that folder too). Small and rotated: at [`MAX_BYTES`] the file moves
//! to `argus.1.log`, and the one before that is dropped.

use std::path::{Path, PathBuf};

pub const MAX_BYTES: u64 = 2_000_000;

/// One timestamped line on stderr (and so in the log): `2026-10-19T02:47:11Z message`.
pub fn note(msg: &str) {
    eprintln!("{} {msg}", crate::library::time::now());
}

/// Where the app logs on this Mac: ARGUS_LOG_DIR, else `~/Library/Logs/Argus`.
pub fn default_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("ARGUS_LOG_DIR") {
        return Some(d.into());
    }
    let home = std::env::var_os("HOME")?;
    Some(Path::new(&home).join("Library/Logs/Argus"))
}

/// Point this process's stderr at `dir/argus.log` (appending), rotating it first if it
/// has grown past [`MAX_BYTES`]. Returns the log's path.
pub fn to_file(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let log = dir.join("argus.log");
    rotate(&log, &dir.join("argus.1.log"), MAX_BYTES)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: dup2 duplicates a descriptor we own onto stderr; both stay valid.
        if unsafe { libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(log)
}

/// Move `log` to `old` (replacing it) when it is larger than `max` bytes. Public for tests.
pub fn rotate(log: &Path, old: &Path, max: u64) -> std::io::Result<()> {
    match std::fs::metadata(log) {
        Ok(m) if m.len() > max => std::fs::rename(log, old),
        _ => Ok(()),
    }
}

/// Whether stderr is a terminal (a dev run, where output should stay on screen).
pub fn stderr_is_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_only_past_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let (log, old) = (dir.path().join("argus.log"), dir.path().join("argus.1.log"));
        std::fs::write(&log, "12345").unwrap();
        rotate(&log, &old, 10).unwrap();
        assert!(log.exists() && !old.exists());
        std::fs::write(&old, "older").unwrap();
        rotate(&log, &old, 4).unwrap();
        assert!(!log.exists());
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "12345");
    }
}
