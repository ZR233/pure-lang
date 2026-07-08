use pl_protocol::{ContentPart, Message, MessageContent, MessageRole};

/// 提取消息内容中模型可见的文本片段。
///
/// 多模态消息中的图片不转换为占位文本；调用方只拿到真实文本 part，
/// 用于 skill 注入、历史摘要和产品层投影等不需要图片 payload 的路径。
pub fn message_content_text(content: &MessageContent) -> String {
    message_content_text_with_separator(content, "")
}

/// 提取消息内容中的文本片段，并用换行连接多模态文本 part。
pub fn message_content_text_lines(content: &MessageContent) -> String {
    message_content_text_with_separator(content, "\n")
}

/// 从模型完成响应中生成短预览文本。
///
/// 预览按 reasoning、assistant content、tool call 的顺序拼接，并使用与 Codex
/// 工具事件一致的单行截断格式，便于产品层和服务探活共享同一展示语义。
pub fn completion_response_preview(response: &pl_model::CompletionResponse) -> String {
    let mut parts = Vec::new();
    if let Some(reasoning) = response
        .reasoning_content
        .as_deref()
        .filter(|text| !text.is_empty())
    {
        parts.push(reasoning.to_string());
    }
    if let Some(content) = response.content.as_deref().filter(|text| !text.is_empty()) {
        parts.push(content.to_string());
    }
    parts.extend(response.tool_calls.iter().map(|call| {
        format!(
            "function_call {} {}: {}",
            call.name,
            call.call_id.as_deref().unwrap_or(&call.id),
            call.payload_text()
        )
    }));
    text_preview(&parts.join("\n"), 500)
}

/// 构造一条普通用户文本消息。
pub fn user_text_message(text: impl Into<String>) -> Message {
    text_message(MessageRole::User, text.into())
}

/// 构造一条普通助手文本消息。
pub fn assistant_text_message(text: impl Into<String>) -> Message {
    text_message(MessageRole::Assistant, text.into())
}

/// 构造一条仅包含 reasoning 内容的助手消息。
pub fn assistant_reasoning_message(content: impl Into<String>) -> Message {
    Message {
        role: MessageRole::Assistant,
        content: MessageContent::Text(String::new()),
        reasoning_content: Some(content.into()),
        metadata: Default::default(),
    }
}

/// 读取用户消息的首个模型可见文本片段。
///
/// 非用户消息返回 `None`；多模态用户消息跳过图片 part，只返回第一个文本 part。
pub fn user_message_text(message: &Message) -> Option<&str> {
    if message.role != MessageRole::User {
        return None;
    }
    match &message.content {
        MessageContent::Text(text) => Some(text.as_str()),
        MessageContent::MultiPart(parts) => parts.iter().find_map(|part| match part {
            ContentPart::Text { text } => Some(text.as_str()),
            ContentPart::Image {
                source: _,
                media_type: _,
                filename: _,
            } => None,
        }),
    }
}

/// 判断一段文本是否使用调用方定义的 compaction summary 前缀。
pub fn is_compaction_summary_text(text: &str, summary_prefix: &str) -> bool {
    text.starts_with(summary_prefix)
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

fn text_message(role: MessageRole, content: String) -> Message {
    Message {
        role,
        content: MessageContent::Text(content),
        reasoning_content: None,
        metadata: Default::default(),
    }
}

fn text_preview(value: &str, max: usize) -> String {
    let mut out = value.replace('\n', "\\n");
    if out.len() > max {
        let boundary = out
            .char_indices()
            .take_while(|(i, c)| i + c.len_utf8() <= max)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        out.truncate(boundary);
        out.push_str("...");
    }
    out
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
    fn message_content_text_lines_joins_text_parts_with_newlines() {
        let content = MessageContent::MultiPart(vec![
            ContentPart::Text {
                text: "first".to_string(),
            },
            ContentPart::Image {
                source: ImageSource::Attachment {
                    attachment_id: "image-1".to_string(),
                },
                media_type: "image/png".to_string(),
                filename: None,
            },
            ContentPart::Text {
                text: "second".to_string(),
            },
        ]);

        assert_eq!(super::message_content_text_lines(&content), "first\nsecond");
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

    #[test]
    fn model_message_helpers_cover_text_roles_and_user_projection() {
        let user = super::user_text_message("hello");
        let assistant = super::assistant_text_message("done");
        let reasoning = super::assistant_reasoning_message("thinking");
        let multipart_user = Message {
            role: MessageRole::User,
            content: MessageContent::MultiPart(vec![
                ContentPart::Image {
                    source: ImageSource::Attachment {
                        attachment_id: "image-1".to_string(),
                    },
                    media_type: "image/png".to_string(),
                    filename: None,
                },
                ContentPart::Text {
                    text: "visible".to_string(),
                },
            ]),
            reasoning_content: None,
            metadata: HashMap::new(),
        };

        assert_eq!(user.role, MessageRole::User);
        assert_eq!(user.content, MessageContent::Text("hello".to_string()));
        assert_eq!(assistant.role, MessageRole::Assistant);
        assert_eq!(assistant.content, MessageContent::Text("done".to_string()));
        assert_eq!(reasoning.role, MessageRole::Assistant);
        assert_eq!(reasoning.reasoning_content.as_deref(), Some("thinking"));
        assert_eq!(super::user_message_text(&multipart_user), Some("visible"));
        assert_eq!(super::user_message_text(&assistant), None);
    }

    #[test]
    fn compaction_summary_text_uses_call_site_prefix() {
        assert!(super::is_compaction_summary_text(
            "Context checkpoint\n\nsummary",
            "Context checkpoint"
        ));
        assert!(!super::is_compaction_summary_text(
            "Other text",
            "Context checkpoint"
        ));
    }

    #[test]
    fn completion_response_preview_includes_reasoning_content_and_tool_calls() {
        let response = pl_model::CompletionResponse {
            response_id: Some("resp_1".to_string()),
            content: Some("answer".to_string()),
            raw_content: None,
            reasoning_content: Some("thinking".to_string()),
            tool_calls: vec![pl_model::ToolCall::function(
                "call_item",
                "read_file",
                serde_json::json!({"path": "Cargo.toml"}),
                Some("call_1".to_string()),
            )],
            trace_events: Vec::new(),
            next_sequence: 0,
            usage: pl_model::TokenUsage::default(),
            finish_reason: pl_model::FinishReason::Stop,
            model: "test-model".to_string(),
        };

        assert_eq!(
            super::completion_response_preview(&response),
            r#"thinking\nanswer\nfunction_call read_file call_1: {"path":"Cargo.toml"}"#
        );
    }

    #[test]
    fn completion_response_preview_truncates_on_char_boundary() {
        let response = pl_model::CompletionResponse {
            response_id: None,
            content: Some("你".repeat(200)),
            raw_content: None,
            reasoning_content: None,
            tool_calls: Vec::new(),
            trace_events: Vec::new(),
            next_sequence: 0,
            usage: pl_model::TokenUsage::default(),
            finish_reason: pl_model::FinishReason::Stop,
            model: "test-model".to_string(),
        };

        let preview = super::completion_response_preview(&response);

        assert!(preview.ends_with("..."));
        assert!(preview.len() <= 503);
        assert!(std::str::from_utf8(preview.as_bytes()).is_ok());
    }
}
