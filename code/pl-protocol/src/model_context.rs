use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::Message;

/// 不含 revision、时间戳等易变元数据的模型可见工作上下文段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelContextSectionSnapshot {
    pub id: ContextSectionId,
    pub title: String,
    pub content: String,
    pub content_hash: String,
}

/// 一次采样时模型应看到的完整工作上下文。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelContextSnapshot {
    #[serde(default)]
    pub sections: Vec<ModelContextSectionSnapshot>,
    #[serde(default)]
    pub session_note_available: bool,
}

/// prompt generation 发生冷启动的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PromptPrefixChangedReason {
    Initial,
    PromptScopeChanged,
    ProviderChanged,
    ModelChanged,
    BaseInstructionsChanged,
    GlobalInstructionsChanged,
    ModeRoleChanged,
    SkillCatalogChanged,
    WorkspaceInstructionsChanged,
    RequestPropertiesChanged,
    FixedPrefixChanged,
    ToolSchemaChanged,
    ContextCompacted,
    ContextAppended,
}

/// Thread 当前 prompt generation 的脱敏诊断快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadPromptSnapshot {
    pub scope: String,
    pub generation: u64,
    pub provider: String,
    #[serde(default)]
    pub provider_hash: String,
    pub model: String,
    pub fixed_prefix_hash: String,
    #[serde(default)]
    pub fixed_prefix_section_hashes: BTreeMap<String, String>,
    #[serde(default)]
    pub request_properties_hash: String,
    pub tool_schema_hash: String,
    pub context_hash: String,
    #[serde(default)]
    pub prompt_cache_policy: String,
    pub prefix_changed_reason: PromptPrefixChangedReason,
    pub updated_at: i64,
}

/// `threads.metadata_json` 中按产品模式/agent role 保存的 prompt generation。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadPromptMetadata {
    pub active_scope: String,
    #[serde(default)]
    pub slots: BTreeMap<String, ThreadPromptSnapshot>,
}

/// 模型上下文的一次 append-only 差量。
///
/// `message` 是真正发送给模型的最小差量；`resulting_context` 只用于重启恢复和
/// 下一次差量计算，Bridge 不得暴露。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelContextPatch {
    pub id: String,
    pub message: Message,
    pub resulting_context: ModelContextSnapshot,
    pub prompt: ThreadPromptSnapshot,
    #[serde(default)]
    pub prompt_snapshots: BTreeMap<String, ThreadPromptSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_section_ids: Vec<String>,
}

/// 独立于可压缩时间线的上下文段标识。
///
/// 标识会作为持久化协议的一部分，必须稳定且非空。产品层应使用带命名空间的
/// 值，例如 `pl.current_todo` 或 `mai.review_manifest`。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ContextSectionId(String);

impl ContextSectionId {
    pub fn new(value: impl Into<String>) -> Result<Self, ContextSectionIdError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ContextSectionIdError);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContextSectionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ContextSectionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// 空上下文段标识错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextSectionIdError;

impl fmt::Display for ContextSectionIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("context section id must not be empty")
    }
}

impl std::error::Error for ContextSectionIdError {}

/// 每次 inference 都重新注入、且不会被 history compaction 替换的工作上下文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinnedContextSection {
    pub id: ContextSectionId,
    pub revision: u64,
    pub title: String,
    pub content: String,
    pub content_hash: String,
    pub updated_at: i64,
}

/// 随 canonical session 持久化、但不会直接进入模型上下文的文本笔记。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNote {
    pub revision: u64,
    pub content: String,
    pub content_hash: String,
    pub updated_at: i64,
}

/// 工具结果的紧凑、可持久化收据。
///
/// 完整工具输出不进入 canonical history；history 只保存有界模型视图、内容
/// 哈希和 artifact 引用，以便压缩后仍能确认已经读取过哪些证据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultReceipt {
    pub call_id: String,
    pub tool_name: String,
    pub arguments_hash: String,
    pub result_hash: String,
    pub total_bytes: u64,
    pub visible_bytes: u64,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reused_from_call_id: Option<String>,
}

/// Provider 无关的模型上下文项。
///
/// 普通对话通过 [`ModelContextItem::Message`] 表达；provider 返回的不可读
/// checkpoint 通过 [`ModelContextItem::Compaction`] 表达。调用方不得把加密
/// checkpoint 当作普通 system/user 文本发送给不支持它的 provider。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ModelContextItem {
    Message {
        message: Message,
    },
    ToolResult {
        /// 可直接投影到 provider wire 的 tool message；其正文即有界 model view。
        message: Message,
        receipt: ToolResultReceipt,
    },
    PinnedContext {
        section: PinnedContextSection,
    },
    SessionNote {
        note: SessionNote,
    },
    ContextPatch {
        patch: ModelContextPatch,
    },
    Compaction {
        #[serde(rename = "encryptedContent")]
        encrypted_content: String,
    },
}

impl ModelContextItem {
    pub fn as_message(&self) -> Option<&Message> {
        match self {
            Self::Message { message } => Some(message),
            Self::ToolResult { message, .. } => Some(message),
            Self::ContextPatch { patch } => Some(&patch.message),
            Self::PinnedContext { .. } | Self::SessionNote { .. } | Self::Compaction { .. } => None,
        }
    }

    pub fn into_message(self) -> Option<Message> {
        match self {
            Self::Message { message } => Some(message),
            Self::ToolResult { message, .. } => Some(message),
            Self::ContextPatch { patch } => Some(patch.message),
            Self::PinnedContext { .. } | Self::SessionNote { .. } | Self::Compaction { .. } => None,
        }
    }

    pub fn is_compaction(&self) -> bool {
        matches!(self, Self::Compaction { .. })
    }

    pub fn as_pinned_context(&self) -> Option<&PinnedContextSection> {
        match self {
            Self::PinnedContext { section } => Some(section),
            Self::Message { .. }
            | Self::ToolResult { .. }
            | Self::ContextPatch { .. }
            | Self::SessionNote { .. }
            | Self::Compaction { .. } => None,
        }
    }

    pub fn as_session_note(&self) -> Option<&SessionNote> {
        match self {
            Self::SessionNote { note } => Some(note),
            Self::Message { .. }
            | Self::ToolResult { .. }
            | Self::PinnedContext { .. }
            | Self::ContextPatch { .. }
            | Self::Compaction { .. } => None,
        }
    }

    pub fn as_tool_result_receipt(&self) -> Option<&ToolResultReceipt> {
        match self {
            Self::ToolResult { receipt, .. } => Some(receipt),
            Self::Message { .. }
            | Self::PinnedContext { .. }
            | Self::ContextPatch { .. }
            | Self::SessionNote { .. }
            | Self::Compaction { .. } => None,
        }
    }

    pub fn is_pinned_context(&self) -> bool {
        matches!(self, Self::PinnedContext { .. })
    }

    pub fn is_session_note(&self) -> bool {
        matches!(self, Self::SessionNote { .. })
    }

    pub fn as_context_patch(&self) -> Option<&ModelContextPatch> {
        match self {
            Self::ContextPatch { patch } => Some(patch),
            Self::Message { .. }
            | Self::ToolResult { .. }
            | Self::PinnedContext { .. }
            | Self::SessionNote { .. }
            | Self::Compaction { .. } => None,
        }
    }
}

impl From<Message> for ModelContextItem {
    fn from(message: Message) -> Self {
        Self::Message { message }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_context_section_id_is_rejected_during_deserialization() {
        let error = serde_json::from_str::<ContextSectionId>("\"  \"").unwrap_err();
        assert!(error.to_string().contains("must not be empty"));
    }
}
