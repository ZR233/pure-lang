//! Immutable model-owned provenance and complete normalized responses for history consumers.
use crate::completion::{CompletionPresentationItemKind, CompletionPresentationPartKind};
use crate::{
    completion::CompletionResponse,
    provider::{ProviderAdapterKind, ProviderWireProtocol},
    runtime::ModelRuntime,
};
use pl_core::context::{ContextContent, OpaquePayload};
use pl_core::model::{
    AggregateChannel, ModelError, ModelFailureKind, ModelProgress, ModelStepOutput,
    ObservedItemKind, ObservedPart, ObservedPartIdentity, ObservedPartKind, ProviderPartIdentity,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Version of the persisted model failure receipt owned by this crate.
///
/// Version 2 stores the live observation as typed parts whose text is materialized once, at this
/// encoding boundary. Version 1 kept the previous full-text preview shape; it is upgraded to the
/// current structure only at the persistence decode boundary in [`model_failure_receipt`], so a
/// receipt written by an earlier incarnation of this same data version still recovers every byte of
/// partial text it had already received. The running producer and live path handle version 2 only.
pub const FAILURE_RECEIPT_VERSION: u32 = 2;

/// Failure receipt encoding written before the live observation became typed parts.
const LEGACY_FAILURE_RECEIPT_VERSION: u32 = 1;

/// Format of the version 1 preview payload holding one encoded presentation item.
const PRESENTATION_PREVIEW_FORMAT: &str = "pl.model.presentation-item";

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
    /// Full provider output retained independently of the bounded live preview.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presentation_items: Vec<crate::completion::CompletionPresentationItem>,
    /// Live observation observed before the failure, with its text materialized for this encoding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_progress: Option<ModelProgress>,
}

/// Reads a producer-owned failure receipt without repricing history using current configuration.
///
/// The current structure is read directly. A receipt written in the version 1 shape is upgraded here,
/// at the only persistence decode boundary, so the partial text already received is preserved rather
/// than dropped as unsupported content. The live path never sees the legacy shape.
///
/// # Errors
/// Rejects unsupported receipt encodings; original error details remain available to raw history views.
pub fn model_failure_receipt(
    error: &ModelError,
) -> Result<Option<ModelFailureReceipt>, ModelError> {
    let Some(payload) = &error.details else {
        return Ok(None);
    };
    if payload.format() != "pl.model.failure" {
        return Err(super::failure(
            ModelFailureKind::UnsupportedContent,
            std::io::Error::other("unsupported model failure receipt"),
        ));
    }
    match payload.version() {
        FAILURE_RECEIPT_VERSION => serde_json::from_str(payload.content())
            .map(Some)
            .map_err(|source| super::failure(ModelFailureKind::UnsupportedContent, source)),
        LEGACY_FAILURE_RECEIPT_VERSION => {
            let legacy: FailureReceiptV1 = serde_json::from_str(payload.content())
                .map_err(|source| super::failure(ModelFailureKind::UnsupportedContent, source))?;
            Ok(Some(legacy.migrate()?))
        }
        _ => Err(super::failure(
            ModelFailureKind::UnsupportedContent,
            std::io::Error::other("unsupported model failure receipt"),
        )),
    }
}

/// Version 1 failure receipt, decoded only to upgrade it into the current structure.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FailureReceiptV1 {
    #[serde(default)]
    provider_failure: Option<pl_protocol::ProviderFailure>,
    #[serde(default)]
    message: String,
    binding: ModelCallBinding,
    accounting: crate::completion::InferenceAccounting,
    #[serde(default)]
    model_observation: Option<crate::completion::InferenceModelObservation>,
    #[serde(default)]
    presentation_items: Vec<crate::completion::CompletionPresentationItem>,
    #[serde(default)]
    partial_progress: Option<FailureProgressV1>,
}

/// Version 1 live preview: one channel-aggregate text/reasoning body plus encoded item previews.
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FailureProgressV1 {
    #[serde(default)]
    content: Vec<ContextContent>,
    #[serde(default)]
    reasoning: Option<OpaquePayload>,
    #[serde(default)]
    presentation: Vec<OpaquePayload>,
}

impl FailureReceiptV1 {
    fn migrate(self) -> Result<ModelFailureReceipt, ModelError> {
        let partial_progress = match self.partial_progress {
            Some(progress) => Some(progress.migrate()?),
            None => None,
        };
        Ok(ModelFailureReceipt {
            provider_failure: self.provider_failure,
            message: self.message,
            binding: self.binding,
            accounting: self.accounting,
            model_observation: self.model_observation,
            presentation_items: self.presentation_items,
            partial_progress,
        })
    }
}

impl FailureProgressV1 {
    /// Rebuilds the observed text as current typed parts without consulting current configuration.
    fn migrate(self) -> Result<ModelProgress, ModelError> {
        let mut parts = Vec::new();
        // Version 1 replaced the channel aggregate with item previews as soon as an item closed, so
        // the two shapes never coexist and item previews win when present.
        for payload in &self.presentation {
            if payload.format() != PRESENTATION_PREVIEW_FORMAT || payload.version() != 1 {
                continue;
            }
            let item: crate::completion::CompletionPresentationItem =
                serde_json::from_str(payload.content()).map_err(|source| {
                    super::failure(ModelFailureKind::UnsupportedContent, source)
                })?;
            let item_kind = presentation_item_kind(item.kind);
            let item_id: Arc<str> = Arc::from(item.provider_item_id.as_str());
            for part in &item.parts {
                let identity = ObservedPartIdentity::Provider(ProviderPartIdentity {
                    item_id: item_id.clone(),
                    output_index: item.output_index,
                    item_kind,
                    part: presentation_part_kind(part.kind),
                    content_index: part.content_index,
                });
                parts.push(ObservedPart::new(identity).authorized(&part.text));
            }
        }
        if parts.is_empty() {
            let text = self
                .content
                .iter()
                .filter_map(|content| match content {
                    ContextContent::Text { text } => Some(text.as_ref()),
                    ContextContent::Resource { .. } | ContextContent::Opaque { .. } => None,
                })
                .collect::<String>();
            if !text.is_empty() {
                parts.push(
                    ObservedPart::new(ObservedPartIdentity::Aggregate {
                        channel: AggregateChannel::Text,
                    })
                    .authorized(&text),
                );
            }
            if let Some(reasoning) = self
                .reasoning
                .as_ref()
                .filter(|payload| payload.format() == "text/plain")
                .map(OpaquePayload::content)
                .filter(|text| !text.is_empty())
            {
                parts.push(
                    ObservedPart::new(ObservedPartIdentity::Aggregate {
                        channel: AggregateChannel::Reasoning,
                    })
                    .authorized(reasoning),
                );
            }
        }
        Ok(ModelProgress::new(1, parts))
    }
}

fn presentation_item_kind(kind: CompletionPresentationItemKind) -> ObservedItemKind {
    match kind {
        CompletionPresentationItemKind::Text(channel) => {
            ObservedItemKind::Text(text_channel(channel))
        }
        CompletionPresentationItemKind::Reasoning => ObservedItemKind::Reasoning,
    }
}

fn presentation_part_kind(kind: CompletionPresentationPartKind) -> ObservedPartKind {
    match kind {
        CompletionPresentationPartKind::OutputText => ObservedPartKind::OutputText,
        CompletionPresentationPartKind::ReasoningText => ObservedPartKind::ReasoningText,
        CompletionPresentationPartKind::SummaryText => ObservedPartKind::SummaryText,
    }
}

fn text_channel(channel: pl_protocol::trace::TraceTextChannel) -> pl_core::model::ModelTextChannel {
    match channel {
        pl_protocol::trace::TraceTextChannel::User => pl_core::model::ModelTextChannel::User,
        pl_protocol::trace::TraceTextChannel::Commentary => {
            pl_core::model::ModelTextChannel::Commentary
        }
        pl_protocol::trace::TraceTextChannel::Final => pl_core::model::ModelTextChannel::Final,
    }
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
    let mut failure = failure;
    let receipt = ModelFailureReceipt {
        provider_failure: failure.source.provider_failure_ref().cloned(),
        message: failure.source.to_string(),
        binding,
        accounting: (*failure.accounting).clone(),
        model_observation,
        presentation_items: std::mem::take(&mut failure.presentation_items),
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
        presentation_items: Vec::new(),
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
        Ok(content) => pl_core::context::OpaquePayload::new(
            "pl.model.failure",
            FAILURE_RECEIPT_VERSION,
            content,
        )
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
