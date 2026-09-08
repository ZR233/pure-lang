//! Immutable complete output retained separately from task and event previews.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{SessionTaskError, ToolTaskResult, ToolTaskSnapshot};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskResultIdentity {
    pub(crate) entry_id: String,
    pub(crate) content_hash: String,
    pub(crate) encoded_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TaskResultRecord {
    pub(crate) identity: TaskResultIdentity,
    pub(crate) content: Option<Arc<ToolTaskResult>>,
}

impl TaskResultRecord {
    pub(crate) fn new(task_id: &str, result: ToolTaskResult) -> Result<Self, serde_json::Error> {
        let payload = serde_json::to_value(&result)?;
        Ok(Self {
            identity: TaskResultIdentity {
                entry_id: format!("pl.toolResult.{task_id}"),
                content_hash: crate::canonical_json_hash(&payload),
                encoded_bytes: serde_json::to_vec(&payload)?.len() as u64,
            },
            content: Some(Arc::new(result)),
        })
    }

    pub(crate) fn validate(&self, task: &ToolTaskSnapshot) -> Result<(), SessionTaskError> {
        if !task.status.is_terminal()
            || self.identity.entry_id != format!("pl.toolResult.{}", task.receipt.task_id)
            || task.result_reference.is_none()
            || self.identity.encoded_bytes == 0
            || !self
                .identity
                .content_hash
                .strip_prefix("sha256:")
                .is_some_and(|hash| {
                    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
        {
            return Err(SessionTaskError::InvalidSnapshot {
                reason: "invalid complete result reference",
            });
        }
        if let Some(content) = &self.content {
            self.validate_content(content)?;
        }
        Ok(())
    }

    pub(crate) fn validate_content(
        &self,
        content: &ToolTaskResult,
    ) -> Result<(), SessionTaskError> {
        let payload = serde_json::to_value(content)?;
        if crate::canonical_json_hash(&payload) != self.identity.content_hash
            || serde_json::to_vec(&payload)?.len() as u64 != self.identity.encoded_bytes
        {
            return Err(SessionTaskError::InvalidSnapshot {
                reason: "complete result content differs from its identity",
            });
        }
        Ok(())
    }
}
