use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use super::report_fallback;

const RETENTION: Duration = Duration::from_secs(48 * 60 * 60);
const CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);

pub(super) struct RetentionGuard {
    stop: Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl RetentionGuard {
    pub(super) fn spawn(log_dir: PathBuf, crash_dir: PathBuf) -> Option<Self> {
        let (stop, receiver) = mpsc::channel();
        let worker = match thread::Builder::new()
            .name("studio-log-retention".to_string())
            .spawn(move || {
                loop {
                    match receiver.recv_timeout(CLEANUP_INTERVAL) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {
                            clean_expired_logs(&log_dir, &crash_dir, SystemTime::now());
                        }
                    }
                }
            }) {
            Ok(worker) => worker,
            Err(error) => {
                report_fallback(&format!("cannot start log retention worker: {error}"));
                return None;
            }
        };
        Some(Self {
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for RetentionGuard {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            report_fallback("log retention worker panicked");
        }
    }
}

pub(super) fn clean_expired_logs(log_dir: &Path, crash_dir: &Path, now: SystemTime) {
    let Some(cutoff) = now.checked_sub(RETENTION) else {
        return;
    };
    clean_directory(log_dir, cutoff, is_owned_log_name);
    clean_directory(crash_dir, cutoff, is_owned_crash_name);
}

fn clean_directory(directory: &Path, cutoff: SystemTime, owns: fn(&str) -> bool) {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            report_fallback(&format!(
                "cannot inspect retention directory {}: {error}",
                directory.display()
            ));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report_fallback(&format!("cannot inspect a retention entry: {error}"));
                continue;
            }
        };
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !owns(file_name) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => continue,
            Err(error) => {
                report_fallback(&format!(
                    "cannot inspect retained log {}: {error}",
                    entry.path().display()
                ));
                continue;
            }
        };
        let is_expired = metadata.modified().is_ok_and(|modified| modified < cutoff);
        if is_expired && let Err(error) = std::fs::remove_file(entry.path()) {
            report_fallback(&format!(
                "cannot remove expired log {}: {error}",
                entry.path().display()
            ));
        }
    }
}

fn is_owned_log_name(file_name: &str) -> bool {
    (file_name.starts_with("studio-")
        || file_name.starts_with("error-")
        || file_name.starts_with("dart-error-"))
        && file_name.ends_with(".log")
}

fn is_owned_crash_name(file_name: &str) -> bool {
    (file_name.starts_with("crash-") || file_name.starts_with("rust-panic-"))
        && file_name.ends_with(".log")
}

#[cfg(test)]
mod tests {
    use filetime::{FileTime, set_file_mtime};

    use super::*;

    #[test]
    fn cleanup_removes_only_owned_files_strictly_older_than_48_hours() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "pure-studio-retention-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_nanos()
        ));
        let log_dir = root.join("logs");
        let crash_dir = root.join("crashes");
        std::fs::create_dir_all(&log_dir)?;
        std::fs::create_dir_all(&crash_dir)?;
        let expired = log_dir.join("studio-2026-08-01.log");
        let dart_error = log_dir.join("dart-error-2026-08-01.log");
        let boundary = log_dir.join("error-2026-08-02.log");
        let unrelated = log_dir.join("other-2026-08-01.log");
        let crash = crash_dir.join("crash-1-2.log");
        for path in [&expired, &dart_error, &boundary, &unrelated, &crash] {
            std::fs::write(path, "fixture")?;
        }

        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100 * 60 * 60);
        let cutoff = now.checked_sub(RETENTION).unwrap();
        let expired_at = cutoff.checked_sub(Duration::from_secs(1)).unwrap();
        set_file_mtime(&expired, FileTime::from_system_time(expired_at))?;
        set_file_mtime(&dart_error, FileTime::from_system_time(expired_at))?;
        set_file_mtime(&boundary, FileTime::from_system_time(cutoff))?;
        set_file_mtime(&unrelated, FileTime::from_system_time(expired_at))?;
        set_file_mtime(&crash, FileTime::from_system_time(expired_at))?;

        clean_expired_logs(&log_dir, &crash_dir, now);

        assert!(!expired.exists());
        assert!(!dart_error.exists());
        assert!(boundary.exists());
        assert!(unrelated.exists());
        assert!(!crash.exists());
        std::fs::remove_dir_all(root)?;
        Ok(())
    }
}
