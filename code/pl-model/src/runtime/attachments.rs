//! Provider-owned attachment preparation. Durable history never contains upload handles.
use base64::{Engine, engine::general_purpose::STANDARD};
use futures::StreamExt;
use pl_protocol::{PureError, Result};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc};

use super::{ModelSession, provider_error::reqwest_error_to_pure, transport};
use crate::completion::{
    AttachmentInput, AttachmentRepresentation, AttachmentSource, CompletionRequest,
    ResolvedAttachment,
};
use crate::model::{ModelInfo, ModelModality};
use crate::provider::{FileUploadCapability, ProviderEndpoint};

pub(super) enum UploadState {
    Pending,
    Ready { id: String, expires_at: i64 },
    Unsupported,
}

pub(super) struct AttachmentBackend<'a> {
    pub endpoint: &'a ProviderEndpoint,
    pub model: &'a ModelInfo,
    pub client: &'a reqwest::Client,
    pub session: &'a ModelSession,
    pub fingerprint: u64,
    pub now: i64,
    pub cancellation: tokio_util::sync::CancellationToken,
}

impl AttachmentBackend<'_> {
    pub async fn prepare(&self, request: &mut CompletionRequest) -> Result<()> {
        self.prepare_inner(request).await.map_err(|mut error| {
            if let PureError::Provider(failure) = &mut error {
                failure.context.stage = pl_protocol::ProviderFailureStage::Attachment;
                failure.context.recovery = pl_protocol::ProviderRecovery::None;
                failure.retry = pl_protocol::RetryDisposition::Permanent;
            }
            error
        })
    }

    async fn prepare_inner(&self, request: &mut CompletionRequest) -> Result<()> {
        if request.attachments.is_empty() {
            return Ok(());
        }
        let mut materialized = Vec::new();
        let mut identities = std::collections::HashSet::new();
        let mut totals = HashMap::new();
        // Validate the entire batch before creating any remote upload.
        for attachment in &request.attachments {
            if !identities.insert(&attachment.attachment_id) {
                return Err(PureError::ConfigError(
                    "duplicate attachment identity".into(),
                ));
            }
            let modality = match attachment.modality {
                crate::completion::AttachmentModality::Image => ModelModality::Image,
                crate::completion::AttachmentModality::Video => ModelModality::Video,
                crate::completion::AttachmentModality::File => ModelModality::File,
            };
            let capability = self
                .model
                .capabilities
                .input_capability(modality)
                .ok_or_else(|| PureError::ConfigError("unsupported attachment modality".into()))?;
            let limits = &capability.limits;
            if !capability.supports_source(crate::model::ModelInputSource::Local) {
                return Err(PureError::ConfigError(
                    "model cannot replay retained attachment content".into(),
                ));
            }
            if let AttachmentSource::Resource { reference, .. } = &attachment.source
                && (reference.byte_len() > limits.max_bytes.unwrap_or(32 * 1024 * 1024)
                    || reference.media_type() != attachment.media_type)
            {
                return Err(PureError::ConfigError(
                    "resource metadata violates attachment limits".into(),
                ));
            }
            let maximum = limits.max_bytes.unwrap_or(32 * 1024 * 1024);
            let bytes = self.materialize(attachment, maximum).await?;
            if bytes.len() as u64 > maximum
                || (!limits.media_types.is_empty()
                    && !limits.media_types.contains(&attachment.media_type))
            {
                return Err(PureError::ConfigError(
                    "attachment violates model size or media type limits".into(),
                ));
            }
            if modality == ModelModality::Image {
                let reader =
                    image::ImageReader::new(std::io::Cursor::new(&*bytes)).with_guessed_format()?;
                if reader
                    .format()
                    .is_none_or(|format| format.to_mime_type() != attachment.media_type)
                {
                    return Err(PureError::ConfigError(
                        "attachment MIME does not match image bytes".into(),
                    ));
                }
                let (width, height) = reader
                    .into_dimensions()
                    .map_err(|error| PureError::ConfigError(error.to_string()))?;
                if limits.max_width.is_some_and(|max| width > max)
                    || limits.max_height.is_some_and(|max| height > max)
                {
                    return Err(PureError::ConfigError(
                        "attachment image dimensions exceed model limits".into(),
                    ));
                }
            }
            let (count, total) = totals.entry(attachment.modality).or_insert((0_u64, 0_u64));
            let occurrences = request.input.iter().map(|item| match item {
                crate::completion::ModelContextItem::ToolMedia { items } => items.iter().filter(|item| item.attachment.id == attachment.attachment_id).count(),
                _ => item.as_message().map_or(0, |message| message.content.parts.iter().filter(|part| matches!(part,
                    crate::completion::ContentPart::Attachment { attachment_id, .. } if attachment_id == &attachment.attachment_id)).count()),
            }).sum::<usize>() as u64;
            if occurrences == 0 {
                return Err(PureError::ConfigError(
                    "unreferenced attachment input".into(),
                ));
            }
            *count = count
                .checked_add(occurrences)
                .ok_or_else(|| PureError::ConfigError("attachment count overflow".into()))?;
            *total = total
                .checked_add(
                    (bytes.len() as u64)
                        .checked_mul(occurrences)
                        .ok_or_else(|| PureError::ConfigError("attachment size overflow".into()))?,
                )
                .ok_or_else(|| PureError::ConfigError("attachment size overflow".into()))?;
            if limits.max_count.is_some_and(|max| *count > u64::from(max))
                || limits.max_total_bytes.is_some_and(|max| *total > max)
            {
                return Err(PureError::ConfigError(
                    "attachment batch exceeds model limits".into(),
                ));
            }
            materialized.push(bytes);
        }
        request.prepared_content.clear();
        for (attachment, bytes) in request.attachments.iter_mut().zip(materialized) {
            let remote_url = match &attachment.source {
                AttachmentSource::Url { url } => Some(url.clone()),
                _ => None,
            };
            let mut sources = vec![AttachmentRepresentation::DataUrl {
                base64: STANDARD.encode(&bytes),
            }];
            if let Some(url) = remote_url {
                sources.push(AttachmentRepresentation::RemoteUrl { url });
            } else if self.endpoint.service_capabilities.files != FileUploadCapability::None
                && self.model.binding.transport.protocol
                    == crate::provider::ProviderWireProtocol::Responses
                && let Some(id) = self.upload(attachment, &bytes).await?
            {
                sources.push(AttachmentRepresentation::ProviderFile { file_id: id });
            }
            attachment.source = AttachmentSource::Bytes { bytes };
            request.prepared_content.push(ResolvedAttachment {
                attachment_id: attachment.attachment_id.clone(),
                modality: attachment.modality,
                media_type: attachment.media_type.clone(),
                filename: attachment.filename.clone(),
                sources,
            });
        }
        Ok(())
    }

    async fn materialize(&self, input: &AttachmentInput, maximum: u64) -> Result<Arc<[u8]>> {
        match &input.source {
            AttachmentSource::Bytes { bytes } => Ok(bytes.clone()),
            AttachmentSource::Base64 { base64 } => {
                if base64.len() as u64
                    > maximum
                        .saturating_add(2)
                        .saturating_div(3)
                        .saturating_mul(4)
                {
                    return Err(PureError::ConfigError(
                        "encoded attachment exceeds model limits".into(),
                    ));
                }
                STANDARD
                    .decode(base64)
                    .map(Arc::from)
                    .map_err(|_| PureError::ConfigError("invalid attachment Base64".into()))
            }
            AttachmentSource::Resource { reference, access } => access
                .read(reference, self.cancellation.clone())
                .await
                .map_err(|error| PureError::Io(std::io::Error::other(error))),
            AttachmentSource::Url { url } => {
                let url = reqwest::Url::parse(url)
                    .map_err(|_| PureError::ConfigError("invalid attachment URL".into()))?;
                if !matches!(url.scheme(), "http" | "https")
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err(PureError::ConfigError(
                        "attachment URL must be HTTP(S) without credentials".into(),
                    ));
                }
                let response = transport::checked(
                    self.client
                        .get(url)
                        .send()
                        .await
                        .map_err(reqwest_error_to_pure)?,
                )
                .await?;
                let mut stream = response.bytes_stream();
                let mut bytes = Vec::new();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(reqwest_error_to_pure)?;
                    if (bytes.len() as u64).saturating_add(chunk.len() as u64) > maximum {
                        return Err(PureError::ConfigError(
                            "downloaded attachment exceeds model limits".into(),
                        ));
                    }
                    bytes.extend_from_slice(&chunk);
                }
                Ok(bytes.into())
            }
        }
    }

    async fn upload(&self, input: &AttachmentInput, bytes: &[u8]) -> Result<Option<String>> {
        let key = (
            self.fingerprint,
            format!(
                "{}:{}",
                self.endpoint.service_capabilities.files.policy_key(),
                hex::encode(Sha256::digest(bytes))
            ),
        );
        {
            let mut cache = self.session.uploaded_files.lock().await;
            match cache.get(&key) {
                Some(UploadState::Ready { id, expires_at }) if *expires_at > self.now.saturating_add(60) => return Ok(Some(id.clone())),
                Some(UploadState::Unsupported) => return Ok(None),
                Some(UploadState::Pending) => return Err(PureError::HttpError("previous attachment upload outcome is unknown; start a new model session before uploading again".into())),
                _ => {}
            }
            cache.insert(key.clone(), UploadState::Pending);
        }
        let uploaded = self
            .endpoint
            .service_capabilities
            .files
            .upload(crate::provider::files::FileUploadRequest {
                client: self.client,
                endpoint: self.endpoint,
                model_headers: &self.model.binding.request.headers,
                input,
                bytes,
                now: self.now,
            })
            .await;
        let uploaded = match uploaded {
            Ok(Some(uploaded)) => uploaded,
            Ok(None) => {
                self.session
                    .uploaded_files
                    .lock()
                    .await
                    .insert(key, UploadState::Unsupported);
                return Ok(None);
            }
            Err(error) => {
                if error
                    .provider_failure_ref()
                    .is_some_and(|failure| failure.http_status.is_some())
                {
                    self.session.uploaded_files.lock().await.remove(&key);
                }
                return Err(error);
            }
        };
        self.session.uploaded_files.lock().await.insert(
            key,
            UploadState::Ready {
                id: uploaded.id.clone(),
                expires_at: uploaded.expires_at,
            },
        );
        Ok(Some(uploaded.id))
    }
}
