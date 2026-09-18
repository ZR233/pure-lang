//! Canonical completion 响应与 trace 上下文。

use serde::{Deserialize, Serialize};

use crate::completion::tool_call::ToolCall;
use pl_protocol::{InferenceAccounting, PureError};
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
    #[serde(default)]
    pub orchestration: pl_protocol::InferenceOrchestrationMetrics,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<InferenceTiming>,
    pub accounting: InferenceAccounting,
    pub model: String,
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
    pub source: PureError,
    pub accounting: Box<InferenceAccounting>,
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
            source,
            accounting,
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

    /// Builds the failure produced by the invocation's own cancellation branch.
    ///
    /// Only the code that drove the call and matched its cancellation branch may use this;
    /// a caller outside pl-model cannot fabricate the cancellation fact.
    pub(crate) fn cancelled(source: PureError, accounting: Box<InferenceAccounting>) -> Self {
        Self {
            source,
            accounting,
            cancelled: true,
        }
    }
}

impl From<PureError> for CompletionFailure {
    fn from(source: PureError) -> Self {
        Self {
            source,
            accounting: Box::default(),
            cancelled: false,
        }
    }
}

impl std::ops::Deref for CompletionFailure {
    type Target = PureError;
    fn deref(&self) -> &PureError {
        &self.source
    }
}

impl From<CompletionFailure> for PureError {
    fn from(failure: CompletionFailure) -> Self {
        failure.source
    }
}
