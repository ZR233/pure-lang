//! Studio runtime 的 remote helper 来源：桌面随包压缩资源或外部安装。

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use pl_tool::remote::{RemoteClientError, RemoteHelperAssets, RemoteHelperTarget, SshManager};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// One decompressed helper must stay far below this bound; it only guards against
/// unbounded decompression of a corrupted or replaced resource.
const MAX_HELPER_BYTES: usize = 64 * 1024 * 1024;
const BUNDLED_HELPER_ARCHIVE: &str = "pl-remote-helper.zst";
const BUNDLED_HELPER_METADATA: &str = "pl-remote-helper.metadata.json";

/// Explicit origin of `pl-remote-helper` bytes for one Studio runtime.
///
/// `Bundled` points at the desktop bundle's `data/remote-helper/` directory and is
/// resolved by the packaged bridge from the application executable location.
/// `External` is the explicit mode of unpackaged servers, development and
/// observation entries; on Linux the local worker is then looked up on `PATH`.
#[derive(Debug, Clone)]
pub enum RemoteHelperSource {
    Bundled(PathBuf),
    External,
}

/// Metadata shipped beside the compressed helper; binds it to one original executable.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BundledHelperMetadata {
    target: String,
    worker_protocol_version: u32,
    sha256: String,
}

pub(crate) fn ssh_manager(source: &RemoteHelperSource) -> SshManager {
    match source {
        RemoteHelperSource::Bundled(root) => {
            SshManager::with_helper_assets(Arc::new(BundledHelperAssets::new(root.clone())))
        }
        RemoteHelperSource::External => SshManager::new(None, None),
    }
}

#[derive(Debug)]
struct BundledHelperAssets {
    root: PathBuf,
    aarch64: OnceLock<Arc<[u8]>>,
    x86_64: OnceLock<Arc<[u8]>>,
}

impl BundledHelperAssets {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            aarch64: OnceLock::new(),
            x86_64: OnceLock::new(),
        }
    }
}

impl RemoteHelperAssets for BundledHelperAssets {
    fn load(&self, target: RemoteHelperTarget) -> Result<Arc<[u8]>, RemoteClientError> {
        let cache = match target {
            RemoteHelperTarget::Aarch64Musl => &self.aarch64,
            RemoteHelperTarget::X8664Musl => &self.x86_64,
        };
        if let Some(bytes) = cache.get() {
            return Ok(bytes.clone());
        }
        let loaded =
            load_bundled_helper(&self.root, target).map_err(RemoteClientError::Protocol)?;
        if cache.set(loaded.clone()).is_ok() {
            return Ok(loaded);
        }
        cache.get().cloned().ok_or_else(|| {
            RemoteClientError::Protocol("bundled helper cache initialization failed".to_string())
        })
    }
}

/// Reads, verifies and decompresses one bundled helper resource.
///
/// Errors report the offending resource path and the failed check so callers can
/// surface an actionable repair message; there is no fallback to `PATH` or the
/// network in bundled mode.
fn load_bundled_helper(root: &Path, target: RemoteHelperTarget) -> Result<Arc<[u8]>, String> {
    let directory = root.join(target.triple());
    let archive_path = directory.join(BUNDLED_HELPER_ARCHIVE);
    let metadata_path = directory.join(BUNDLED_HELPER_METADATA);
    let archive = fs::read(&archive_path)
        .map_err(|error| format!("read bundled helper {}: {error}", archive_path.display()))?;
    let metadata_bytes = fs::read(&metadata_path).map_err(|error| {
        format!(
            "read bundled helper metadata {}: {error}",
            metadata_path.display()
        )
    })?;
    let metadata: BundledHelperMetadata =
        serde_json::from_slice(&metadata_bytes).map_err(|error| {
            format!(
                "parse bundled helper metadata {}: {error}",
                metadata_path.display()
            )
        })?;
    if metadata.target != target.triple() {
        return Err(format!(
            "bundled helper metadata {} declares target '{}', expected '{}'",
            metadata_path.display(),
            metadata.target,
            target.triple()
        ));
    }
    if metadata.worker_protocol_version
        != pl_protocol::process_worker::PROCESS_WORKER_PROTOCOL_VERSION
    {
        return Err(format!(
            "bundled helper metadata {} declares worker protocol {}, expected {}; rebuild the \
             application bundle",
            metadata_path.display(),
            metadata.worker_protocol_version,
            pl_protocol::process_worker::PROCESS_WORKER_PROTOCOL_VERSION
        ));
    }
    let executable = zstd::bulk::decompress(&archive, MAX_HELPER_BYTES).map_err(|error| {
        format!(
            "decompress bundled helper {}: {error}",
            archive_path.display()
        )
    })?;
    let digest = hex::encode(Sha256::digest(&executable));
    if !digest.eq_ignore_ascii_case(&metadata.sha256) {
        return Err(format!(
            "bundled helper {} content digest {digest} does not match metadata {}",
            archive_path.display(),
            metadata.sha256
        ));
    }
    Ok(Arc::from(executable.into_boxed_slice()))
}

/// Immutable host executable lease retained by session executors, including old generations.
#[cfg(target_os = "linux")]
#[derive(Clone)]
pub(crate) struct LocalWorkerAsset {
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

#[cfg(target_os = "linux")]
pub(crate) async fn local_worker(source: RemoteHelperSource) -> crate::Result<LocalWorkerAsset> {
    tokio::task::spawn_blocking(move || materialize_local_worker(&source))
        .await
        .map_err(|error| {
            crate::PureError::ConfigError(format!("local worker preparation failed: {error}"))
        })?
}

#[cfg(target_os = "linux")]
fn materialize_local_worker(source: &RemoteHelperSource) -> crate::Result<LocalWorkerAsset> {
    match source {
        RemoteHelperSource::Bundled(root) => {
            let target = host_helper_target()?;
            let executable =
                load_bundled_helper(root, target).map_err(crate::PureError::ConfigError)?;
            materialize_anonymous_worker(&executable)
        }
        RemoteHelperSource::External => open_external_worker(),
    }
}

#[cfg(target_os = "linux")]
fn host_helper_target() -> crate::Result<RemoteHelperTarget> {
    match std::env::consts::ARCH {
        "x86_64" => Ok(RemoteHelperTarget::X8664Musl),
        "aarch64" => Ok(RemoteHelperTarget::Aarch64Musl),
        architecture => Err(crate::PureError::ConfigError(format!(
            "unsupported local worker architecture {architecture}"
        ))),
    }
}

/// Writes the verified bytes into an anonymous executable image and reopens it
/// read-only, so later resource replacements cannot mutate a running image.
#[cfg(target_os = "linux")]
fn materialize_anonymous_worker(bytes: &[u8]) -> crate::Result<LocalWorkerAsset> {
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::PermissionsExt;

    let mut file = tempfile::tempfile()
        .map_err(|error| crate::PureError::ConfigError(format!("create local worker: {error}")))?;
    file.write_all(bytes)
        .and_then(|()| file.flush())
        .map_err(|error| {
            crate::PureError::ConfigError(format!("materialize local worker: {error}"))
        })?;
    file.set_permissions(std::fs::Permissions::from_mode(0o700))
        .map_err(|error| {
            crate::PureError::ConfigError(format!("configure local worker permissions: {error}"))
        })?;
    // Reopen read-only before releasing the writer, otherwise exec can fail ETXTBSY.
    // The anonymous inode has no directory entry to leak if the GUI is killed.
    let image =
        std::fs::File::open(format!("/proc/self/fd/{}", file.as_raw_fd())).map_err(|error| {
            crate::PureError::ConfigError(format!("retain local worker image: {error}"))
        })?;
    drop(file);
    let path = std::path::PathBuf::from(format!("/proc/self/fd/{}", image.as_raw_fd()));
    Ok(LocalWorkerAsset {
        image: std::sync::Arc::new(image),
        path,
    })
}

#[cfg(target_os = "linux")]
fn open_external_worker() -> crate::Result<LocalWorkerAsset> {
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
                "install pl-remote-helper on PATH for non-bundled Studio execution".into(),
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
}
