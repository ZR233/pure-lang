//! Persistent resource bytes, independent of tool payload formats and session SQL schemas.

mod remote;
pub use remote::RemoteCommandOutputArchive;
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use pl_core::context::{ResourceReadError, ResourceReader, ResourceReference};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

const ID_PREFIX: &str = "pl.studio.resource:";

/// Content-addressed resource service rooted in a host-selected persistent directory.
/// Clones share immutable objects; closing a Thread never deletes another Thread's bytes.
#[derive(Debug, Clone)]
pub struct FileResourceStore {
    root: Arc<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum ResourceStoreError {
    #[error("resource store IO failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("resource byte stream could not be read")]
    Read(#[source] std::io::Error),
    #[error("resource source is not a regular file: {0}")]
    NotFile(PathBuf),
    #[error("resource identity is not owned by this store")]
    Identity,
    #[error("resource metadata or bytes failed integrity validation")]
    Integrity(#[from] pl_core::context::ResourceError),
    #[error("resource worker failed")]
    Worker(#[from] tokio::task::JoinError),
}

impl FileResourceStore {
    /// Selects the persistent directory without creating files or reading configuration.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: Arc::new(root),
        }
    }

    /// Retains exact source bytes. The capture is not removed, including on failure.
    ///
    /// # Errors
    /// Rejects non-files, invalid media descriptions, corrupt existing objects and IO failure.
    pub async fn retain_file(
        &self,
        source: &Path,
        media_type: &str,
    ) -> Result<ResourceReference, ResourceStoreError> {
        let source = source.to_owned();
        let media_type = media_type.to_owned();
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || retain(&root, &source, media_type)).await?
    }

    /// Retains exact in-memory bytes with the same immutable identity and durability as file captures.
    ///
    /// # Errors
    /// Returns invalid media metadata, existing-content corruption, or storage failure.
    pub async fn retain_bytes(
        &self,
        bytes: Arc<[u8]>,
        media_type: &str,
    ) -> Result<ResourceReference, ResourceStoreError> {
        let root = self.root.clone();
        let media_type = media_type.to_owned();
        tokio::task::spawn_blocking(move || {
            retain_input(
                &root,
                ResourceInput {
                    reader: std::io::Cursor::new(bytes),
                    source_path: None,
                },
                media_type,
            )
        })
        .await?
    }

    /// Builds an archive adapter only for commands whose capture is on the local host.
    pub fn local_command_archive(&self) -> LocalCommandOutputArchive {
        LocalCommandOutputArchive(self.clone())
    }
}

/// Local-capture adapter; remote capture needs a remote read adapter before this store.
#[derive(Debug, Clone)]
pub struct LocalCommandOutputArchive(FileResourceStore);

impl pl_tool::exec::CommandOutputArchive for LocalCommandOutputArchive {
    async fn retain(
        &self,
        _: &str,
        snapshot: &pl_tool::command::CommandOutputSnapshot,
    ) -> Result<ResourceReference, pl_core::tool::opaque::ToolError> {
        self.0
            .retain_file(&snapshot.capture_file, "application/octet-stream")
            .await
            .map_err(pl_core::tool::opaque::ToolError::new)
    }
}

impl ResourceReader for FileResourceStore {
    async fn read(
        &self,
        reference: ResourceReference,
        cancellation: CancellationToken,
    ) -> Result<Arc<[u8]>, ResourceReadError> {
        if cancellation.is_cancelled() {
            return Err(ResourceReadError::Cancelled);
        }
        let path = resource_path(&self.root, &reference).map_err(unavailable)?;
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|source| unavailable(ResourceStoreError::Io { path, source }))?;
        if cancellation.is_cancelled() {
            return Err(ResourceReadError::Cancelled);
        }
        reference.verify(&bytes)?;
        Ok(Arc::from(bytes))
    }
}

fn unavailable(source: ResourceStoreError) -> ResourceReadError {
    ResourceReadError::Unavailable {
        source: Box::new(source),
    }
}

fn retain(
    root: &Path,
    source: &Path,
    media_type: String,
) -> Result<ResourceReference, ResourceStoreError> {
    let source_error = |source_error| ResourceStoreError::Io {
        path: source.to_owned(),
        source: source_error,
    };
    let input = File::open(source).map_err(source_error)?;
    if !input.metadata().map_err(source_error)?.is_file() {
        return Err(ResourceStoreError::NotFile(source.to_owned()));
    }
    retain_input(
        root,
        ResourceInput {
            reader: input,
            source_path: Some(source.to_owned()),
        },
        media_type,
    )
}

struct ResourceInput<R> {
    reader: R,
    source_path: Option<PathBuf>,
}

fn retain_input(
    root: &Path,
    mut input: ResourceInput<impl Read>,
    media_type: String,
) -> Result<ResourceReference, ResourceStoreError> {
    let store_error = |source_error| ResourceStoreError::Io {
        path: root.to_owned(),
        source: source_error,
    };
    std::fs::create_dir_all(root).map_err(store_error)?;
    let mut temporary = tempfile::NamedTempFile::new_in(root).map_err(store_error)?;
    let mut digest = Sha256::new();
    let mut length = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input
            .reader
            .read(&mut buffer)
            .map_err(|source| match &input.source_path {
                Some(path) => ResourceStoreError::Io {
                    path: path.clone(),
                    source,
                },
                None => ResourceStoreError::Read(source),
            })?;
        if count == 0 {
            break;
        }
        temporary.write_all(&buffer[..count]).map_err(store_error)?;
        digest.update(&buffer[..count]);
        length = length
            .checked_add(count as u64)
            .ok_or_else(|| store_error(std::io::Error::other("resource length overflow")))?;
    }
    let hash = format!("{:x}", digest.finalize());
    let reference = ResourceReference::new(
        format!("{ID_PREFIX}{hash}"),
        format!("sha256:{hash}"),
        length,
        media_type,
    )?;
    temporary.as_file().sync_all().map_err(store_error)?;
    let destination = resource_path(root, &reference)?;
    match temporary.persist_noclobber(&destination) {
        Ok(_) => {}
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            // Preserve a conflicting object for diagnosis. Never overwrite historical bytes.
            verify_file(&destination, &reference)?;
        }
        Err(error) => {
            return Err(ResourceStoreError::Io {
                path: destination,
                source: error.error,
            });
        }
    }
    #[cfg(unix)]
    for directory in std::fs::canonicalize(root)
        .map_err(store_error)?
        .ancestors()
    {
        File::open(directory)
            .and_then(|directory| directory.sync_all())
            .map_err(store_error)?;
    }
    Ok(reference)
}

fn resource_path(
    root: &Path,
    reference: &ResourceReference,
) -> Result<PathBuf, ResourceStoreError> {
    reference.validate()?;
    let hash = reference
        .id()
        .strip_prefix(ID_PREFIX)
        .ok_or(ResourceStoreError::Identity)?;
    if hash.len() != 64
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        || reference.content_digest() != format!("sha256:{hash}")
    {
        return Err(ResourceStoreError::Identity);
    }
    Ok(root.join(hash))
}

fn verify_file(path: &Path, reference: &ResourceReference) -> Result<(), ResourceStoreError> {
    let mut input = File::open(path).map_err(|source| ResourceStoreError::Io {
        path: path.to_owned(),
        source,
    })?;
    if input
        .metadata()
        .map_err(|source| ResourceStoreError::Io {
            path: path.to_owned(),
            source,
        })?
        .len()
        != reference.byte_len()
    {
        return Err(ResourceStoreError::Integrity(
            pl_core::context::ResourceError::ContentMismatch,
        ));
    }
    let mut digest = Sha256::new();
    let mut length = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|source| ResourceStoreError::Io {
                path: path.to_owned(),
                source,
            })?;
        if count == 0 {
            break;
        }
        length = length
            .checked_add(count as u64)
            .filter(|length| *length <= reference.byte_len())
            .ok_or(ResourceStoreError::Integrity(
                pl_core::context::ResourceError::ContentMismatch,
            ))?;
        digest.update(&buffer[..count]);
    }
    if length != reference.byte_len()
        || format!("sha256:{:x}", digest.finalize()) != reference.content_digest()
    {
        return Err(ResourceStoreError::Integrity(
            pl_core::context::ResourceError::ContentMismatch,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::context::ResourceAccess;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn archived_bytes_survive_capture_removal_and_store_recreation() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("resources");
        let capture = directory.path().join("capture");
        let bytes = b"\x00\xfforiginal\r\noutput";
        std::fs::write(&capture, bytes).unwrap();
        let store = FileResourceStore::new(root.clone());
        let (first, second) = tokio::join!(
            store.retain_file(&capture, "application/octet-stream"),
            store.retain_bytes(Arc::from(&bytes[..]), "application/octet-stream"),
        );
        let reference = first.unwrap();
        assert_eq!(second.unwrap(), reference);
        std::fs::remove_file(&capture).unwrap();
        let access = ResourceAccess::new(FileResourceStore::new(root));
        let restored = access
            .read(&reference, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(restored.as_ref(), bytes);
    }

    #[tokio::test]
    async fn corrupt_existing_objects_fail_without_overwriting_historical_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("resources");
        let capture = directory.path().join("capture");
        std::fs::write(&capture, b"original").unwrap();
        let store = FileResourceStore::new(root.clone());
        let reference = store.retain_file(&capture, "text/plain").await.unwrap();
        let object = resource_path(&root, &reference).unwrap();
        std::fs::write(&object, b"corrupt!").unwrap();
        assert!(matches!(
            store.retain_file(&capture, "text/plain").await,
            Err(ResourceStoreError::Integrity(_))
        ));
        assert_eq!(std::fs::read(&object).unwrap(), b"corrupt!");
        assert!(
            ResourceAccess::new(store)
                .read(&reference, CancellationToken::new())
                .await
                .is_err()
        );
    }
}
