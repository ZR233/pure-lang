use std::sync::Arc;

use tokio::io::AsyncReadExt;

use super::lifecycle::apply_transition;
use super::{CommandOutputStream, CommandProcessEntry, CommandProcessTransition, StreamKind};
use crate::tool::command::{CommandBackend, CommandCaptureStream, CommandReader};

pub(super) async fn read_stdout<B>(
    entry: Arc<CommandProcessEntry>,
    stdout: CommandReader,
    backend: Arc<B>,
) where
    B: CommandBackend,
{
    read_stream(entry, stdout, StreamKind::Stdout, backend).await;
}

pub(super) async fn read_stderr<B>(
    entry: Arc<CommandProcessEntry>,
    stderr: CommandReader,
    backend: Arc<B>,
) where
    B: CommandBackend,
{
    read_stream(entry, stderr, StreamKind::Stderr, backend).await;
}

async fn read_stream<R, B>(
    entry: Arc<CommandProcessEntry>,
    mut reader: R,
    stream: StreamKind,
    backend: Arc<B>,
) where
    R: tokio::io::AsyncRead + Unpin,
    B: CommandBackend,
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
                let capture_stream = match stream {
                    StreamKind::Stdout => CommandCaptureStream::Stdout,
                    StreamKind::Stderr => CommandCaptureStream::Stderr,
                };
                if let Err(error) = backend
                    .append_output_chunk(&entry.output_target, capture_stream, chunk)
                    .await
                {
                    let mut state = entry.state.lock().await;
                    state.record_output_error(error.to_string());
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
