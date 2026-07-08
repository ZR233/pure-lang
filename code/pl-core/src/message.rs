use pl_protocol::{ContentPart, Message, MessageContent};

/// 提取消息内容中模型可见的文本片段。
///
/// 多模态消息中的图片不转换为占位文本；调用方只拿到真实文本 part，
/// 用于 skill 注入、历史摘要和产品层投影等不需要图片 payload 的路径。
pub fn message_content_text(content: &MessageContent) -> String {
    message_content_text_with_separator(content, "")
}

fn message_content_text_with_separator(content: &MessageContent, separator: &str) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::MultiPart(parts) => parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { text } => Some(text.as_str()),
                ContentPart::Image {
                    source: _,
                    media_type: _,
                    filename: _,
                } => None,
            })
            .collect::<Vec<_>>()
            .join(separator),
    }
}

/// 将一条可选 fragment message 的文本追加到基础消息后。
///
/// 空白 fragment 会被忽略；非空 fragment 以空行分隔，保持宿主拼接 skill
/// 或上下文片段时的稳定格式。
pub fn append_message_fragment_text(message: String, fragment: Option<&Message>) -> String {
    let Some(fragment) = fragment else {
        return message;
    };
    let fragment_text = message_content_text_with_separator(&fragment.content, "\n");
    if fragment_text.trim().is_empty() {
        message
    } else {
        format!("{message}\n\n{fragment_text}")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pl_protocol::{ContentPart, ImageSource, Message, MessageContent, MessageRole};
    use pretty_assertions::assert_eq;

    #[test]
    fn append_message_fragment_text_appends_text_parts_and_ignores_images() {
        let fragment = Message {
            role: MessageRole::User,
            content: MessageContent::MultiPart(vec![
                ContentPart::Text {
                    text: "skill one".to_string(),
                },
                ContentPart::Image {
                    source: ImageSource::Attachment {
                        attachment_id: "image-1".to_string(),
                    },
                    media_type: "image/png".to_string(),
                    filename: None,
                },
                ContentPart::Text {
                    text: "skill two".to_string(),
                },
            ]),
            reasoning_content: None,
            metadata: HashMap::new(),
        };

        assert_eq!(
            super::append_message_fragment_text("hello".to_string(), Some(&fragment)),
            "hello\n\nskill one\nskill two"
        );
    }

    #[test]
    fn append_message_fragment_text_skips_empty_fragment() {
        let fragment = Message {
            role: MessageRole::User,
            content: MessageContent::Text("   ".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        };

        assert_eq!(
            super::append_message_fragment_text("hello".to_string(), Some(&fragment)),
            "hello"
        );
        assert_eq!(
            super::append_message_fragment_text("hello".to_string(), None),
            "hello"
        );
    }
}
