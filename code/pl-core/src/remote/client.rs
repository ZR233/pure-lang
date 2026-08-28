use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use pl_protocol::remote::{
    RemoteError, RemoteEvent, RemoteMessage, RemoteOutputStream, RemoteProcessExit, RemoteRequest,
    RemoteResponse, RemoteSpawnRequest,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::sync::{Mutex, Notify, oneshot};

use super::codec::{read_frame, write_frame};

type SharedWriter = Arc<Mutex<Box<dyn AsyncWrite + Send + Unpin>>>;

#[derive(Debug, thiserror::Error)]
pub enum RemoteClientError {
    #[error("SSH password is required")]
    CredentialRequired,
    #[error("remoteDisconnected")]
    Disconnected,
    #[error("remote helper protocol failed: {0}")]
    Protocol(String),
    #[error("remote helper rejected the request ({code:?}): {message}")]
    Remote {
        code: pl_protocol::remote::RemoteErrorCode,
        message: String,
    },
}

impl From<io::Error> for RemoteClientError {
    fn from(error: io::Error) -> Self {
        Self::Protocol(error.to_string())
    }
}

#[derive(Debug)]
pub struct RemoteReply {
    pub response: RemoteResponse,
    pub body: Vec<u8>,
}

struct RemoteProcessChannels {
    stdout: DuplexStream,
    stderr: DuplexStream,
    exit: Option<oneshot::Sender<Result<RemoteProcessExit, RemoteClientError>>>,
}

impl std::fmt::Debug for RemoteProcessChannels {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteProcessChannels")
            .finish_non_exhaustive()
    }
}

struct RemoteClientInner {
    writer: SharedWriter,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<RemoteReply, RemoteClientError>>>>,
    processes: Mutex<HashMap<String, RemoteProcessChannels>>,
    next_request_id: AtomicU64,
    last_output_sequence: AtomicU64,
    disconnected: AtomicBool,
    disconnected_notify: Notify,
}

impl std::fmt::Debug for RemoteClientInner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteClientInner")
            .field("next_request_id", &self.next_request_id)
            .field("disconnected", &self.disconnected)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct RemoteClient {
    inner: Arc<RemoteClientInner>,
}

pub struct RemoteProcessTransport {
    pub stdin: DuplexStream,
    pub stdout: DuplexStream,
    pub stderr: DuplexStream,
    pub exit: oneshot::Receiver<Result<RemoteProcessExit, RemoteClientError>>,
}

impl std::fmt::Debug for RemoteProcessTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteProcessTransport")
            .finish_non_exhaustive()
    }
}

impl RemoteClient {
    pub fn from_streams<R, W>(reader: R, writer: W) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let inner = Arc::new(RemoteClientInner {
            writer: Arc::new(Mutex::new(Box::new(writer))),
            pending: Mutex::new(HashMap::new()),
            processes: Mutex::new(HashMap::new()),
            next_request_id: AtomicU64::new(0),
            last_output_sequence: AtomicU64::new(0),
            disconnected: AtomicBool::new(false),
            disconnected_notify: Notify::new(),
        });
        tokio::spawn(read_loop(reader, inner.clone()));
        Self { inner }
    }

    pub async fn request(
        &self,
        request: RemoteRequest,
        body: &[u8],
    ) -> Result<RemoteReply, RemoteClientError> {
        if self.inner.disconnected.load(Ordering::Acquire) {
            return Err(RemoteClientError::Disconnected);
        }
        let request_id = self
            .inner
            .next_request_id
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let (sender, receiver) = oneshot::channel();
        self.inner.pending.lock().await.insert(request_id, sender);
        let write_result = write_frame(
            &mut *self.inner.writer.lock().await,
            Some(request_id),
            RemoteMessage::Request(request),
            body,
        )
        .await;
        if let Err(error) = write_result {
            self.inner.pending.lock().await.remove(&request_id);
            if is_disconnect_error(&error) {
                mark_disconnected(&self.inner).await;
                return Err(RemoteClientError::Disconnected);
            }
            return Err(RemoteClientError::Protocol(error.to_string()));
        }
        receiver
            .await
            .unwrap_or(Err(RemoteClientError::Disconnected))
    }

    pub async fn spawn_process(
        &self,
        request: RemoteSpawnRequest,
    ) -> Result<RemoteProcessTransport, RemoteClientError> {
        let process_id = request.process_id.clone();
        let (stdin, stdin_reader) = tokio::io::duplex(64 * 1024);
        let (stdout, event_stdout) = tokio::io::duplex(64 * 1024);
        let (stderr, event_stderr) = tokio::io::duplex(64 * 1024);
        let (exit_sender, exit) = oneshot::channel();
        self.inner.processes.lock().await.insert(
            process_id.clone(),
            RemoteProcessChannels {
                stdout: event_stdout,
                stderr: event_stderr,
                exit: Some(exit_sender),
            },
        );
        match self.request(RemoteRequest::Spawn(request), &[]).await {
            Ok(RemoteReply {
                response:
                    RemoteResponse::ProcessSpawned {
                        process_id: spawned,
                    },
                ..
            }) if spawned == process_id => {}
            Ok(reply) => {
                self.inner.processes.lock().await.remove(&process_id);
                return Err(RemoteClientError::Protocol(format!(
                    "unexpected spawn response: {:?}",
                    reply.response
                )));
            }
            Err(error) => {
                self.inner.processes.lock().await.remove(&process_id);
                return Err(error);
            }
        }
        let client = self.clone();
        let stdin_process_id = process_id.clone();
        tokio::spawn(async move {
            forward_stdin(client, stdin_process_id, stdin_reader).await;
        });
        Ok(RemoteProcessTransport {
            stdin,
            stdout,
            stderr,
            exit,
        })
    }

    pub async fn terminate_process(&self, process_id: &str) -> Result<(), RemoteClientError> {
        expect_ack(
            self.request(
                RemoteRequest::Terminate {
                    process_id: process_id.to_string(),
                },
                &[],
            )
            .await?,
        )
    }

    pub fn is_disconnected(&self) -> bool {
        self.inner.disconnected.load(Ordering::Acquire)
    }

    pub async fn wait_disconnected(&self) {
        if self.is_disconnected() {
            return;
        }
        self.inner.disconnected_notify.notified().await;
    }

    pub(crate) fn is_same_connection(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

fn is_disconnect_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::NotConnected
            | io::ErrorKind::UnexpectedEof
    )
}

async fn forward_stdin(client: RemoteClient, process_id: String, mut reader: DuplexStream) {
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => {
                let _ = client
                    .request(
                        RemoteRequest::CloseStdin {
                            process_id: process_id.clone(),
                        },
                        &[],
                    )
                    .await;
                break;
            }
            Ok(count) => {
                if client
                    .request(
                        RemoteRequest::WriteStdin {
                            process_id: process_id.clone(),
                        },
                        &buffer[..count],
                    )
                    .await
                    .and_then(expect_ack)
                    .is_err()
                {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

async fn read_loop<R>(mut reader: R, inner: Arc<RemoteClientInner>)
where
    R: AsyncRead + Unpin,
{
    while let Ok(Some(frame)) = read_frame(&mut reader).await {
        match frame.message {
            RemoteMessage::Response(response) => {
                let Some(request_id) = frame.request_id else {
                    break;
                };
                let result = match response {
                    RemoteResponse::Error(RemoteError { code, message }) => {
                        Err(RemoteClientError::Remote { code, message })
                    }
                    response => Ok(RemoteReply {
                        response,
                        body: frame.body,
                    }),
                };
                if let Some(sender) = inner.pending.lock().await.remove(&request_id) {
                    let _ = sender.send(result);
                }
            }
            RemoteMessage::Event(event) => {
                if handle_event(&inner, event, frame.body).await.is_err() {
                    break;
                }
            }
            RemoteMessage::Request(_) => break,
        }
    }
    mark_disconnected(&inner).await;
}

async fn handle_event(
    inner: &RemoteClientInner,
    event: RemoteEvent,
    body: Vec<u8>,
) -> Result<(), io::Error> {
    match event {
        RemoteEvent::ProcessOutput(output) => {
            let previous = inner.last_output_sequence.load(Ordering::Relaxed);
            if output.sequence <= previous {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "remote process output sequence {} is not greater than {previous}",
                        output.sequence
                    ),
                ));
            }
            inner
                .last_output_sequence
                .store(output.sequence, Ordering::Relaxed);
            let mut processes = inner.processes.lock().await;
            let Some(channels) = processes.get_mut(&output.process_id) else {
                return Ok(());
            };
            let writer = match output.stream {
                RemoteOutputStream::Stdout => &mut channels.stdout,
                RemoteOutputStream::Stderr => &mut channels.stderr,
            };
            writer.write_all(&body).await
        }
        RemoteEvent::ProcessExit(exit) => {
            if let Some(mut channels) = inner.processes.lock().await.remove(&exit.process_id)
                && let Some(sender) = channels.exit.take()
            {
                let _ = sender.send(Ok(exit));
            }
            Ok(())
        }
    }
}

async fn mark_disconnected(inner: &RemoteClientInner) {
    if inner.disconnected.swap(true, Ordering::AcqRel) {
        return;
    }
    inner.disconnected_notify.notify_waiters();
    for (_, sender) in inner.pending.lock().await.drain() {
        let _ = sender.send(Err(RemoteClientError::Disconnected));
    }
    for (_, mut channels) in inner.processes.lock().await.drain() {
        if let Some(sender) = channels.exit.take() {
            let _ = sender.send(Err(RemoteClientError::Disconnected));
        }
    }
}

pub(super) fn expect_ack(reply: RemoteReply) -> Result<(), RemoteClientError> {
    match reply.response {
        RemoteResponse::Ack => Ok(()),
        response => Err(RemoteClientError::Protocol(format!(
            "expected ack, received {response:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_transport_writes_are_classified_as_disconnects() {
        for kind in [
            io::ErrorKind::BrokenPipe,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::NotConnected,
        ] {
            assert!(is_disconnect_error(&io::Error::from(kind)));
        }
        assert!(!is_disconnect_error(&io::Error::from(
            io::ErrorKind::InvalidData
        )));
    }
}
