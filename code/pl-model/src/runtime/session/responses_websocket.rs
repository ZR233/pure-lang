use futures::{SinkExt, StreamExt};
use pl_protocol::PureError;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::runtime::responses_websocket::error::connection_error;

type RawResponsesWebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

enum WebSocketCommand {
    Send {
        message: Message,
        result: oneshot::Sender<std::result::Result<(), PureError>>,
    },
}

/// 持续驱动物理 WebSocket 的连接句柄。
///
/// 收包循环独立于模型事件消费者，因此 agent 执行工具或等待下一轮输入时仍会及时
/// 回复 ping。该类型只管理物理连接，不持有 continuation 或 provider 配置。
pub(crate) struct ResponsesWebSocketConnection {
    command_tx: mpsc::Sender<WebSocketCommand>,
    message_rx: mpsc::UnboundedReceiver<std::result::Result<Message, PureError>>,
    pump_task: tokio::task::JoinHandle<()>,
}

impl ResponsesWebSocketConnection {
    pub(crate) fn new(mut socket: RawResponsesWebSocket) -> Self {
        let (command_tx, mut command_rx) = mpsc::channel(8);
        let (message_tx, message_rx) = mpsc::unbounded_channel();
        let pump_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    command = command_rx.recv() => {
                        let Some(command) = command else {
                            break;
                        };
                        match command {
                            WebSocketCommand::Send { message, result } => {
                                let send_result = socket
                                    .send(message)
                                    .await
                                    .map_err(|error| connection_error(error.to_string()));
                                let should_stop = send_result.is_err();
                                let _ = result.send(send_result);
                                if should_stop {
                                    break;
                                }
                            }
                        }
                    }
                    message = socket.next() => {
                        match message {
                            Some(Ok(Message::Ping(payload))) => {
                                if let Err(error) = socket.send(Message::Pong(payload)).await {
                                    let _ = message_tx.send(Err(connection_error(error.to_string())));
                                    break;
                                }
                            }
                            Some(Ok(Message::Pong(_))) => {}
                            Some(Ok(message)) => {
                                let is_close = matches!(message, Message::Close(_));
                                if message_tx.send(Ok(message)).is_err() || is_close {
                                    break;
                                }
                            }
                            Some(Err(error)) => {
                                let _ = message_tx.send(Err(connection_error(error.to_string())));
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }
        });
        Self {
            command_tx,
            message_rx,
            pump_task,
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
        self.message_rx.recv().await
    }
}

impl Drop for ResponsesWebSocketConnection {
    fn drop(&mut self) {
        self.pump_task.abort();
    }
}
