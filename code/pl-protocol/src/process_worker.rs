//! Private-host process supervision transport, separate from model tool results.

use serde::{Deserialize, Serialize};

/// The host must validate Ready before sending Start; mismatches must never execute a command.
pub const PROCESS_WORKER_PROTOCOL_VERSION: u32 = 2;

/// Maximum JSON bootstrap payload, preceded by a four-byte big-endian byte count.
pub const PROCESS_WORKER_MAX_CONFIGURATION_BYTES: usize = 1024 * 1024;

/// Unix command configuration on the private control socket. Bytes preserve non-UTF-8
/// paths and arguments; environment values must never enter diagnostics or worker argv.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessWorkerConfiguration {
    pub program: Vec<u8>,
    pub arguments: Vec<Vec<u8>>,
    pub directory: Option<Vec<u8>>,
    pub environment: Vec<ProcessWorkerEnvironment>,
}

impl std::fmt::Debug for ProcessWorkerConfiguration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessWorkerConfiguration")
            .field("argument_count", &self.arguments.len())
            .field("environment_count", &self.environment.len())
            .finish_non_exhaustive()
    }
}

/// One literal environment override or removal, never applied to the supervisor.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessWorkerEnvironment {
    pub key: Vec<u8>,
    pub value: Option<Vec<u8>>,
}

impl std::fmt::Debug for ProcessWorkerEnvironment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessWorkerEnvironment")
            .finish_non_exhaustive()
    }
}

/// Single-byte commands on a dedicated worker control socket. EOF also requests cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ProcessWorkerCommand {
    Start = 0,
    Cancel = 1,
    RetryCleanup = 2,
}

impl TryFrom<u8> for ProcessWorkerCommand {
    type Error = UnknownProcessWorkerCommand;

    fn try_from(opcode: u8) -> Result<Self, Self::Error> {
        match opcode {
            0 => Ok(Self::Start),
            1 => Ok(Self::Cancel),
            2 => Ok(Self::RetryCleanup),
            opcode => Err(UnknownProcessWorkerCommand { opcode }),
        }
    }
}

/// An unknown control opcode, not a model-visible instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("unknown process worker opcode {opcode}")]
pub struct UnknownProcessWorkerCommand {
    pub opcode: u8,
}

/// Newline-delimited JSON events on the dedicated control socket, never stdout/stderr.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProcessWorkerEvent {
    Ready {
        protocol_version: u32,
    },
    StoppedBeforeStart,
    Started {
        pid: u32,
    },
    StartFailed {
        os_error: Option<i32>,
    },
    CleanupFailed {
        operation: String,
        os_error: Option<i32>,
    },
    Exited {
        exit_code: Option<i32>,
        signal: Option<i32>,
    },
}
