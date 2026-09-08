//! The existing SSH physical registry owns resource jobs, not Thread or model task state.

mod owner;
mod resource;
mod streams;

use pl_protocol::remote::{
    RemoteError, RemoteErrorCode, RemoteShellDescriptor, RemoteSpawnRequest,
};
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use super::outbound::Outbound;
use crate::path::remote_error;

type Reply = oneshot::Sender<Result<(), RemoteError>>;

enum Request {
    Spawn {
        request: RemoteSpawnRequest,
        cwd: PathBuf,
        capture: PathBuf,
        reply: Reply,
    },
    Input {
        id: String,
        body: Option<Vec<u8>>,
        reply: Reply,
    },
    Cancel {
        id: String,
        reply: Reply,
    },
}

#[derive(Debug, Clone)]
pub(super) struct ProcessRegistry {
    requests: mpsc::Sender<Request>,
    close: watch::Sender<bool>,
    completion: watch::Receiver<Option<Result<(), RemoteError>>>,
}

pub(super) struct RegistryTask(JoinHandle<()>);

impl ProcessRegistry {
    pub(super) fn new(writer: Outbound, shell: RemoteShellDescriptor) -> (Self, RegistryTask) {
        let (requests, receiver) = mpsc::channel(32);
        let (close, closing) = watch::channel(false);
        let (finished, completion) = watch::channel(None);
        let task = tokio::spawn(async move {
            let outcome = owner::run(receiver, closing, writer, shell).await;
            finished.send_replace(Some(outcome));
        });
        (
            Self {
                requests,
                close,
                completion,
            },
            RegistryTask(task),
        )
    }

    pub(super) async fn spawn(
        &self,
        request: RemoteSpawnRequest,
        cwd: PathBuf,
        capture: PathBuf,
    ) -> Result<(), RemoteError> {
        let (reply, received) = oneshot::channel();
        self.send(Request::Spawn {
            request,
            cwd,
            capture,
            reply,
        })
        .await?;
        received.await.map_err(|_| closed())?
    }

    pub(super) async fn write_stdin(
        &self,
        process_id: &str,
        body: &[u8],
    ) -> Result<(), RemoteError> {
        self.input(process_id, Some(body.to_vec())).await
    }

    pub(super) async fn close_stdin(&self, process_id: &str) -> Result<(), RemoteError> {
        self.input(process_id, None).await
    }

    async fn input(&self, id: &str, body: Option<Vec<u8>>) -> Result<(), RemoteError> {
        let (reply, received) = oneshot::channel();
        self.send(Request::Input {
            id: id.into(),
            body,
            reply,
        })
        .await?;
        received.await.map_err(|_| closed())?
    }

    pub(super) async fn terminate(&self, id: &str) -> Result<(), RemoteError> {
        let (reply, received) = oneshot::channel();
        self.send(Request::Cancel {
            id: id.into(),
            reply,
        })
        .await?;
        received.await.map_err(|_| closed())?
    }

    pub(super) async fn terminate_all(&self) -> Result<(), RemoteError> {
        self.close.send_replace(true);
        let mut completed = self.completion.clone();
        match completed.wait_for(Option::is_some).await {
            Ok(outcome) => outcome.as_ref().cloned().ok_or_else(closed)?,
            Err(_) => Err(remote_error(
                RemoteErrorCode::Io,
                "physical registry lost its completion observer",
            )),
        }
    }

    async fn send(&self, request: Request) -> Result<(), RemoteError> {
        if *self.close.borrow() {
            return Err(closed());
        }
        self.requests.send(request).await.map_err(|_| closed())
    }
}

impl RegistryTask {
    pub(super) async fn wait(&mut self) -> Result<(), RemoteError> {
        (&mut self.0).await.map_err(|error| {
            remote_error(
                RemoteErrorCode::Io,
                format!("physical registry failed: {error}"),
            )
        })
    }
}

impl Drop for RegistryTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn closed() -> RemoteError {
    remote_error(
        RemoteErrorCode::RemoteDisconnected,
        "physical registry is closing",
    )
}
