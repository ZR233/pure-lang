use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use super::report_fallback;

const RETENTION: Duration = Duration::from_secs(48 * 60 * 60);
const CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);
const LEGACY_LOG_BYTES: u64 = 64 * 1024 * 1024;

pub(super) struct RetentionGuard {
    stop: Sender<()>,
    worker: Option<JoinHandle<()>>,
    log_dir: PathBuf,
    crash_dir: PathBuf,
}

impl RetentionGuard {
    pub(super) fn spawn(log_dir: PathBuf, crash_dir: PathBuf) -> Option<Self> {
        let (stop, receiver) = mpsc::channel();
        let worker_log_dir = log_dir.clone();
        let worker_crash_dir = crash_dir.clone();
        let worker = match thread::Builder::new()
            .name("studio-log-retention".to_string())
            .spawn(move || {
                loop {
                    match receiver.recv_timeout(CLEANUP_INTERVAL) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {
                            clean_expired_logs(
                                &worker_log_dir,
                                &worker_crash_dir,
                                SystemTime::now(),
                            );
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
            log_dir,
            crash_dir,
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
        clean_expired_logs(&self.log_dir, &self.crash_dir, SystemTime::now());
    }
}

pub(super) fn clean_expired_logs(log_dir: &Path, crash_dir: &Path, now: SystemTime) {
    let Some(cutoff) = now.checked_sub(RETENTION) else {
        return;
    };
    clean_directory(log_dir, cutoff, is_owned_log_name);
    clean_directory(crash_dir, cutoff, is_owned_crash_name);
    clean_legacy_size(log_dir);
}

/// One upgrade boundary for the old date-named files (including the Dart fallback).
/// New numbered Rust logs are bounded by rolling-file and are not deleted behind its handle.
fn clean_legacy_size(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut files = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            if !is_legacy_log_name(name) {
                return None;
            }
            let kind = entry.file_type().ok()?;
            if !kind.is_file() || kind.is_symlink() {
                return None;
            }
            let meta = entry.metadata().ok()?;
            Some((meta.modified().ok()?, meta.len(), entry.path()))
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|(modified, _, _)| *modified);
    let mut total = files
        .iter()
        .fold(0u64, |sum, (_, bytes, _)| sum.saturating_add(*bytes));
    for (_, bytes, path) in files {
        if total <= LEGACY_LOG_BYTES {
            break;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => total = total.saturating_sub(bytes),
            Err(error) => report_fallback(&format!(
                "cannot remove oversized legacy log {}: {error}",
                path.display()
            )),
        }
    }
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
        if !entry
            .file_type()
            .is_ok_and(|kind| kind.is_file() && !kind.is_symlink())
        {
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
    is_legacy_log_name(file_name)
        || ["studio.log.", "error.log."].iter().any(|prefix| {
            file_name.strip_prefix(prefix).is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
}

fn is_legacy_log_name(file_name: &str) -> bool {
    if file_name == "studio.log" || file_name == "error.log" {
        return false;
    }
    (file_name.starts_with("studio.")
        || file_name.starts_with("error-")
        || file_name.starts_with("dart-error-"))
        && file_name.ends_with(".log")
}

fn is_owned_crash_name(file_name: &str) -> bool {
    (file_name.starts_with("crash-") || file_name.starts_with("rust-panic-"))
        && file_name.ends_with(".log")
}
