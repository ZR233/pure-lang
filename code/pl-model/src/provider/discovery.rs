//! Endpoint-owned model discovery, independent of inference/model selection.
mod decode;

pub(crate) use decode::is_unknown_model_fallback;

use super::{ProviderAdapterKind, ProviderEndpoint};
use crate::{
    config::{ProviderConfig, ProviderModelCatalogConfig},
    model::ModelInfo,
    runtime::transport,
};
use futures::StreamExt;
use reqwest::{
    Url,
    header::{ETAG, IF_NONE_MATCH},
};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, time::Duration};
use thiserror::Error;

pub use pl_protocol::ProviderFailureKind;

const BODY_LIMIT: usize = 4 * 1024 * 1024;

/// Safe diagnostic categories; never includes URLs, headers, response text or credentials.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelCatalogQueryError {
    #[error(transparent)]
    Definition(#[from] crate::model::ModelDefinitionError),
    #[error("model discovery is not supported by this adapter/catalog source")]
    Unsupported,
    #[error("model discovery endpoint or headers are invalid")]
    Configuration,
    #[error("model discovery timed out")]
    Timeout,
    #[error("model discovery transport failed ({kind:?})")]
    Transport {
        kind: ProviderFailureKind,
        http_status: Option<u16>,
    },
    #[error("model discovery returned HTTP {status}")]
    Http { status: u16 },
    #[error("model discovery response exceeded 4 MiB")]
    TooLarge,
    #[error("model discovery returned an invalid declaration")]
    Protocol,
    #[error("model discovery cache does not belong to this query")]
    CacheIdentity,
    #[error("model discovery returned 304 without a same-identity successful cache")]
    UnexpectedNotModified,
}

/// Only a successful, same-query observation may be used as a conditional request or metadata source.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelCatalogQueryCache {
    pub identity: String,
    pub etag: Option<String>,
    /// Cached/API pricing must be Unknown. Local current prices are joined by ProviderConfig.
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelCatalogQueryResult {
    Updated {
        identity: String,
        etag: Option<String>,
        models: Vec<ModelInfo>,
    },
    NotModified {
        identity: String,
        etag: Option<String>,
    },
}

/// Immutable request identity + credentials. Debug intentionally exposes only safe fingerprint.
pub struct ModelCatalogQuery {
    endpoint: ProviderEndpoint,
    url: Url,
    headers: reqwest::header::HeaderMap,
    identity: String,
    defaults: Vec<ModelInfo>,
}

impl std::fmt::Debug for ModelCatalogQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelCatalogQuery")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

impl ModelCatalogQuery {
    /// Prepare a bundled discovery request from the current endpoint, without choosing a model.
    /// Explicit catalogs and unsupported adapters never automatically acquire discovery.
    /// # Errors
    /// Unsupported source/adapter, malformed URL/header or bundled packaging failure.
    pub fn for_provider(provider: &ProviderConfig) -> Result<Self, ModelCatalogQueryError> {
        let ProviderModelCatalogConfig::Bundled { catalog, .. } = &provider.catalog else {
            return Err(ModelCatalogQueryError::Unsupported);
        };
        if !provider.supports_model_discovery() {
            return Err(ModelCatalogQueryError::Unsupported);
        }
        let defaults = crate::model::bundled_model_definition(catalog.as_str())?.models;
        let endpoint = provider
            .to_endpoint()
            .map_err(|_| ModelCatalogQueryError::Configuration)?;
        let mut url =
            Url::parse(&endpoint.base_url).map_err(|_| ModelCatalogQueryError::Configuration)?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(ModelCatalogQueryError::Configuration);
        }
        url.path_segments_mut()
            .map_err(|_| ModelCatalogQueryError::Configuration)?
            .pop_if_empty()
            .push("models");
        let headers = transport::headers(
            endpoint.bearer_token.as_deref(),
            endpoint.http_headers.as_ref(),
            &HashMap::new(),
        )
        .map_err(|_| ModelCatalogQueryError::Configuration)?;
        let mut hash = Sha256::new();
        // Length-prefix every component; canonical HeaderMap casing and order are independent of HashMap ordering.
        let mut part = |bytes: &[u8]| {
            hash.update((bytes.len() as u64).to_be_bytes());
            hash.update(bytes);
        };
        part(b"model-catalog-query-v1");
        part(catalog.as_str().as_bytes());
        part(match endpoint.adapter {
            ProviderAdapterKind::OpenAi => b"openai",
            _ => b"deepseek",
        });
        part(url.as_str().as_bytes());
        let mut sorted = headers.iter().collect::<Vec<_>>();
        sorted.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
        for (name, value) in sorted {
            part(name.as_str().as_bytes());
            part(value.as_bytes());
        }
        let identity = hex::encode(hash.finalize());
        Ok(Self {
            endpoint,
            url,
            headers,
            identity,
            defaults,
        })
    }

    /// Stable SHA-256 of URL + explicit adapter/source + effective query headers/credential.
    /// Provider instance identity is an additional outer namespace owned by the host.
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Execute exactly one GET, no redirects/retries, bounded to 10 seconds including body reads.
    /// Dropping this future releases the request; it never spawns a detached task or mutates a cache.
    /// # Errors
    /// Typed/redacted transport, HTTP, envelope, declaration or conditional-cache failure.
    pub async fn execute(
        &self,
        previous: Option<&ModelCatalogQueryCache>,
    ) -> Result<ModelCatalogQueryResult, ModelCatalogQueryError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        if let Some(cache) = previous {
            if cache.identity != self.identity {
                return Err(ModelCatalogQueryError::CacheIdentity);
            }
            crate::model::validate_inventory(&cache.models)
                .map_err(|_| ModelCatalogQueryError::Protocol)?;
            if cache.models.iter().any(|model| {
                !matches!(model.pricing, crate::model::ModelPricing::Unknown)
                    || !valid_discovery_transport(&model.binding.transport, self.endpoint.adapter)
            }) {
                return Err(ModelCatalogQueryError::Protocol);
            }
        }
        let result = tokio::time::timeout_at(deadline, self.execute_inner(previous))
            .await
            .map_err(|_| ModelCatalogQueryError::Timeout)?;
        // Synchronous bounded decoding cannot yield to Tokio's timer. Never publish
        // a late success if decoding/normalization crossed the operation deadline.
        if tokio::time::Instant::now() >= deadline {
            return Err(ModelCatalogQueryError::Timeout);
        }
        result
    }

    async fn execute_inner(
        &self,
        previous: Option<&ModelCatalogQueryCache>,
    ) -> Result<ModelCatalogQueryResult, ModelCatalogQueryError> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .pool_max_idle_per_host(0)
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(query_transport_error)?;
        let mut request = client.get(self.url.clone()).headers(self.headers.clone());
        if let Some(etag) = previous.and_then(|cache| cache.etag.as_deref()) {
            let mut value = reqwest::header::HeaderValue::from_str(etag)
                .map_err(|_| ModelCatalogQueryError::Configuration)?;
            value.set_sensitive(true);
            request = request.header(IF_NONE_MATCH, value);
        }
        let response = request.send().await.map_err(query_transport_error)?;
        let status = response.status().as_u16();
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.len() <= 1024)
            .map(str::to_owned);
        if status == 304 {
            if previous.is_none() {
                return Err(ModelCatalogQueryError::UnexpectedNotModified);
            }
            return Ok(ModelCatalogQueryResult::NotModified {
                identity: self.identity.clone(),
                etag: etag.or_else(|| previous.and_then(|p| p.etag.clone())),
            });
        }
        if !(200..300).contains(&status) {
            return Err(ModelCatalogQueryError::Http { status });
        }
        if response
            .content_length()
            .is_some_and(|len| len > BODY_LIMIT as u64)
        {
            return Err(ModelCatalogQueryError::TooLarge);
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(query_transport_error)?;
            if chunk.len() > BODY_LIMIT - bytes.len() {
                return Err(ModelCatalogQueryError::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        let models = decode::models(
            &bytes,
            self.endpoint.adapter,
            previous.map(|p| p.models.as_slice()),
            &self.defaults,
        )?;
        Ok(ModelCatalogQueryResult::Updated {
            identity: self.identity.clone(),
            etag,
            models,
        })
    }
}

pub(crate) fn discovery_transport(
    adapter: ProviderAdapterKind,
) -> crate::model::ModelTransportProfile {
    if adapter == ProviderAdapterKind::OpenAi {
        crate::model::ModelTransportProfile::responses_websocket()
    } else {
        crate::model::ModelTransportProfile::responses_http()
    }
}

pub(crate) fn valid_discovery_transport(
    profile: &crate::model::ModelTransportProfile,
    adapter: ProviderAdapterKind,
) -> bool {
    let expected = discovery_transport(adapter);
    profile.protocol == expected.protocol
        && profile.default_connection_mode == expected.default_connection_mode
        && profile
            .supported_connection_modes
            .contains(&profile.default_connection_mode)
        && profile
            .supported_connection_modes
            .iter()
            .enumerate()
            .all(|(index, mode)| {
                expected.supported_connection_modes.contains(mode)
                    && !profile.supported_connection_modes[..index].contains(mode)
            })
}

fn query_transport_error(error: reqwest::Error) -> ModelCatalogQueryError {
    if error.is_timeout() {
        return ModelCatalogQueryError::Timeout;
    }
    // Reuse the inference transport's classification/redaction, but never expose its diagnostic prose.
    match transport::reqwest_error_to_pure(error) {
        pl_protocol::PureError::Provider(failure) => ModelCatalogQueryError::Transport {
            kind: failure.kind,
            http_status: failure.http_status,
        },
        _ => ModelCatalogQueryError::Configuration,
    }
}
