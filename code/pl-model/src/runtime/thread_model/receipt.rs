//! Immutable model-owned provenance and complete normalized responses for history consumers.
use crate::{
    completion::CompletionResponse,
    provider::{ProviderAdapterKind, ProviderWireProtocol},
    runtime::ModelRuntime,
};
use pl_core::model::{ModelError, ModelProgress, ModelStepOutput};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_observation: Option<crate::completion::InferenceModelObservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_progress: Option<ModelProgress>,
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
    partial_progress: Option<ModelProgress>,
) -> ModelError {
    // The invocation's own cancellation fact, recorded at the observation point, decides the
    // class. Provider failures, timeouts and transport errors keep the previous classification.
    let kind = if failure.is_cancelled() {
        pl_core::model::ModelFailureKind::Cancelled
    } else {
        pl_core::model::ModelFailureKind::Unavailable
    };
    let usage = super::usage(&failure.accounting.usage);
    let model_observation = failure.model_observation().cloned();
    let receipt = ModelFailureReceipt {
        provider_failure: failure.source.provider_failure_ref().cloned(),
        message: failure.source.to_string(),
        binding,
        accounting: (*failure.accounting).clone(),
        model_observation,
        partial_progress,
    };
    let details = failure_details(&receipt);
    ModelError {
        kind,
        details: Some(Box::new(details)),
        usage,
        source: Some(Box::new(failure)),
    }
}

pub(super) fn postprocess_failure_error(
    binding: ModelCallBinding,
    accounting: crate::completion::InferenceAccounting,
    model_observation: Option<crate::completion::InferenceModelObservation>,
    partial_progress: Option<ModelProgress>,
    error: ModelError,
) -> ModelError {
    let receipt = ModelFailureReceipt {
        provider_failure: None,
        message: error.to_string(),
        binding,
        accounting,
        model_observation,
        partial_progress,
    };
    let details = failure_details(&receipt);
    ModelError {
        kind: error.kind,
        details: Some(Box::new(details)),
        usage: error.usage.clone(),
        source: Some(Box::new(error)),
    }
}

fn failure_details(receipt: &ModelFailureReceipt) -> pl_core::context::OpaquePayload {
    match serde_json::to_string(receipt) {
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
