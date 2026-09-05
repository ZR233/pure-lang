//! 附件内容归一化：按实际字节识别图片/视频/文件模态并产出媒体类型与尺寸，以及文件名清洗。

use anyhow::{Context, Result, bail};

use pl_protocol::studio::StudioAttachmentModality;

use crate::studio::store::attachment::normalize_image_attachment;

pub(super) struct NormalizedSource {
    pub(super) modality: StudioAttachmentModality,
    pub(super) media_type: String,
    pub(super) bytes: Vec<u8>,
    pub(super) width: Option<u32>,
    pub(super) height: Option<u32>,
}

pub(super) fn normalize_loaded_source(filename: &str, bytes: Vec<u8>) -> Result<NormalizedSource> {
    if let Ok(format) = image::guess_format(&bytes) {
        let media_type = match format {
            image::ImageFormat::Png => "image/png",
            image::ImageFormat::Jpeg => "image/jpeg",
            image::ImageFormat::Gif => "image/gif",
            image::ImageFormat::WebP => "image/webp",
            _ => bail!("unsupported image attachment format"),
        };
        let decoded = image::load_from_memory_with_format(&bytes, format)
            .context("failed to decode image attachment")?;
        let normalized = normalize_image_attachment(media_type, bytes, decoded)?;
        return Ok(NormalizedSource {
            modality: StudioAttachmentModality::Image,
            media_type: normalized.media_type.to_string(),
            bytes: normalized.bytes,
            width: Some(normalized.dimensions.0),
            height: Some(normalized.dimensions.1),
        });
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Ok(NormalizedSource {
            modality: StudioAttachmentModality::Video,
            media_type: if filename.to_ascii_lowercase().ends_with(".mov") {
                "video/quicktime".to_string()
            } else {
                "video/mp4".to_string()
            },
            bytes,
            width: None,
            height: None,
        });
    }
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Ok(NormalizedSource {
            modality: StudioAttachmentModality::Video,
            media_type: "video/webm".to_string(),
            bytes,
            width: None,
            height: None,
        });
    }
    let media_type = if bytes.starts_with(b"%PDF-") {
        "application/pdf"
    } else if std::str::from_utf8(&bytes).is_ok() {
        "text/plain"
    } else if bytes.starts_with(b"PK\x03\x04") {
        "application/zip"
    } else {
        "application/octet-stream"
    };
    Ok(NormalizedSource {
        modality: StudioAttachmentModality::File,
        media_type: media_type.to_string(),
        bytes,
        width: None,
        height: None,
    })
}

pub(super) fn sanitize_filename(filename: &str) -> String {
    filename
        .chars()
        .take(120)
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '.' | '-' | '_' | ' ') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string()
}

#[cfg(test)]
mod tests {
    use pl_protocol::studio::StudioAttachmentDraftSource;

    use super::super::runtime::AttachmentDraftRuntime;
    use super::super::runtime::tests::{deepseek_vision, gif_bytes};
    use super::*;

    #[tokio::test]
    async fn deepseek_vision_accepts_gif_from_actual_file_content() {
        let root = tempfile::tempdir().unwrap();
        let drafts = AttachmentDraftRuntime::new(root.path().join("drafts")).unwrap();
        let image = root.path().join("animation.gif");
        tokio::fs::write(&image, gif_bytes()).await.unwrap();

        let admitted = drafts
            .admit(
                vec![StudioAttachmentDraftSource::LocalFile {
                    path: image.to_string_lossy().to_string(),
                }],
                &deepseek_vision(),
            )
            .await
            .unwrap();

        assert_eq!(admitted.drafts[0].modality, StudioAttachmentModality::Image);
        assert_eq!(admitted.drafts[0].media_type, "image/gif");
    }
}
