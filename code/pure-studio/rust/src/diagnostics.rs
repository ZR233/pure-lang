use std::backtrace::Backtrace;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Once, OnceLock};

use tracing_subscriber::EnvFilter;

static INITIALIZE: Once = Once::new();
static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

pub(crate) fn initialize() {
    INITIALIZE.call_once(|| {
        let root = diagnostics_root();
        let log_dir = root.join("logs");
        if let Err(error) = std::fs::create_dir_all(&log_dir) {
            let _ = writeln!(
                std::io::stderr().lock(),
                "Pure Studio cannot create diagnostics directory {}: {error}",
                log_dir.display()
            );
            install_panic_hook(root.join("crashes"));
            return;
        }

        let appender = tracing_appender::rolling::daily(&log_dir, "pure-studio.log");
        let (writer, guard) = tracing_appender::non_blocking(appender);
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info,pl_core::session_event=debug"));
        let initialized = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(true)
            .with_writer(writer)
            .try_init()
            .is_ok();
        if initialized {
            let _ = LOG_GUARD.set(guard);
        }
        install_panic_hook(root.join("crashes"));
        tracing::info!(
            application = "Pure Studio",
            app_version = env!("CARGO_PKG_VERSION"),
            protocol_version = pl_protocol::SESSION_EVENT_SCHEMA_VERSION,
            "Studio diagnostics initialized"
        );
    });
}

fn install_panic_hook(crash_dir: PathBuf) {
    drop(std::panic::take_hook());
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::fs::create_dir_all(&crash_dir);
        let marker = crash_dir.join(format!(
            "rust-panic-{}-{}.log",
            unix_seconds(),
            std::process::id()
        ));
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(marker) {
            let _ = writeln!(
                file,
                "appVersion={}\nprotocolVersion={}\nthread={:?}\npanic={info}\nbacktrace={}",
                env!("CARGO_PKG_VERSION"),
                pl_protocol::SESSION_EVENT_SCHEMA_VERSION,
                std::thread::current().name(),
                Backtrace::force_capture()
            );
        }
        tracing::error!(
            app_version = env!("CARGO_PKG_VERSION"),
            protocol_version = pl_protocol::SESSION_EVENT_SCHEMA_VERSION,
            panic = %info,
            "Rust panic"
        );
    }));
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

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
