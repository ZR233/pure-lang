//! 关机阶段进度广播。
//!
//! 独立的短生命周期通道：product stream 在关机早期被取消，不能承载关机进度。
//! 并发 shutdown 共享同一次阶段序列；订阅方只读，不产生副作用。

use std::sync::Arc;

use tokio::sync::broadcast;

use crate::StudioShutdownProgress;

/// 关机进度通道 owner；随 `StudioRuntime` clone 共享。
#[derive(Clone)]
pub struct ShutdownProgressBus {
    tx: Arc<broadcast::Sender<StudioShutdownProgress>>,
}

impl Default for ShutdownProgressBus {
    fn default() -> Self {
        Self::new()
    }
}

impl ShutdownProgressBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(32);
        Self { tx: Arc::new(tx) }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StudioShutdownProgress> {
        self.tx.subscribe()
    }

    pub fn emit(&self, progress: StudioShutdownProgress) {
        let _ = self.tx.send(progress);
    }
}
