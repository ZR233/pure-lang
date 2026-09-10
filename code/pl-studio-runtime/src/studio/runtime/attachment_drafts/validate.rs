//! 附件准入校验：来源模态预测、模型能力、数量/字节/混合策略等批次约束，以及拒绝错误的包装。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result, ensure};
use pl_model::model::{
    MediaMixPolicy, MediaRepresentation, ModelInfo, ModelInputSource, ModelModality,
};
use pl_protocol::PureError;
use pl_protocol::studio::{StudioAttachmentDraftSource, StudioAttachmentModality};
use url::Url;

use crate::studio::store::attachment::AttachmentDraftObject;

use super::normalize::{NormalizedSource, sanitize_filename};
use super::source::validate_remote_url;

fn validate_predicted_counts(
    model: &ModelInfo,
    modalities: impl Iterator<Item = StudioAttachmentModality>,
) -> Result<()> {
    let mut counts = BTreeMap::new();
    for modality in modalities {
        *counts.entry(modality).or_insert(0_u32) += 1;
    }
    for (modality, count) in counts {
        let capability = model
            .capabilities
            .input_capability(model_modality(modality))
            .with_context(|| {
                format!("model {} does not support {:?} input", model.slug, modality)
            })?;
        if let Some(max_count) = capability.limits.max_count {
            ensure!(count <= max_count, "attachment count exceeds model limit");
        }
    }
    Ok(())
}

fn validate_total_bytes(model: &ModelInfo, drafts: &[AttachmentDraftObject]) -> Result<()> {
    let mut totals = BTreeMap::new();
    for draft in drafts {
        let total = totals.entry(draft.modality).or_insert(0_u64);
        *total = total
            .checked_add(draft.byte_size)
            .context("attachment batch byte count overflowed")?;
    }
    for (modality, total) in totals {
        let capability = model
            .capabilities
            .input_capability(model_modality(modality))
            .context("attachment modality is not enabled")?;
        if let Some(max_total_bytes) = capability.limits.max_total_bytes {
            ensure!(
                total <= max_total_bytes,
                "attachment batch exceeds model total byte limit"
            );
        }
    }
    Ok(())
}

pub(super) fn validate_actual_limits(model: &ModelInfo, source: &NormalizedSource) -> Result<()> {
    let capability = model
        .capabilities
        .input_capability(model_modality(source.modality))
        .context("attachment modality is not enabled")?;
    if let Some(max_bytes) = capability.limits.max_bytes {
        ensure!(
            source.bytes.len() as u64 <= max_bytes,
            "attachment exceeds model byte limit"
        );
    }
    if let Some(max_width) = capability.limits.max_width {
        ensure!(
            source.width.is_some_and(|width| width <= max_width),
            "attachment image width exceeds model limit"
        );
    }
    if let Some(max_height) = capability.limits.max_height {
        ensure!(
            source.height.is_some_and(|height| height <= max_height),
            "attachment image height exceeds model limit"
        );
    }
    if !capability.limits.media_types.is_empty() {
        ensure!(
            capability
                .limits
                .media_types
                .iter()
                .any(|media_type| media_type == &source.media_type),
            "attachment media type is not supported by the model"
        );
    }
    Ok(())
}

pub(super) fn validate_draft_batch(
    model: &ModelInfo,
    drafts: &[AttachmentDraftObject],
) -> Result<()> {
    validate_mix_policy(model, drafts.iter().map(|draft| draft.modality))?;
    validate_predicted_counts(model, drafts.iter().map(|draft| draft.modality))?;
    validate_total_bytes(model, drafts)?;
    for draft in drafts {
        validate_model_source(
            model,
            draft.modality,
            if draft.initial_remote_url.is_some() {
                DraftSourceKind::RemoteUrl
            } else {
                DraftSourceKind::Local
            },
        )?;
        let source = NormalizedSource {
            modality: draft.modality,
            media_type: draft.media_type.clone(),
            bytes: Vec::new(),
            width: draft.width,
            height: draft.height,
        };
        let capability = model
            .capabilities
            .input_capability(model_modality(draft.modality))
            .context("attachment modality is not enabled")?;
        if let Some(max_bytes) = capability.limits.max_bytes {
            ensure!(
                draft.byte_size <= max_bytes,
                "attachment exceeds model byte limit"
            );
        }
        if let Some(max_width) = capability.limits.max_width {
            ensure!(
                draft.width.is_some_and(|width| width <= max_width),
                "attachment image width exceeds model limit"
            );
        }
        if let Some(max_height) = capability.limits.max_height {
            ensure!(
                draft.height.is_some_and(|height| height <= max_height),
                "attachment image height exceeds model limit"
            );
        }
        if !capability.limits.media_types.is_empty() {
            ensure!(
                capability
                    .limits
                    .media_types
                    .iter()
                    .any(|media_type| media_type == &source.media_type),
                "attachment media type is not supported by the model"
            );
        }
    }
    Ok(())
}

pub(super) fn preflight_sources(
    sources: &[StudioAttachmentDraftSource],
    model: &ModelInfo,
) -> Result<Vec<(String, StudioAttachmentModality, DraftSourceKind)>> {
    ensure!(!sources.is_empty(), "attachment source batch is empty");
    let mut predicted = Vec::with_capacity(sources.len());
    for source in sources {
        let (filename, modality, source_kind) = predict_source(source)?;
        validate_model_source(model, modality, source_kind)?;
        predicted.push((filename, modality, source_kind));
    }
    validate_mix_policy(model, predicted.iter().map(|entry| entry.1))?;
    validate_predicted_counts(model, predicted.iter().map(|entry| entry.1))?;
    Ok(predicted)
}

pub(super) fn admission_rejection(error: anyhow::Error) -> anyhow::Error {
    anyhow::Error::new(PureError::ConfigError(format!(
        "attachment admission rejected: {error:#}"
    )))
}

#[derive(Debug, Clone, Copy)]
pub(super) enum DraftSourceKind {
    Local,
    RemoteUrl,
}

fn predict_source(
    source: &StudioAttachmentDraftSource,
) -> Result<(String, StudioAttachmentModality, DraftSourceKind)> {
    match source {
        StudioAttachmentDraftSource::LocalFile { path } => {
            let filename = Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .map(sanitize_filename)
                .filter(|name| !name.is_empty())
                .context("attachment local file has no safe filename")?;
            let modality = modality_from_filename(&filename)?;
            Ok((filename, modality, DraftSourceKind::Local))
        }
        StudioAttachmentDraftSource::RemoteUrl { url, filename } => {
            let parsed = validate_remote_url(Url::parse(url).context("invalid attachment URL")?)?;
            let filename = filename
                .as_deref()
                .map(sanitize_filename)
                .filter(|name| !name.is_empty())
                .or_else(|| parsed.path_segments()?.next_back().map(sanitize_filename))
                .filter(|name| !name.is_empty())
                .context("remote attachment URL needs a filename with an extension")?;
            let modality = modality_from_filename(&filename)?;
            Ok((filename, modality, DraftSourceKind::RemoteUrl))
        }
    }
}

pub(super) fn validate_model_source(
    model: &ModelInfo,
    modality: StudioAttachmentModality,
    source: DraftSourceKind,
) -> Result<()> {
    let model_modality = model_modality(modality);
    let capability = model
        .capabilities
        .input_capability(model_modality)
        .with_context(|| format!("model {} does not support {:?} input", model.slug, modality))?;
    let source = match source {
        DraftSourceKind::Local => ModelInputSource::Local,
        DraftSourceKind::RemoteUrl => ModelInputSource::RemoteUrl,
    };
    ensure!(
        capability.supports_source(source),
        "model {} does not support {:?} {:?} input",
        model.slug,
        source,
        modality
    );
    let profile = model
        .binding
        .request
        .media_profile(model_modality)
        .with_context(|| format!("model {} has no {:?} request profile", model.slug, modality))?;
    ensure!(
        profile.replay.iter().any(|representation| matches!(
            representation,
            MediaRepresentation::DataUrl | MediaRepresentation::ProviderFile
        )),
        "model {} cannot replay {:?} snapshots",
        model.slug,
        modality
    );
    Ok(())
}

fn validate_mix_policy(
    model: &ModelInfo,
    modalities: impl Iterator<Item = StudioAttachmentModality>,
) -> Result<()> {
    if model.binding.request.media_mix_policy != MediaMixPolicy::SingleModality {
        return Ok(());
    }
    let unique = modalities.collect::<BTreeSet<_>>();
    ensure!(
        unique.len() <= 1,
        "model {} accepts only one attachment modality per request",
        model.slug
    );
    Ok(())
}

pub(super) fn model_modality(modality: StudioAttachmentModality) -> ModelModality {
    match modality {
        StudioAttachmentModality::Image => ModelModality::Image,
        StudioAttachmentModality::Video => ModelModality::Video,
        StudioAttachmentModality::File => ModelModality::File,
    }
}

fn modality_from_filename(filename: &str) -> Result<StudioAttachmentModality> {
    let extension = Path::new(filename)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .context("attachment filename needs an extension")?;
    Ok(match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" => StudioAttachmentModality::Image,
        "mp4" | "mov" | "webm" | "mkv" => StudioAttachmentModality::Video,
        _ => StudioAttachmentModality::File,
    })
}
