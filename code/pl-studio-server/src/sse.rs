use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::Stream;
use pl_protocol::{ThreadNotification, ThreadSubscriptionRequest, ThreadSubscriptionUpdate};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::AppState;
use crate::error::ApiError;
use crate::routes::StudioApiErrors;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StaleEvent {
    reason: &'static str,
    dropped: Option<u64>,
    resync: &'static str,
}

#[utoipa::path(
    get,
    path = "/api/v1/events/product",
    operation_id = "studio.subscribeProduct",
    responses(StudioApiErrors, (status = 200, description = "Product event stream", body = String, content_type = "text/event-stream"))
)]
pub(crate) async fn product_events(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let permit = state
        .streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::overloaded())?;
    let mut events = state.runtime.subscribe_product();
    let shutdown = state.shutdown.clone();
    let has_cursor = headers.contains_key("last-event-id");
    let (sender, receiver) = mpsc::channel(64);
    tokio::spawn(async move {
        let _permit = permit;
        if has_cursor
            && !send_json(
                &sender,
                "stale",
                None,
                &StaleEvent {
                    reason: "replayUnsupported",
                    dropped: None,
                    resync: "/api/v1/state",
                },
            )
        {
            return;
        }
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                event = events.recv() => match event {
                    Ok(event) => {
                        if !send_json(&sender, "event", Some(&event.event_id), &event) {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                        if !send_json(
                            &sender,
                            "stale",
                            None,
                            &StaleEvent {
                                reason: "lagged",
                                dropped: Some(dropped),
                                resync: "/api/v1/state",
                            },
                        )
                        {
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
        let _ = sender.try_send(Ok(Event::default().event("closed").data("{}")));
    });
    Ok(Sse::new(ReceiverStream::new(receiver))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
}

#[utoipa::path(
    get,
    path = "/api/v1/threads/{thread_id}/events",
    operation_id = "thread.subscribe",
    params(("thread_id" = String, Path, description = "Thread ID")),
    responses(StudioApiErrors, (status = 200, description = "Authoritative Thread stream", body = String, content_type = "text/event-stream"))
)]
pub(crate) async fn thread_events(
    State(state): State<AppState>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let permit = state
        .streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::overloaded())?;
    let mut events = state
        .runtime
        .subscribe_thread(ThreadSubscriptionRequest {
            thread_id: thread_id.clone(),
        })
        .await
        .map_err(ApiError::from)?;
    let residency_pin = state.runtime.pin_thread(&thread_id);
    let shutdown = state.shutdown.clone();
    let has_cursor = headers.contains_key("last-event-id");
    let (sender, receiver) = mpsc::channel(128);
    tokio::spawn(async move {
        let _permit = permit;
        let _residency_pin = residency_pin;
        if has_cursor
            && !send_json(
                &sender,
                "stale",
                None,
                &StaleEvent {
                    reason: "replayUnsupported",
                    dropped: None,
                    resync: "resubscribe",
                },
            )
        {
            return;
        }
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                update = events.recv() => {
                    let Some(update) = update else { break; };
                    let (event_name, event_id) = match &update {
                        ThreadSubscriptionUpdate::Snapshot { snapshot } => (
                            "snapshot",
                            Some(format!("thread:{}", snapshot.revision)),
                        ),
                        ThreadSubscriptionUpdate::Notification { notification } => {
                            let name = if matches!(notification.notification, ThreadNotification::Lagged { .. }) {
                                "lagged"
                            } else {
                                "notification"
                            };
                            (name, Some(format!("thread:{}", notification.revision)))
                        }
                    };
                    if !send_json(&sender, event_name, event_id.as_deref(), &update) {
                        return;
                    }
                }
            }
        }
        let _ = sender.try_send(Ok(Event::default().event("closed").data("{}")));
    });
    Ok(Sse::new(ReceiverStream::new(receiver))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("")))
}

fn send_json(
    sender: &mpsc::Sender<Result<Event, Infallible>>,
    event_name: &'static str,
    event_id: Option<&str>,
    value: &impl Serialize,
) -> bool {
    let mut event = Event::default().event(event_name);
    if let Some(event_id) = event_id {
        event = event.id(event_id);
    }
    let Ok(event) = event.json_data(value) else {
        return false;
    };
    sender.try_send(Ok(event)).is_ok()
}
