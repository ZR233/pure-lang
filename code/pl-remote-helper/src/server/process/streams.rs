use pl_protocol::remote::{
    RemoteError, RemoteErrorCode, RemoteEvent, RemoteMessage, RemoteOutputStream,
    RemoteProcessOutput,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, watch};

use super::super::outbound::Outbound;
use super::Reply;
use crate::client::{ChildStderr, ChildStdin, ChildStdout};
use crate::path::{io_error, remote_error};

pub(super) struct Input {
    pub body: Option<Vec<u8>>,
    pub reply: Reply,
}

pub(super) async fn input(
    mut stdin: Option<ChildStdin>,
    mut requests: mpsc::Receiver<Input>,
    mut stopped: watch::Receiver<bool>,
) {
    loop {
        let request = tokio::select! {
            biased;
            _ = stopped.wait_for(|stopped| *stopped) => break,
            request = requests.recv() => match request { Some(request) => request, None => break },
        };
        let result = match (request.body, stdin.as_mut()) {
            (None, _) => {
                stdin = None;
                Ok(())
            }
            (Some(body), Some(writer)) => tokio::select! {
                biased;
                _ = stopped.wait_for(|stopped| *stopped) => Err(input_closed()),
                result = writer.write_all(&body) => result.map_err(|error| io_error("write process stdin", error)),
            },
            (Some(_), None) => Err(input_closed()),
        };
        let _ = request.reply.send(result);
    }
    requests.close();
    while let Some(request) = requests.recv().await {
        let _ = request.reply.send(Err(input_closed()));
    }
}

fn input_closed() -> RemoteError {
    remote_error(RemoteErrorCode::ProcessNotFound, "process stdin is closed")
}

pub(super) struct Output<'a> {
    pub id: &'a str,
    pub stdout: Option<ChildStdout>,
    pub stderr: Option<ChildStderr>,
    pub capture: tokio::fs::File,
    pub writer: &'a Outbound,
    pub cancel: &'a watch::Sender<bool>,
}

pub(super) async fn output(mut output: Output<'_>) -> Option<RemoteError> {
    let mut stdout_buffer = [0; 8192];
    let mut stderr_buffer = [0; 8192];
    let mut failure = None;
    let mut capture_open = true;
    let mut wire_open = true;
    while output.stdout.is_some() || output.stderr.is_some() {
        let (stream, result) = tokio::select! {
            result = read_stdout(&mut output.stdout, &mut stdout_buffer), if output.stdout.is_some() => (RemoteOutputStream::Stdout, result),
            result = read_stderr(&mut output.stderr, &mut stderr_buffer), if output.stderr.is_some() => (RemoteOutputStream::Stderr, result),
        };
        let count = match result {
            Ok(count) => count,
            Err(error) => {
                failure.get_or_insert_with(|| io_error("read process output", error));
                output.cancel.send_replace(true);
                0
            }
        };
        if count == 0 {
            match stream {
                RemoteOutputStream::Stdout => output.stdout = None,
                RemoteOutputStream::Stderr => output.stderr = None,
            }
            continue;
        }
        let (label, body) = match stream {
            RemoteOutputStream::Stdout => ("=== STDOUT ===\n", &stdout_buffer[..count]),
            RemoteOutputStream::Stderr => ("=== STDERR ===\n", &stderr_buffer[..count]),
        };
        if capture_open {
            let result = async {
                output.capture.write_all(label.as_bytes()).await?;
                output.capture.write_all(body).await?;
                if !body.ends_with(b"\n") {
                    output.capture.write_all(b"\n").await?;
                }
                Ok::<_, std::io::Error>(())
            }
            .await;
            if let Err(error) = result {
                failure.get_or_insert_with(|| io_error("capture process output", error));
                capture_open = false;
                output.cancel.send_replace(true);
            }
        }
        if wire_open
            && let Err(error) = output
                .writer
                .write(
                    None,
                    RemoteMessage::Event(RemoteEvent::ProcessOutput(RemoteProcessOutput {
                        process_id: output.id.into(),
                        sequence: 0,
                        stream,
                    })),
                    body,
                )
                .await
        {
            failure.get_or_insert_with(|| io_error("stream process output", error));
            wire_open = false;
            output.cancel.send_replace(true);
        }
    }
    if let Err(error) = output.capture.flush().await {
        failure.get_or_insert_with(|| io_error("flush process capture", error));
    }
    failure
}

async fn read_stdout(
    reader: &mut Option<ChildStdout>,
    buffer: &mut [u8],
) -> std::io::Result<usize> {
    match reader {
        Some(reader) => reader.read(buffer).await,
        None => Ok(0),
    }
}

async fn read_stderr(
    reader: &mut Option<ChildStderr>,
    buffer: &mut [u8],
) -> std::io::Result<usize> {
    match reader {
        Some(reader) => reader.read(buffer).await,
        None => Ok(0),
    }
}
