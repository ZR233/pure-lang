//! Canonical completion 响应与 trace 上下文。

use pl_protocol::trace::TraceTextChannel;
use serde::{Deserialize, Serialize};

use crate::completion::tool_call::ToolCall;
use pl_protocol::{InferenceAccounting, InferenceModelObservation, PureError};
use pl_protocol::{InferenceTiming, ResponsesContextItem};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub responses_context_items: Vec<ResponsesContextItem>,
    /// Ordered provider output items with their original item and content-part identities.
    /// Absent for protocols that do not report distinct output items.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presentation_items: Vec<CompletionPresentationItem>,
    #[serde(default)]
    pub orchestration: pl_protocol::InferenceOrchestrationMetrics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<InferenceTiming>,
    pub accounting: InferenceAccounting,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_observation: Option<InferenceModelObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionPresentationItem {
    pub provider_item_id: String,
    pub output_index: Option<u32>,
    pub kind: CompletionPresentationItemKind,
    pub parts: Vec<CompletionPresentationPart>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "channel", rename_all = "camelCase")]
pub enum CompletionPresentationItemKind {
    Text(TraceTextChannel),
    Reasoning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionPresentationPart {
    pub content_index: u32,
    pub provider_part_id: Option<String>,
    pub kind: CompletionPresentationPartKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompletionPresentationPartKind {
    OutputText,
    ReasoningText,
    SummaryText,
}

#[derive(Debug, Clone)]
pub struct CompletionTraceContext {
    pub session_id: String,
    pub turn_id: String,
    pub inference_id: String,
}

/// Invocation failure retaining all service-reported usage received before failure.
///
/// `cancelled` carries the implementation's own observation that this invocation was
/// targeted by a runtime interrupt and unwound through its cancellation branch. It is
/// set only at the observation point that produced the failure; it is never inferred
/// from error text, nor from another signal that merely coincides with the failure.
#[derive(Debug, thiserror::Error)]
#[error("{source}")]
pub struct CompletionFailure {
    #[source]
    pub source: Box<PureError>,
    pub accounting: Box<InferenceAccounting>,
    pub model_observation: Option<Box<InferenceModelObservation>>,
    /// All provider output items received before this invocation failed.
    pub presentation_items: Vec<CompletionPresentationItem>,
    pub(crate) cancelled: bool,
}

impl CompletionFailure {
    /// Builds a failure that is explicitly **not** the invocation's own cancellation.
    ///
    /// Use this to preserve the observed accounting when folding a secondary cause (for
    /// example a session-close error) into a failure that was already produced elsewhere.
    /// The cancellation fact cannot be forged here: `new(..).is_cancelled()` is always
    /// `false`, and only pl-model's own cancellation branch may produce a cancelled failure.
    pub fn new(source: PureError, accounting: Box<InferenceAccounting>) -> Self {
        Self {
            source: Box::new(source),
            accounting,
            model_observation: None,
            presentation_items: Vec::new(),
            cancelled: false,
        }
    }

    /// Whether this invocation itself terminated because it observed a targeting cancellation.
    ///
    /// Provider failures, timeouts and transport errors keep this `false`, even when a
    /// cancellation happens to arrive around the same time.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    pub fn model_observation(&self) -> Option<&InferenceModelObservation> {
        self.model_observation.as_deref()
    }

    pub(crate) fn with_model_observation(
        mut self,
        model_observation: InferenceModelObservation,
    ) -> Self {
        self.model_observation = Some(Box::new(model_observation));
        self
    }

    pub(crate) fn with_optional_model_observation(
        mut self,
        model_observation: Option<InferenceModelObservation>,
    ) -> Self {
        self.model_observation = model_observation.map(Box::new);
        self
    }

    /// Builds the failure produced by the invocation's own cancellation branch.
    ///
    /// Only the code that drove the call and matched its cancellation branch may use this;
    /// a caller outside pl-model cannot fabricate the cancellation fact.
    pub(crate) fn cancelled(source: PureError, accounting: Box<InferenceAccounting>) -> Self {
        Self {
            source: Box::new(source),
            accounting,
            model_observation: None,
            presentation_items: Vec::new(),
            cancelled: true,
        }
    }
}

impl From<PureError> for CompletionFailure {
    fn from(source: PureError) -> Self {
        Self {
            source: Box::new(source),
            accounting: Box::default(),
            model_observation: None,
            presentation_items: Vec::new(),
            cancelled: false,
        }
    }
}

impl std::ops::Deref for CompletionFailure {
    type Target = PureError;
    fn deref(&self) -> &PureError {
        self.source.as_ref()
    }
}

impl From<CompletionFailure> for PureError {
    fn from(failure: CompletionFailure) -> Self {
        *failure.source
    }
}
