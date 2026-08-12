use crate::{PureError, working_set};

/// 可安全复用的工具失败类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolFailureClassV1 {
    DeterministicLocalRead,
}

/// 同一 mutation epoch 内可复用的有界失败事实。
#[derive(Debug, Clone)]
pub(crate) struct ToolFailureEnvelopeV1 {
    pub(super) class: ToolFailureClassV1,
    pub(super) tool_name: String,
    pub(super) original_call_id: String,
    error_hash: String,
    summary: String,
}

pub(super) fn deterministic_failure(
    tool_name: &str,
    original_call_id: String,
    error: &PureError,
) -> Option<ToolFailureEnvelopeV1> {
    if !matches!(tool_name, "read_file" | "list_files" | "stat_path") {
        return None;
    }
    if !matches!(
        error,
        PureError::ToolExecutionFailed { .. }
            | PureError::Io(_)
            | PureError::ConfigError(_)
            | PureError::SandboxError(_)
    ) {
        return None;
    }
    let full = error.to_string();
    let summary = full.chars().take(512).collect::<String>();
    Some(ToolFailureEnvelopeV1 {
        class: ToolFailureClassV1::DeterministicLocalRead,
        tool_name: tool_name.to_string(),
        original_call_id,
        error_hash: working_set::canonical_content_hash(full.as_bytes()),
        summary,
    })
}

impl ToolFailureEnvelopeV1 {
    pub(super) fn duplicate_error(&self) -> PureError {
        PureError::ToolExecutionFailed {
            tool: self.tool_name.clone(),
            error: serde_json::json!({
                "duplicateFailure": true,
                "class": "deterministicLocalRead",
                "reusedFromCallId": self.original_call_id,
                "errorHash": self.error_hash,
                "summary": self.summary,
            })
            .to_string(),
        }
    }
}
