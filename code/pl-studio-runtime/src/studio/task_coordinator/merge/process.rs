use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::super::MergeVerificationFailureKind;

#[cfg(not(test))]
const CHECK_TIMEOUT: Duration = Duration::from_secs(300);
#[cfg(test)]
const CHECK_TIMEOUT: Duration = Duration::from_secs(1);

pub(super) struct ProcessOutput {
    pub(super) success: bool,
    pub(super) exit_code: Option<i32>,
    pub(super) combined: String,
}

pub(super) struct ProcessFailure {
    pub(super) kind: MergeVerificationFailureKind,
    pub(super) message: String,
}

pub(super) async fn run_process(
    cwd: impl AsRef<Path>,
    program: &str,
    arguments: Vec<String>,
) -> Result<ProcessOutput, ProcessFailure> {
    let cwd = cwd.as_ref().to_path_buf();
    let program = program.to_string();
    tokio::task::spawn_blocking(move || run_blocking(cwd, program, arguments))
        .await
        .map_err(|error| ProcessFailure {
            kind: MergeVerificationFailureKind::RuntimeFailed,
            message: format!("merge verifier process task failed: {error}"),
        })?
}

fn run_blocking(
    cwd: PathBuf,
    program: String,
    arguments: Vec<String>,
) -> Result<ProcessOutput, ProcessFailure> {
    let mut command = Command::new(&program);
    command
        .current_dir(cwd)
        .args(&arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::process::configure_background_std_command(&mut command);
    let child = command.spawn().map_err(|error| ProcessFailure {
        kind: MergeVerificationFailureKind::StartFailed,
        message: format!("failed to start {program}: {error}"),
    })?;
    let mut child = KillOnDropChild::new(child);
    let stdout = child
        .child
        .stdout
        .take()
        .ok_or_else(|| runtime_failure("process stdout is missing"))?;
    let stderr = child
        .child
        .stderr
        .take()
        .ok_or_else(|| runtime_failure("process stderr is missing"))?;
    let stdout_reader = std::thread::spawn(move || read_pipe(stdout));
    let stderr_reader = std::thread::spawn(move || read_pipe(stderr));
    let deadline = Instant::now() + CHECK_TIMEOUT;
    let status = loop {
        if let Some(status) = child.child.try_wait().map_err(|error| ProcessFailure {
            kind: MergeVerificationFailureKind::RuntimeFailed,
            message: format!("failed to poll verifier: {error}"),
        })? {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill_and_wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(ProcessFailure {
                kind: MergeVerificationFailureKind::TimedOut,
                message: format!("{program} timed out after {}s", CHECK_TIMEOUT.as_secs()),
            });
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    child.completed = true;
    let stdout = stdout_reader
        .join()
        .map_err(|_| runtime_failure("verifier stdout reader panicked"))?
        .map_err(|error| runtime_failure(format!("failed to read verifier stdout: {error}")))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| runtime_failure("verifier stderr reader panicked"))?
        .map_err(|error| runtime_failure(format!("failed to read verifier stderr: {error}")))?;
    let mut combined = String::from_utf8_lossy(&stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
    if !stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&stderr);
    }
    Ok(ProcessOutput {
        success: status.success(),
        exit_code: status.code(),
        combined,
    })
}

fn runtime_failure(message: impl Into<String>) -> ProcessFailure {
    ProcessFailure {
        kind: MergeVerificationFailureKind::RuntimeFailed,
        message: message.into(),
    }
}

fn read_pipe(mut pipe: impl Read) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    pipe.read_to_end(&mut output)?;
    Ok(output)
}

struct KillOnDropChild {
    child: Child,
    completed: bool,
}

impl KillOnDropChild {
    fn new(child: Child) -> Self {
        Self {
            child,
            completed: false,
        }
    }

    fn kill_and_wait(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.completed = true;
    }
}

impl Drop for KillOnDropChild {
    fn drop(&mut self) {
        if !self.completed {
            self.kill_and_wait();
        }
    }
}
