use std::path::PathBuf;

use crate::StudioAttachment;
use anyhow::{Context, Result, bail};
use base64::Engine;
use image::GenericImageView;
use pl_trace::TraceAttachment;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::mappers::attachment_record;
use crate::studio::paths::default_attachments_dir;
use crate::studio::records::{AttachmentRecord, MaterializedAttachment};
use crate::studio::store::StudioStore;

impl StudioStore {
    pub async fn create_image_attachment(
        &self,
        session_id: &str,
        data_url: &str,
        filename: Option<String>,
    ) -> Result<AttachmentRecord> {
        let (media_type, bytes) = decode_image_data_url(data_url)?;
        let decoded_image =
            image::load_from_memory(&bytes).with_context(|| "failed to decode image attachment")?;
        let normalized = normalize_image_attachment(media_type, bytes, decoded_image)?;
        let attachment_id = new_id("attachment");
        let extension = extension_for_media_type(normalized.media_type)?;
        let dir = default_attachments_dir()?.join(session_id);
        tokio::fs::create_dir_all(&dir).await?;
        let storage_path = dir.join(format!("{attachment_id}.{extension}"));
        tokio::fs::write(&storage_path, &normalized.bytes).await?;

        use entities::attachment;
        let now = unix_seconds();
        let row = attachment::ActiveModel {
            id: Set(attachment_id),
            session_id: Set(session_id.to_string()),
            message_id: Set(None),
            media_type: Set(normalized.media_type.to_string()),
            filename: Set(filename.filter(|name| !name.trim().is_empty())),
            storage_path: Set(storage_path.to_string_lossy().to_string()),
            byte_size: Set(normalized.bytes.len() as i64),
            width: Set(Some(normalized.dimensions.0 as i64)),
            height: Set(Some(normalized.dimensions.1 as i64)),
            created_at: Set(now),
        }
        .insert(&self.db)
        .await?;
        Ok(attachment_record(row))
    }

    pub async fn list_session_attachments(
        &self,
        session_id: &str,
    ) -> Result<Vec<AttachmentRecord>> {
        use entities::attachment;
        let rows = attachment::Entity::find()
            .filter(attachment::Column::SessionId.eq(session_id.to_string()))
            .order_by_asc(attachment::Column::CreatedAt)
            .order_by_asc(attachment::Column::Id)
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(attachment_record).collect())
    }

    pub async fn load_attachments(
        &self,
        session_id: &str,
        attachment_ids: &[String],
    ) -> Result<Vec<AttachmentRecord>> {
        if attachment_ids.is_empty() {
            return Ok(Vec::new());
        }
        use entities::attachment;
        let rows = attachment::Entity::find()
            .filter(attachment::Column::SessionId.eq(session_id.to_string()))
            .filter(attachment::Column::Id.is_in(attachment_ids.iter().cloned()))
            .all(&self.db)
            .await?;
        Ok(rows.into_iter().map(attachment_record).collect())
    }

    pub async fn materialize_session_attachments(
        &self,
        session_id: &str,
    ) -> Result<Vec<MaterializedAttachment>> {
        let records = self.list_session_attachments(session_id).await?;
        materialize_attachments(records).await
    }

    pub async fn materialize_attachments(
        &self,
        session_id: &str,
        attachment_ids: &[String],
    ) -> Result<Vec<MaterializedAttachment>> {
        let records = self.load_attachments(session_id, attachment_ids).await?;
        materialize_attachments(records).await
    }
}

pub(super) const MAX_IMAGE_SIDE: u32 = 2000;
pub(super) const MAX_BASE64_IMAGE_BYTES: usize = 5 * 1024 * 1024;
const JPEG_COMPRESSION_QUALITIES: [u8; 6] = [85, 75, 65, 55, 45, 35];
const JPEG_COMPRESSION_MAX_SIDES: [u32; 6] = [2000, 1600, 1280, 1024, 768, 512];

pub(super) struct NormalizedImageAttachment {
    pub(super) media_type: &'static str,
    pub(super) bytes: Vec<u8>,
    pub(super) dimensions: (u32, u32),
}

fn decode_image_data_url(data_url: &str) -> Result<(&'static str, Vec<u8>)> {
    let (header, data) = data_url
        .split_once(',')
        .context("image attachment must be a data URL")?;
    let media_type = header
        .strip_prefix("data:")
        .and_then(|value| value.strip_suffix(";base64"))
        .context("image attachment must be base64 encoded")?;
    let media_type = normalize_image_media_type(media_type)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .with_context(|| "invalid base64 image attachment")?;
    Ok((media_type, bytes))
}

fn normalize_image_media_type(media_type: &str) -> Result<&'static str> {
    match media_type {
        "image/png" => Ok("image/png"),
        "image/jpeg" | "image/jpg" => Ok("image/jpeg"),
        "image/webp" => Ok("image/webp"),
        other => bail!("unsupported image attachment media type: {other}"),
    }
}

fn extension_for_media_type(media_type: &str) -> Result<&'static str> {
    match media_type {
        "image/png" => Ok("png"),
        "image/jpeg" => Ok("jpg"),
        "image/webp" => Ok("webp"),
        other => bail!("unsupported image attachment media type: {other}"),
    }
}

pub(super) fn normalize_image_attachment(
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
            media_type: record.media_type,
            filename: record.filename,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
            byte_size: record.byte_size,
            width: record.width,
            height: record.height,
        });
    }
    Ok(materialized)
}

pub(crate) fn trace_attachment(record: &AttachmentRecord) -> TraceAttachment {
    TraceAttachment {
        id: record.id.clone(),
        media_type: record.media_type.clone(),
        filename: record.filename.clone(),
        width: record.width,
        height: record.height,
        byte_size: record.byte_size,
        data_url: None,
    }
}

pub fn studio_attachment(record: &AttachmentRecord) -> StudioAttachment {
    StudioAttachment {
        id: record.id.clone(),
        media_type: record.media_type.clone(),
        filename: record.filename.clone(),
        width: record.width,
        height: record.height,
        byte_size: record.byte_size,
        data_url: None,
    }
}
