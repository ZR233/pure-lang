use futures::{SinkExt, StreamExt};
use pl_protocol::PureError;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::runtime::responses_websocket::error::{connection_error, socket_error};

type RawResponsesWebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

enum WebSocketCommand {
    Send {
        message: Message,
        result: oneshot::Sender<std::result::Result<(), PureError>>,
    },
}

struct QueuedMessage {
    message: Message,
    _bytes: OwnedSemaphorePermit,
}

/// 持续驱动物理 WebSocket 的连接句柄。
///
/// 收包循环独立于模型事件消费者，因此 agent 执行工具或等待下一轮输入时仍会及时
/// 回复 ping。该类型只管理物理连接，不持有 continuation 或 provider 配置。
pub(crate) struct ResponsesWebSocketConnection {
    command_tx: mpsc::Sender<WebSocketCommand>,
    message_rx: mpsc::Receiver<QueuedMessage>,
    terminal_rx: Option<oneshot::Receiver<PureError>>,
    pump_task: Option<tokio::task::JoinHandle<()>>,
}

impl ResponsesWebSocketConnection {
    pub(crate) fn new(mut socket: RawResponsesWebSocket) -> Self {
        let (command_tx, mut command_rx) = mpsc::channel(8);
        let (message_tx, message_rx) = mpsc::channel(32);
        let (terminal_tx, terminal_rx) = oneshot::channel();
        // Bound bytes as well as frame count; otherwise 32 large JSON frames could
        // retain gigabytes while the consumer is stalled. Ping/Pong never enter this queue.
        let queued_bytes = Arc::new(Semaphore::new(64 * 1024 * 1024));
        let pump_task = tokio::spawn(async move {
            let terminal = loop {
                tokio::select! {
                    command = command_rx.recv() => {
                        let Some(command) = command else {
                            break connection_error("connection owner closed");
                        };
                        match command {
                            WebSocketCommand::Send { message, result } => {
                                let send_result = socket
                                    .send(message)
                                    .await
                                    .map_err(socket_error);
                                match send_result {
                                    Ok(()) => { let _ = result.send(Ok(())); }
                                    Err(error) => {
                                        let _ = result.send(Err(error));
                                        break connection_error("request send failed");
                                    }
                                }
                            }
                        }
                    }
                    message = socket.next() => {
                        match message {
                            Some(Ok(Message::Ping(payload))) => {
                                if let Err(error) = socket.send(Message::Pong(payload)).await {
                                    break socket_error(error);
                                }
                            }
                            Some(Ok(Message::Pong(_))) => {}
                            Some(Ok(message)) => {
                                let is_close = matches!(message, Message::Close(_));
                                let bytes = u32::try_from(message.len().saturating_add(64)).ok();
                                let Some(permit) = bytes.and_then(|bytes| queued_bytes.clone().try_acquire_many_owned(bytes).ok()) else {
                                    break connection_error("response event backlog exceeded its byte bound");
                                };
                                // Never block ping/command handling on an abandoned consumer.
                                // Overflow terminates this socket rather than growing memory.
                                if message_tx.try_send(QueuedMessage { message, _bytes: permit }).is_err() {
                                    break connection_error("response event backlog exceeded its bound");
                                }
                                if is_close {
                                    break connection_error("server closed the connection");
                                }
                            }
                            Some(Err(error)) => {
                                break socket_error(error);
                            }
                            None => break connection_error("connection ended without a terminal response"),
                        }
                    }
                }
            };
            let _ = terminal_tx.send(terminal);
        });
        Self {
            command_tx,
            message_rx,
            terminal_rx: Some(terminal_rx),
            pump_task: Some(pump_task),
        }
    }

    pub(crate) async fn close(&mut self) -> Result<(), PureError> {
        let Some(task) = self.pump_task.as_mut() else {
            return Ok(());
        };
        task.abort();
        let outcome = task.await;
        self.pump_task = None;
        match outcome {
            Ok(()) => Ok(()),
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(connection_error(format!(
                "Responses WebSocket pump failed while closing: {error}"
            ))),
        }
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.command_tx.is_closed()
            || self
                .pump_task
                .as_ref()
                .is_none_or(|task| task.is_finished())
    }

    pub(crate) fn retire(&self) {
        if let Some(task) = &self.pump_task {
            task.abort();
        }
    }

    pub(crate) async fn send(&self, message: Message) -> std::result::Result<(), PureError> {
        let (result_tx, result_rx) = oneshot::channel();
        self.command_tx
            .send(WebSocketCommand::Send {
                message,
                result: result_tx,
            })
            .await
            .map_err(|_| connection_error("Responses WebSocket connection is closed"))?;
        result_rx
            .await
            .unwrap_or_else(|_| Err(connection_error("Responses WebSocket connection is closed")))
    }

    pub(crate) async fn next(&mut self) -> Option<std::result::Result<Message, PureError>> {
        if let Some(message) = self.message_rx.recv().await {
            return Some(Ok(message.message));
        }
        let terminal = self.terminal_rx.as_mut()?;
        let error = terminal
            .await
            .unwrap_or_else(|_| connection_error("connection pump stopped"));
        self.terminal_rx = None;
        Some(Err(error))
    }
}

impl Drop for ResponsesWebSocketConnection {
    fn drop(&mut self) {
        if let Some(task) = &self.pump_task {
            task.abort();
        }
    }
}
