use super::client::StudioUpdater;
use super::error::{StudioUpdateError, StudioUpdateErrorCode};
use super::manifest::{MAX_INSTALLER_BYTES, validate_update};
use super::types::{StudioUpdate, StudioUpdateEvent};
use futures::StreamExt;
use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};
use std::future::Future;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use url::Url;

const MAX_SIGNATURE_BYTES: u64 = 16 * 1024;

#[derive(Clone)]
pub struct StudioUpdateCancellation {
    inner: Arc<StudioUpdateCancellationInner>,
}

struct StudioUpdateCancellationInner {
    token: CancellationToken,
    phase: Mutex<StudioUpdateCancellationPhase>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StudioUpdateCancellationPhase {
    Downloading,
    Cancelled,
    Launching,
    Finished,
}

impl StudioUpdateCancellation {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(StudioUpdateCancellationInner {
                token: CancellationToken::new(),
                phase: Mutex::new(StudioUpdateCancellationPhase::Downloading),
            }),
        }
    }

    pub fn cancel(&self) -> Result<(), StudioUpdateError> {
        let mut phase = self
            .inner
            .phase
            .lock()
            .expect("update cancellation phase lock must not be poisoned");
        match *phase {
            StudioUpdateCancellationPhase::Downloading => {
                *phase = StudioUpdateCancellationPhase::Cancelled;
                self.inner.token.cancel();
                Ok(())
            }
            StudioUpdateCancellationPhase::Cancelled => Ok(()),
            StudioUpdateCancellationPhase::Launching | StudioUpdateCancellationPhase::Finished => {
                Err(StudioUpdateError::new(
                    StudioUpdateErrorCode::CancellationTooLate,
                    "the verified installer is already launching",
                ))
            }
        }
    }

    fn check(&self) -> Result<(), StudioUpdateError> {
        if self.inner.token.is_cancelled() {
            return Err(cancelled_error());
        }
        Ok(())
    }

    fn begin_launch(&self) -> Result<(), StudioUpdateError> {
        let mut phase = self
            .inner
            .phase
            .lock()
            .expect("update cancellation phase lock must not be poisoned");
        match *phase {
            StudioUpdateCancellationPhase::Downloading => {
                *phase = StudioUpdateCancellationPhase::Launching;
                Ok(())
            }
            StudioUpdateCancellationPhase::Cancelled => Err(cancelled_error()),
            StudioUpdateCancellationPhase::Launching | StudioUpdateCancellationPhase::Finished => {
                Err(StudioUpdateError::new(
                    StudioUpdateErrorCode::CancellationTooLate,
                    "the verified installer is already launching",
                ))
            }
        }
    }

    fn finish(&self) {
        let mut phase = self
            .inner
            .phase
            .lock()
            .expect("update cancellation phase lock must not be poisoned");
        if *phase == StudioUpdateCancellationPhase::Launching {
            *phase = StudioUpdateCancellationPhase::Finished;
        }
    }
}

impl Default for StudioUpdateCancellation {
    fn default() -> Self {
        Self::new()
    }
}

pub(super) async fn install_after<F, Fut>(
    updater: &StudioUpdater,
    update: StudioUpdate,
    progress: UnboundedSender<StudioUpdateEvent>,
    cancellation: StudioUpdateCancellation,
    before_launch: F,
) -> Result<(), StudioUpdateError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<(), StudioUpdateError>>,
{
    let guard = InstallGuard::acquire(updater)?;
    let result: Result<(), StudioUpdateError> = async {
        validate_update(&update)?;
        cancellation.check()?;
        let _ = progress.send(StudioUpdateEvent::Started {
            total: update.installer.size,
        });
        let installer = prepare_installer(updater, &update, &progress, &cancellation).await?;
        cancellation.check()?;
        before_launch().await?;
        cancellation.begin_launch()?;
        launch_installer(&installer)?;
        cancellation.finish();
        let _ = progress.send(StudioUpdateEvent::InstallerLaunched);
        Ok(())
    }
    .await;
    if let Err(error) = &result {
        let _ = progress.send(StudioUpdateEvent::Failed {
            code: error.code().as_str().to_string(),
            message: error.to_string(),
        });
    }
    drop(guard);
    result
}

async fn prepare_installer(
    updater: &StudioUpdater,
    update: &StudioUpdate,
    progress: &UnboundedSender<StudioUpdateEvent>,
    cancellation: &StudioUpdateCancellation,
) -> Result<PathBuf, StudioUpdateError> {
    cancellation.check()?;
    let version_dir = updater.cache_dir.join(&update.version);
    tokio::fs::create_dir_all(&version_dir)
        .await
        .map_err(io_error)?;
    let filename = asset_filename(&update.installer.url)?;
    let installer = version_dir.join(&filename);
    let signature = version_dir.join(format!("{filename}.minisig"));

    if installer.is_file()
        && signature.is_file()
        && verify_cached(
            updater,
            update,
            &installer,
            &signature,
            progress,
            cancellation,
        )
        .await?
    {
        return Ok(installer);
    }
    remove_if_exists(&installer).await?;
    remove_if_exists(&signature).await?;

    let signature_url = Url::parse(&update.installer.signature).map_err(|error| {
        StudioUpdateError::new(
            StudioUpdateErrorCode::InvalidManifest,
            format!("invalid signature URL: {error}"),
        )
    })?;
    let signature_bytes = tokio::select! {
        _ = cancellation.inner.token.cancelled() => return Err(cancelled_error()),
        result = updater.request_bytes(signature_url, MAX_SIGNATURE_BYTES) => result?,
    };
    cancellation.check()?;
    let signature_partial = PathBuf::from(format!("{}.partial", signature.display()));
    remove_if_exists(&signature_partial).await?;
    let signature_result = async {
        write_partial(&signature_partial, &signature_bytes).await?;
        rename_replacing(&signature_partial, &signature).await
    }
    .await;
    if let Err(error) = signature_result {
        let _ = remove_if_exists(&signature_partial).await;
        return Err(error);
    }

    let partial = PathBuf::from(format!("{}.partial", installer.display()));
    remove_if_exists(&partial).await?;
    let prepare_result = async {
        download_installer(updater, update, &partial, progress, cancellation).await?;
        cancellation.check()?;
        let _ = progress.send(StudioUpdateEvent::Verifying);
        verify_file(updater.public_key, &signature, &partial).await
    }
    .await;
    if let Err(error) = prepare_result {
        let _ = remove_if_exists(&partial).await;
        return Err(error);
    }
    rename_replacing(&partial, &installer).await?;
    Ok(installer)
}

async fn verify_cached(
    updater: &StudioUpdater,
    update: &StudioUpdate,
    installer: &Path,
    signature: &Path,
    progress: &UnboundedSender<StudioUpdateEvent>,
    cancellation: &StudioUpdateCancellation,
) -> Result<bool, StudioUpdateError> {
    cancellation.check()?;
    if tokio::fs::metadata(installer)
        .await
        .map_err(io_error)?
        .len()
        != update.installer.size
    {
        return Ok(false);
    }
    let actual_hash = hash_file(installer).await?;
    cancellation.check()?;
    if actual_hash != update.installer.sha256 {
        return Ok(false);
    }
    let _ = progress.send(StudioUpdateEvent::Verifying);
    match verify_file(updater.public_key, signature, installer).await {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

async fn download_installer(
    updater: &StudioUpdater,
    update: &StudioUpdate,
    partial: &Path,
    progress: &UnboundedSender<StudioUpdateEvent>,
    cancellation: &StudioUpdateCancellation,
) -> Result<(), StudioUpdateError> {
    let url = Url::parse(&update.installer.url).map_err(|error| {
        StudioUpdateError::new(
            StudioUpdateErrorCode::InvalidManifest,
            format!("invalid installer URL: {error}"),
        )
    })?;
    let response = tokio::select! {
        _ = cancellation.inner.token.cancelled() => return Err(cancelled_error()),
        result = updater.request(url) => result?,
    };
    if response
        .content_length()
        .is_some_and(|size| size != update.installer.size)
    {
        return Err(StudioUpdateError::new(
            StudioUpdateErrorCode::DownloadIncomplete,
            "installer Content-Length does not match the manifest",
        ));
    }
    let mut file = tokio::fs::File::create(partial).await.map_err(io_error)?;
    let mut stream = response.bytes_stream();
    let mut downloaded = 0_u64;
    let mut hasher = Sha256::new();
    while let Some(chunk) = stream.next().await {
        cancellation.check()?;
        let chunk = chunk.map_err(|error| {
            StudioUpdateError::new(
                StudioUpdateErrorCode::Network,
                format!("installer download failed: {error}"),
            )
        })?;
        downloaded = downloaded
            .checked_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX))
            .ok_or_else(|| {
                StudioUpdateError::new(
                    StudioUpdateErrorCode::DownloadTooLarge,
                    "installer download size overflow",
                )
            })?;
        if downloaded > update.installer.size || downloaded > MAX_INSTALLER_BYTES {
            return Err(StudioUpdateError::new(
                StudioUpdateErrorCode::DownloadTooLarge,
                "installer exceeded its declared size",
            ));
        }
        file.write_all(&chunk).await.map_err(io_error)?;
        hasher.update(&chunk);
        let _ = progress.send(StudioUpdateEvent::Progress {
            downloaded,
            total: update.installer.size,
        });
    }
    file.flush().await.map_err(io_error)?;
    if downloaded != update.installer.size {
        return Err(StudioUpdateError::new(
            StudioUpdateErrorCode::DownloadIncomplete,
            format!(
                "installer download length {downloaded} does not match {}",
                update.installer.size
            ),
        ));
    }
    let actual_hash = format!("{:x}", hasher.finalize());
    if actual_hash != update.installer.sha256 {
        return Err(StudioUpdateError::new(
            StudioUpdateErrorCode::HashMismatch,
            "installer SHA-256 does not match the manifest",
        ));
    }
    Ok(())
}

fn cancelled_error() -> StudioUpdateError {
    StudioUpdateError::new(
        StudioUpdateErrorCode::Cancelled,
        "Studio update installation was cancelled",
    )
}

async fn verify_file(
    public_key: &'static str,
    signature_path: &Path,
    message_path: &Path,
) -> Result<(), StudioUpdateError> {
    let signature_path = signature_path.to_path_buf();
    let message_path = message_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let public_key = PublicKey::decode(public_key).map_err(signature_error)?;
        let signature_text = std::fs::read_to_string(&signature_path).map_err(io_error)?;
        let signature = Signature::decode(&signature_text).map_err(signature_error)?;
        let mut verifier = public_key
            .verify_stream(&signature)
            .map_err(signature_error)?;
        let mut file = std::fs::File::open(&message_path).map_err(io_error)?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).map_err(io_error)?;
            if count == 0 {
                break;
            }
            verifier.update(&buffer[..count]);
        }
        verifier.finalize().map_err(signature_error)
    })
    .await
    .map_err(|error| {
        StudioUpdateError::new(
            StudioUpdateErrorCode::SignatureInvalid,
            format!("signature verification task failed: {error}"),
        )
    })?
}

async fn hash_file(path: &Path) -> Result<String, StudioUpdateError> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(path).map_err(io_error)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).map_err(io_error)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    })
    .await
    .map_err(|error| {
        StudioUpdateError::new(
            StudioUpdateErrorCode::Io,
            format!("hash task failed: {error}"),
        )
    })?
}

fn launch_installer(path: &Path) -> Result<(), StudioUpdateError> {
    if !cfg!(target_os = "windows") {
        return Err(StudioUpdateError::new(
            StudioUpdateErrorCode::UnsupportedPlatform,
            "Pure Studio in-app installation currently supports Windows only",
        ));
    }
    let mut command = Command::new(path);
    command.args(installer_arguments());
    crate::process::configure_background_std_command(&mut command);
    command.spawn().map(|_| ()).map_err(|error| {
        StudioUpdateError::new(
            StudioUpdateErrorCode::InstallerLaunchFailed,
            format!("failed to launch verified installer: {error}"),
        )
    })
}

fn installer_arguments() -> [&'static str; 5] {
    [
        "/SILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART",
        "/CLOSEAPPLICATIONS",
        "/RESTARTAPPLICATIONS",
    ]
}

fn asset_filename(raw: &str) -> Result<String, StudioUpdateError> {
    Url::parse(raw)
        .ok()
        .and_then(|url| url.path_segments()?.next_back().map(str::to_string))
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            StudioUpdateError::new(
                StudioUpdateErrorCode::InvalidManifest,
                "installer URL has no filename",
            )
        })
}

async fn write_partial(path: &Path, bytes: &[u8]) -> Result<(), StudioUpdateError> {
    let mut file = tokio::fs::File::create(path).await.map_err(io_error)?;
    file.write_all(bytes).await.map_err(io_error)?;
    file.flush().await.map_err(io_error)
}

async fn rename_replacing(from: &Path, to: &Path) -> Result<(), StudioUpdateError> {
    remove_if_exists(to).await?;
    tokio::fs::rename(from, to).await.map_err(io_error)
}

async fn remove_if_exists(path: &Path) -> Result<(), StudioUpdateError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

fn io_error(error: std::io::Error) -> StudioUpdateError {
    StudioUpdateError::new(
        StudioUpdateErrorCode::Io,
        format!("update file operation failed: {error}"),
    )
}

fn signature_error(error: minisign_verify::Error) -> StudioUpdateError {
    StudioUpdateError::new(
        StudioUpdateErrorCode::SignatureInvalid,
        format!("Minisign verification failed: {error}"),
    )
}

struct InstallGuard<'a> {
    updater: &'a StudioUpdater,
}

impl<'a> InstallGuard<'a> {
    fn acquire(updater: &'a StudioUpdater) -> Result<Self, StudioUpdateError> {
        updater
            .install_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                StudioUpdateError::new(
                    StudioUpdateErrorCode::InstallInProgress,
                    "another Studio update installation is already running",
                )
            })?;
        Ok(Self { updater })
    }
}

impl Drop for InstallGuard<'_> {
    fn drop(&mut self) {
        self.updater.install_active.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const TEST_PUBLIC_KEY: &str = "untrusted comment: minisign public key 2\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const TEST_SIGNATURE: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";

    #[test]
    fn minisign_accepts_valid_content_and_rejects_tampering() {
        let public_key = PublicKey::decode(TEST_PUBLIC_KEY).unwrap();
        let signature = Signature::decode(TEST_SIGNATURE).unwrap();
        public_key.verify(b"test", &signature, true).unwrap();
        assert!(public_key.verify(b"tampered", &signature, true).is_err());

        let wrong_signature = Signature::decode(&TEST_SIGNATURE.replacen("559r3", "558r3", 1))
            .expect("modified signature should remain structurally valid");
        assert!(public_key.verify(b"test", &wrong_signature, true).is_err());
    }

    #[tokio::test]
    async fn verified_cache_is_reused() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let cache_dir = std::env::temp_dir().join(format!(
            "pure-studio-updater-test-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&cache_dir).unwrap();
        let installer = cache_dir.join("setup.exe");
        let signature = cache_dir.join("setup.exe.minisig");
        std::fs::write(&installer, b"test").unwrap();
        std::fs::write(&signature, TEST_SIGNATURE).unwrap();
        let updater = StudioUpdater {
            client: reqwest::Client::new(),
            cache_dir: cache_dir.clone(),
            public_key: TEST_PUBLIC_KEY,
            install_active: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        let update = StudioUpdate {
            version: "1.2.3".to_string(),
            published_at: 1,
            notes_url: "https://github.com/ZR233/pure-lang/releases/tag/v1.2.3".to_string(),
            installer: super::super::types::StudioUpdateAsset {
                url: "https://github.com/ZR233/pure-lang/releases/download/v1.2.3/Pure-Studio-1.2.3-windows-x86_64-setup.exe".to_string(),
                size: 4,
                sha256: format!("{:x}", Sha256::digest(b"test")),
                signature: "https://github.com/ZR233/pure-lang/releases/download/v1.2.3/Pure-Studio-1.2.3-windows-x86_64-setup.exe.minisig".to_string(),
            },
        };
        let (progress, mut events) = tokio::sync::mpsc::unbounded_channel();

        assert!(
            verify_cached(
                &updater,
                &update,
                &installer,
                &signature,
                &progress,
                &StudioUpdateCancellation::new(),
            )
            .await
            .unwrap()
        );
        assert_eq!(events.recv().await, Some(StudioUpdateEvent::Verifying));
        std::fs::remove_dir_all(cache_dir).unwrap();
    }

    #[test]
    fn installer_command_requests_close_and_restart() {
        assert_eq!(
            installer_arguments(),
            [
                "/SILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/CLOSEAPPLICATIONS",
                "/RESTARTAPPLICATIONS",
            ]
        );
    }

    #[test]
    fn concurrent_install_guard_is_rejected() {
        let updater = StudioUpdater {
            client: reqwest::Client::new(),
            cache_dir: PathBuf::new(),
            public_key: TEST_PUBLIC_KEY,
            install_active: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        let first = InstallGuard::acquire(&updater).unwrap();
        let second = match InstallGuard::acquire(&updater) {
            Ok(_) => panic!("second install guard must be rejected"),
            Err(error) => error,
        };
        assert_eq!(second.code(), StudioUpdateErrorCode::InstallInProgress);
        drop(first);
        assert!(InstallGuard::acquire(&updater).is_ok());
    }

    #[test]
    fn cancellation_is_idempotent_before_installer_launch() {
        let cancellation = StudioUpdateCancellation::new();

        cancellation.cancel().unwrap();
        cancellation.cancel().unwrap();

        assert_eq!(
            cancellation.check().unwrap_err().code(),
            StudioUpdateErrorCode::Cancelled
        );
        assert_eq!(
            cancellation.begin_launch().unwrap_err().code(),
            StudioUpdateErrorCode::Cancelled
        );
    }

    #[test]
    fn cancellation_is_too_late_after_installer_launch_begins() {
        let cancellation = StudioUpdateCancellation::new();

        cancellation.begin_launch().unwrap();

        assert_eq!(
            cancellation.cancel().unwrap_err().code(),
            StudioUpdateErrorCode::CancellationTooLate
        );
    }
}
