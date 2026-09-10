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

/// Extracts the single native checkpoint returned by a remote v2 compaction adapter.
///
/// # Errors
/// Rejects missing or ambiguous checkpoints instead of silently continuing with empty context.
pub fn remote_compaction_checkpoint(
    output: Vec<pl_protocol::ModelContextItem>,
) -> Result<pl_protocol::ModelContextItem, pl_protocol::PureError> {
    let mut checkpoints = output
        .into_iter()
        .filter(pl_protocol::ModelContextItem::is_compaction);
    let first = checkpoints.next().ok_or_else(|| {
        pl_protocol::PureError::LlmError("remote compaction returned no checkpoint".into())
    })?;
    if checkpoints.next().is_some() {
        return Err(pl_protocol::PureError::LlmError(
            "remote compaction returned multiple checkpoints".into(),
        ));
    }
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_checkpoint_keeps_opaque_content_and_rejects_ambiguous_output() {
        let checkpoint = ModelContextItem::Compaction {
            encrypted_content: "  opaque-provider-checkpoint\n".into(),
        };
        assert_eq!(
            remote_compaction_checkpoint(vec![checkpoint.clone()]).unwrap(),
            checkpoint
        );
        assert!(remote_compaction_checkpoint(Vec::new()).is_err());
        assert!(remote_compaction_checkpoint(vec![checkpoint.clone(), checkpoint]).is_err());
    }
}
