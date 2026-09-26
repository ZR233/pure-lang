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

/// Identity prefix owned by this store; the only producer of durable tool media references.
pub const RESOURCE_ID_PREFIX: &str = ID_PREFIX;

/// Largest single **command capture** this store will archive.
///
/// This ceiling is deliberately scoped to the command-output archive, not to every retained resource:
/// an attachment, import or other user resource keeps its existing unbounded streaming semantics, and
/// the generic `retain_file`/`retain_bytes` paths impose no such limit. The command capture copy is
/// bounded in chunks, never read whole into memory, and a capture past this ceiling is refused with a
/// typed error instead of being copied without bound. It sits above the command operation's own
/// capture ceiling, so a complete capture plus its framing headers still archives, while a runaway
/// capture cannot fill the disk with one copy.
pub const MAX_COMMAND_ARCHIVE_BYTES: u64 = 32 * 1024 * 1024;

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
    #[error("resource exceeds the {limit}-byte retention ceiling")]
    TooLarge { limit: u64 },
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
        tokio::task::spawn_blocking(move || retain(&root, &source, media_type, None)).await?
    }

    /// Retains a completed command capture under the command-archive ceiling.
    ///
    /// It shares the exact streaming copy and idempotent content-addressed identity of
    /// [`Self::retain_file`], but bounds the copy at [`MAX_COMMAND_ARCHIVE_BYTES`] so a runaway capture
    /// cannot fill the disk. The limit is scoped to this command path only: it never narrows the
    /// generic resource/attachment semantics.
    ///
    /// # Errors
    /// Returns [`ResourceStoreError::TooLarge`] for a capture past the ceiling and the same IO,
    /// integrity and metadata failures as [`Self::retain_file`].
    pub async fn retain_command_capture(
        &self,
        source: &Path,
    ) -> Result<ResourceReference, ResourceStoreError> {
        let source = source.to_owned();
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            retain(
                &root,
                &source,
                "application/octet-stream".to_owned(),
                Some(MAX_COMMAND_ARCHIVE_BYTES),
            )
        })
        .await?
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
                None,
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
            .retain_command_capture(&snapshot.capture_file)
            .await
            .map_err(retention_failure)
    }
}

/// Maps a resource retention failure to the tool boundary.
///
/// A real disk failure — the capture could not be copied or made durable — is reported as the typed
/// storage category core latches, so the Thread pauses further admission instead of continuing as if
/// the bytes were archived. An oversized or malformed source is a policy refusal that leaves the
/// capture intact for inspection, so it stays a plain tool error rather than a Thread-wide pause. The
/// category is chosen from the typed error, never parsed out of its text.
fn retention_failure(error: ResourceStoreError) -> pl_core::tool::opaque::ToolError {
    match error {
        ResourceStoreError::Io { source, .. } | ResourceStoreError::Read(source) => {
            retention_io_fault(source)
        }
        other => pl_core::tool::opaque::ToolError::new(other),
    }
}

/// Maps a local disk failure while staging or retaining bytes to the typed storage category.
///
/// The remote archive stages the transfer locally before it content-addresses it, so a local write or
/// flush failure has to reach core as the same typed storage fact a local capture failure does; a
/// remote *transfer* failure stays a plain tool error because it is not a storage fault.
fn retention_io_fault(error: std::io::Error) -> pl_core::tool::opaque::ToolError {
    let source = pl_core::thread::cold::ColdStoreError {
        source: Box::new(error),
    };
    pl_core::tool::opaque::ToolError::new(pl_core::thread::cold::OutputStorageFault::new(
        pl_core::thread::cold::StorageFaultKind::BlobFailed,
        Arc::new(source),
    ))
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
    limit: Option<u64>,
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
        limit,
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
    limit: Option<u64>,
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
        length = length
            .checked_add(count as u64)
            .ok_or_else(|| store_error(std::io::Error::other("resource length overflow")))?;
        // Refuse a source past the caller's ceiling while copying, before writing the chunk that would
        // cross it: a huge file is never materialized as an unbounded second copy, and the caller gets
        // a typed reason instead of a silently truncated object. `None` keeps the generic
        // resource/attachment semantics unbounded.
        if let Some(limit) = limit
            && length > limit
        {
            return Err(ResourceStoreError::TooLarge { limit });
        }
        temporary.write_all(&buffer[..count]).map_err(store_error)?;
        digest.update(&buffer[..count]);
    }
    let hash = hex::encode(digest.finalize());
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
        || format!("sha256:{}", hex::encode(digest.finalize())) != reference.content_digest()
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

    /// The command-archive ceiling is a typed refusal on the command path only, not a new generic limit.
    #[tokio::test]
    async fn the_command_archive_ceiling_is_scoped_to_the_command_path() {
        let root = tempfile::tempdir().expect("an isolated retention root");
        let store = FileResourceStore::new(root.path().to_path_buf());
        let source = root.path().join("capture.bin");
        let oversized = vec![0_u8; MAX_COMMAND_ARCHIVE_BYTES as usize + 1];
        std::fs::write(&source, &oversized).expect("the oversized capture is written");

        let error = store
            .retain_command_capture(&source)
            .await
            .expect_err("a command capture past the ceiling is a typed refusal");
        assert!(matches!(
            error,
            ResourceStoreError::TooLarge { limit } if limit == MAX_COMMAND_ARCHIVE_BYTES
        ));

        // The generic resource/attachment path keeps its unbounded streaming semantics: the same
        // oversized source still archives, so the command ceiling is not a new generic limit.
        store
            .retain_file(&source, "application/octet-stream")
            .await
            .expect("a generic resource is not bounded by the command archive ceiling");
    }

    /// A repeated command-archive retry re-saves the same bytes under the same identity.
    #[tokio::test]
    async fn repeating_the_command_archive_retry_is_idempotent() {
        let root = tempfile::tempdir().expect("an isolated retention root");
        let store = FileResourceStore::new(root.path().to_path_buf());
        let source = root.path().join("capture.bin");
        let accepted = b"accepted capture fragment".to_vec();
        std::fs::write(&source, &accepted).expect("the capture fragment is written");

        let first = store
            .retain_command_capture(&source)
            .await
            .expect("the first archive stores the fragment");
        let retry = store
            .retain_command_capture(&source)
            .await
            .expect("a repeated retry stores the same fragment");
        assert_eq!(
            first, retry,
            "a repeated retry produces the same content-addressed reference, not a duplicate"
        );
        assert_eq!(retry.byte_len(), accepted.len() as u64);
    }

    /// A local disk failure is the typed storage fact core latches; a policy refusal is not.
    #[test]
    fn retention_failure_maps_local_io_to_the_typed_storage_fault() {
        let io_failure =
            retention_failure(ResourceStoreError::Read(std::io::Error::other("disk full")));
        let fault = io_failure
            .source
            .downcast_ref::<pl_core::thread::cold::OutputStorageFault>()
            .expect("a local IO failure is the typed storage fault core latches");
        assert_eq!(
            fault.kind,
            pl_core::thread::cold::StorageFaultKind::BlobFailed
        );

        let refusal = retention_failure(ResourceStoreError::TooLarge {
            limit: MAX_COMMAND_ARCHIVE_BYTES,
        });
        assert!(
            refusal
                .source
                .downcast_ref::<pl_core::thread::cold::OutputStorageFault>()
                .is_none(),
            "an oversized source is a policy refusal, not a Thread-wide storage fault"
        );
    }
}
