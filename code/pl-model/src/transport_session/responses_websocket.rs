use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type RawResponsesWebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

enum WebSocketCommand {
    Send {
        message: Message,
        result: oneshot::Sender<std::result::Result<(), String>>,
    },
}

/// 持续驱动物理 WebSocket 的连接句柄。
///
/// 收包循环独立于模型事件消费者，因此 agent 执行工具或等待下一轮输入时仍会及时
/// 回复 ping。该类型只管理物理连接，不持有 continuation 或 provider 配置。
pub(crate) struct ResponsesWebSocketConnection {
    command_tx: mpsc::Sender<WebSocketCommand>,
    message_rx: mpsc::UnboundedReceiver<std::result::Result<Message, String>>,
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
                                    .map_err(|error| error.to_string());
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
                                    let _ = message_tx.send(Err(error.to_string()));
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
                                let _ = message_tx.send(Err(error.to_string()));
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

    pub(crate) async fn send(&self, message: Message) -> std::result::Result<(), String> {
        let (result_tx, result_rx) = oneshot::channel();
        self.command_tx
            .send(WebSocketCommand::Send {
                message,
                result: result_tx,
            })
            .await
            .map_err(|_| "Responses WebSocket connection is closed".to_string())?;
        result_rx
            .await
            .unwrap_or_else(|_| Err("Responses WebSocket connection is closed".to_string()))
    }

    pub(crate) async fn next(&mut self) -> Option<std::result::Result<Message, String>> {
        self.message_rx.recv().await
    }
}

impl Drop for ResponsesWebSocketConnection {
    fn drop(&mut self) {
        self.pump_task.abort();
    }
}
