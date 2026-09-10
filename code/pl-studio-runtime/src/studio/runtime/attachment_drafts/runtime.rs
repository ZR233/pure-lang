//! 附件草稿运行时：维护草稿的内存目录与磁盘存储，负责准入、解析、提交、读取与过期清理。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use pl_model::model::ModelInfo;
use pl_protocol::studio::{
    AdmitAttachmentDraftsResponse, StudioAttachmentDraft, StudioAttachmentDraftSource,
    StudioAttachmentModality,
};
use sha2::{Digest, Sha256};

use crate::studio::store::attachment::AttachmentDraftObject;

use super::normalize::normalize_loaded_source;
use super::source::{
    LoadedSource, MAX_GENERIC_SOURCE_BYTES, MAX_IMAGE_SOURCE_BYTES, fetch_remote_snapshot,
    read_local_file,
};
use super::validate::{
    admission_rejection, model_modality, preflight_sources, validate_actual_limits,
    validate_draft_batch, validate_model_source,
};

const ATTACHMENT_DRAFT_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Clone)]
pub(in crate::studio::runtime) struct AttachmentDraftRuntime {
    root: Arc<PathBuf>,
    entries: Arc<tokio::sync::Mutex<BTreeMap<String, AttachmentDraftObject>>>,
}

impl AttachmentDraftRuntime {
    pub(in crate::studio::runtime) fn new(root: PathBuf) -> Result<Self> {
        if root.exists() {
            std::fs::remove_dir_all(&root).context("failed to clear expired attachment drafts")?;
        }
        std::fs::create_dir_all(&root).context("failed to create attachment draft directory")?;
        Ok(Self {
            root: Arc::new(root),
            entries: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
        })
    }

    pub(super) async fn admit(
        &self,
        sources: Vec<StudioAttachmentDraftSource>,
        model: &ModelInfo,
    ) -> Result<AdmitAttachmentDraftsResponse> {
        self.remove_expired().await;
        let predicted = preflight_sources(&sources, model).map_err(admission_rejection)?;

        let mut admitted = Vec::with_capacity(sources.len());
        for (source, (filename, predicted_modality, source_kind)) in
            sources.into_iter().zip(predicted)
        {
            let max_bytes = model
                .capabilities
                .input_capability(model_modality(predicted_modality))
                .and_then(|capability| capability.limits.max_bytes)
                .unwrap_or(match predicted_modality {
                    StudioAttachmentModality::Image => MAX_IMAGE_SOURCE_BYTES,
                    StudioAttachmentModality::Video | StudioAttachmentModality::File => {
                        MAX_GENERIC_SOURCE_BYTES
                    }
                });
            let result = async {
                let loaded = match source {
                    StudioAttachmentDraftSource::LocalFile { path } => LoadedSource {
                        bytes: read_local_file(Path::new(&path), max_bytes)
                            .await
                            .map_err(admission_rejection)?,
                        initial_remote_url: None,
                    },
                    StudioAttachmentDraftSource::RemoteUrl { url, .. } => {
                        fetch_remote_snapshot(&url, max_bytes)
                            .await
                            .map_err(admission_rejection)?
                    }
                };
                let normalized = normalize_loaded_source(&filename, loaded.bytes)
                    .map_err(admission_rejection)?;
                if normalized.modality != predicted_modality {
                    return Err(admission_rejection(anyhow::anyhow!(
                        "attachment content does not match its filename type"
                    )));
                }
                validate_model_source(model, normalized.modality, source_kind)
                    .map_err(admission_rejection)?;
                validate_actual_limits(model, &normalized).map_err(admission_rejection)?;
                let draft_id = crate::studio::ids::new_id("attachment-draft");
                let storage_path = self.root.join(&draft_id);
                tokio::fs::write(&storage_path, &normalized.bytes).await?;
                Ok::<_, anyhow::Error>(AttachmentDraftObject {
                    draft_id,
                    modality: normalized.modality,
                    media_type: normalized.media_type,
                    filename,
                    storage_path,
                    byte_size: normalized.bytes.len() as u64,
                    content_sha256: format!("{:x}", Sha256::digest(&normalized.bytes)),
                    width: normalized.width,
                    height: normalized.height,
                    initial_remote_url: loaded.initial_remote_url,
                    admitted_at: Instant::now(),
                })
            }
            .await;
            let draft = match result {
                Ok(draft) => draft,
                Err(error) => {
                    cleanup_drafts(&admitted).await;
                    return Err(error);
                }
            };
            admitted.push(draft);
        }

        if let Err(error) = validate_draft_batch(model, &admitted).map_err(admission_rejection) {
            cleanup_drafts(&admitted).await;
            return Err(error);
        }
        let response = AdmitAttachmentDraftsResponse {
            drafts: admitted.iter().map(public_draft).collect(),
        };
        let mut entries = self.entries.lock().await;
        for draft in admitted {
            entries.insert(draft.draft_id.clone(), draft);
        }
        Ok(response)
    }

    pub(in crate::studio::runtime) fn validate_for_model(
        &self,
        model: &ModelInfo,
        drafts: &[AttachmentDraftObject],
    ) -> Result<()> {
        validate_draft_batch(model, drafts)
    }

    pub(in crate::studio::runtime) async fn resolve(
        &self,
        draft_ids: &[String],
    ) -> Result<Vec<AttachmentDraftObject>> {
        self.remove_expired().await;
        let entries = self.entries.lock().await;
        let mut seen = BTreeSet::new();
        let mut drafts = Vec::with_capacity(draft_ids.len());
        for draft_id in draft_ids {
            ensure!(seen.insert(draft_id), "duplicate attachment draft id");
            let draft = entries
                .get(draft_id)
                .with_context(|| format!("attachment draft {draft_id} is unavailable"))?;
            drafts.push(draft.clone());
        }
        Ok(drafts)
    }

    pub(in crate::studio::runtime) async fn remove(&self, draft_id: &str) -> Result<bool> {
        let draft = self.entries.lock().await.remove(draft_id);
        if let Some(draft) = draft {
            let _ = tokio::fs::remove_file(draft.storage_path).await;
            return Ok(true);
        }
        Ok(false)
    }

    pub(in crate::studio::runtime) async fn commit(&self, draft_ids: &[String]) {
        let mut entries = self.entries.lock().await;
        let drafts = draft_ids
            .iter()
            .filter_map(|draft_id| entries.remove(draft_id))
            .collect::<Vec<_>>();
        drop(entries);
        cleanup_drafts(&drafts).await;
    }

    pub(in crate::studio::runtime) async fn read(&self, draft_id: &str) -> Result<Vec<u8>> {
        self.remove_expired().await;
        let path = self
            .entries
            .lock()
            .await
            .get(draft_id)
            .map(|draft| draft.storage_path.clone())
            .with_context(|| format!("attachment draft {draft_id} is unavailable"))?;
        Ok(tokio::fs::read(path).await?)
    }

    async fn remove_expired(&self) {
        let mut entries = self.entries.lock().await;
        let now = Instant::now();
        let expired_ids = entries
            .iter()
            .filter(|(_, draft)| now.duration_since(draft.admitted_at) >= ATTACHMENT_DRAFT_TTL)
            .map(|(draft_id, _)| draft_id.clone())
            .collect::<Vec<_>>();
        let expired = expired_ids
            .iter()
            .filter_map(|draft_id| entries.remove(draft_id))
            .collect::<Vec<_>>();
        drop(entries);
        cleanup_drafts(&expired).await;
    }
}

fn public_draft(draft: &AttachmentDraftObject) -> StudioAttachmentDraft {
    StudioAttachmentDraft {
        draft_id: draft.draft_id.clone(),
        modality: draft.modality,
        media_type: draft.media_type.clone(),
        filename: draft.filename.clone(),
        byte_size: draft.byte_size,
        width: draft.width,
        height: draft.height,
    }
}

async fn cleanup_drafts(drafts: &[AttachmentDraftObject]) {
    for draft in drafts {
        let _ = tokio::fs::remove_file(&draft.storage_path).await;
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat};
    use pl_model::model::ModelModality;

    use super::*;

    pub(in crate::studio::runtime::attachment_drafts) fn glm_flash() -> ModelInfo {
        pl_model::model::default_models()
            .into_iter()
            .find(|model| model.slug == "glm-5.3-flash")
            .expect("GLM-5.3-Flash must be present in the canonical catalog")
    }

    pub(in crate::studio::runtime::attachment_drafts) fn deepseek_vision() -> ModelInfo {
        pl_model::model::default_models()
            .into_iter()
            .find(|model| model.slug == "deepseek-v4-flash-vision-exp")
            .expect("DeepSeek vision model must be present in the canonical catalog")
    }

    pub(in crate::studio::runtime::attachment_drafts) fn png_bytes() -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(2, 2)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    pub(in crate::studio::runtime::attachment_drafts) fn gif_bytes() -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(2, 2)
            .write_to(&mut bytes, ImageFormat::Gif)
            .unwrap();
        bytes.into_inner()
    }

    #[tokio::test]
    async fn unsupported_modality_is_rejected_before_local_file_io() {
        let root = tempfile::tempdir().unwrap();
        let drafts = AttachmentDraftRuntime::new(root.path().join("drafts")).unwrap();
        let missing_pdf = root.path().join("definitely-missing.pdf");

        let error = drafts
            .admit(
                vec![StudioAttachmentDraftSource::LocalFile {
                    path: missing_pdf.to_string_lossy().to_string(),
                }],
                &glm_flash(),
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("does not support File input"));
        assert!(!error.to_string().contains("failed to open"));
    }

    #[tokio::test]
    async fn failed_batch_admission_removes_every_partial_draft() {
        let root = tempfile::tempdir().unwrap();
        let draft_root = root.path().join("drafts");
        let drafts = AttachmentDraftRuntime::new(draft_root.clone()).unwrap();
        let valid = root.path().join("first.png");
        let invalid = root.path().join("second.png");
        tokio::fs::write(&valid, png_bytes()).await.unwrap();
        tokio::fs::write(&invalid, b"not an image").await.unwrap();

        let error = drafts
            .admit(
                vec![
                    StudioAttachmentDraftSource::LocalFile {
                        path: valid.to_string_lossy().to_string(),
                    },
                    StudioAttachmentDraftSource::LocalFile {
                        path: invalid.to_string_lossy().to_string(),
                    },
                ],
                &glm_flash(),
            )
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not match its filename type")
        );
        assert!(drafts.entries.lock().await.is_empty());
        assert!(
            tokio::fs::read_dir(&draft_root)
                .await
                .unwrap()
                .next_entry()
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn successful_batch_preserves_order_and_remove_revokes_the_draft() {
        let root = tempfile::tempdir().unwrap();
        let drafts = AttachmentDraftRuntime::new(root.path().join("drafts")).unwrap();
        let first = root.path().join("first.png");
        let second = root.path().join("second.png");
        let bytes = png_bytes();
        tokio::fs::write(&first, &bytes).await.unwrap();
        tokio::fs::write(&second, &bytes).await.unwrap();

        let admitted = drafts
            .admit(
                vec![
                    StudioAttachmentDraftSource::LocalFile {
                        path: first.to_string_lossy().to_string(),
                    },
                    StudioAttachmentDraftSource::LocalFile {
                        path: second.to_string_lossy().to_string(),
                    },
                ],
                &glm_flash(),
            )
            .await
            .unwrap();

        assert_eq!(admitted.drafts[0].filename, "first.png");
        assert_eq!(admitted.drafts[1].filename, "second.png");
        let first_id = admitted.drafts[0].draft_id.clone();
        let second_id = admitted.drafts[1].draft_id.clone();
        let resolved = drafts
            .resolve(&[second_id.clone(), first_id.clone()])
            .await
            .unwrap();
        assert_eq!(resolved[0].draft_id, second_id);
        assert_eq!(resolved[1].draft_id, first_id);
        assert_eq!(
            resolved[1].content_sha256,
            format!("{:x}", Sha256::digest(&bytes))
        );
        assert!(
            drafts
                .resolve(&[first_id.clone(), first_id.clone()])
                .await
                .unwrap_err()
                .to_string()
                .contains("duplicate attachment draft id")
        );
        assert!(
            drafts
                .resolve(&["missing-draft".to_string()])
                .await
                .unwrap_err()
                .to_string()
                .contains("is unavailable")
        );
        assert_eq!(drafts.read(&first_id).await.unwrap(), bytes);
        assert!(drafts.remove(&first_id).await.unwrap());
        assert!(drafts.read(&first_id).await.is_err());
    }

    #[tokio::test]
    async fn expired_draft_is_deleted_before_it_can_be_read() {
        let root = tempfile::tempdir().unwrap();
        let drafts = AttachmentDraftRuntime::new(root.path().join("drafts")).unwrap();
        let image = root.path().join("expiring.png");
        tokio::fs::write(&image, png_bytes()).await.unwrap();

        let response = drafts
            .admit(
                vec![StudioAttachmentDraftSource::LocalFile {
                    path: image.to_string_lossy().to_string(),
                }],
                &glm_flash(),
            )
            .await
            .unwrap();
        let draft_id = response.drafts[0].draft_id.clone();
        let storage_path = {
            let mut entries = drafts.entries.lock().await;
            let draft = entries.get_mut(&draft_id).unwrap();
            draft.admitted_at = Instant::now() - ATTACHMENT_DRAFT_TTL;
            draft.storage_path.clone()
        };

        let error = drafts.read(&draft_id).await.unwrap_err();

        assert!(error.to_string().contains("is unavailable"));
        assert!(!tokio::fs::try_exists(storage_path).await.unwrap());
    }

    #[tokio::test]
    async fn image_dimensions_are_checked_against_the_model_profile() {
        let root = tempfile::tempdir().unwrap();
        let drafts = AttachmentDraftRuntime::new(root.path().join("drafts")).unwrap();
        let image = root.path().join("too-wide.png");
        tokio::fs::write(&image, png_bytes()).await.unwrap();
        let mut model = glm_flash();
        model
            .capabilities
            .input
            .iter_mut()
            .find(|capability| capability.modality == ModelModality::Image)
            .unwrap()
            .limits
            .max_width = Some(1);

        let error = drafts
            .admit(
                vec![StudioAttachmentDraftSource::LocalFile {
                    path: image.to_string_lossy().to_string(),
                }],
                &model,
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("width exceeds model limit"));
    }

    #[tokio::test]
    async fn image_batch_total_bytes_are_checked_atomically() {
        let root = tempfile::tempdir().unwrap();
        let draft_root = root.path().join("drafts");
        let drafts = AttachmentDraftRuntime::new(draft_root.clone()).unwrap();
        let first = root.path().join("first.png");
        let second = root.path().join("second.png");
        let bytes = png_bytes();
        tokio::fs::write(&first, &bytes).await.unwrap();
        tokio::fs::write(&second, &bytes).await.unwrap();
        let mut model = glm_flash();
        model
            .capabilities
            .input
            .iter_mut()
            .find(|capability| capability.modality == ModelModality::Image)
            .unwrap()
            .limits
            .max_total_bytes = Some((bytes.len() * 2 - 1) as u64);

        let error = drafts
            .admit(
                vec![
                    StudioAttachmentDraftSource::LocalFile {
                        path: first.to_string_lossy().to_string(),
                    },
                    StudioAttachmentDraftSource::LocalFile {
                        path: second.to_string_lossy().to_string(),
                    },
                ],
                &model,
            )
            .await
            .unwrap_err();

        assert!(error.to_string().contains("total byte limit"));
        assert!(drafts.entries.lock().await.is_empty());
        assert!(
            tokio::fs::read_dir(draft_root)
                .await
                .unwrap()
                .next_entry()
                .await
                .unwrap()
                .is_none()
        );
    }
}
