//! Immutable model-owned provenance and complete normalized responses for history consumers.
use crate::{
    completion::CompletionResponse,
    provider::{ProviderAdapterKind, ProviderWireProtocol},
    runtime::ModelRuntime,
};
use pl_core::model::{ModelError, ModelStepOutput};
use serde::{Deserialize, Serialize};

/// Non-secret binding facts selected before execution; diagnostic purpose is not a permission.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCallBinding {
    pub provider_instance_id: String,
    pub requested_model: String,
    pub adapter: ProviderAdapterKind,
    pub protocol: ProviderWireProtocol,
    pub isolation: String,
    pub purpose: String,
    /// Context capacity frozen with the bound model before execution; `None` stays unknown.
    ///
    /// Receipts saved before this field existed decode as unknown rather than reading a
    /// current model catalog, and an absent value never falls back to output limits or usage.
    #[serde(default)]
    pub context_window: Option<u64>,
}

impl ModelCallBinding {
    pub(super) fn capture(runtime: &ModelRuntime, purpose: &str) -> Self {
        Self {
            provider_instance_id: runtime.provider_instance_id().to_owned(),
            requested_model: runtime.model().slug.clone(),
            adapter: runtime.endpoint().adapter,
            protocol: runtime.model().binding.transport.protocol,
            isolation: crate::runtime::binding_cache_namespace(
                runtime.provider_instance_id(),
                runtime.endpoint(),
            ),
            purpose: purpose.to_owned(),
            context_window: runtime.model().resolved_context_window(),
        }
    }
}

/// Preserves all normalized response fields, including identifiers, pricing, timing and retries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelResponseReceipt {
    pub binding: ModelCallBinding,
    pub response: CompletionResponse,
}

/// Complete accounting already observed by the adapter before an invocation failed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFailureReceipt {
    #[serde(default)]
    pub provider_failure: Option<pl_protocol::ProviderFailure>,
    #[serde(default)]
    pub message: String,
    pub binding: ModelCallBinding,
    pub accounting: crate::completion::InferenceAccounting,
}

/// Reads a producer-owned failure receipt without repricing history using current configuration.
///
/// # Errors
/// Rejects unsupported receipt encodings; original error details remain available to raw history views.
pub fn model_failure_receipt(
    error: &ModelError,
) -> Result<Option<ModelFailureReceipt>, ModelError> {
    let Some(payload) = &error.details else {
        return Ok(None);
    };
    if payload.format() != "pl.model.failure" || payload.version() != 1 {
        return Err(super::failure(
            pl_core::model::ModelFailureKind::UnsupportedContent,
            std::io::Error::other("unsupported model failure receipt"),
        ));
    }
    serde_json::from_str(payload.content())
        .map(Some)
        .map_err(|source| {
            super::failure(pl_core::model::ModelFailureKind::UnsupportedContent, source)
        })
}

pub(super) fn failure_error(
    binding: ModelCallBinding,
    failure: crate::completion::CompletionFailure,
) -> ModelError {
    let usage = super::usage(&failure.accounting.usage);
    let receipt = ModelFailureReceipt {
        provider_failure: failure.source.provider_failure_ref().cloned(),
        message: failure.source.to_string(),
        binding,
        accounting: (*failure.accounting).clone(),
    };
    let details = match serde_json::to_string(&receipt) {
        Ok(content) => pl_core::context::OpaquePayload::new("pl.model.failure", 1, content)
            .expect("static format and version are valid"),
        Err(encoding) => pl_core::context::OpaquePayload::new(
            "pl.model.failure-diagnostic",
            1,
            format!(
                "Failure receipt could not be encoded: {encoding}\nObserved receipt: {receipt:?}"
            ),
        )
        .expect("static format and version are valid"),
    };
    ModelError {
        kind: pl_core::model::ModelFailureKind::Unavailable,
        details: Some(Box::new(details)),
        usage,
        source: Some(Box::new(failure)),
    }
}

/// Frozen normalized request metadata, including provider-hosted declarations absent from core's registry.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRequestReceipt {
    pub binding: ModelCallBinding,
    pub tools: Vec<crate::completion::ToolSpec>,
    pub tool_choice: String,
    pub parallel_tool_calls: bool,
    pub reasoning: Option<crate::completion::ReasoningConfig>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u64>,
}

/// Reads the model-owned record saved at admission, including attempts that failed before any response.
///
/// # Errors
/// Rejects unsupported record formats and malformed payloads without consulting current configuration.
pub fn model_request_receipt(
    payload: &pl_core::context::OpaquePayload,
) -> Result<ModelRequestReceipt, ModelError> {
    if payload.format() != "pl.model.prepared-request" || payload.version() != 1 {
        return Err(super::failure(
            pl_core::model::ModelFailureKind::UnsupportedContent,
            std::io::Error::other("unsupported prepared model request record"),
        ));
    }
    serde_json::from_str(payload.content()).map_err(|source| {
        super::failure(pl_core::model::ModelFailureKind::UnsupportedContent, source)
    })
}

/// Decodes model output provenance without constructing a provider, tool or physical session.
/// Returns None for outputs from another model implementation with no recognized receipt.
///
/// # Errors
/// Rejects unsupported receipt versions, duplicate frames and inconsistent call associations.
pub fn model_response_receipt(
    output: &ModelStepOutput,
) -> Result<Option<ModelResponseReceipt>, ModelError> {
    super::codec::receipt(output)
}

pub(super) fn request_metadata(
    binding: &ModelCallBinding,
    request: &crate::completion::CompletionRequest,
) -> Result<pl_core::context::OpaquePayload, ModelError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct RequestMetadata<'a> {
        binding: &'a ModelCallBinding,
        tools: &'a [crate::completion::ToolSpec],
        tool_choice: &'a str,
        parallel_tool_calls: bool,
        reasoning: &'a Option<crate::completion::ReasoningConfig>,
        temperature: Option<f32>,
        max_tokens: Option<u64>,
    }
    let content = serde_json::to_string(&RequestMetadata {
        binding,
        tools: &request.tools,
        tool_choice: &request.tool_choice,
        parallel_tool_calls: request.parallel_tool_calls,
        reasoning: &request.reasoning,
        temperature: request.temperature,
        max_tokens: request.max_tokens,
    })
    .map_err(|source| {
        super::failure(pl_core::model::ModelFailureKind::UnsupportedContent, source)
    })?;
    pl_core::context::OpaquePayload::new("pl.model.prepared-request", 1, content).map_err(
        |source| super::failure(pl_core::model::ModelFailureKind::UnsupportedContent, source),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn catalog_runtime(slug: &str) -> ModelRuntime {
        let info = crate::model::default_models()
            .into_iter()
            .find(|model| model.slug == slug)
            .expect("bundled catalog model exists");
        ModelRuntime::new(crate::provider::ProviderEndpoint::deepseek(None), info)
            .expect("bundled model binds to its provider endpoint")
    }

    #[test]
    fn binding_freezes_the_resolved_deepseek_catalog_capacity() {
        for slug in ["deepseek-flash", "deepseek-v4-pro"] {
            let runtime = catalog_runtime(slug);
            let binding = ModelCallBinding::capture(&runtime, "turn");
            assert_eq!(binding.requested_model, slug);
            assert_eq!(binding.context_window, Some(1_000_000), "slug {slug}");
        }
    }

    #[test]
    fn binding_falls_back_to_max_context_window_and_keeps_missing_capacity_unknown() {
        let mut fallback = crate::model::ModelInfo::compatible("fallback-model");
        fallback.context_window = None;
        fallback.max_context_window = Some(200_000);
        let runtime =
            ModelRuntime::new(crate::provider::ProviderEndpoint::deepseek(None), fallback)
                .expect("compatible model binds to its provider endpoint");
        assert_eq!(
            ModelCallBinding::capture(&runtime, "turn").context_window,
            Some(200_000)
        );

        let mut unknown = crate::model::ModelInfo::compatible("unknown-model");
        unknown.context_window = None;
        unknown.max_context_window = None;
        let runtime = ModelRuntime::new(crate::provider::ProviderEndpoint::deepseek(None), unknown)
            .expect("compatible model binds to its provider endpoint");
        assert_eq!(
            ModelCallBinding::capture(&runtime, "turn").context_window,
            None
        );
    }

    #[test]
    fn saved_request_receipt_without_a_capacity_field_decodes_as_unknown() {
        let runtime = catalog_runtime("deepseek-flash");
        let binding = ModelCallBinding::capture(&runtime, "turn");
        let payload = request_metadata(
            &binding,
            &crate::completion::CompletionRequest::builder().build(),
        )
        .expect("request metadata encodes");
        let mut saved: serde_json::Value =
            serde_json::from_str(payload.content()).expect("request metadata is JSON");
        saved["binding"]
            .as_object_mut()
            .expect("binding is a JSON object")
            .remove("contextWindow");
        let legacy =
            pl_core::context::OpaquePayload::new("pl.model.prepared-request", 1, saved.to_string())
                .expect("static format and version are valid");
        let decoded = model_request_receipt(&legacy).expect("legacy record decodes");
        assert_eq!(decoded.binding.requested_model, "deepseek-flash");
        assert_eq!(decoded.binding.context_window, None);
    }

    #[test]
    fn failure_history_retains_frozen_accounting_and_binding_without_repricing() {
        let accounting = crate::completion::InferenceAccounting {
            usage: pl_protocol::UsageReport {
                input_tokens: Some(123),
                cache_read_tokens: Some(100),
                output_tokens: Some(7),
                ..Default::default()
            },
            price_snapshot: Some(pl_protocol::ModelPricingSnapshot {
                currency: Some("USD".into()),
                input_per_mtok: Some(2.0),
                output_per_mtok: Some(5.0),
                cache_read_per_mtok: Some(0.2),
                cache_write_per_mtok: None,
            }),
            request_started_at: Some(1000),
            ..Default::default()
        };
        let original = accounting.clone();
        let binding = ModelCallBinding {
            provider_instance_id: "original-provider".into(),
            requested_model: "original-model".into(),
            adapter: ProviderAdapterKind::DeepSeek,
            protocol: ProviderWireProtocol::ChatCompletions,
            isolation: "opaque-boundary".into(),
            purpose: "review".into(),
            context_window: Some(1_000_000),
        };
        let error = failure_error(
            binding,
            crate::completion::CompletionFailure {
                source: pl_protocol::PureError::LlmError("stream interrupted".into()),
                accounting: Box::new(accounting),
            },
        );
        let encoded = serde_json::to_string(&error).unwrap();
        let restored: ModelError = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored.usage.input_tokens, Some(123));
        let receipt = model_failure_receipt(&restored).unwrap().unwrap();
        assert_eq!(receipt.accounting, original);
        assert_eq!(receipt.binding.purpose, "review");
        assert_eq!(receipt.binding.provider_instance_id, "original-provider");
        assert_eq!(receipt.binding.context_window, Some(1_000_000));
    }
}
