use std::collections::HashMap;

use pl_protocol::remote::{RemoteError, RemoteErrorCode, RemoteShellDescriptor};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;

use super::super::outbound::Outbound;
use super::{Request, closed, resource, streams::Input};
use crate::path::remote_error;

struct Entry {
    cancel: watch::Sender<bool>,
    input: mpsc::Sender<Input>,
}

pub(super) async fn run(
    mut requests: mpsc::Receiver<Request>,
    mut closing: watch::Receiver<bool>,
    writer: Outbound,
    shell: RemoteShellDescriptor,
) -> Result<(), RemoteError> {
    let mut entries: HashMap<String, Entry> = HashMap::new();
    let mut jobs = JoinSet::new();
    let mut failure = None;
    loop {
        let request = tokio::select! {
            biased;
            _ = closing.wait_for(|closing| *closing) => break,
            error = writer.failed() => {
                failure = Some(remote_error(RemoteErrorCode::Io, error.to_string()));
                break;
            }
            result = jobs.join_next(), if !jobs.is_empty() => {
                if let Some(Err(error)) = result {
                    failure = Some(remote_error(RemoteErrorCode::Io, format!("resource observation failed: {error}")));
                    break;
                }
                continue;
            }
            request = requests.recv() => match request { Some(request) => request, None => break },
        };
        match request {
            Request::Spawn {
                request,
                cwd,
                capture,
                reply,
            } => {
                if entries.contains_key(&request.process_id) || entries.len() >= 1024 {
                    let _ = reply.send(Err(remote_error(
                        RemoteErrorCode::InvalidRequest,
                        "process identity already exists or registry capacity is exhausted",
                    )));
                    continue;
                }
                let (cancel, cancelled) = watch::channel(false);
                let (input, incoming) = mpsc::channel(8);
                entries.insert(
                    request.process_id.clone(),
                    Entry {
                        cancel: cancel.clone(),
                        input,
                    },
                );
                // Admission and job ownership are established without awaiting external code.
                jobs.spawn(resource::run(resource::Launch {
                    request,
                    cwd,
                    capture,
                    shell: shell.clone(),
                    writer: writer.clone(),
                    cancel,
                    cancelled,
                    incoming,
                    reply,
                }));
            }
            Request::Input { id, body, reply } => match entries.get(&id) {
                Some(entry) => {
                    if let Err(error) = entry.input.try_send(Input { body, reply }) {
                        let _ = error.into_inner().reply.send(Err(remote_error(
                            RemoteErrorCode::Io,
                            "process input is closed or backpressured",
                        )));
                    }
                }
                None => {
                    let _ = reply.send(Err(not_found(&id)));
                }
            },
            Request::Cancel { id, reply } => {
                let result = match entries.get(&id) {
                    Some(entry) => {
                        entry.cancel.send_replace(true);
                        Ok(())
                    }
                    None => Err(not_found(&id)),
                };
                let _ = reply.send(result);
            }
        }
    }
    requests.close();
    for entry in entries.values() {
        entry.cancel.send_replace(true);
    }
    while let Ok(request) = requests.try_recv() {
        let reply = match request {
            Request::Spawn { reply, .. }
            | Request::Input { reply, .. }
            | Request::Cancel { reply, .. } => reply,
        };
        let _ = reply.send(Err(closed()));
    }
    while let Some(result) = jobs.join_next().await {
        if let Err(error) = result {
            failure.get_or_insert_with(|| {
                remote_error(
                    RemoteErrorCode::Io,
                    format!("resource observation failed: {error}"),
                )
            });
        }
    }
    // Do not clear live resources before joining: all worker and stream futures ended here.
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn not_found(id: &str) -> RemoteError {
    remote_error(
        RemoteErrorCode::ProcessNotFound,
        format!("unknown process id '{id}'"),
    )
}
