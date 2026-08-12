use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::MakeWriter;

use super::{current_date, report_fallback};

pub(super) struct DailyFileWriter {
    file: Option<RollingFileAppender>,
}

impl DailyFileWriter {
    pub(super) fn new(directory: PathBuf, prefix: &'static str) -> Self {
        let file = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix(prefix)
            .filename_suffix("log")
            .build(&directory)
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

    fn write_fallback(buffer: &[u8], error: &dyn std::fmt::Display) -> io::Result<usize> {
        report_fallback(&format!("cannot append the daily log: {error}"));
        std::io::stderr().write(buffer)
    }

    fn write_file(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self.file.as_mut() {
            Some(file) => file
                .write(buffer)
                .or_else(|error| Self::write_fallback(buffer, &error)),
            None => Self::write_fallback(buffer, &"rolling log is unavailable"),
        }
    }
}

impl Write for DailyFileWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.write_file(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

#[derive(Clone)]
pub(super) struct SyncErrorMakeWriter {
    directory: PathBuf,
}

impl SyncErrorMakeWriter {
    pub(super) fn new(directory: PathBuf) -> Self {
        Self { directory }
    }
}

impl<'writer> MakeWriter<'writer> for SyncErrorMakeWriter {
    type Writer = SyncErrorWriter;

    fn make_writer(&'writer self) -> Self::Writer {
        let path = self.directory.join(format!("error-{}.log", current_date()));
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => SyncErrorWriter { file: Some(file) },
            Err(error) => {
                report_fallback(&format!(
                    "cannot open synchronous error log {}: {error}",
                    path.display()
                ));
                SyncErrorWriter { file: None }
            }
        }
    }
}

pub(super) struct SyncErrorWriter {
    file: Option<File>,
}

impl Write for SyncErrorWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let result = match self.file.as_mut() {
            Some(file) => file.write(buffer),
            None => std::io::stderr().write(buffer),
        };
        if let Some(file) = self.file.as_mut() {
            file.flush()?;
        }
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.flush(),
            None => std::io::stderr().flush(),
        }
    }
}
