//! GUI 二进制内嵌 remote helper 的宿主 adapter。

#[cfg(feature = "embedded-remote-helpers")]
use pl_core::remote::{RemoteClientError, RemoteHelperAssets, RemoteHelperTarget, SshManager};
#[cfg(feature = "embedded-remote-helpers")]
use rust_embed::Embed;
#[cfg(feature = "embedded-remote-helpers")]
use std::sync::{Arc, OnceLock};

#[cfg(feature = "embedded-remote-helpers")]
#[derive(Embed)]
#[folder = "../../dist/remote-helper/aarch64-unknown-linux-musl/"]
#[compression = "zstd"]
struct BundledAarch64Helper;

#[cfg(feature = "embedded-remote-helpers")]
#[derive(Embed)]
#[folder = "../../dist/remote-helper/x86_64-unknown-linux-musl/"]
#[compression = "zstd"]
struct BundledX8664Helper;

#[cfg(feature = "embedded-remote-helpers")]
#[derive(Debug, Default)]
struct BundledRemoteHelpers {
    aarch64: OnceLock<Arc<[u8]>>,
    x86_64: OnceLock<Arc<[u8]>>,
}

#[cfg(feature = "embedded-remote-helpers")]
impl RemoteHelperAssets for BundledRemoteHelpers {
    fn load(&self, target: RemoteHelperTarget) -> Result<Arc<[u8]>, RemoteClientError> {
        let cache = match target {
            RemoteHelperTarget::Aarch64Musl => &self.aarch64,
            RemoteHelperTarget::X8664Musl => &self.x86_64,
        };
        if let Some(bytes) = cache.get() {
            return Ok(bytes.clone());
        }
        let asset = match target {
            RemoteHelperTarget::Aarch64Musl => BundledAarch64Helper::get("pl-remote-helper"),
            RemoteHelperTarget::X8664Musl => BundledX8664Helper::get("pl-remote-helper"),
        }
        .ok_or_else(|| {
            RemoteClientError::Protocol(format!(
                "embedded helper for {} is missing",
                target.triple()
            ))
        })?;
        let loaded = Arc::<[u8]>::from(asset.data.into_owned());
        if cache.set(loaded.clone()).is_ok() {
            return Ok(loaded);
        }
        cache.get().cloned().ok_or_else(|| {
            RemoteClientError::Protocol("embedded helper cache initialization failed".to_string())
        })
    }
}

#[cfg(feature = "embedded-remote-helpers")]
pub(super) fn ssh_manager() -> SshManager {
    SshManager::with_helper_assets(Arc::new(BundledRemoteHelpers::default()))
}

#[cfg(not(feature = "embedded-remote-helpers"))]
pub(super) fn ssh_manager() -> pl_core::remote::SshManager {
    pl_core::remote::SshManager::new(None, None)
}

#[cfg(all(test, feature = "embedded-remote-helpers"))]
mod tests {
    use super::*;

    #[test]
    fn both_embedded_targets_decompress_on_demand() {
        let helpers = BundledRemoteHelpers::default();
        let aarch64 = helpers
            .load(RemoteHelperTarget::Aarch64Musl)
            .expect("embedded aarch64 helper");
        assert!(!aarch64.is_empty());
        assert!(helpers.x86_64.get().is_none());
        let cached = helpers
            .load(RemoteHelperTarget::Aarch64Musl)
            .expect("cached aarch64 helper");
        assert!(Arc::ptr_eq(&aarch64, &cached));

        let x86_64 = helpers
            .load(RemoteHelperTarget::X8664Musl)
            .expect("embedded x86_64 helper");
        assert!(!x86_64.is_empty());
    }
}
