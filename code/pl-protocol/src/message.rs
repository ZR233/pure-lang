use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// 一条模型消息是否应投影到产品 GUI。
///
/// `Hidden` 只影响产品展示；消息仍属于 canonical AgentSession，必须照常持久化并发送给 provider。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessagePresentation {
    #[default]
    Visible,
    Hidden,
}

impl MessagePresentation {
    pub const fn is_visible(&self) -> bool {
        matches!(self, Self::Visible)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub role: MessageRole,
    pub content: MessageContent,
    #[serde(default, skip_serializing_if = "MessagePresentation::is_visible")]
    pub presentation: MessagePresentation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    /// assistant 消息携带的工具调用集合；provider wire 映射忽略该字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallRecord>>,
    /// tool 结果消息携带的配对工具调用记录；provider wire 映射忽略该字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<ToolResultRecord>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageContent {
    pub parts: Vec<ContentPart>,
}

impl MessageContent {
    pub fn new(parts: Vec<ContentPart>) -> Self {
        Self { parts }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self {
            parts: vec![ContentPart::Text { text: text.into() }],
        }
    }

    pub fn text_value(&self) -> String {
        self.parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::Attachment { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ContentPart {
    Text {
        text: String,
    },
    Attachment {
        attachment_id: String,
        modality: AttachmentModality,
        media_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AttachmentModality {
    Image,
    Video,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolCallKind {
    Function,
    Custom,
}

/// Responses Programmatic Tool Calling 中嵌套工具调用的来源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolCallCaller {
    Program { caller_id: String },
}

impl ToolCallKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Custom => "custom",
        }
    }
}

/// assistant 消息中的 typed 工具调用记录。
///
/// `item_id` 是 provider 返回的工具调用 item id，`call_id` 是跨协议回放的
/// canonical 调用 id，两者必填。`kind` 为 `function` 时 `arguments` 是解析后的
/// JSON；无法解析的原始参数与 `custom` 工具的输入文本存为字符串字面量，
/// 回放时按原始文本发送。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecord {
    pub item_id: String,
    pub call_id: String,
    pub name: String,
    pub kind: ToolCallKind,
    pub arguments: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<ToolCallCaller>,
}

/// tool 结果消息中的 typed 工具调用配对记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultRecord {
    pub item_id: String,
    pub call_id: String,
    pub name: String,
    pub kind: ToolCallKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(presentation: MessagePresentation) -> Message {
        Message {
            role: MessageRole::User,
            content: MessageContent::text("internal input"),
            presentation,
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn visible_is_the_omitted_wire_default() {
        let value = serde_json::to_value(message(MessagePresentation::Visible)).unwrap();
        assert!(value.get("presentation").is_none());
        let restored: Message = serde_json::from_value(value).unwrap();
        assert_eq!(restored.presentation, MessagePresentation::Visible);
    }

    #[test]
    fn hidden_round_trips_as_generic_message_protocol() {
        let value = serde_json::to_value(message(MessagePresentation::Hidden)).unwrap();
        assert_eq!(value["presentation"], "hidden");
        let restored: Message = serde_json::from_value(value).unwrap();
        assert_eq!(restored.presentation, MessagePresentation::Hidden);
    }
}
