use anyhow::Result;
use pl_studio_runtime::{StudioRuntime, StudioRuntimeStatus};
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
            studio: StudioRuntime::default_app().await?,
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
    match bridge.studio.runtime_snapshot().await?.status {
        StudioRuntimeStatus::Ready => Ok(bridge),
        StudioRuntimeStatus::Uninitialized | StudioRuntimeStatus::Initializing => {
            Err(BridgeError::not_initialized())
        }
        StudioRuntimeStatus::ShuttingDown
        | StudioRuntimeStatus::Stopped
        | StudioRuntimeStatus::Failed => Err(BridgeError::runtime_stopped()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::studio::types::BridgeErrorCode;

    #[tokio::test]
    async fn read_boundary_does_not_install_runtime_before_explicit_start() {
        assert!(BRIDGE.get().is_none());

        for _ in 0..3 {
            let error = match active_bridge().await {
                Ok(_) => panic!("query boundary must not install the Bridge runtime"),
                Err(error) => error,
            };
            assert_eq!(error.code, BridgeErrorCode::NotInitialized);
            assert!(BRIDGE.get().is_none());
        }
    }
}
