//! Pure Studio process diagnostics and durable log lifecycle.

mod retention;
mod writer;

use std::backtrace::Backtrace;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, OnceLock};

use time::{Date, OffsetDateTime};
use tracing::Level;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::writer::MakeWriterExt;

use self::retention::RetentionGuard;
use self::writer::{DailyFileWriter, SyncErrorMakeWriter};

const CLI_LOG_LEVEL_ENV: &str = "PURE_STUDIO_LOG_LEVEL";
const DEFAULT_LOG_FILTER: &str = "warn";

static INITIALIZE: Once = Once::new();
static DIAGNOSTICS: OnceLock<Mutex<Option<DiagnosticsGuard>>> = OnceLock::new();

struct DiagnosticsGuard {
    retention: Option<RetentionGuard>,
    log_writer: Option<tracing_appender::non_blocking::WorkerGuard>,
}

impl Drop for DiagnosticsGuard {
    fn drop(&mut self) {
        drop(self.retention.take());
        drop(self.log_writer.take());
    }
}

pub(crate) fn initialize() {
    INITIALIZE.call_once(initialize_once);
}

/// Flushes asynchronous diagnostics after Studio runtime shutdown completes.
pub(crate) fn shutdown() {
    tracing::info!(
        application = "Pure Studio",
        "Studio diagnostics shutting down"
    );
    let guard = DIAGNOSTICS
        .get()
        .and_then(|diagnostics| match diagnostics.lock() {
            Ok(mut diagnostics) => diagnostics.take(),
            Err(poisoned) => {
                report_fallback("diagnostics guard lock was poisoned during shutdown");
                poisoned.into_inner().take()
            }
        });
    drop(guard);
}

fn initialize_once() {
    let root = diagnostics_root();
    let log_dir = root.join("logs");
    let crash_dir = root.join("crashes");
    install_panic_hook(crash_dir.clone());

    if let Err(error) = std::fs::create_dir_all(&log_dir) {
        report_fallback(&format!(
            "cannot create diagnostics directory {}: {error}",
            log_dir.display()
        ));
        return;
    }
    if let Err(error) = std::fs::create_dir_all(&crash_dir) {
        report_fallback(&format!(
            "cannot create crash directory {}: {error}",
            crash_dir.display()
        ));
    }

    retention::clean_expired_logs(&log_dir, &crash_dir, std::time::SystemTime::now());

    let main_writer = DailyFileWriter::new(log_dir.clone(), "studio");
    let (main_writer, log_guard) = tracing_appender::non_blocking(main_writer);
    let error_writer = SyncErrorMakeWriter::new(log_dir.clone());
    let filter = configured_filter();
    let initialized = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_writer(main_writer.and(error_writer.with_max_level(Level::ERROR)))
        .try_init()
        .is_ok();

    if !initialized {
        report_fallback("cannot install the global tracing subscriber");
        drop(log_guard);
        return;
    }

    let retention = RetentionGuard::spawn(log_dir, crash_dir);
    let diagnostics = DiagnosticsGuard {
        retention,
        log_writer: Some(log_guard),
    };
    if DIAGNOSTICS.set(Mutex::new(Some(diagnostics))).is_err() {
        report_fallback("diagnostics guard was already initialized");
    }

    tracing::info!(
        application = "Pure Studio",
        app_version = env!("CARGO_PKG_VERSION"),
        protocol_version = pl_protocol::THREAD_SCHEMA_VERSION,
        "Studio diagnostics initialized"
    );
}

fn configured_filter() -> EnvFilter {
    let cli_level = std::env::var(CLI_LOG_LEVEL_ENV).ok();
    let rust_log = std::env::var(EnvFilter::DEFAULT_ENV).ok();
    filter_from_sources(cli_level.as_deref(), rust_log.as_deref())
}

fn filter_from_sources(cli_level: Option<&str>, rust_log: Option<&str>) -> EnvFilter {
    if let Some(level) = cli_level {
        if is_supported_log_level(level) {
            return EnvFilter::new(level);
        }
        report_fallback(&format!(
            "ignoring invalid {CLI_LOG_LEVEL_ENV} value {level:?}"
        ));
    }
    if let Some(directives) = rust_log {
        match EnvFilter::try_new(directives) {
            Ok(filter) => return filter,
            Err(error) => report_fallback(&format!("ignoring invalid RUST_LOG: {error}")),
        }
    }
    EnvFilter::new(DEFAULT_LOG_FILTER)
}

fn is_supported_log_level(value: &str) -> bool {
    matches!(value, "error" | "warn" | "info" | "debug" | "trace")
}

fn install_panic_hook(crash_dir: PathBuf) {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        persist_panic(&crash_dir, info);
        tracing::error!(
            app_version = env!("CARGO_PKG_VERSION"),
            protocol_version = pl_protocol::THREAD_SCHEMA_VERSION,
            panic = %info,
            "Rust panic"
        );
        previous_hook(info);
    }));
}

fn persist_panic(crash_dir: &Path, info: &std::panic::PanicHookInfo<'_>) {
    if let Err(error) = std::fs::create_dir_all(crash_dir) {
        report_fallback(&format!(
            "cannot create panic directory {}: {error}",
            crash_dir.display()
        ));
        return;
    }
    let marker = crash_dir.join(format!(
        "crash-{}-{}.log",
        unix_seconds(),
        std::process::id()
    ));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new().create(true).append(true).open(&marker)?;
        writeln!(
            file,
            "appVersion={}\nprotocolVersion={}\nthread={:?}\npanic={info}\nbacktrace={}",
            env!("CARGO_PKG_VERSION"),
            pl_protocol::THREAD_SCHEMA_VERSION,
            std::thread::current().name(),
            Backtrace::force_capture()
        )?;
        file.flush()?;
        file.sync_all()
    })();
    if let Err(error) = result {
        report_fallback(&format!(
            "cannot persist panic file {}: {error}",
            marker.display()
        ));
    }
}

fn diagnostics_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("Pure Studio"))
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .map(|path| path.join(".pure").join("studio"))
        })
        .unwrap_or_else(|| PathBuf::from(".").join("pure-studio-diagnostics"))
}

fn current_date() -> Date {
    OffsetDateTime::now_local()
        .unwrap_or_else(|_| OffsetDateTime::now_utc())
        .date()
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn report_fallback(message: &str) {
    let message = format!("Pure Studio diagnostics: {message}");
    let _ = writeln!(std::io::stderr().lock(), "{message}");
    report_windows_debug(&message);
}

#[cfg(windows)]
fn report_windows_debug(message: &str) {
    use std::os::windows::ffi::OsStrExt;

    let wide = std::ffi::OsStr::new(message)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: `wide` is a live, NUL-terminated UTF-16 buffer for the duration of the call.
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringW(wide.as_ptr());
    }
}

#[cfg(not(windows))]
fn report_windows_debug(_message: &str) {}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::time::SystemTime;

    use anyhow::{Context, Result, bail};

    use super::*;

    #[test]
    fn child_process_verifies_filter_priority_error_mirror_and_flush() -> Result<()> {
        let default_root = child_root("default-filter")?;
        let default = run_child(
            "diagnostics::tests::diagnostics_child_emits_levels",
            &default_root,
            None,
            None,
        )?;
        if !default.status.success() {
            bail!(
                "default diagnostics child failed: {}",
                String::from_utf8_lossy(&default.stderr)
            );
        }
        let default_logs = read_main_logs(&default_root.join("Pure Studio").join("logs"))?;
        let default_errors = read_logs(&default_root.join("Pure Studio").join("logs"), "error-")?;
        assert!(default_logs.contains("FILTER_WARN"));
        assert!(default_logs.contains("FILTER_ERROR"));
        assert!(!default_logs.contains("FILTER_INFO"));
        assert!(!default_logs.contains("FILTER_TRACE"));
        assert!(default_errors.contains("FILTER_ERROR"));

        let rust_log_root = child_root("rust-log-filter")?;
        let rust_log = run_child(
            "diagnostics::tests::diagnostics_child_emits_levels",
            &rust_log_root,
            None,
            Some("info"),
        )?;
        if !rust_log.status.success() {
            bail!(
                "RUST_LOG diagnostics child failed: {}",
                String::from_utf8_lossy(&rust_log.stderr)
            );
        }
        let rust_log_contents = read_main_logs(&rust_log_root.join("Pure Studio").join("logs"))?;
        assert!(rust_log_contents.contains("FILTER_INFO"));
        assert!(!rust_log_contents.contains("FILTER_TRACE"));

        let cli_root = child_root("cli-filter")?;
        let cli = run_child(
            "diagnostics::tests::diagnostics_child_emits_levels",
            &cli_root,
            Some("trace"),
            Some("error"),
        )?;
        if !cli.status.success() {
            bail!(
                "CLI diagnostics child failed: {}",
                String::from_utf8_lossy(&cli.stderr)
            );
        }
        let cli_logs = read_main_logs(&cli_root.join("Pure Studio").join("logs"))?;
        assert!(cli_logs.contains("FILTER_TRACE"));
        assert!(cli_logs.contains("FILTER_INFO"));

        std::fs::remove_dir_all(default_root)?;
        std::fs::remove_dir_all(rust_log_root)?;
        std::fs::remove_dir_all(cli_root)?;
        Ok(())
    }

    #[test]
    fn child_process_persists_panic_to_crash_and_error_logs() -> Result<()> {
        let root = child_root("panic")?;
        let output = run_child(
            "diagnostics::tests::diagnostics_child_panics",
            &root,
            Some("trace"),
            None,
        )?;
        assert!(!output.status.success());

        let diagnostics_root = root.join("Pure Studio");
        let crash = read_logs(&diagnostics_root.join("crashes"), "crash-")?;
        let errors = read_logs(&diagnostics_root.join("logs"), "error-")?;
        assert!(crash.contains("DIAGNOSTICS_PANIC_FIXTURE"));
        assert!(crash.contains("backtrace="));
        assert!(errors.contains("Rust panic"));

        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    #[ignore = "spawned by the diagnostics parent test"]
    fn diagnostics_child_emits_levels() {
        initialize();
        tracing::trace!(marker = "FILTER_TRACE", "diagnostics filter fixture");
        tracing::info!(marker = "FILTER_INFO", "diagnostics filter fixture");
        tracing::warn!(marker = "FILTER_WARN", "diagnostics filter fixture");
        tracing::error!(marker = "FILTER_ERROR", "diagnostics filter fixture");
        shutdown();
    }

    #[test]
    #[ignore = "spawned by the diagnostics parent test"]
    fn diagnostics_child_panics() {
        initialize();
        panic!("DIAGNOSTICS_PANIC_FIXTURE");
    }

    fn child_root(label: &str) -> Result<PathBuf> {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "pure-studio-diagnostics-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root)?;
        Ok(root)
    }

    fn run_child(
        test_name: &str,
        local_app_data: &Path,
        cli_level: Option<&str>,
        rust_log: Option<&str>,
    ) -> Result<std::process::Output> {
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args(["--ignored", "--exact", test_name, "--nocapture"])
            .env("LOCALAPPDATA", local_app_data)
            .env_remove(CLI_LOG_LEVEL_ENV)
            .env_remove(EnvFilter::DEFAULT_ENV);
        if let Some(level) = cli_level {
            command.env(CLI_LOG_LEVEL_ENV, level);
        }
        if let Some(filter) = rust_log {
            command.env(EnvFilter::DEFAULT_ENV, filter);
        }
        command
            .output()
            .with_context(|| format!("failed to run diagnostics child {test_name}"))
    }

    fn read_logs(directory: &Path, prefix: &str) -> Result<String> {
        let mut contents = String::new();
        for entry in std::fs::read_dir(directory)
            .with_context(|| format!("failed to read {}", directory.display()))?
        {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(prefix) && name.ends_with(".log") {
                contents.push_str(&std::fs::read_to_string(entry.path())?);
            }
        }
        Ok(contents)
    }

    fn read_main_logs(directory: &Path) -> Result<String> {
        read_logs(directory, "studio.")
    }
}
