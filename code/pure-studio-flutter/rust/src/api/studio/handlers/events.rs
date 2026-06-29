use crate::api::studio::convert::bridge_event_envelope;
use crate::api::studio::runtime::bridge;
use crate::api::studio::types::{BridgeEventEnvelope, BridgeStudioEventsResponse};
use crate::frb_generated::StreamSink;
use anyhow::Result;
pub fn load_studio_events(
    session_id: String,
    after_sequence: Option<i64>,
    limit: Option<i64>,
) -> Result<BridgeStudioEventsResponse> {
    let bridge = bridge()?;
    bridge.block_on(async {
        let events = bridge
            .studio
            .store()
            .load_studio_events(&session_id, after_sequence, limit)
            .await?;
        let events = events
            .into_iter()
            .filter_map(bridge_event_envelope)
            .collect::<Vec<_>>();
        let next_sequence = bridge
            .studio
            .store()
            .next_studio_event_sequence(&session_id)
            .await? as u64;
        Ok(BridgeStudioEventsResponse {
            session_id,
            events,
            next_sequence,
        })
    })
}

pub fn subscribe_session_events(
    session_id: String,
    sink: StreamSink<BridgeEventEnvelope>,
) -> Result<()> {
    let bridge = bridge()?;
    let stale_session_id = session_id.clone();
    let mut events = bridge.studio.events().subscribe_session(session_id);
    bridge.tokio.spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let Some(event) = bridge_event_envelope(event) else {
                        continue;
                    };
                    if sink.add(event).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(lagged_events)) => {
                    if sink
                        .add(BridgeEventEnvelope::stale(
                            Some(stale_session_id.clone()),
                            lagged_events,
                        ))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}

pub fn subscribe_global_events(sink: StreamSink<BridgeEventEnvelope>) -> Result<()> {
    let bridge = bridge()?;
    let mut events = bridge.studio.events().subscribe_global();
    bridge.tokio.spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let Some(event) = bridge_event_envelope(event) else {
                        continue;
                    };
                    if sink.add(event).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(lagged_events)) => {
                    if sink
                        .add(BridgeEventEnvelope::stale(None, lagged_events))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(())
}
