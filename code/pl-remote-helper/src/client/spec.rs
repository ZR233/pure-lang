use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use super::WorkerClientError;
use pl_protocol::process_worker::{
    PROCESS_WORKER_MAX_CONFIGURATION_BYTES, ProcessWorkerConfiguration, ProcessWorkerEnvironment,
};

/// Business process arguments and environment, never applied to the supervisor executable.
pub struct ProcessCommand {
    program: OsString,
    arguments: Vec<OsString>,
    directory: Option<PathBuf>,
    environment: BTreeMap<OsString, Option<OsString>>,
}

impl std::fmt::Debug for ProcessCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessCommand")
            .field("program", &self.program)
            .field("argument_count", &self.arguments.len())
            .field("directory", &self.directory)
            .field("environment_keys", &self.environment.keys())
            .finish_non_exhaustive()
    }
}

impl ProcessCommand {
    /// Selects the business executable, resolved by the worker's inherited PATH.
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            arguments: Vec::new(),
            directory: None,
            environment: BTreeMap::new(),
        }
    }

    /// Appends literal arguments without shell expansion.
    pub fn args(mut self, arguments: impl IntoIterator<Item = impl Into<OsString>>) -> Self {
        self.arguments.extend(arguments.into_iter().map(Into::into));
        self
    }

    /// Sets the business working directory without moving the supervisor.
    pub fn current_dir(mut self, directory: impl Into<PathBuf>) -> Self {
        self.directory = Some(directory.into());
        self
    }

    /// Overrides one business environment variable.
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.environment.insert(key.into(), Some(value.into()));
        self
    }

    /// Removes an inherited variable from the business process.
    pub fn env_remove(mut self, key: impl Into<OsString>) -> Self {
        self.environment.insert(key.into(), None);
        self
    }

    pub(super) fn encode(self) -> Result<Vec<u8>, WorkerClientError> {
        let configuration = ProcessWorkerConfiguration {
            program: self.program.into_vec(),
            arguments: self.arguments.into_iter().map(OsString::into_vec).collect(),
            directory: self.directory.map(|path| path.into_os_string().into_vec()),
            environment: self
                .environment
                .into_iter()
                .map(|(key, value)| ProcessWorkerEnvironment {
                    key: key.into_vec(),
                    value: value.map(OsString::into_vec),
                })
                .collect(),
        };
        let mut output = ConfigurationBuffer(Vec::new());
        if let Err(error) = serde_json::to_writer(&mut output, &configuration) {
            if error.is_io() {
                return Err(WorkerClientError::Protocol(
                    "configuration exceeds size limit",
                ));
            }
            return Err(error.into());
        }
        Ok(output.0)
    }
}

struct ConfigurationBuffer(Vec<u8>);

impl std::io::Write for ConfigurationBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > PROCESS_WORKER_MAX_CONFIGURATION_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("configuration exceeds size limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
