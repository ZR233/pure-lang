use std::sync::Arc;

use pl_core::{UserInputCallback, UserInputRequest, UserInputResponse};
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;

use crate::dto::{UserInputRequestPayload, UserInputResolvedPayload};
use crate::state::{UserInputWaiter, UserInputWaiters};

pub fn user_input_callback(
    waiters: UserInputWaiters,
    app: AppHandle,
    session_id: String,
) -> UserInputCallback {
    Arc::new(move |request: UserInputRequest| {
        let waiters = waiters.clone();
        let app = app.clone();
        let session_id = session_id.clone();
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            waiters.lock().await.insert(
                request.request_id.clone(),
                UserInputWaiter {
                    session_id: session_id.clone(),
                    sender: tx,
                },
            );
            let _ = app.emit(
                "studio-user-input-requested",
                UserInputRequestPayload {
                    request_id: request.request_id,
                    session_id,
                    tool_id: request.tool_id,
                    questions: request.questions,
                },
            );

            rx.await.unwrap_or_default()
        })
    })
}

pub async fn resolve_user_input(
    request_id: String,
    response: UserInputResponse,
    app: AppHandle,
    waiters: UserInputWaiters,
) {
    if let Some(waiter) = waiters.lock().await.remove(&request_id) {
        let _ = waiter.sender.send(response);
    }
    let _ = app.emit(
        "studio-user-input-resolved",
        UserInputResolvedPayload { request_id },
    );
}

pub async fn cancel_session_user_inputs(
    session_id: &str,
    app: &AppHandle,
    waiters: UserInputWaiters,
) {
    let cancelled = {
        let mut waiters = waiters.lock().await;
        let request_ids: Vec<String> = waiters
            .iter()
            .filter(|(_, waiter)| waiter.session_id == session_id)
            .map(|(request_id, _)| request_id.clone())
            .collect();
        request_ids
            .into_iter()
            .filter_map(|request_id| {
                waiters
                    .remove(&request_id)
                    .map(|waiter| (request_id, waiter))
            })
            .collect::<Vec<_>>()
    };

    for (request_id, waiter) in cancelled {
        let _ = waiter.sender.send(UserInputResponse::default());
        let _ = app.emit(
            "studio-user-input-resolved",
            UserInputResolvedPayload { request_id },
        );
    }
}
