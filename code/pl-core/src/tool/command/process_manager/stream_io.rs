use std::sync::Arc;

use pl_protocol::PureError;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{ChildStderr, ChildStdout};

use super::lifecycle::apply_transition;
use super::{
    CommandOutputStream, CommandProcessEntry, CommandProcessTransition, StreamKind, tool_error,
};

pub(super) async fn read_stdout(entry: Arc<CommandProcessEntry>, stdout: ChildStdout) {
    read_stream(entry, stdout, StreamKind::Stdout).await;
}

pub(super) async fn read_stderr(entry: Arc<CommandProcessEntry>, stderr: ChildStderr) {
    read_stream(entry, stderr, StreamKind::Stderr).await;
}

async fn read_stream<R>(entry: Arc<CommandProcessEntry>, mut reader: R, stream: StreamKind)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(n) => {
                let chunk = &buffer[..n];
                let revision = {
                    let mut state = entry.state.lock().await;
                    state.record_output(stream, chunk)
                };
                if let Some(observer) = &entry.output_observer {
                    observer.output_chunk(stream.into(), chunk, revision);
                }
                if let Err(error) = append_output_chunk(&entry, stream, chunk).await {
                    let mut state = entry.state.lock().await;
                    state.record_output_error(error);
                }
                entry.notify.notify_waiters();
            }
            Err(error) => {
                let mut state = entry.state.lock().await;
                state.record_output_error(format!("failed to read process output: {error}"));
                break;
            }
        }
    }
    apply_transition(&entry, CommandProcessTransition::StreamClosed(stream)).await;
}

impl From<StreamKind> for CommandOutputStream {
    fn from(value: StreamKind) -> Self {
        match value {
            StreamKind::Stdout => Self::Stdout,
            StreamKind::Stderr => Self::Stderr,
        }
    }
}

pub(super) async fn prepare_output_file(
    output_file: &std::path::Path,
    command: &str,
    working_directory: &std::path::Path,
) -> Result<(), PureError> {
    if let Some(parent) = output_file.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            tool_error(
                "exec",
                format!("failed to create output directory: {error}"),
            )
        })?;
    }
    let header = format!(
        "=== COMMAND ===\n{command}\n\n=== CWD ===\n{}\n\n",
        working_directory.display()
    );
    tokio::fs::write(output_file, header.as_bytes())
        .await
        .map_err(|error| tool_error("exec", format!("failed to write output file: {error}")))
}

async fn append_output_chunk(
    entry: &CommandProcessEntry,
    stream: StreamKind,
    chunk: &[u8],
) -> Result<(), String> {
    let _guard = entry.output_file_lock.lock().await;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(entry.output_target.capture_file())
        .await
        .map_err(|error| format!("failed to open output file: {error}"))?;
    let label = match stream {
        StreamKind::Stdout => "STDOUT",
        StreamKind::Stderr => "STDERR",
    };
    file.write_all(format!("=== {label} ===\n").as_bytes())
        .await
        .map_err(|error| format!("failed to write output label: {error}"))?;
    file.write_all(chunk)
        .await
        .map_err(|error| format!("failed to write output chunk: {error}"))?;
    if !chunk.ends_with(b"\n") {
        file.write_all(b"\n")
            .await
            .map_err(|error| format!("failed to finish output chunk: {error}"))?;
    }
    if let Some(stream_file) = match stream {
        StreamKind::Stdout => entry.output_target.stdout_capture_file(),
        StreamKind::Stderr => entry.output_target.stderr_capture_file(),
    } {
        let mut stream_capture = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(stream_file)
            .await
            .map_err(|error| format!("failed to open stream capture file: {error}"))?;
        stream_capture
            .write_all(chunk)
            .await
            .map_err(|error| format!("failed to write stream capture: {error}"))?;
    }
    Ok(())
}
