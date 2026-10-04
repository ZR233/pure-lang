//! Instance-scoped successful model observations; independent of persisted settings.
use std::{collections::BTreeMap, fs, io::Read, path::PathBuf};

use pl_model::{
    config::{ProviderConfig, ProviderId},
    model::{ModelInfo, ModelPricing},
    provider::discovery::{ModelCatalogQuery, ModelCatalogQueryCache, ModelCatalogQueryError},
};
use pl_protocol::studio::{
    StudioModelCatalogCacheWarning as CacheWarning, StudioModelCatalogError as CatalogError,
    StudioModelCatalogSource as Source, StudioModelCatalogStatus as Status,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use super::{ConfigPaths, StudioConfig};
use crate::Result;

const CACHE_BYTES_LIMIT: usize = 8 * 1024 * 1024;
const NORMALIZATION_REVISION: u32 = 1;

#[derive(Clone)]
pub(super) struct Observation {
    pub identity: Option<String>,
    pub generation: u64,
    pub cancellation: CancellationToken,
    pub cache: Option<SuccessCache>,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SuccessCache {
    schema: u32,
    #[serde(default)]
    normalization_revision: u32,
    identity: String,
    pub success_at: i64,
    pub checked_at: i64,
    pub etag: Option<String>,
    pub models: Vec<ModelInfo>,
}

pub(crate) struct CatalogProbe {
    pub provider: ProviderId,
    pub generation: u64,
    pub query: ModelCatalogQuery,
    pub previous: Option<ModelCatalogQueryCache>,
    pub cancellation: CancellationToken,
}

impl SuccessCache {
    pub fn new(
        provider: &ProviderId,
        identity: &str,
        etag: Option<String>,
        models: Vec<ModelInfo>,
        success_at: i64,
    ) -> Self {
        Self {
            schema: 1,
            normalization_revision: NORMALIZATION_REVISION,
            identity: full_identity(provider, identity),
            success_at,
            checked_at: crate::studio::unix_seconds(),
            etag,
            models,
        }
    }
    pub fn query_etag(&self) -> Option<&str> {
        // A previous normalization cannot be upgraded from a bodyless 304. Keep
        // its snapshot for failures but fetch the body under the current rules.
        (self.normalization_revision == NORMALIZATION_REVISION)
            .then_some(self.etag.as_deref())
            .flatten()
    }
    pub fn not_modified(&self, etag: Option<String>) -> Self {
        Self {
            checked_at: crate::studio::unix_seconds(),
            etag,
            ..self.clone()
        }
    }
    pub fn persist(
        &self,
        paths: &ConfigPaths,
        provider: &ProviderId,
        identity: &str,
    ) -> Result<()> {
        let path = cache_path(paths, provider, identity);
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("model cache path has no parent"))?;
        fs::create_dir_all(parent)?;
        // On Windows canonicalization supplies the verbatim path prefix needed
        // by the atomic writer's native move, including its longer temporary name.
        let path = parent.canonicalize()?.join(
            path.file_name()
                .ok_or_else(|| std::io::Error::other("model cache path has no filename"))?,
        );
        let bytes = serde_json::to_vec(self).map_err(|_| {
            pl_protocol::PureError::ConfigError("could not encode model observation".into())
        })?;
        if bytes.len() > CACHE_BYTES_LIMIT {
            return Err(pl_protocol::PureError::ConfigError(
                "model observation exceeds cache size limit".into(),
            ));
        }
        pl_tool::workspace::write_file_atomically(&path, &bytes)?;
        Ok(())
    }
}

pub(super) fn reconcile(
    paths: &ConfigPaths,
    desired: &StudioConfig,
    old: &BTreeMap<ProviderId, Observation>,
) -> Result<BTreeMap<ProviderId, Observation>> {
    let mut next = BTreeMap::new();
    for (id, provider) in &desired.models.providers {
        let query = if provider.supports_model_discovery() {
            match ModelCatalogQuery::for_provider(provider) {
                Ok(query) => Some(query),
                Err(ModelCatalogQueryError::Definition(error)) => return Err(error.into()),
                Err(_) => None,
            }
        } else {
            None
        };
        let identity = query.as_ref().map(|query| query.identity().to_owned());
        if let Some(previous) = old.get(id)
            && previous.identity == identity
            && previous.status.supported == provider.supports_model_discovery()
        {
            next.insert(id.clone(), previous.clone());
            continue;
        }
        let mut observation = Observation {
            identity: identity.clone(),
            generation: old
                .get(id)
                .map_or(1, |previous| previous.generation.saturating_add(1)),
            cancellation: CancellationToken::new(),
            cache: None,
            status: Status {
                supported: provider.supports_model_discovery(),
                source: Source::Default,
                probing: false,
                last_success_at: None,
                checked_at: None,
                error: None,
                cache_warning: None,
            },
        };
        if let Some(identity) = identity {
            match read_cache(paths, id, &identity, provider) {
                Ok(Some(cache)) => {
                    observation.status.source = Source::Cached;
                    observation.status.last_success_at = Some(cache.success_at);
                    observation.status.checked_at = Some(cache.checked_at);
                    observation.cache = Some(cache);
                }
                Ok(None) => {}
                Err(warning) => observation.status.cache_warning = Some(warning),
            }
        }
        next.insert(id.clone(), observation);
    }
    Ok(next)
}

pub(super) fn effective(
    desired: &StudioConfig,
    observations: &BTreeMap<ProviderId, Observation>,
) -> Result<StudioConfig> {
    let mut config = desired.clone();
    for (id, provider) in &mut config.models.providers {
        provider.clear_model_catalog_overlay();
        if let Some(cache) = observations
            .get(id)
            .and_then(|observation| observation.cache.as_ref())
        {
            provider.set_model_catalog_overlay(cache.models.clone())?;
        }
    }
    Ok(config)
}

fn read_cache(
    paths: &ConfigPaths,
    provider_id: &ProviderId,
    identity: &str,
    provider: &ProviderConfig,
) -> std::result::Result<Option<SuccessCache>, CacheWarning> {
    let file = match fs::File::open(cache_path(paths, provider_id, identity)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(CacheWarning::Read),
    };
    let mut bytes = Vec::new();
    file.take(CACHE_BYTES_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CacheWarning::Read)?;
    if bytes.len() > CACHE_BYTES_LIMIT {
        return Err(CacheWarning::Schema);
    }
    let cache: SuccessCache = serde_json::from_slice(&bytes).map_err(|_| CacheWarning::Schema)?;
    if cache.schema != 1
        || cache.normalization_revision > NORMALIZATION_REVISION
        || cache.success_at <= 0
        || cache.checked_at < cache.success_at
    {
        return Err(CacheWarning::Schema);
    }
    if cache.identity != full_identity(provider_id, identity) {
        return Err(CacheWarning::Identity);
    }
    if cache
        .etag
        .as_ref()
        .is_some_and(|etag| etag.len() > 1024 || etag.bytes().any(|byte| byte < 32 || byte == 127))
    {
        return Err(CacheWarning::Declaration);
    }
    if cache
        .models
        .iter()
        .any(|model| !matches!(model.pricing, ModelPricing::Unknown))
    {
        return Err(CacheWarning::Declaration);
    }
    let mut checked = provider.clone();
    checked
        .set_model_catalog_overlay(cache.models.clone())
        .map_err(|_| CacheWarning::Declaration)?;
    Ok(Some(cache))
}

fn full_identity(provider: &ProviderId, query: &str) -> String {
    let mut digest = Sha256::new();
    for part in [
        b"studio-model-catalog-v1".as_slice(),
        provider.as_str().as_bytes(),
        query.as_bytes(),
    ] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    hex::encode(digest.finalize())
}
fn cache_path(paths: &ConfigPaths, provider: &ProviderId, query: &str) -> PathBuf {
    paths
        .config_dir()
        .join("v2/model-catalogs")
        .join(hex::encode(Sha256::digest(provider.as_str().as_bytes())))
        .join(full_identity(provider, query))
        .join("model.json")
}

pub(crate) fn query_error(error: ModelCatalogQueryError) -> Result<CatalogError> {
    Ok(match error {
        ModelCatalogQueryError::Definition(error) => return Err(error.into()),
        ModelCatalogQueryError::Unsupported => CatalogError::Unsupported,
        ModelCatalogQueryError::Configuration => CatalogError::Configuration,
        ModelCatalogQueryError::Timeout => CatalogError::Timeout,
        ModelCatalogQueryError::Transport { http_status, .. } => {
            CatalogError::Transport { http_status }
        }
        ModelCatalogQueryError::Http { status } => CatalogError::Http { status },
        ModelCatalogQueryError::TooLarge => CatalogError::TooLarge,
        ModelCatalogQueryError::Protocol => CatalogError::Protocol,
        ModelCatalogQueryError::CacheIdentity => CatalogError::CacheIdentity,
        ModelCatalogQueryError::UnexpectedNotModified => CatalogError::UnexpectedNotModified,
    })
}
