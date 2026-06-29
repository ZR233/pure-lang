use std::future::Future;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use pl_core::StudioRuntime;

static BRIDGE: OnceLock<BridgeRuntime> = OnceLock::new();

pub(crate) struct BridgeRuntime {
    pub(crate) tokio: tokio::runtime::Runtime,
    pub(crate) studio: StudioRuntime,
}

impl BridgeRuntime {
    fn new() -> Result<Self> {
        let tokio = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("pl-studio-bridge")
            .build()?;
        let studio = tokio.block_on(StudioRuntime::default_app())?;
        Ok(Self { tokio, studio })
    }

    pub(crate) fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        self.tokio.block_on(future)
    }
}

pub(crate) fn bridge() -> Result<&'static BridgeRuntime> {
    if let Some(runtime) = BRIDGE.get() {
        return Ok(runtime);
    }
    let runtime = BridgeRuntime::new()?;
    let _ = BRIDGE.set(runtime);
    BRIDGE
        .get()
        .context("Studio bridge runtime was not initialized")
}
