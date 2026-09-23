//! anywork process diagnostics and durable log lifecycle.

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

const CLI_LOG_LEVEL_ENV: &str = "ANYWORK_LOG_LEVEL";
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
    tracing::info!(application = "anywork", "Studio diagnostics shutting down");
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
        application = "anywork",
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
        .map(|path| path.join("anywork"))
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .map(|path| path.join(".anywork").join("studio"))
        })
        .unwrap_or_else(|| PathBuf::from(".").join("anywork-diagnostics"))
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
    let message = format!("anywork diagnostics: {message}");
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
