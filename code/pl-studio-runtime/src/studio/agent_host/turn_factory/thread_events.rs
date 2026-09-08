//! Publishes canonical thread-title changes through the public session source boundary.

use std::future::Future;

use pl_core::session_runtime::{
    SessionBuildContext, SessionEventSource, SessionEventSubscription, SessionMessage,
    SessionSourceError,
};

use crate::StudioProductEventKind;
use crate::studio::ProductEventBus;

pub(super) struct ThreadTitleEvents {
    pub(super) events: ProductEventBus,
    pub(super) thread_id: String,
}

impl SessionEventSource for ThreadTitleEvents {
    fn initialize(
        &self,
        context: SessionBuildContext,
    ) -> impl Future<Output = Result<SessionEventSubscription, SessionSourceError>> + Send {
        let events = self.events.clone();
        let thread_id = self.thread_id.clone();
        async move {
            let mut receiver = events.subscribe();
            let mut title = events
                .thread_snapshot(&thread_id)
                .map(|thread| thread.title)
                .ok_or_else(|| {
                    SessionSourceError::new(
                        "bind thread events",
                        std::io::Error::new(std::io::ErrorKind::NotFound, "thread is not resident"),
                    )
                })?;
            let sender = context
                .messages()
                .map_err(|error| SessionSourceError::new("bind thread publisher", error))?;
            let cancellation = context.cancellation_token();
            Ok(SessionEventSubscription::new(async move {
                loop {
                    let update = tokio::select! {
                        result = receiver.recv() => result,
                        _ = cancellation.cancelled() => return Ok(()),
                    };
                    let (sequence, created_at) = match update {
                        Ok(event)
                            if matches!(
                                event.kind,
                                StudioProductEventKind::ThreadDirectoryChanged(_)
                            ) =>
                        {
                            (event.sequence, event.created_at)
                        }
                        Ok(_) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            (events.current_sequence(), crate::studio::unix_seconds())
                        }
                        Err(error) => {
                            return Err(SessionSourceError::new("receive thread events", error));
                        }
                    };
                    let Some(thread) = events.thread_snapshot(&thread_id) else {
                        continue;
                    };
                    if thread.title == title {
                        continue;
                    }
                    let payload = serde_json::json!({"threadId": thread_id, "title": thread.title});
                    let message = SessionMessage {
                        id: pl_core::canonical_json_hash(&serde_json::json!([
                            payload, sequence, created_at
                        ])),
                        kind: "threadRenamed".into(),
                        text: format!("Thread title changed to {}", thread.title),
                        payload: Some(payload),
                    };
                    tokio::select! {
                        result = sender.publish_wait(message) => { result.map_err(|error| SessionSourceError::new("publish thread rename", error))?; },
                        _ = cancellation.cancelled() => return Ok(()),
                    }
                    title = thread.title;
                }
            }))
        }
    }
}
