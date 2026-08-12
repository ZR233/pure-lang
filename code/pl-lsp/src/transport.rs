use std::io::{BufReader, Write};
use std::thread::{self, JoinHandle};

use lsp_server::Message;
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::mpsc;
use tokio_util::io::SyncIoBridge;

use crate::types::{LspResult, LspRuntimeError};

const MESSAGE_CHANNEL_CAPACITY: usize = 256;

enum OutboundMessage {
    Message(Message),
    Close,
}

#[derive(Clone)]
pub(crate) struct TransportSender {
    outbound: mpsc::Sender<OutboundMessage>,
}

pub(crate) struct LspTransport {
    outbound: Option<mpsc::Sender<OutboundMessage>>,
    reader: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
}

impl LspTransport {
    pub(crate) fn spawn(
        stdin: ChildStdin,
        stdout: ChildStdout,
    ) -> LspResult<(Self, mpsc::Receiver<LspResult<Message>>)> {
        let (outbound, mut outbound_rx) =
            mpsc::channel::<OutboundMessage>(MESSAGE_CHANNEL_CAPACITY);
        let (inbound_tx, inbound) = mpsc::channel(MESSAGE_CHANNEL_CAPACITY);
        let writer = thread::Builder::new()
            .name("pl-lsp-writer".to_string())
            .spawn(move || {
                let mut writer = SyncIoBridge::new(stdin);
                while let Some(outbound) = outbound_rx.blocking_recv() {
                    match outbound {
                        OutboundMessage::Message(message) => {
                            if message.write(&mut writer).is_err() || writer.flush().is_err() {
                                break;
                            }
                        }
                        OutboundMessage::Close => break,
                    }
                }
            })
            .map_err(|error| {
                LspRuntimeError::Unavailable(format!("failed to start LSP writer thread: {error}"))
            })?;
        let reader = thread::Builder::new()
            .name("pl-lsp-reader".to_string())
            .spawn(move || {
                let mut reader = BufReader::new(SyncIoBridge::new(stdout));
                loop {
                    let message = match Message::read(&mut reader) {
                        Ok(Some(message)) => Ok(message),
                        Ok(None) => break,
                        Err(error) => Err(LspRuntimeError::Io(error)),
                    };
                    let terminal = message.is_err();
                    if inbound_tx.blocking_send(message).is_err() || terminal {
                        break;
                    }
                }
            });
        let reader = match reader {
            Ok(reader) => reader,
            Err(error) => {
                drop(outbound);
                let _ = writer.join();
                return Err(LspRuntimeError::Unavailable(format!(
                    "failed to start LSP reader thread: {error}"
                )));
            }
        };
        Ok((
            Self {
                outbound: Some(outbound),
                reader: Some(reader),
                writer: Some(writer),
            },
            inbound,
        ))
    }

    pub(crate) fn sender(&self) -> LspResult<TransportSender> {
        self.outbound
            .clone()
            .map(|outbound| TransportSender { outbound })
            .ok_or_else(|| LspRuntimeError::Unavailable("LSP transport is closed".to_string()))
    }

    pub(crate) fn close(&mut self) {
        if let Some(outbound) = self.outbound.take() {
            let _ = outbound.blocking_send(OutboundMessage::Close);
        }
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl TransportSender {
    pub(crate) async fn send(&self, message: Message) -> LspResult<()> {
        self.outbound
            .send(OutboundMessage::Message(message))
            .await
            .map_err(|_| LspRuntimeError::Unavailable("LSP writer channel closed".to_string()))
    }

    #[cfg(test)]
    pub(crate) fn test_channel(capacity: usize) -> (Self, TestTransportReceiver) {
        let (outbound, receiver) = mpsc::channel(capacity);
        (Self { outbound }, TestTransportReceiver { receiver })
    }
}

#[cfg(test)]
pub(crate) struct TestTransportReceiver {
    receiver: mpsc::Receiver<OutboundMessage>,
}

#[cfg(test)]
impl TestTransportReceiver {
    pub(crate) async fn recv(&mut self) -> Option<Message> {
        match self.receiver.recv().await? {
            OutboundMessage::Message(message) => Some(message),
            OutboundMessage::Close => None,
        }
    }
}
