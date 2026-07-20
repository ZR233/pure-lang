use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::Message;

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
            Self::PinnedContext { .. } | Self::Compaction { .. } => None,
        }
    }

    pub fn into_message(self) -> Option<Message> {
        match self {
            Self::Message { message } => Some(message),
            Self::ToolResult { message, .. } => Some(message),
            Self::PinnedContext { .. } | Self::Compaction { .. } => None,
        }
    }

    pub fn is_compaction(&self) -> bool {
        matches!(self, Self::Compaction { .. })
    }

    pub fn as_pinned_context(&self) -> Option<&PinnedContextSection> {
        match self {
            Self::PinnedContext { section } => Some(section),
            Self::Message { .. } | Self::ToolResult { .. } | Self::Compaction { .. } => None,
        }
    }

    pub fn as_tool_result_receipt(&self) -> Option<&ToolResultReceipt> {
        match self {
            Self::ToolResult { receipt, .. } => Some(receipt),
            Self::Message { .. } | Self::PinnedContext { .. } | Self::Compaction { .. } => None,
        }
    }

    pub fn is_pinned_context(&self) -> bool {
        matches!(self, Self::PinnedContext { .. })
    }
}

impl From<Message> for ModelContextItem {
    fn from(message: Message) -> Self {
        Self::Message { message }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{MessageContent, MessageRole};

    use super::*;

    #[test]
    fn model_context_items_round_trip_with_camel_case_wire_fields() {
        let items = vec![
            ModelContextItem::from(Message {
                role: MessageRole::User,
                content: MessageContent::Text("hello".to_string()),
                reasoning_content: None,
                metadata: HashMap::new(),
            }),
            ModelContextItem::Compaction {
                encrypted_content: "encrypted".to_string(),
            },
            ModelContextItem::PinnedContext {
                section: PinnedContextSection {
                    id: ContextSectionId::new("pl.current_todo").unwrap(),
                    revision: 2,
                    title: "Current Todo".to_string(),
                    content: "- inspect".to_string(),
                    content_hash: "sha256:todo".to_string(),
                    updated_at: 42,
                },
            },
        ];

        let value = serde_json::to_value(&items).unwrap();
        assert_eq!(value[1]["type"], "compaction");
        assert_eq!(value[1]["encryptedContent"], "encrypted");
        assert_eq!(value[2]["section"]["id"], "pl.current_todo");
        assert_eq!(
            serde_json::from_value::<Vec<ModelContextItem>>(value).unwrap(),
            items
        );
    }

    #[test]
    fn empty_context_section_id_is_rejected_during_deserialization() {
        let error = serde_json::from_str::<ContextSectionId>("\"  \"").unwrap_err();
        assert!(error.to_string().contains("must not be empty"));
    }
}
