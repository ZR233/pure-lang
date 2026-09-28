use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use rolling_file::{BasicRollingFileAppender, RollingConditionBasic};
use tracing_subscriber::fmt::MakeWriter;

use super::report_fallback;

const MAX_LOG_BYTES: u64 = 16 * 1024 * 1024;
const ARCHIVED_LOG_FILES: usize = 7;

/// Rotation and numbered-file cleanup are owned by the library, shared by both Rust log streams.
pub(super) struct RollingLogWriter {
    file: Option<BasicRollingFileAppender>,
}

impl RollingLogWriter {
    pub(super) fn new(directory: PathBuf, prefix: &'static str) -> Self {
        let file = BasicRollingFileAppender::new(
            directory.join(format!("{prefix}.log")),
            RollingConditionBasic::new().daily().max_size(MAX_LOG_BYTES),
            ARCHIVED_LOG_FILES,
        )
        .map_err(|error| {
            report_fallback(&format!(
                "cannot initialize rolling log in {}: {error}",
                directory.display()
            ));
            error
        })
        .ok();
        Self { file }
    }
}

impl Write for RollingLogWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self.file.as_mut() {
            Some(file) => file.write(buffer).or_else(|error| {
                report_fallback(&format!("cannot append rolling log: {error}"));
                std::io::stderr().write(buffer)
            }),
            None => std::io::stderr().write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.flush(),
            None => std::io::stderr().flush(),
        }
    }
}

#[derive(Clone)]
pub(super) struct SyncErrorMakeWriter(Arc<Mutex<RollingLogWriter>>);

impl SyncErrorMakeWriter {
    pub(super) fn new(directory: PathBuf) -> Self {
        Self(Arc::new(Mutex::new(RollingLogWriter::new(
            directory, "error",
        ))))
    }
}

impl<'writer> MakeWriter<'writer> for SyncErrorMakeWriter {
    type Writer = SyncErrorWriter<'writer>;

    fn make_writer(&'writer self) -> Self::Writer {
        SyncErrorWriter(
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

pub(super) struct SyncErrorWriter<'a>(MutexGuard<'a, RollingLogWriter>);

impl Write for SyncErrorWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.0.write(buffer)?;
        self.0.flush()?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
