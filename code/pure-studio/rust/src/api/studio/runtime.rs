use anyhow::Result;
use pl_studio_runtime::StudioRuntime;
use tokio::sync::{Mutex, Notify, OnceCell};
use tokio_util::sync::CancellationToken;

use super::subscription::BridgeTaskRegistry;
use super::types::BridgeError;

static BRIDGE: OnceCell<BridgeRuntime> = OnceCell::const_new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BridgeLifecycle {
    Initialized,
    Started,
    ShuttingDown,
    Stopped,
}

pub(crate) struct BridgeRuntime {
    pub(crate) studio: StudioRuntime,
    pub(crate) subscriptions: BridgeTaskRegistry,
    pub(crate) shutdown: CancellationToken,
    pub(crate) shutdown_complete: Notify,
    pub(crate) lifecycle: Mutex<BridgeLifecycle>,
}

impl BridgeRuntime {
    async fn new() -> Result<Self> {
        Ok(Self {
            studio: StudioRuntime::default_app().await?,
            subscriptions: BridgeTaskRegistry::new(),
            shutdown: CancellationToken::new(),
            shutdown_complete: Notify::new(),
            lifecycle: Mutex::new(BridgeLifecycle::Initialized),
        })
    }
}

pub(crate) async fn bridge() -> Result<&'static BridgeRuntime> {
    BRIDGE.get_or_try_init(BridgeRuntime::new).await
}

pub(crate) async fn active_bridge() -> Result<&'static BridgeRuntime, BridgeError> {
    let bridge = bridge().await?;
    match *bridge.lifecycle.lock().await {
        BridgeLifecycle::Initialized | BridgeLifecycle::Started => Ok(bridge),
        BridgeLifecycle::ShuttingDown | BridgeLifecycle::Stopped => {
            Err(BridgeError::runtime_stopped())
        }
    }
}
