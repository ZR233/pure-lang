use crate::api::studio::bridge_runtime::active_bridge;
use crate::api::studio::convert::runtime::{bridge_persistence_queue, bridge_persistence_state};
use crate::api::studio::types::{
    BridgeError, BridgePersistenceQueueSnapshot, BridgePersistenceStateSnapshot,
};

/// 跳过当前退避并立即重试保存积压的内存事实。
pub async fn retry_persistence() -> Result<BridgePersistenceStateSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_persistence_state(
        bridge.studio.retry_persistence().await?,
    ))
}

/// Retry exactly one latched Thread fault; returns only after its fixed save fence is confirmed.
pub async fn retry_thread_history(
    thread_id: String,
    fault_generation: u64,
) -> Result<BridgePersistenceQueueSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_persistence_queue(
        bridge
            .studio
            .retry_thread_history(&thread_id, fault_generation)
            .await?,
    ))
}

/// 显式继续：在保存重试已按同一代数确认后再解除准入闩，让下一轮模型/工具工作可以开始。
///
/// 与 [`retry_thread_history`] 是两个动作：重试只把积压事实写下去，本命令才恢复执行。代数过期、
/// 后端仍在上报故障或仍有未上交批次时会失败，调用方必须按 typed 存储状态重试而不是假定成功。
pub async fn resume_thread_history(
    thread_id: String,
    fault_generation: u64,
) -> Result<BridgePersistenceQueueSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_persistence_queue(
        bridge
            .studio
            .resume_thread_history(&thread_id, fault_generation)
            .await?,
    ))
}

/// 读取进程级持久化队列压力与逐 Thread 水位。
///
/// 返回协调器已观测到的真实值：队列操作数、字节、在途字节、最老待保存年龄与最近错误，
/// 以及每个 Thread 的 checkpoint/history/calls 水位。纯读取，不触发写入、重试或恢复。
pub async fn read_persistence_queue() -> Result<BridgePersistenceQueueSnapshot, BridgeError> {
    let bridge = active_bridge().await?;
    Ok(bridge_persistence_queue(
        bridge.studio.persistence_queue_snapshot(),
    ))
}
