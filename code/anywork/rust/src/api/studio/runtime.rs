use anyhow::Result;
use pl_studio_runtime::{StudioRuntime, StudioRuntimeStateKind};
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;

use super::subscription::BridgeTaskRegistry;
use super::types::BridgeError;

static BRIDGE: OnceCell<BridgeRuntime> = OnceCell::const_new();

pub(crate) struct BridgeRuntime {
    pub(crate) studio: StudioRuntime,
    pub(crate) subscriptions: BridgeTaskRegistry,
    pub(crate) shutdown: CancellationToken,
}

impl BridgeRuntime {
    async fn new() -> Result<Self> {
        Ok(Self {
            studio: StudioRuntime::with_startup_observer(
                pl_studio_runtime::StudioRuntimeOptions::desktop(),
                std::sync::Arc::new(super::handlers::lifecycle::publish_startup_stage),
            )
            .await?,
            subscriptions: BridgeTaskRegistry::new(),
            shutdown: CancellationToken::new(),
        })
    }
}

/// 构造并安装 Bridge runtime；只能由显式启动命令调用。
pub(crate) async fn install_bridge_runtime() -> Result<&'static BridgeRuntime> {
    BRIDGE.get_or_try_init(BridgeRuntime::new).await
}

pub(crate) fn installed_bridge() -> Result<&'static BridgeRuntime, BridgeError> {
    BRIDGE.get().ok_or_else(BridgeError::not_initialized)
}

pub(crate) async fn active_bridge() -> Result<&'static BridgeRuntime, BridgeError> {
    let bridge = installed_bridge()?;
    match bridge.studio.runtime_snapshot().await?.state.kind() {
        StudioRuntimeStateKind::Ready => Ok(bridge),
        StudioRuntimeStateKind::Uninitialized | StudioRuntimeStateKind::Initializing => {
            Err(BridgeError::not_initialized())
        }
        StudioRuntimeStateKind::ShuttingDown
        | StudioRuntimeStateKind::Stopped
        | StudioRuntimeStateKind::Failed => Err(BridgeError::runtime_stopped()),
    }
}
