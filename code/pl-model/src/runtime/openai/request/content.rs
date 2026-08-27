use std::collections::HashMap;

use pl_protocol::{AttachmentModality, ContentPart, MessageContent, ModelContextItem, Result};

use crate::completion::{CompletionRequest, PreparedContentPart, PreparedContentSource};
use crate::model::info::{MediaMixPolicy, MediaRepresentation, MediaWireFormat, ModelInfo};

use super::protocol_error;
pub(super) fn message_content_text(content: &MessageContent) -> String {
    content.text_value()
}

#[derive(Debug, Default)]
pub(super) struct MediaRepresentationPlan {
    representations: HashMap<AttachmentModality, MediaRepresentation>,
    wires: HashMap<AttachmentModality, MediaWireFormat>,
}

impl MediaRepresentationPlan {
    pub(super) fn for_request(request: &CompletionRequest, model: &ModelInfo) -> Result<Self> {
        let mut attachment_ids = HashMap::<AttachmentModality, Vec<&str>>::new();
        let mut seen = HashMap::<&str, AttachmentModality>::new();

        for content in request
            .input
            .iter()
            .filter_map(ModelContextItem::as_message)
        {
            for part in &content.content.parts {
                let ContentPart::Attachment {
                    attachment_id,
                    modality,
                    ..
                } = part
                else {
                    continue;
                };
                if let Some(existing) = seen.insert(attachment_id, *modality) {
                    if existing != *modality {
                        return Err(protocol_error(format!(
                            "model={} attachment modality conflict for attachment {attachment_id}",
                            model.slug
                        )));
                    }
                    continue;
                }
                attachment_ids
                    .entry(*modality)
                    .or_default()
                    .push(attachment_id);
            }
        }

        if model.request_profile.media_mix_policy == MediaMixPolicy::SingleModality
            && attachment_ids.len() > 1
        {
            return Err(protocol_error(format!(
                "model={} rejects mixed media modalities",
                model.slug
            )));
        }

        let mut representations = HashMap::new();
        let mut wires = HashMap::new();
        for (modality, ids) in attachment_ids {
            let media = ids
                .iter()
                .map(|attachment_id| prepared_content(request, attachment_id, modality, model))
                .collect::<Result<Vec<_>>>()?;
            let profile = model
                .request_profile
                .media_profile(model_modality(modality))
                .ok_or_else(|| {
                    protocol_error(format!(
                        "model={} modality={} count={} has no media request profile",
                        model.slug,
                        modality_label(modality),
                        media.len()
                    ))
                })?;
            let candidates = if media
                .iter()
                .all(|item| item.sources.iter().all(is_snapshot_source))
            {
                &profile.replay
            } else {
                &profile.first_send
            };
            let representation = candidates
                .iter()
                .copied()
                .find(|candidate| {
                    media
                        .iter()
                        .all(|item| has_representation(item, *candidate))
                })
                .ok_or_else(|| {
                    protocol_error(format!(
                        "model={} modality={} count={} has no common media representation",
                        model.slug,
                        modality_label(modality),
                        media.len()
                    ))
                })?;
            tracing::info!(
                model = %model.slug,
                modality = modality_label(modality),
                representation = representation_label(representation),
                count = media.len(),
                "planned media request representation"
            );
            representations.insert(modality, representation);
            wires.insert(modality, profile.wire);
        }

        Ok(Self {
            representations,
            wires,
        })
    }

    fn representation(&self, modality: AttachmentModality) -> Result<MediaRepresentation> {
        self.representations.get(&modality).copied().ok_or_else(|| {
            protocol_error(format!(
                "modality={} was not planned before serialization",
                modality_label(modality)
            ))
        })
    }

    pub(super) fn wire(&self, modality: AttachmentModality) -> Result<MediaWireFormat> {
        self.wires.get(&modality).copied().ok_or_else(|| {
            protocol_error(format!(
                "modality={} has no planned provider wire mapping",
                modality_label(modality)
            ))
        })
    }
}

pub(super) fn media_url(
    attachment_id: &str,
    media_type: &str,
    modality: AttachmentModality,
    prepared_content: &[PreparedContentPart],
    plan: &MediaRepresentationPlan,
) -> Result<String> {
    let media = prepared_content
        .iter()
        .find(|media| media.attachment_id == attachment_id)
        .ok_or_else(|| {
            protocol_error(format!(
                "attachment {attachment_id} was not prepared before model request"
            ))
        })?;
    if media.modality != modality {
        return Err(protocol_error(format!(
            "attachment {attachment_id} prepared modality does not match message content"
        )));
    }
    let representation = plan.representation(modality)?;
    let source = media
        .sources
        .iter()
        .find(|source| source_matches(source, representation))
        .ok_or_else(|| {
            protocol_error(format!(
                "attachment {attachment_id} is missing its planned media representation"
            ))
        })?;
    match source {
        PreparedContentSource::DataUrl { base64 } => {
            let actual_media_type = if media_type.is_empty() {
                media.media_type.as_str()
            } else {
                media_type
            };
            Ok(format!("data:{actual_media_type};base64,{base64}"))
        }
        PreparedContentSource::RemoteUrl { url } => Ok(url.clone()),
        PreparedContentSource::ProviderFile { .. } => Err(protocol_error(
            "provider file cannot be serialized as a URL content part",
        )),
    }
}

fn prepared_content<'a>(
    request: &'a CompletionRequest,
    attachment_id: &str,
    modality: AttachmentModality,
    model: &ModelInfo,
) -> Result<&'a PreparedContentPart> {
    let matching = request
        .prepared_content
        .iter()
        .filter(|media| media.attachment_id == attachment_id)
        .collect::<Vec<_>>();
    let [media] = matching.as_slice() else {
        return Err(protocol_error(format!(
            "model={} modality={} attachment {attachment_id} must have exactly one prepared payload",
            model.slug,
            modality_label(modality)
        )));
    };
    if media.modality != modality {
        return Err(protocol_error(format!(
            "model={} attachment {attachment_id} prepared modality does not match message content",
            model.slug
        )));
    }
    Ok(media)
}

fn has_representation(media: &PreparedContentPart, representation: MediaRepresentation) -> bool {
    media
        .sources
        .iter()
        .any(|source| source_matches(source, representation))
}

fn source_matches(source: &PreparedContentSource, representation: MediaRepresentation) -> bool {
    matches!(
        (source, representation),
        (
            PreparedContentSource::RemoteUrl { .. },
            MediaRepresentation::RemoteUrl
        ) | (
            PreparedContentSource::ProviderFile { .. },
            MediaRepresentation::ProviderFile
        ) | (
            PreparedContentSource::DataUrl { .. },
            MediaRepresentation::DataUrl
        )
    )
}

fn is_snapshot_source(source: &PreparedContentSource) -> bool {
    matches!(source, PreparedContentSource::DataUrl { .. })
}

fn model_modality(modality: AttachmentModality) -> crate::ModelModality {
    match modality {
        AttachmentModality::Image => crate::ModelModality::Image,
        AttachmentModality::Video => crate::ModelModality::Video,
        AttachmentModality::File => crate::ModelModality::File,
    }
}

fn modality_label(modality: AttachmentModality) -> &'static str {
    match modality {
        AttachmentModality::Image => "image",
        AttachmentModality::Video => "video",
        AttachmentModality::File => "file",
    }
}

fn representation_label(representation: MediaRepresentation) -> &'static str {
    match representation {
        MediaRepresentation::RemoteUrl => "remoteUrl",
        MediaRepresentation::ProviderFile => "providerFile",
        MediaRepresentation::DataUrl => "dataUrl",
    }
}
