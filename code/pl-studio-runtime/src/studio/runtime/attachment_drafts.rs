use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use futures::StreamExt;

use pl_protocol::PureError;
use pl_protocol::studio::{
    AdmitAttachmentDraftsRequest, AdmitAttachmentDraftsResponse, StudioAttachmentAdmissionContext,
    StudioAttachmentDraft, StudioAttachmentDraftSource, StudioAttachmentModality,
};
use reqwest::header::LOCATION;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use url::Url;

use crate::config::{StudioMode, StudioRole};
use crate::studio::store::attachment::{AttachmentDraftObject, normalize_image_attachment};

use super::StudioRuntime;
use pl_model::model::{
    MediaMixPolicy, MediaRepresentation, ModelInfo, ModelInputSource, ModelModality,
};

const MAX_IMAGE_SOURCE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_GENERIC_SOURCE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_REDIRECTS: usize = 3;
const REMOTE_FETCH_TOTAL_TIMEOUT: Duration = Duration::from_secs(45);
const ATTACHMENT_DRAFT_TTL: Duration = Duration::from_secs(30 * 60);

#[derive(Clone)]
pub(super) struct AttachmentDraftRuntime {
    root: Arc<PathBuf>,
    entries: Arc<tokio::sync::Mutex<BTreeMap<String, AttachmentDraftObject>>>,
}

impl AttachmentDraftRuntime {
    pub(super) fn new(root: PathBuf) -> Result<Self> {
        if root.exists() {
            std::fs::remove_dir_all(&root).context("failed to clear expired attachment drafts")?;
        }
        std::fs::create_dir_all(&root).context("failed to create attachment draft directory")?;
        Ok(Self {
            root: Arc::new(root),
            entries: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
        })
    }

    async fn admit(
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

    pub(super) fn validate_for_model(
        &self,
        model: &ModelInfo,
        drafts: &[AttachmentDraftObject],
    ) -> Result<()> {
        validate_draft_batch(model, drafts)
    }

    pub(super) async fn resolve(&self, draft_ids: &[String]) -> Result<Vec<AttachmentDraftObject>> {
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

    pub(super) async fn remove(&self, draft_id: &str) -> Result<bool> {
        let draft = self.entries.lock().await.remove(draft_id);
        if let Some(draft) = draft {
            let _ = tokio::fs::remove_file(draft.storage_path).await;
            return Ok(true);
        }
        Ok(false)
    }

    pub(super) async fn commit(&self, draft_ids: &[String]) {
        let mut entries = self.entries.lock().await;
        let drafts = draft_ids
            .iter()
            .filter_map(|draft_id| entries.remove(draft_id))
            .collect::<Vec<_>>();
        drop(entries);
        cleanup_drafts(&drafts).await;
    }

    pub(super) async fn read(&self, draft_id: &str) -> Result<Vec<u8>> {
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

fn validate_actual_limits(model: &ModelInfo, source: &NormalizedSource) -> Result<()> {
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

fn validate_draft_batch(model: &ModelInfo, drafts: &[AttachmentDraftObject]) -> Result<()> {
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

impl StudioRuntime {
    pub async fn preflight_attachment_drafts(
        &self,
        request: &AdmitAttachmentDraftsRequest,
    ) -> Result<()> {
        let model = self.attachment_model_for_context(&request.context).await?;
        preflight_sources(&request.sources, &model)
            .map(|_| ())
            .map_err(admission_rejection)
    }

    pub async fn admit_attachment_drafts(
        &self,
        request: AdmitAttachmentDraftsRequest,
    ) -> Result<AdmitAttachmentDraftsResponse> {
        let model = self.attachment_model_for_context(&request.context).await?;
        self.attachment_drafts.admit(request.sources, &model).await
    }

    async fn attachment_model_for_context(
        &self,
        context: &StudioAttachmentAdmissionContext,
    ) -> Result<ModelInfo> {
        let role = match context {
            StudioAttachmentAdmissionContext::ExistingThread { thread_id } => {
                let thread = self.read_owned_thread(thread_id).await?;
                StudioRole::from_key(&thread.role).context("Thread has an invalid model role")?
            }
            StudioAttachmentAdmissionContext::NewThread { mode } => {
                StudioMode::from_label(mode)
                    .map_err(|_| anyhow::anyhow!("mode must be an available mode.* Skill id"))?;
                StudioRole::Planner
            }
        };
        let config = self.config_runtime.read()?;
        let route = config.config.models.resolve(&role.id())?;
        Ok(route.model)
    }

    pub async fn remove_attachment_draft(&self, draft_id: String) -> Result<bool> {
        self.attachment_drafts.remove(&draft_id).await
    }

    pub async fn read_attachment_draft(&self, draft_id: String) -> Result<Vec<u8>> {
        self.attachment_drafts.read(&draft_id).await
    }

    pub async fn read_thread_attachment(
        &self,
        thread_id: String,
        attachment_id: String,
    ) -> Result<Vec<u8>> {
        self.read_owned_thread(&thread_id).await?;
        self.store
            .read_attachment_bytes(&thread_id, &attachment_id)
            .await
    }
}

fn preflight_sources(
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

fn admission_rejection(error: anyhow::Error) -> anyhow::Error {
    anyhow::Error::new(PureError::ConfigError(format!(
        "attachment admission rejected: {error:#}"
    )))
}

#[derive(Debug, Clone, Copy)]
enum DraftSourceKind {
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

fn validate_model_source(
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
        .request_profile
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
    if model.request_profile.media_mix_policy != MediaMixPolicy::SingleModality {
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

fn model_modality(modality: StudioAttachmentModality) -> ModelModality {
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

async fn read_local_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let file = tokio::fs::File::open(path)
        .await
        .context("failed to open attachment file")?;
    let metadata = file
        .metadata()
        .await
        .context("failed to inspect attachment file")?;
    ensure!(metadata.is_file(), "attachment source is not a file");
    ensure!(metadata.len() <= max_bytes, "attachment file is too large");
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .await
        .context("failed to read attachment file")?;
    ensure!(
        bytes.len() as u64 <= max_bytes,
        "attachment file is too large"
    );
    Ok(bytes)
}

struct LoadedSource {
    bytes: Vec<u8>,
    initial_remote_url: Option<String>,
}

async fn fetch_remote_snapshot(raw_url: &str, max_bytes: u64) -> Result<LoadedSource> {
    tokio::time::timeout(
        REMOTE_FETCH_TOTAL_TIMEOUT,
        fetch_remote_snapshot_with_redirects(raw_url, max_bytes),
    )
    .await
    .context("attachment URL fetch exceeded the total timeout")?
}

async fn fetch_remote_snapshot_with_redirects(
    raw_url: &str,
    max_bytes: u64,
) -> Result<LoadedSource> {
    let mut url = validate_remote_url(Url::parse(raw_url).context("invalid attachment URL")?)?;
    let original = url.as_str().to_string();
    for redirect in 0..=MAX_REDIRECTS {
        let host = url
            .host_str()
            .context("attachment URL has no host")?
            .to_string();
        let port = url
            .port_or_known_default()
            .context("attachment URL has no port")?;
        let addresses = tokio::net::lookup_host((host.as_str(), port))
            .await
            .context("attachment URL host cannot be resolved")?
            .collect::<Vec<_>>();
        ensure!(
            !addresses.is_empty(),
            "attachment URL host has no addresses"
        );
        for address in &addresses {
            ensure_public_ip(address.ip())?;
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .resolve(&host, addresses[0])
            .build()?;
        let response = client
            .get(url.clone())
            .send()
            .await
            .context("attachment URL fetch failed")?;
        if response.status().is_redirection() {
            ensure!(
                redirect < MAX_REDIRECTS,
                "attachment URL has too many redirects"
            );
            let location = response
                .headers()
                .get(LOCATION)
                .context("attachment redirect has no Location")?
                .to_str()
                .context("attachment redirect Location is invalid")?;
            url = validate_remote_url(url.join(location).context("invalid attachment redirect")?)?;
            continue;
        }
        ensure!(
            response.status().is_success(),
            "attachment URL returned an error status"
        );
        if let Some(length) = response.content_length() {
            ensure!(length <= max_bytes, "remote attachment is too large");
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("remote attachment stream failed")?;
            ensure!(
                bytes.len().saturating_add(chunk.len()) as u64 <= max_bytes,
                "remote attachment is too large"
            );
            bytes.extend_from_slice(&chunk);
        }
        return Ok(LoadedSource {
            bytes,
            initial_remote_url: Some(original),
        });
    }
    bail!("attachment URL redirect handling failed")
}

fn validate_remote_url(url: Url) -> Result<Url> {
    ensure!(url.scheme() == "https", "attachment URL must use HTTPS");
    ensure!(
        url.username().is_empty(),
        "attachment URL must not contain credentials"
    );
    ensure!(
        url.password().is_none(),
        "attachment URL must not contain credentials"
    );
    ensure!(
        url.fragment().is_none(),
        "attachment URL must not contain a fragment"
    );
    let host = url.host_str().context("attachment URL has no host")?;
    ensure!(
        !host.eq_ignore_ascii_case("localhost"),
        "attachment URL host is not public"
    );
    if let Ok(ip) = host.parse::<IpAddr>() {
        ensure_public_ip(ip)?;
    }
    Ok(url)
}

fn ensure_public_ip(ip: IpAddr) -> Result<()> {
    let public = match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    };
    ensure!(public, "attachment URL resolved to a non-public address");
    Ok(())
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || ip.is_unspecified()
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 198 && (18..=19).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0))
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(ipv4) = ip.to_ipv4_mapped() {
        return is_public_ipv4(ipv4);
    }
    let segments = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        || (segments[0] == 0x2001 && segments[1] == 0x0002)
        || (segments[0] == 0x0100 && segments[1] == 0 && segments[2] == 0 && segments[3] == 0)
        || (segments[0] & 0xffc0) == 0xfec0)
}

struct NormalizedSource {
    modality: StudioAttachmentModality,
    media_type: String,
    bytes: Vec<u8>,
    width: Option<u32>,
    height: Option<u32>,
}

fn normalize_loaded_source(filename: &str, bytes: Vec<u8>) -> Result<NormalizedSource> {
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

fn sanitize_filename(filename: &str) -> String {
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
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat};

    use super::*;

    fn glm_flash() -> ModelInfo {
        pl_model::model::default_models()
            .into_iter()
            .find(|model| model.slug == "glm-5.3-flash")
            .expect("GLM-5.3-Flash must be present in the canonical catalog")
    }

    fn deepseek_vision() -> ModelInfo {
        pl_model::model::default_models()
            .into_iter()
            .find(|model| model.slug == "deepseek-v4-flash-vision-exp")
            .expect("DeepSeek vision model must be present in the canonical catalog")
    }

    fn png_bytes() -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(2, 2)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    fn gif_bytes() -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(2, 2)
            .write_to(&mut bytes, ImageFormat::Gif)
            .unwrap();
        bytes.into_inner()
    }

    #[test]
    fn private_and_metadata_addresses_are_rejected() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "192.168.1.2",
            "100.64.0.1",
            "::1",
            "fe80::1",
            "fc00::1",
            "2001:db8::1",
            "2001:2::1",
            "::ffff:127.0.0.1",
            "fec0::1",
        ] {
            assert!(ensure_public_ip(ip.parse().unwrap()).is_err(), "{ip}");
        }
    }

    #[test]
    fn url_credentials_fragments_and_non_https_are_rejected() {
        for url in [
            "http://example.com/a.png",
            "https://user:secret@example.com/a.png",
            "https://example.com/a.png#fragment",
            "https://localhost/a.png",
        ] {
            assert!(
                validate_remote_url(Url::parse(url).unwrap()).is_err(),
                "{url}"
            );
        }
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
