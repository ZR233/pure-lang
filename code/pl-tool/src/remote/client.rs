use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use pl_protocol::remote::{
    RemoteError, RemoteEvent, RemoteMessage, RemoteOutputStream, RemoteProcessExit, RemoteRequest,
    RemoteResponse, RemoteSpawnRequest,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::codec::{EncodedFrame, encode_frame, read_frame};

const REQUEST_CAPACITY: usize = 32;

struct PendingRequest {
    reply: oneshot::Sender<Result<RemoteReply, RemoteClientError>>,
    _permit: OwnedSemaphorePermit,
}

struct TransportLifetime {
    cancellation: CancellationToken,
    completion: Shared<BoxFuture<'static, Result<(), Arc<tokio::task::JoinError>>>>,
}

impl Drop for TransportLifetime {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl std::fmt::Debug for TransportLifetime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TransportLifetime")
            .field("cancellation", &self.cancellation)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RemoteClientError {
    /// The host has sealed SSH connection admission for shutdown.
    #[error("SSH manager is closing")]
    ManagerClosing,
    #[error("SSH password is required")]
    CredentialRequired,
    #[error("remoteDisconnected")]
    Disconnected,
    /// The bounded transport request capacity is exhausted; no frame was admitted.
    #[error("remote request capacity is exhausted")]
    Backpressure,
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
    writer: mpsc::Sender<EncodedFrame>,
    capacity: Arc<Semaphore>,
    pending: Mutex<HashMap<u64, PendingRequest>>,
    processes: Mutex<HashMap<String, RemoteProcessChannels>>,
    next_request_id: AtomicU64,
    last_output_sequence: AtomicU64,
    disconnected: CancellationToken,
    disconnect_reason: std::sync::Mutex<Option<String>>,
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
    transport: Arc<TransportLifetime>,
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
        let (sender, receiver) = mpsc::channel(REQUEST_CAPACITY);
        let inner = Arc::new(RemoteClientInner {
            writer: sender,
            capacity: Arc::new(Semaphore::new(REQUEST_CAPACITY)),
            pending: Mutex::new(HashMap::new()),
            processes: Mutex::new(HashMap::new()),
            next_request_id: AtomicU64::new(0),
            last_output_sequence: AtomicU64::new(0),
            disconnected: CancellationToken::new(),
            disconnect_reason: Default::default(),
        });
        let read = tokio::spawn(read_loop(reader, inner.clone()));
        let write = tokio::spawn(write_loop(writer, receiver, inner.clone()));
        let transport = Arc::new(TransportLifetime {
            cancellation: inner.disconnected.clone(),
            completion: async move {
                let (read, write) = tokio::join!(read, write);
                read.and(write).map_err(Arc::new)
            }
            .boxed()
            .shared(),
        });
        Self { inner, transport }
    }

    /// Admits a bounded request and waits for the remote response.
    ///
    /// Once admitted, canceling this future does not cancel frame transmission or
    /// remote side effects. The outstanding slot is released by the response or
    /// transport closure, not by cancellation of the caller's wait.
    ///
    /// # Errors
    /// Returns `Backpressure` without admitting a frame when capacity is exhausted,
    /// or a validation, disconnection, or remote rejection error.
    pub async fn request(
        &self,
        request: RemoteRequest,
        body: &[u8],
    ) -> Result<RemoteReply, RemoteClientError> {
        if self.is_disconnected() {
            return Err(RemoteClientError::Disconnected);
        }
        let capacity = self
            .inner
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| RemoteClientError::Backpressure)?;
        let request_id = self
            .inner
            .next_request_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| {
                RemoteClientError::Protocol("remote request identity exhausted".to_string())
            })?
            + 1;
        let frame = encode_frame(Some(request_id), RemoteMessage::Request(request), body)?;
        let admission = tokio::select! {
            biased;
            _ = self.inner.disconnected.cancelled() => return Err(RemoteClientError::Disconnected),
            permit = self.inner.writer.reserve() => permit.map_err(|_| RemoteClientError::Disconnected)?,
        };
        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self.inner.pending.lock().await;
            // Serialize admission with disconnection's pending-request drain.
            if self.is_disconnected() {
                return Err(RemoteClientError::Disconnected);
            }
            pending.insert(
                request_id,
                PendingRequest {
                    reply: sender,
                    _permit: capacity,
                },
            );
        }
        // No cancellation point between pending registration and frame admission.
        admission.send(frame);
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
        self.inner.disconnected.is_cancelled()
    }

    pub async fn wait_disconnected(&self) {
        self.inner.disconnected.cancelled().await;
    }

    /// Closes this transport and waits for both owned IO tasks to exit.
    ///
    /// Canceling this wait does not discard their completion handles.
    /// # Errors
    /// Reports a transport task panic after both tasks have been joined.
    pub async fn close(&self) -> Result<(), RemoteClientError> {
        self.inner.disconnected.cancel();
        self.transport.completion.clone().await.map_err(|error| {
            RemoteClientError::Protocol(format!("remote transport task failed: {error}"))
        })
    }

    /// First transport failure, excluding request contents and credentials.
    pub fn disconnect_reason(&self) -> Option<String> {
        self.inner
            .disconnect_reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn is_same_connection(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
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

struct DisconnectOnExit(CancellationToken);

impl Drop for DisconnectOnExit {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

async fn write_loop<W>(
    mut writer: W,
    mut frames: mpsc::Receiver<EncodedFrame>,
    inner: Arc<RemoteClientInner>,
) where
    W: AsyncWrite + Unpin,
{
    let _disconnect = DisconnectOnExit(inner.disconnected.clone());
    tokio::select! {
        biased;
        _ = inner.disconnected.cancelled() => {},
        _ = async {
            while let Some(frame) = frames.recv().await {
                if let Err(error) = frame.write(&mut writer).await {
                    retain_disconnect_reason(&inner, format!("write transport: {}", error.kind()));
                    break;
                }
            }
        } => {},
    }
    // Dropping the stream is mandatory even when cancellation interrupted a frame.
    drop(writer);
    drop(frames);
    mark_disconnected(&inner).await;
}

async fn read_loop<R>(reader: R, inner: Arc<RemoteClientInner>)
where
    R: AsyncRead + Unpin,
{
    let _disconnect = DisconnectOnExit(inner.disconnected.clone());
    tokio::select! {
        biased;
        _ = inner.disconnected.cancelled() => {},
        result = read_frames(reader, &inner) => {
            retain_disconnect_reason(&inner, match result {
                Ok(()) => "read transport: EOF".into(),
                Err(error) => format!("read transport: {error}"),
            });
        },
    }
    mark_disconnected(&inner).await;
}

async fn read_frames<R>(mut reader: R, inner: &RemoteClientInner) -> io::Result<()>
where
    R: AsyncRead + Unpin,
{
    while let Some(frame) = read_frame(&mut reader).await? {
        match frame.message {
            RemoteMessage::Response(response) => {
                let Some(request_id) = frame.request_id else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "response missing request identity",
                    ));
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
                    let _ = sender.reply.send(result);
                }
            }
            RemoteMessage::Event(event) => {
                handle_event(inner, event, frame.body).await?;
            }
            RemoteMessage::Request(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unexpected helper request",
                ));
            }
        }
    }
    Ok(())
}

fn retain_disconnect_reason(inner: &RemoteClientInner, reason: String) {
    let mut stored = inner
        .disconnect_reason
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if stored.is_none() {
        *stored = Some(reason.chars().take(512).collect());
    }
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
            match writer.write_all(&body).await {
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
                result => result,
            }
        }
        RemoteEvent::ProcessExit(exit) => {
            if let Some(mut channels) = inner.processes.lock().await.remove(&exit.process_id)
                && let Some(sender) = channels.exit.take()
            {
                let result = match exit.failure.as_ref() {
                    Some(error) => Err(RemoteClientError::Remote {
                        code: error.code,
                        message: error.message.clone(),
                    }),
                    None => Ok(exit),
                };
                let _ = sender.send(result);
            }
            Ok(())
        }
    }
}

async fn mark_disconnected(inner: &RemoteClientInner) {
    // Repeated drains are intentional: a canceled cleanup waiter must not prevent
    // another observer from finishing delivery of the disconnected result.
    inner.disconnected.cancel();
    for (_, sender) in inner.pending.lock().await.drain() {
        let _ = sender.reply.send(Err(RemoteClientError::Disconnected));
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

    #[tokio::test]
    async fn closed_process_output_does_not_disconnect_other_requests_and_protocol_failure_has_reason()
     {
        let (transport, mut peer) = tokio::io::duplex(8192);
        let (reader, writer) = tokio::io::split(transport);
        let client = RemoteClient::from_streams(reader, writer);
        let (stdout, consumed) = tokio::io::duplex(64);
        let (stderr, _stderr_consumer) = tokio::io::duplex(64);
        let (exit, _exit_receiver) = oneshot::channel();
        client.inner.processes.lock().await.insert(
            "retired".into(),
            RemoteProcessChannels {
                stdout,
                stderr,
                exit: Some(exit),
            },
        );
        drop(consumed);
        let peer_task = tokio::spawn(async move {
            encode_frame(
                None,
                RemoteMessage::Event(RemoteEvent::ProcessOutput(
                    pl_protocol::remote::RemoteProcessOutput {
                        process_id: "retired".into(),
                        sequence: 1,
                        stream: RemoteOutputStream::Stdout,
                    },
                )),
                b"late output",
            )
            .unwrap()
            .write(&mut peer)
            .await
            .unwrap();
            let request = read_frame(&mut peer).await.unwrap().unwrap();
            encode_frame(
                request.request_id,
                RemoteMessage::Response(RemoteResponse::Ack),
                &[],
            )
            .unwrap()
            .write(&mut peer)
            .await
            .unwrap();
            peer.write_u32(0).await.unwrap();
        });
        let reply = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client.request(
                RemoteRequest::CloseStdin {
                    process_id: "other".into(),
                },
                &[],
            ),
        )
        .await
        .unwrap();
        assert!(
            reply.is_ok(),
            "a retired output consumer disconnected the shared transport: {reply:?}"
        );
        peer_task.await.unwrap();
        client.wait_disconnected().await;
        assert!(
            client
                .disconnect_reason()
                .unwrap()
                .contains("header length 0")
        );
        client.close().await.unwrap();
    }
}
