use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use base64::Engine;
use image::GenericImageView;
use pl_protocol::{AttachmentModality, ThreadAttachment};
use pl_trace::TraceAttachment;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, TransactionTrait,
};

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::attachment_record;
use crate::studio::records::{AttachmentRecord, MaterializedAttachment};
use crate::studio::store::StudioStore;

impl StudioStore {
    pub async fn list_thread_attachments(&self, thread_id: &str) -> Result<Vec<AttachmentRecord>> {
        use entities::attachment;
        let rows = attachment::Entity::find()
            .filter(attachment::Column::ThreadId.eq(thread_id.to_string()))
            .order_by_asc(attachment::Column::CreatedAt)
            .order_by_asc(attachment::Column::Id)
            .all(&self.db)
            .await?;
        rows.into_iter().map(attachment_record).collect()
    }

    pub async fn load_attachments(
        &self,
        thread_id: &str,
        attachment_ids: &[String],
    ) -> Result<Vec<AttachmentRecord>> {
        if attachment_ids.is_empty() {
            return Ok(Vec::new());
        }
        use entities::attachment;
        let rows = attachment::Entity::find()
            .filter(attachment::Column::ThreadId.eq(thread_id.to_string()))
            .filter(attachment::Column::Id.is_in(attachment_ids.iter().cloned()))
            .all(&self.db)
            .await?;
        let mut records = rows
            .into_iter()
            .map(attachment_record)
            .collect::<Result<Vec<_>>>()?;
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
        materialize_attachments(records).await
    }

    pub async fn materialize_attachments(
        &self,
        thread_id: &str,
        attachment_ids: &[String],
    ) -> Result<Vec<MaterializedAttachment>> {
        let records = self.load_attachments(thread_id, attachment_ids).await?;
        materialize_attachments(records).await
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

    pub(crate) async fn promote_attachment_drafts(
        &self,
        thread_id: &str,
        drafts: &[AttachmentDraftObject],
    ) -> Result<Vec<AttachmentRecord>> {
        let mut created_paths = Vec::new();
        let mut prepared = Vec::with_capacity(drafts.len());
        for draft in drafts {
            let result = async {
                let dir = self
                    .attachments_dir()
                    .join("objects")
                    .join(&draft.content_sha256[..2]);
                tokio::fs::create_dir_all(&dir).await?;
                let storage_path = dir.join(&draft.content_sha256);
                let created = if tokio::fs::try_exists(&storage_path).await? {
                    false
                } else {
                    tokio::fs::copy(&draft.storage_path, &storage_path).await?;
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

        let result = async {
            let transaction = self.db.begin().await?;
            let mut records = Vec::with_capacity(drafts.len());
            for (draft, storage_path) in prepared {
                let row = entities::attachment::ActiveModel {
                    id: Set(new_id("attachment")),
                    thread_id: Set(thread_id.to_string()),
                    kind: Set(attachment_kind_label(draft.modality).to_string()),
                    media_type: Set(draft.media_type.clone()),
                    filename: Set(Some(draft.filename.clone())),
                    storage_path: Set(storage_path.to_string_lossy().to_string()),
                    byte_size: Set(i64::try_from(draft.byte_size)
                        .context("attachment byte size exceeds SQLite range")?),
                    content_sha256: Set(draft.content_sha256.clone()),
                    width: Set(draft.width.map(i64::from)),
                    height: Set(draft.height.map(i64::from)),
                    created_at: Set(unix_seconds()),
                }
                .insert(&transaction)
                .await?;
                records.push(attachment_record(row)?);
            }
            transaction.commit().await?;
            Ok::<_, anyhow::Error>(records)
        }
        .await;
        if result.is_err() {
            cleanup_created_blobs(created_paths).await;
        }
        result
    }

    #[cfg(test)]
    pub(crate) async fn persist_tool_image(
        &self,
        thread_id: &str,
        input: pl_core::ToolImageAttachmentInput,
    ) -> Result<ThreadAttachment> {
        let mut attachments = self.persist_tool_images(thread_id, vec![input]).await?;
        attachments
            .pop()
            .context("tool image batch returned no row")
    }

    pub(crate) async fn persist_tool_images(
        &self,
        thread_id: &str,
        inputs: Vec<pl_core::ToolImageAttachmentInput>,
    ) -> Result<Vec<ThreadAttachment>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        for input in &inputs {
            let actual_sha256 = sha256_hex(&input.data);
            if actual_sha256 != input.content_sha256 {
                bail!("tool image content digest does not match its bytes");
            }
            if input.width == 0 || input.height == 0 {
                bail!("tool image dimensions must be non-zero");
            }
            if !matches!(
                input.media_type.as_str(),
                "image/png" | "image/jpeg" | "image/webp" | "image/gif"
            ) {
                bail!("tool image media type is not supported");
            }
        }

        let mut created_paths = Vec::new();
        let mut prepared = Vec::with_capacity(inputs.len());
        for input in inputs {
            let result = async {
                let dir = self
                    .attachments_dir()
                    .join("objects")
                    .join(&input.content_sha256[..2]);
                tokio::fs::create_dir_all(&dir).await?;
                let storage_path = dir.join(&input.content_sha256);
                let created = !tokio::fs::try_exists(&storage_path).await?;
                if created {
                    let path = storage_path.clone();
                    let data = input.data.clone();
                    tokio::task::spawn_blocking(move || {
                        pl_core::atomic_file::write_file_atomically(&path, &data)
                    })
                    .await
                    .context("tool image blob writer task failed")??;
                }
                Ok::<_, anyhow::Error>((input, storage_path, created))
            }
            .await;
            match result {
                Ok((input, storage_path, created)) => {
                    if created {
                        created_paths.push(storage_path.clone());
                    }
                    prepared.push((input, storage_path));
                }
                Err(error) => {
                    cleanup_created_blobs(created_paths).await;
                    return Err(error);
                }
            }
        }

        let result = async {
            let transaction = self.db.begin().await?;
            let mut attachments = Vec::with_capacity(prepared.len());
            for (input, storage_path) in prepared {
                let row = entities::attachment::ActiveModel {
                    id: Set(new_id("attachment")),
                    thread_id: Set(thread_id.to_string()),
                    kind: Set("image".to_string()),
                    media_type: Set(input.media_type),
                    filename: Set(Some(input.filename)),
                    storage_path: Set(storage_path.to_string_lossy().to_string()),
                    byte_size: Set(i64::try_from(input.data.len())
                        .context("tool image byte size exceeds SQLite range")?),
                    content_sha256: Set(input.content_sha256),
                    width: Set(Some(i64::from(input.width))),
                    height: Set(Some(i64::from(input.height))),
                    created_at: Set(unix_seconds()),
                }
                .insert(&transaction)
                .await?;
                let record = attachment_record(row)?;
                attachments.push(thread_attachment(&record));
            }
            transaction.commit().await?;
            Ok::<_, anyhow::Error>(attachments)
        }
        .await;
        if result.is_err() {
            cleanup_created_blobs(created_paths).await;
        }
        result
    }

    pub(crate) async fn delete_attachments(
        &self,
        thread_id: &str,
        attachment_ids: &[String],
    ) -> Result<()> {
        if attachment_ids.is_empty() {
            return Ok(());
        }
        let records = self.load_attachments(thread_id, attachment_ids).await?;
        entities::attachment::Entity::delete_many()
            .filter(entities::attachment::Column::ThreadId.eq(thread_id.to_string()))
            .filter(entities::attachment::Column::Id.is_in(attachment_ids.iter().cloned()))
            .exec(&self.db)
            .await?;
        for record in records {
            let remaining = entities::attachment::Entity::find()
                .filter(
                    entities::attachment::Column::ContentSha256.eq(record.content_sha256.clone()),
                )
                .count(&self.db)
                .await?;
            if remaining == 0 {
                let _ = tokio::fs::remove_file(record.storage_path).await;
            }
        }
        Ok(())
    }
}

async fn cleanup_created_blobs(paths: Vec<PathBuf>) {
    for path in paths {
        let _ = tokio::fs::remove_file(path).await;
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

fn attachment_kind_label(modality: pl_protocol::studio::StudioAttachmentModality) -> &'static str {
    match modality {
        pl_protocol::studio::StudioAttachmentModality::Image => "image",
        pl_protocol::studio::StudioAttachmentModality::Video => "video",
        pl_protocol::studio::StudioAttachmentModality::File => "file",
    }
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

async fn materialize_attachments(
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

pub(crate) fn trace_attachment(record: &AttachmentRecord) -> TraceAttachment {
    TraceAttachment {
        id: record.id.clone(),
        modality: match record.modality {
            pl_protocol::studio::StudioAttachmentModality::Image => {
                pl_trace::TraceAttachmentModality::Image
            }
            pl_protocol::studio::StudioAttachmentModality::Video => {
                pl_trace::TraceAttachmentModality::Video
            }
            pl_protocol::studio::StudioAttachmentModality::File => {
                pl_trace::TraceAttachmentModality::File
            }
        },
        media_type: record.media_type.clone(),
        filename: record.filename.clone(),
        width: record.width,
        height: record.height,
        byte_size: record.byte_size,
    }
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
