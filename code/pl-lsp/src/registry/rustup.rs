use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::process::spawn_background;
use crate::server_definition::LspServerDefinition;
use crate::server_definition::RUST_ANALYZER_ID;

pub(crate) const RUST_ANALYZER_COMMAND: &str = "rust-analyzer";
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const RUSTUP_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub(crate) enum ProbeError {
    MissingCommand,
    MissingRustupComponent,
    Failed(String),
}

pub(crate) fn rust_analyzer_definition(
    workspace_root: &Path,
    command: &str,
) -> LspServerDefinition {
    LspServerDefinition {
        id: RUST_ANALYZER_ID.to_string(),
        display_name: "rust-analyzer".to_string(),
        command: command.to_string(),
        args: Vec::new(),
        extensions: vec![".rs".to_string()],
        language_ids: vec!["rust".to_string()],
        workspace_root: workspace_root.to_path_buf(),
    }
}

pub(crate) async fn probe_rust_analyzer(command: &str) -> Result<String, ProbeError> {
    probe_command(command).await
}

async fn probe_command(command: &str) -> Result<String, ProbeError> {
    let output =
        run_command_capture(command, &["--version"], PROBE_TIMEOUT, "version check").await?;
    Ok(String::from_utf8_lossy(&output).trim().to_string())
}

pub(crate) async fn rustup_is_available() -> bool {
    run_command_capture("rustup", &["--version"], PROBE_TIMEOUT, "rustup check")
        .await
        .is_ok()
}

pub(crate) async fn install_rust_analyzer_component() -> Result<(), ProbeError> {
    run_command_capture(
        "rustup",
        &["component", "add", "rust-analyzer"],
        RUSTUP_TIMEOUT,
        "rustup component add",
    )
    .await
    .map(|_| ())
    .map_err(|error| match &error {
        ProbeError::Failed(_) => ProbeError::MissingRustupComponent,
        _ => error,
    })
}

async fn run_command_capture(
    command: &str,
    args: &[&str],
    timeout: Duration,
    timeout_message: &str,
) -> Result<Vec<u8>, ProbeError> {
    let mut command_process = Command::new(command);
    command_process
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn_background(command_process).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ProbeError::MissingCommand
        } else {
            ProbeError::Failed(error.to_string())
        }
    })?;
    let stdout = child.stdout().take();
    let stderr = child.stderr().take();
    let stdout_task = tokio::spawn(read_child_output(stdout));
    let stderr_task = tokio::spawn(read_child_output(stderr));
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(error)) => return Err(ProbeError::Failed(error.to_string())),
        Err(_) => {
            let kill = child.kill();
            let _ = std::pin::Pin::from(kill).await;
            return Err(ProbeError::Failed(timeout_message.to_string()));
        }
    };
    let stdout = stdout_task.await.unwrap_or_else(|e| {
        tracing::warn!("rustup stdout task failed: {e}");
        Vec::new()
    });
    let stderr = stderr_task.await.unwrap_or_default();
    if status.success() {
        return Ok(if stdout.is_empty() { stderr } else { stdout });
    }
    let msg = command_failure_message(status, &stdout, &stderr);
    if is_rustup_missing_component_error(std::str::from_utf8(&stderr).unwrap_or_default()) {
        return Err(ProbeError::MissingRustupComponent);
    }
    Err(ProbeError::Failed(msg))
}

fn command_failure_message(status: ExitStatus, stdout: &[u8], stderr: &[u8]) -> String {
    let mut parts = Vec::new();
    parts.push(format!("command failed with {status}"));
    let stdout = String::from_utf8_lossy(stdout);
    if !stdout.is_empty() {
        parts.push(format!("stdout: {stdout}"));
    }
    let stderr = String::from_utf8_lossy(stderr);
    if !stderr.is_empty() {
        parts.push(format!("stderr: {stderr}"));
    }
    parts.join("\n")
}

pub(crate) fn missing_rust_analyzer_message() -> String {
    "rust-analyzer command not found; use the explicit repair action when rustup owns the component"
        .to_string()
}

pub(crate) fn is_rustup_missing_component_error(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    stderr.contains("unknown binary")
        && (stderr.contains("rust-analyzer") || stderr.contains("rust_analyzer"))
}

async fn read_child_output(stream: Option<impl tokio::io::AsyncRead + Unpin>) -> Vec<u8> {
    let Some(mut stream) = stream else {
        return Vec::new();
    };
    let mut output = Vec::new();
    let _ = stream.read_to_end(&mut output).await;
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rustup_unknown_binary_is_missing_component() {
        assert!(is_rustup_missing_component_error(
            "error: Unknown binary 'rust-analyzer.exe' in official toolchain 'stable-x86_64-pc-windows-msvc'."
        ));
        assert!(!is_rustup_missing_component_error(
            "error: Unknown binary 'cargo-miri.exe' in official toolchain"
        ));
    }
}
