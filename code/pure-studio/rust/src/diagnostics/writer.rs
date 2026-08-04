use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use time::Date;
use tracing_subscriber::fmt::MakeWriter;

use super::{current_date, report_fallback};

pub(super) struct DailyFileWriter {
    directory: PathBuf,
    prefix: &'static str,
    active_date: Option<Date>,
    file: Option<BufWriter<File>>,
}

impl DailyFileWriter {
    pub(super) fn new(directory: PathBuf, prefix: &'static str) -> Self {
        Self {
            directory,
            prefix,
            active_date: None,
            file: None,
        }
    }

    fn active_file(&mut self) -> io::Result<&mut BufWriter<File>> {
        let date = current_date();
        if self.active_date != Some(date) {
            if let Some(file) = self.file.as_mut() {
                file.flush()?;
            }
            let path = self.directory.join(format!("{}-{date}.log", self.prefix));
            let file = OpenOptions::new().create(true).append(true).open(path)?;
            self.file = Some(BufWriter::new(file));
            self.active_date = Some(date);
        }
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("daily log file was not initialized"))
    }

    fn write_fallback(&self, buffer: &[u8], error: &io::Error) -> io::Result<usize> {
        report_fallback(&format!("cannot append the daily log: {error}"));
        std::io::stderr().write(buffer)
    }
}

impl Write for DailyFileWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self.active_file() {
            Ok(file) => file.write(buffer),
            Err(error) => self.write_fallback(buffer, &error),
        }
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
