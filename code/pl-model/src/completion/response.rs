//! Canonical completion 响应与 trace 上下文。

use serde::{Deserialize, Serialize};

use crate::completion::tool_call::ToolCall;
use crate::completion::usage::TokenUsage;
use pl_protocol::ResponsesContextItem;

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
    pub usage: TokenUsage,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct CompletionTraceContext {
    pub session_id: String,
    pub turn_id: String,
    pub inference_id: String,
    pub plan_mode: bool,
    pub trace_sequence_base: u64,
}
