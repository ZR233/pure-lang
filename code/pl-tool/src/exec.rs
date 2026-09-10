//! Command tool declarations and Thread-owned implementations.
mod progress;
mod thread;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
pub use thread::{CommandAccess, CommandOutputArchive, ThreadExecTool};
pub const TOOL_EXEC: &str = "exec";
pub const TOOL_WRITE_STDIN: &str = "write_stdin";
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_MODEL_OUTPUT_CHARS: usize = 64 * 1024;
/// `exec` 的结构化输入。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecInput {
    /// The shell command to execute.
    pub command: String,
    /// Optional working directory. Use `.` for the workspace root or a workspace-relative path
    /// such as `src`. SSH execution rejects absolute paths; local absolute paths remain subject to
    /// the active permission and workspace policy.
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    /// Optional total timeout in seconds (default: 60).
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub timeout_seconds: Option<u64>,
    /// Maximum stdout/stderr characters returned to the model.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub max_output_chars: Option<usize>,
}

/// `write_stdin` 的结构化输入。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteStdinInput {
    /// Task id returned by exec's acceptance receipt.
    pub task_id: String,
    /// Nonempty text to write to stdin. This tool does not wait or poll.
    #[schemars(length(min = 1))]
    pub chars: String,
    /// Maximum stdout/stderr characters returned to the model.
    #[serde(default)]
    #[schemars(range(min = 1))]
    pub max_output_chars: Option<usize>,
}
