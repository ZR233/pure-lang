use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use utoipa::ToSchema;

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

/// 独立于 append-only transcript 的可替换 Agent 工作状态。
///
/// pinned sections、会话笔记和 prompt generation 状态都通过 replacement
/// 持久化；它们不得作为历史消息重复进入模型上下文。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWorkingState {
    #[serde(default)]
    pub sections: Vec<PinnedContextSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_note: Option<SessionNote>,
    #[serde(default)]
    pub prompt: ThreadPromptMetadata,
    #[serde(default)]
    pub conversation_recovery: ConversationRecoveryState,
    #[serde(default)]
    pub revision: u64,
}

/// 对话上下文恢复方式；两种方式都保留外部世界状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum ConversationRecoveryMode {
    RewindTail,
    RebuildThread,
}

/// 对话恢复对工作区、Git 和产品状态采用的固定策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConversationExternalStatePolicy {
    #[default]
    Preserved,
}

/// 一次连续 Turn 后缀回退的审计范围。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRecoveryTurnRange {
    #[serde(default)]
    pub turn_ids: Vec<String>,
}

/// 最近一次已提交 conversation recovery 的不可变审计记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRecoveryRecord {
    pub recovery_id: String,
    pub revision: u64,
    pub mode: ConversationRecoveryMode,
    #[serde(default)]
    pub target_turn_ids: Vec<String>,
    pub before_transcript_hash: String,
    pub after_transcript_hash: String,
    pub removed_input_count: u64,
    pub removed_item_count: u64,
    pub runtime_revision: u64,
    pub thread_revision: u64,
    pub recovered_at: i64,
}

/// 与 canonical session 一起持久化的 conversation recovery 状态。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationRecoveryState {
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub rolled_back_turn_ranges: Vec<ConversationRecoveryTurnRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_recovery: Option<ConversationRecoveryRecord>,
    #[serde(default)]
    pub external_state_policy: ConversationExternalStatePolicy,
}

/// 可由产品 repository 原子保存和恢复的 Agent session 快照。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionSnapshot {
    #[serde(default)]
    pub transcript: Vec<ModelContextItem>,
    #[serde(default)]
    pub working_state: AgentWorkingState,
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
    ContextRecovered,
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
    /// 延迟加载 Tool Search catalog 的 canonical 哈希；仅诊断，不参与轮换比较。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_catalog_hash: Option<String>,
    /// 冻结工具 lease 的注册表全局 revision；仅诊断，不参与轮换比较。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_revision: Option<u64>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("context section id must not be empty")]
pub struct ContextSectionIdError;

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

/// 必须在 `store: false` Responses 请求中按原顺序回放的 provider 原生 item 类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponsesContextItemKind {
    Reasoning,
    WebSearchCall,
    ToolSearchCall,
    ToolSearchOutput,
    Program,
    ProgramOutput,
    Unknown,
}

/// 有界为已知类别、但保留完整 wire 字段的 Responses 上下文项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponsesContextItem {
    pub kind: ResponsesContextItemKind,
    pub value: serde_json::Value,
}

impl ResponsesContextItem {
    pub fn from_wire(value: serde_json::Value) -> Option<Self> {
        let kind = match value.get("type").and_then(serde_json::Value::as_str)? {
            "reasoning" => ResponsesContextItemKind::Reasoning,
            "web_search_call" => ResponsesContextItemKind::WebSearchCall,
            "tool_search_call" => ResponsesContextItemKind::ToolSearchCall,
            "tool_search_output" => ResponsesContextItemKind::ToolSearchOutput,
            "program" => ResponsesContextItemKind::Program,
            "program_output" => ResponsesContextItemKind::ProgramOutput,
            "message"
            | "function_call"
            | "custom_tool_call"
            | "function_call_output"
            | "custom_tool_call_output"
            | "file_search_call"
            | "computer_call"
            | "computer_call_output"
            | "mcp_call"
            | "code_interpreter_call" => return None,
            _ => ResponsesContextItemKind::Unknown,
        };
        Some(Self { kind, value })
    }
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
    Compaction {
        #[serde(rename = "encryptedContent")]
        encrypted_content: String,
    },
    Responses {
        item: ResponsesContextItem,
    },
}

impl ModelContextItem {
    pub fn as_message(&self) -> Option<&Message> {
        match self {
            Self::Message { message } => Some(message),
            Self::ToolResult { message, .. } => Some(message),
            Self::Compaction { .. } | Self::Responses { .. } => None,
        }
    }

    pub fn into_message(self) -> Option<Message> {
        match self {
            Self::Message { message } => Some(message),
            Self::ToolResult { message, .. } => Some(message),
            Self::Compaction { .. } | Self::Responses { .. } => None,
        }
    }

    pub fn is_compaction(&self) -> bool {
        matches!(self, Self::Compaction { .. })
    }

    pub fn as_tool_result_receipt(&self) -> Option<&ToolResultReceipt> {
        match self {
            Self::ToolResult { receipt, .. } => Some(receipt),
            Self::Message { .. } | Self::Compaction { .. } | Self::Responses { .. } => None,
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
