use serde::{Deserialize, Serialize};

use crate::Message;

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
    Compaction {
        #[serde(rename = "encryptedContent")]
        encrypted_content: String,
    },
}

impl ModelContextItem {
    pub fn as_message(&self) -> Option<&Message> {
        match self {
            Self::Message { message } => Some(message),
            Self::Compaction { .. } => None,
        }
    }

    pub fn into_message(self) -> Option<Message> {
        match self {
            Self::Message { message } => Some(message),
            Self::Compaction { .. } => None,
        }
    }

    pub fn is_compaction(&self) -> bool {
        matches!(self, Self::Compaction { .. })
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
        ];

        let value = serde_json::to_value(&items).unwrap();
        assert_eq!(value[1]["type"], "compaction");
        assert_eq!(value[1]["encryptedContent"], "encrypted");
        assert_eq!(
            serde_json::from_value::<Vec<ModelContextItem>>(value).unwrap(),
            items
        );
    }
}
