//! Provider 远程上下文压缩请求与响应。

use crate::completion::usage::ReasoningConfig;
use pl_protocol::InferenceAccounting;
use pl_protocol::ModelContextItem;
use pl_protocol::ToolSpec;
use serde::{Deserialize, Serialize};

/// OpenAI provider 的上下文压缩协议选择。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiCompactionMode {
    #[default]
    RemoteV2,
    Local,
}

impl OpenAiCompactionMode {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// 统一的 provider 压缩请求。
#[derive(Debug, Clone)]
pub struct ModelCompactionRequest {
    pub mode: OpenAiCompactionMode,
    pub instructions: String,
    pub input: Vec<ModelContextItem>,
    pub tools: Vec<ToolSpec>,
    pub parallel_tool_calls: bool,
    pub reasoning: Option<ReasoningConfig>,
    pub prompt_cache_key: Option<String>,
}

/// Provider 完成远程压缩后返回的替换历史。
#[derive(Debug, Clone)]
pub struct ModelCompactionResponse {
    pub input: Vec<ModelContextItem>,
    pub accounting: InferenceAccounting,
}
