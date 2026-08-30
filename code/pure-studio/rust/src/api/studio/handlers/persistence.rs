use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::runtime::bridge_persistence_state;
use crate::api::studio::types::{BridgeError, BridgePersistenceStateSnapshot};

/// 跳过当前退避并立即重试保存积压的内存事实。
pub async fn retry_persistence() -> Result<BridgePersistenceStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_persistence_state(
        bridge.studio.retry_persistence().await?,
    ))
}
