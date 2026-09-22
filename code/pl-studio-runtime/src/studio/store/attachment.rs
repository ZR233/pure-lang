use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use base64::Engine;
use image::GenericImageView;
use pl_protocol::{AttachmentModality, ThreadAttachment};

use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::records::{AttachmentRecord, MaterializedAttachment};
use crate::studio::store::StudioStore;

pub(in crate::studio) const ATTACHMENT_CATALOG_SCHEMA_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::studio) struct AttachmentCatalog {
    pub(in crate::studio) schema_version: u32,
    pub(in crate::studio) thread_id: String,
    pub(in crate::studio) records: Vec<AttachmentRecord>,
}

impl StudioStore {
    pub async fn list_thread_attachments(&self, thread_id: &str) -> Result<Vec<AttachmentRecord>> {
        let _guard = self.attachment_lock().lock().await;
        self.load_attachment_catalog(thread_id).await
    }

    pub(in crate::studio) async fn record_attachments(
        &self,
        records: Vec<AttachmentRecord>,
    ) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let thread_id = records[0].thread_id.clone();
        anyhow::ensure!(
            records.iter().all(|record| record.thread_id == thread_id),
            "attachment batch mixes Thread identities"
        );
        let _guard = self.attachment_lock().lock().await;
        let mut current = self.load_attachment_catalog(&thread_id).await?;
        let mut ids = current
            .iter()
            .map(|record| record.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for record in &records {
            anyhow::ensure!(ids.insert(&record.id), "duplicate attachment id");
        }
        current.extend(records);
        self.save_attachment_catalog(&thread_id, &current).await
    }

    pub async fn load_attachments(
        &self,
        thread_id: &str,
        attachment_ids: &[String],
    ) -> Result<Vec<AttachmentRecord>> {
        if attachment_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut records = self.list_thread_attachments(thread_id).await?;
        let mut ordered = Vec::with_capacity(attachment_ids.len());
        let mut seen = std::collections::BTreeSet::new();
        for attachment_id in attachment_ids {
            if !seen.insert(attachment_id) {
                bail!("duplicate attachment id: {attachment_id}");
            }
            let index = records
                .iter()
                .position(|record| record.id == *attachment_id)
                .with_context(|| format!("attachment {attachment_id} does not belong to Thread"))?;
            ordered.push(records.swap_remove(index));
        }
        Ok(ordered)
    }

    pub async fn materialize_thread_attachments(
        &self,
        thread_id: &str,
    ) -> Result<Vec<MaterializedAttachment>> {
        let records = self.list_thread_attachments(thread_id).await?;
        materialize_attachment_records(records).await
    }

    pub async fn materialize_attachments(
        &self,
        thread_id: &str,
        attachment_ids: &[String],
    ) -> Result<Vec<MaterializedAttachment>> {
        let records = self.load_attachments(thread_id, attachment_ids).await?;
        materialize_attachment_records(records).await
    }

    pub(crate) async fn read_attachment_bytes(
        &self,
        thread_id: &str,
        attachment_id: &str,
    ) -> Result<Vec<u8>> {
        let record = self
            .load_attachments(thread_id, &[attachment_id.to_string()])
            .await?
            .into_iter()
            .next()
            .context("attachment is unavailable")?;
        tokio::fs::read(record.storage_path)
            .await
            .with_context(|| format!("failed to load attachment {attachment_id}"))
    }

    /// Unified blob fence: makes every attachment blob a checkpoint still names durable.
    ///
    /// A published `state.toml` must never name a blob whose bytes or directory entry are not on
    /// disk yet. New blobs are synced when they are promoted, but the writer validates and waits on
    /// this fence explicitly instead of depending on that scattered call order. A missing catalog
    /// record or a missing blob is *not* "nothing to fence": the checkpoint names a blob that is
    /// not provably durable, so publication fails closed and is retried, surfacing the failure as
    /// persistence diagnostics instead of publishing a `state.toml` that points at absent bytes.
    pub(in crate::studio) async fn blob_fence(
        &self,
        thread_id: &str,
        attachment_ids: &[String],
    ) -> Result<()> {
        if attachment_ids.is_empty() {
            return Ok(());
        }
        let records = self.list_thread_attachments(thread_id).await?;
        for attachment_id in attachment_ids {
            let Some(record) = records.iter().find(|record| &record.id == attachment_id) else {
                anyhow::bail!(
                    "checkpoint names attachment {attachment_id} with no catalog record for Thread {thread_id}"
                );
            };
            let path = std::path::Path::new(&record.storage_path);
            if !tokio::fs::try_exists(path).await? {
                anyhow::bail!(
                    "checkpoint names missing attachment blob {} for Thread {thread_id}",
                    path.display()
                );
            }
            sync_blob(path).await?;
        }
        Ok(())
    }

    pub(crate) async fn promote_attachment_drafts(
        &self,
        thread_id: &str,
        drafts: &[AttachmentDraftObject],
    ) -> Result<Vec<AttachmentRecord>> {
        let mut created_paths = Vec::new();
        let mut prepared = Vec::with_capacity(drafts.len());
        for draft in drafts {
            let result = async {
                // New and migrated attachments share the same per-session content-addressed blob
                // root (`sessions/<storage-key>/blobs`), so the catalog path can never diverge from
                // the one the one-time migration coordinator wrote.
                let dir = self
                    .thread_blobs_dir(thread_id)
                    .join(&draft.content_sha256[..2]);
                tokio::fs::create_dir_all(&dir).await?;
                let storage_path = dir.join(&draft.content_sha256);
                let created = if tokio::fs::try_exists(&storage_path).await? {
                    false
                } else {
                    tokio::fs::copy(&draft.storage_path, &storage_path).await?;
                    // 引用 blob 的 catalog/checkpoint 只会在本调用返回之后发布，所以 blob 的字节
                    // 和它的目录项必须先落盘，否则崩溃后 TOML 会指向一个不存在的附件。
                    if let Err(error) = sync_blob(&storage_path).await {
                        let _ = tokio::fs::remove_file(&storage_path).await;
                        return Err(error);
                    }
                    true
                };
                Ok::<_, anyhow::Error>((storage_path, created))
            }
            .await;
            match result {
                Ok((storage_path, created)) => {
                    if created {
                        created_paths.push(storage_path.clone());
                    }
                    prepared.push((draft, storage_path));
                }
                Err(error) => {
                    cleanup_created_blobs(created_paths).await;
                    return Err(error);
                }
            }
        }

        Ok(prepared
            .into_iter()
            .map(|(draft, storage_path)| AttachmentRecord {
                id: new_id("attachment"),
                thread_id: thread_id.to_string(),
                modality: draft.modality,
                media_type: draft.media_type.clone(),
                filename: Some(draft.filename.clone()),
                storage_path: storage_path.to_string_lossy().to_string(),
                byte_size: draft.byte_size,
                content_sha256: draft.content_sha256.clone(),
                width: draft.width,
                height: draft.height,
                created_at: unix_seconds(),
            })
            .collect())
    }

    async fn load_attachment_catalog(&self, thread_id: &str) -> Result<Vec<AttachmentRecord>> {
        let path = self.thread_storage_dir(thread_id).join("attachments.toml");
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                let mut catalog: AttachmentCatalog = toml::from_str(&content)
                    .with_context(|| format!("invalid attachment catalog {}", path.display()))?;
                anyhow::ensure!(
                    catalog.schema_version == ATTACHMENT_CATALOG_SCHEMA_VERSION
                        && catalog.thread_id == thread_id
                        && catalog
                            .records
                            .iter()
                            .all(|record| record.thread_id == thread_id),
                    "attachment catalog identity or schema mismatch"
                );
                let directory = self.thread_storage_dir(thread_id);
                let blobs = self.thread_blobs_dir(thread_id);
                for record in &mut catalog.records {
                    let recorded = PathBuf::from(&record.storage_path);
                    let resolved = if recorded.is_absolute() {
                        recorded
                    } else {
                        directory.join(recorded)
                    };
                    anyhow::ensure!(
                        resolved.starts_with(&blobs),
                        "attachment {} is outside its Thread blob root",
                        record.id
                    );
                    record.storage_path = resolved.to_string_lossy().into_owned();
                }
                Ok(catalog.records)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // Attachment catalogs are created by the one-time migration coordinator or by this
                // owner; a missing file simply means the Thread has no attachments yet.
                Ok(Vec::new())
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn save_attachment_catalog(
        &self,
        thread_id: &str,
        records: &[AttachmentRecord],
    ) -> Result<()> {
        let directory = self.thread_storage_dir(thread_id);
        tokio::fs::create_dir_all(&directory).await?;
        write_attachment_catalog(&directory.join("attachments.toml"), thread_id, records).await
    }
}

/// Writes one canonical `attachments.toml`, shared with the one-time migration coordinator so both
/// producers emit byte-identical catalogs.
pub(in crate::studio) async fn write_attachment_catalog(
    path: &std::path::Path,
    thread_id: &str,
    records: &[AttachmentRecord],
) -> Result<()> {
    let contents = toml::to_string(&AttachmentCatalog {
        schema_version: ATTACHMENT_CATALOG_SCHEMA_VERSION,
        thread_id: thread_id.to_owned(),
        records: records.to_vec(),
    })?
    .into_bytes();
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        pl_tool::workspace::write_file_atomically(&path, &contents)
    })
    .await??;
    Ok(())
}

async fn cleanup_created_blobs(paths: Vec<PathBuf>) {
    for path in paths {
        let _ = tokio::fs::remove_file(path).await;
    }
}

/// Makes one newly written attachment blob and its directory entry durable.
///
/// The catalog and every checkpoint that references the blob are published only after promotion
/// returns, so the file contents and the directory entry must be synced first. Directory sync is
/// skipped on Windows, which has no equivalent operation.
async fn sync_blob(path: &std::path::Path) -> Result<()> {
    let file = path.to_owned();
    // Windows backs `sync_all` with `FlushFileBuffers`, which requires a writable handle;
    // opening the blob read-only returns `ERROR_ACCESS_DENIED` there.
    tokio::task::spawn_blocking(move || {
        let handle = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(file)?;
        handle.sync_all()
    })
    .await??;
    #[cfg(unix)]
    if let Some(directory) = path.parent() {
        let directory = directory.to_owned();
        tokio::task::spawn_blocking(move || std::fs::File::open(directory)?.sync_all()).await??;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct AttachmentDraftObject {
    pub draft_id: String,
    pub modality: pl_protocol::studio::StudioAttachmentModality,
    pub media_type: String,
    pub filename: String,
    pub storage_path: PathBuf,
    pub byte_size: u64,
    pub content_sha256: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub initial_remote_url: Option<String>,
    pub admitted_at: Instant,
}

pub(super) const MAX_IMAGE_SIDE: u32 = 2000;
pub(super) const MAX_BASE64_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const JPEG_COMPRESSION_QUALITIES: [u8; 6] = [85, 75, 65, 55, 45, 35];
const JPEG_COMPRESSION_MAX_SIDES: [u32; 6] = [2000, 1600, 1280, 1024, 768, 512];

pub(in crate::studio) struct NormalizedImageAttachment {
    pub(in crate::studio) media_type: &'static str,
    pub(in crate::studio) bytes: Vec<u8>,
    pub(in crate::studio) dimensions: (u32, u32),
}

pub(in crate::studio) fn normalize_image_attachment(
    media_type: &'static str,
    bytes: Vec<u8>,
    decoded_image: image::DynamicImage,
) -> Result<NormalizedImageAttachment> {
    let dimensions = decoded_image.dimensions();
    if image_within_limits(&bytes, dimensions) {
        return Ok(NormalizedImageAttachment {
            media_type,
            bytes,
            dimensions,
        });
    }

    let (compressed, dimensions) = compress_image_attachment(&decoded_image)?;
    Ok(NormalizedImageAttachment {
        media_type: "image/jpeg",
        bytes: compressed,
        dimensions,
    })
}

fn image_within_limits(bytes: &[u8], dimensions: (u32, u32)) -> bool {
    dimensions.0 <= MAX_IMAGE_SIDE
        && dimensions.1 <= MAX_IMAGE_SIDE
        && base64_encoded_len(bytes.len()) <= MAX_BASE64_IMAGE_BYTES
}

pub(super) fn base64_encoded_len(byte_len: usize) -> usize {
    byte_len.div_ceil(3) * 4
}

fn compress_image_attachment(decoded_image: &image::DynamicImage) -> Result<(Vec<u8>, (u32, u32))> {
    for max_side in JPEG_COMPRESSION_MAX_SIDES {
        let candidate = if decoded_image.width() > max_side || decoded_image.height() > max_side {
            decoded_image.thumbnail(max_side, max_side)
        } else {
            decoded_image.clone()
        };
        let dimensions = candidate.dimensions();
        for quality in JPEG_COMPRESSION_QUALITIES {
            let bytes = encode_jpeg(&candidate, quality)?;
            if image_within_limits(&bytes, dimensions) {
                return Ok((bytes, dimensions));
            }
        }
    }
    bail!("image attachment is too large after compression")
}

fn encode_jpeg(image: &image::DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let rgb = image.to_rgb8();
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality);
    encoder
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .with_context(|| "failed to compress image attachment")?;
    Ok(bytes)
}

pub(crate) async fn materialize_attachment_records(
    records: Vec<AttachmentRecord>,
) -> Result<Vec<MaterializedAttachment>> {
    let mut materialized = Vec::with_capacity(records.len());
    for record in records {
        let bytes = tokio::fs::read(PathBuf::from(&record.storage_path))
            .await
            .with_context(|| {
                let id = &record.id;
                format!("failed to read attachment {id}")
            })?;
        materialized.push(MaterializedAttachment {
            attachment_id: record.id,
            modality: match record.modality {
                pl_protocol::studio::StudioAttachmentModality::Image => {
                    pl_protocol::AttachmentModality::Image
                }
                pl_protocol::studio::StudioAttachmentModality::Video => {
                    pl_protocol::AttachmentModality::Video
                }
                pl_protocol::studio::StudioAttachmentModality::File => {
                    pl_protocol::AttachmentModality::File
                }
            },
            media_type: record.media_type,
            filename: record.filename,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
            byte_size: record.byte_size,
            width: record.width,
            height: record.height,
            initial_remote_url: None,
        });
    }
    Ok(materialized)
}

pub(crate) fn thread_attachment(record: &AttachmentRecord) -> ThreadAttachment {
    ThreadAttachment {
        id: record.id.clone(),
        modality: match record.modality {
            pl_protocol::studio::StudioAttachmentModality::Image => AttachmentModality::Image,
            pl_protocol::studio::StudioAttachmentModality::Video => AttachmentModality::Video,
            pl_protocol::studio::StudioAttachmentModality::File => AttachmentModality::File,
        },
        media_type: record.media_type.clone(),
        filename: record.filename.clone(),
        width: record.width,
        height: record.height,
        byte_size: record.byte_size,
    }
}
