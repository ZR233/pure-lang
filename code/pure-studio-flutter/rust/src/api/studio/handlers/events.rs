use crate::api::studio::convert::event::bridge_product_event;
use crate::api::studio::runtime::bridge;
use crate::api::studio::types::BridgeProductEventEnvelope;
use crate::frb_generated::StreamSink;
use anyhow::Result;

/// FRB 的透明传输容器。
///
/// `payload_json` 始终由 `pl-protocol::SessionStreamFrame` 直接序列化，Flutter
/// 因而消费与 Mai SSE 完全相同的 canonical JSON，而不复制一套 Rust wire 类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeSessionStreamFrame {
    pub payload_json: String,
}

pub fn subscribe_session_events(
    session_id: String,
    after_sequence: Option<u64>,
    sink: StreamSink<BridgeSessionStreamFrame>,
) -> Result<()> {
    let bridge = bridge()?;
    let mut events = bridge.block_on(bridge.studio.subscribe_session_events(
        pl_protocol::SessionSubscriptionRequest {
            session_id,
            after_sequence,
        },
    ))?;
    bridge.tokio.spawn(async move {
        while let Some(frame) = events.recv().await {
            let payload_json = match serde_json::to_string(&frame) {
                Ok(payload_json) => payload_json,
                Err(_) => break,
            };
            if sink.add(BridgeSessionStreamFrame { payload_json }).is_err() {
                break;
            }
        }
    });
    Ok(())
}

pub fn subscribe_product_events(sink: StreamSink<BridgeProductEventEnvelope>) -> Result<()> {
    let bridge = bridge()?;
    let mut events = bridge.studio.product_events().subscribe();
    bridge.tokio.spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if sink.add(bridge_product_event(event)).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(lagged_events)) => {
                    if sink
                        .add(BridgeProductEventEnvelope::stale(lagged_events))
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
