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

/// Immutable host executable lease retained by session executors, including old generations.
#[cfg(target_os = "linux")]
#[derive(Clone)]
pub(in crate::studio) struct LocalWorkerAsset {
    image: std::sync::Arc<std::fs::File>,
    path: std::path::PathBuf,
}

#[cfg(target_os = "linux")]
impl std::fmt::Debug for LocalWorkerAsset {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalWorkerAsset")
            .field("image", &self.image)
            .finish_non_exhaustive()
    }
}

#[cfg(target_os = "linux")]
impl AsRef<std::path::Path> for LocalWorkerAsset {
    fn as_ref(&self) -> &std::path::Path {
        &self.path
    }
}

#[cfg(all(target_os = "linux", feature = "embedded-remote-helpers"))]
pub(in crate::studio) async fn local_worker() -> crate::Result<LocalWorkerAsset> {
    tokio::task::spawn_blocking(|| {
        use std::io::Write;
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::PermissionsExt;
        let target = match std::env::consts::ARCH {
            "x86_64" => RemoteHelperTarget::X8664Musl,
            "aarch64" => RemoteHelperTarget::Aarch64Musl,
            architecture => {
                return Err(crate::PureError::ConfigError(format!(
                    "unsupported local worker architecture {architecture}"
                )));
            }
        };
        let bytes = BundledRemoteHelpers::default()
            .load(target)
            .map_err(|error| crate::PureError::ConfigError(error.to_string()))?;
        let mut file = tempfile::tempfile().map_err(|error| {
            crate::PureError::ConfigError(format!("create local worker: {error}"))
        })?;
        file.write_all(&bytes)
            .and_then(|()| file.flush())
            .map_err(|error| {
                crate::PureError::ConfigError(format!("materialize local worker: {error}"))
            })?;
        file.set_permissions(std::fs::Permissions::from_mode(0o700))
            .map_err(|error| {
                crate::PureError::ConfigError(format!(
                    "configure local worker permissions: {error}"
                ))
            })?;
        // Reopen read-only before releasing the writer, otherwise exec can fail ETXTBSY.
        // The anonymous inode has no directory entry to leak if the GUI is killed.
        let image = std::fs::File::open(format!("/proc/self/fd/{}", file.as_raw_fd())).map_err(
            |error| crate::PureError::ConfigError(format!("retain local worker image: {error}")),
        )?;
        drop(file);
        let path = std::path::PathBuf::from(format!("/proc/self/fd/{}", image.as_raw_fd()));
        Ok(LocalWorkerAsset {
            image: Arc::new(image),
            path,
        })
    })
    .await
    .map_err(|error| {
        crate::PureError::ConfigError(format!("local worker preparation failed: {error}"))
    })?
}

#[cfg(all(target_os = "linux", not(feature = "embedded-remote-helpers")))]
pub(in crate::studio) async fn local_worker() -> crate::Result<LocalWorkerAsset> {
    tokio::task::spawn_blocking(|| {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::var_os("PATH")
            .and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|directory| directory.join("pl-remote-helper"))
                    .find(|candidate| {
                        candidate.metadata().is_ok_and(|metadata| {
                            metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                        })
                    })
            })
            .ok_or_else(|| {
                crate::PureError::ConfigError(
                    "install pl-remote-helper on PATH for non-embedded Studio execution".into(),
                )
            })?;
        let image = std::fs::File::open(&path).map_err(|error| {
            crate::PureError::ConfigError(format!("open local worker {}: {error}", path.display()))
        })?;
        let path = std::path::PathBuf::from(format!("/proc/self/fd/{}", image.as_raw_fd()));
        Ok(LocalWorkerAsset {
            image: std::sync::Arc::new(image),
            path,
        })
    })
    .await
    .map_err(|error| {
        crate::PureError::ConfigError(format!("local worker preparation failed: {error}"))
    })?
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
