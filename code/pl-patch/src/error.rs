//! patch 解析与应用失败的统一错误类型和重试指引文案。

pub type PatchResult<T> = Result<T, PatchError>;

pub(crate) const PATCH_RETRY_GUIDANCE: &str = "Recovery: read the target file again, then retry with a smaller Codex-style patch built from the current file contents. Do not repeat the same failed patch.";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct PatchError {
    message: String,
}

impl PatchError {
    pub fn new(message: impl std::fmt::Display) -> Self {
        Self {
            message: message.to_string(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn into_message(self) -> String {
        self.message
    }
}
