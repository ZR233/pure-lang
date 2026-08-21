use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fs4::{FileExt, TryLockError};
use pl_protocol::studio::{StudioError, StudioResult};
use serde::Serialize;

use super::paths::StudioPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioHostKind {
    Desktop,
    HttpServer,
    Test,
}

impl StudioHostKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::HttpServer => "httpServer",
            Self::Test => "test",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StudioRuntimeOptions {
    pub studio_home: Option<PathBuf>,
    pub host: StudioHostKind,
}

impl StudioRuntimeOptions {
    pub fn desktop() -> Self {
        Self {
            studio_home: None,
            host: StudioHostKind::Desktop,
        }
    }

    pub fn http_server(studio_home: Option<PathBuf>) -> Self {
        Self {
            studio_home,
            host: StudioHostKind::HttpServer,
        }
    }
}

#[derive(Debug)]
pub(super) struct RuntimeLock {
    file: File,
}

#[derive(Clone, Default)]
pub(super) struct RuntimeLockOwner {
    lock: Arc<Mutex<Option<RuntimeLock>>>,
}

impl RuntimeLockOwner {
    pub(super) fn new(lock: Option<RuntimeLock>) -> Self {
        Self {
            lock: Arc::new(Mutex::new(lock)),
        }
    }

    pub(super) fn release(&self) {
        let mut lock = match self.lock.lock() {
            Ok(lock) => lock,
            Err(poisoned) => poisoned.into_inner(),
        };
        lock.take();
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeLockMetadata<'a> {
    pid: u32,
    host: &'a str,
    started_at: u64,
}

impl RuntimeLock {
    pub(super) fn acquire(path: &Path, host: StudioHostKind) -> StudioResult<Self> {
        let parent = path.parent().ok_or_else(|| {
            StudioError::invalid_argument("Studio runtime lock path has no parent directory")
        })?;
        std::fs::create_dir_all(parent).map_err(|error| {
            tracing::error!(error = %error, "failed to create Studio runtime directory");
            StudioError::storage()
        })?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|error| {
                tracing::error!(error = %error, "failed to open Studio runtime lock");
                StudioError::storage()
            })?;
        match FileExt::try_lock(&file) {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(StudioError::instance_busy()),
            Err(TryLockError::Error(error)) => {
                tracing::error!(error = %error, "failed to acquire Studio runtime lock");
                return Err(StudioError::storage());
            }
        }
        let metadata = RuntimeLockMetadata {
            pid: std::process::id(),
            host: host.as_str(),
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_secs()),
        };
        let encoded = serde_json::to_vec(&metadata).map_err(|error| {
            tracing::error!(error = %error, "failed to encode Studio runtime lock metadata");
            StudioError::internal()
        })?;
        file.set_len(0)
            .and_then(|()| file.seek(SeekFrom::Start(0)))
            .and_then(|_| file.write_all(&encoded))
            .and_then(|()| file.sync_data())
            .map_err(|error| {
                tracing::error!(error = %error, "failed to write Studio runtime lock metadata");
                StudioError::storage()
            })?;
        Ok(Self { file })
    }
}

impl Drop for RuntimeLock {
    fn drop(&mut self) {
        if let Err(error) = FileExt::unlock(&self.file) {
            tracing::warn!(error = %error, "failed to release Studio runtime lock explicitly");
        }
    }
}

pub(super) struct ResolvedRuntimeOptions {
    pub paths: StudioPaths,
    pub host: StudioHostKind,
}

impl StudioRuntimeOptions {
    pub(super) fn resolve(self) -> StudioResult<ResolvedRuntimeOptions> {
        let paths = StudioPaths::resolve(self.studio_home)
            .map_err(|error| StudioError::invalid_argument(error.to_string()))?;
        Ok(ResolvedRuntimeOptions {
            paths,
            host: self.host,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    const CREDENTIAL_ISOLATION_CHILD: &str = "PURE_STUDIO_CREDENTIAL_ISOLATION_CHILD";

    #[test]
    fn lock_owner_releases_across_all_clones() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("studio/runtime.lock");
        let owner = RuntimeLockOwner::new(Some(
            RuntimeLock::acquire(&path, StudioHostKind::Test).unwrap(),
        ));
        let clone = owner.clone();

        let busy = RuntimeLock::acquire(&path, StudioHostKind::Test).unwrap_err();
        assert_eq!(
            busy.code,
            pl_protocol::studio::StudioErrorCode::InstanceBusy
        );
        clone.release();
        RuntimeLock::acquire(&path, StudioHostKind::Test).unwrap();
        drop(owner);
    }

    #[tokio::test]
    async fn complete_shutdown_allows_a_new_runtime_for_the_same_home() {
        let home = tempfile::tempdir().unwrap();
        let options = || StudioRuntimeOptions {
            studio_home: Some(home.path().to_path_buf()),
            host: StudioHostKind::Test,
        };
        let first = crate::StudioRuntime::with_options(options()).await.unwrap();
        first.start_runtime().await.unwrap();
        let surviving_clone = first.clone();

        let busy = match crate::StudioRuntime::with_options(options()).await {
            Ok(_) => panic!("second runtime unexpectedly acquired the same Studio home"),
            Err(error) => error,
        };
        assert_eq!(
            busy.code,
            pl_protocol::studio::StudioErrorCode::InstanceBusy
        );

        first.shutdown_runtime().await.unwrap();
        crate::StudioRuntime::with_options(options()).await.unwrap();
        drop(surviving_clone);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_host_runtime_does_not_require_a_desktop_credential_service() {
        if std::env::var_os(CREDENTIAL_ISOLATION_CHILD).is_some() {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let home = tempfile::tempdir().unwrap();
                crate::StudioRuntime::with_options(StudioRuntimeOptions {
                    studio_home: Some(home.path().to_path_buf()),
                    host: StudioHostKind::Test,
                })
                .await
                .unwrap();
            });
            return;
        }

        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg(
                "studio::runtime_lock::tests::test_host_runtime_does_not_require_a_desktop_credential_service",
            )
            .arg("--nocapture")
            .env(CREDENTIAL_ISOLATION_CHILD, "1")
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/definitely-missing/pure-studio-ci-bus",
            )
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "isolated test runtime unexpectedly required the Linux desktop credential service\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}
