//! Canonical completion 响应与 trace 上下文。

use serde::{Deserialize, Serialize};

use crate::completion::tool_call::ToolCall;
use pl_protocol::{InferenceAccounting, PureError};
use pl_protocol::{InferenceTiming, ResponsesContextItem};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, thiserror::Error)]
#[error("{source}")]
pub struct CompletionFailure {
    #[source]
    pub source: PureError,
    pub accounting: Box<InferenceAccounting>,
}

impl From<PureError> for CompletionFailure {
    fn from(source: PureError) -> Self {
        Self {
            source,
            accounting: Box::default(),
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
