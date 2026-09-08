use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::process::Command;

use pl_protocol::process_worker::{
    PROCESS_WORKER_MAX_CONFIGURATION_BYTES, ProcessWorkerConfiguration,
};

use super::{channel::Channel, kernel::Kernel};

pub(super) fn read_command(channel: &mut Channel, kernel: &Kernel) -> io::Result<Command> {
    let mut length = [0; 4];
    channel.read_bootstrap(kernel, &mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > PROCESS_WORKER_MAX_CONFIGURATION_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid configuration length",
        ));
    }
    let mut bytes = vec![0; length];
    channel.read_bootstrap(kernel, &mut bytes)?;
    let configuration: ProcessWorkerConfiguration = serde_json::from_slice(&bytes)
        // Decode diagnostics can contain input; configuration is private, not diagnostic text.
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid configuration schema"))?;
    if configuration.program.is_empty()
        || configuration.program.contains(&0)
        || configuration
            .arguments
            .iter()
            .any(|value| value.contains(&0))
        || configuration
            .directory
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.contains(&0))
        || configuration.environment.iter().any(|entry| {
            entry.key.is_empty()
                || entry.key.contains(&0)
                || entry.key.contains(&b'=')
                || entry.value.as_ref().is_some_and(|value| value.contains(&0))
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid command configuration",
        ));
    }
    let mut command = Command::new(OsString::from_vec(configuration.program));
    command.args(configuration.arguments.into_iter().map(OsString::from_vec));
    if let Some(directory) = configuration.directory {
        command.current_dir(OsString::from_vec(directory));
    }
    for entry in configuration.environment {
        let key = OsString::from_vec(entry.key);
        match entry.value {
            Some(value) => {
                command.env(key, OsString::from_vec(value));
            }
            None => {
                command.env_remove(key);
            }
        }
    }
    Ok(command)
}
