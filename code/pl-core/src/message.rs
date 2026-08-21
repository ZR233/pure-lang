use pl_protocol::{ContentPart, Message, MessageContent, MessageRole};

use crate::runtime_usage::ModelTokenUsageSnapshot;

/// 提取消息内容中模型可见的文本片段。
///
/// 多模态消息中的图片不转换为占位文本；调用方只拿到真实文本 part，
/// 用于 skill 注入、历史摘要和产品层投影等不需要图片 payload 的路径。
pub fn message_content_text(content: &MessageContent) -> String {
    message_content_text_with_separator(content, "")
}

/// 生成按字符数截断的自然文本预览。
///
/// 该 helper 会先 trim 首尾空白；未超限时返回完整文本，超限时保留前
/// `max_chars` 个字符并追加省略号。适用于 UI/service 事件的摘要文本。
pub fn text_preview_chars(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut preview = trimmed.chars().take(max_chars).collect::<String>();
    preview.push_str("...");
    preview
}

/// 模型完成响应面向宿主产品的结构化快照。
///
/// 产品层可以把该快照投影到自己的 Web/API DTO，但不应重复解释
/// `pl_model::CompletionResponse` 的 reasoning、文本、tool call 和 usage 语义。
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionResponseSnapshot {
    id: Option<String>,
    model: String,
    output: Vec<CompletionResponseOutputSnapshot>,
    usage: ModelTokenUsageSnapshot,
}

impl CompletionResponseSnapshot {
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn output(&self) -> &[CompletionResponseOutputSnapshot] {
        &self.output
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn usage(&self) -> &ModelTokenUsageSnapshot {
        &self.usage
    }
}

/// 模型完成响应中的结构化输出项。
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionResponseOutputSnapshot {
    kind: CompletionResponseOutputKind,
}

#[derive(Debug, Clone, PartialEq)]
enum CompletionResponseOutputKind {
    Reasoning {
        content: String,
    },
    Message {
        text: String,
    },
    FunctionCall {
        call_id: String,
        name: String,
        arguments: serde_json::Value,
        raw_arguments: String,
    },
}

pub struct CompletionResponseFunctionCallSnapshot<'a> {
    call_id: &'a str,
    name: &'a str,
    arguments: &'a serde_json::Value,
    raw_arguments: &'a str,
}

impl<'a> CompletionResponseFunctionCallSnapshot<'a> {
    pub fn call_id(&self) -> &'a str {
        self.call_id
    }

    pub fn name(&self) -> &'a str {
        self.name
    }

    pub fn arguments(&self) -> &'a serde_json::Value {
        self.arguments
    }

    pub fn raw_arguments(&self) -> &'a str {
        self.raw_arguments
    }
}

impl CompletionResponseOutputSnapshot {
    pub fn reasoning(content: impl Into<String>) -> Self {
        Self {
            kind: CompletionResponseOutputKind::Reasoning {
                content: content.into(),
            },
        }
    }

    pub fn message(text: impl Into<String>) -> Self {
        Self {
            kind: CompletionResponseOutputKind::Message { text: text.into() },
        }
    }

    pub fn function_call(
        call_id: impl Into<String>,
        name: impl Into<String>,
        arguments: serde_json::Value,
        raw_arguments: impl Into<String>,
    ) -> Self {
        Self {
            kind: CompletionResponseOutputKind::FunctionCall {
                call_id: call_id.into(),
                name: name.into(),
                arguments,
                raw_arguments: raw_arguments.into(),
            },
        }
    }

    pub fn as_reasoning(&self) -> Option<&str> {
        match &self.kind {
            CompletionResponseOutputKind::Reasoning { content } => Some(content.as_str()),
            CompletionResponseOutputKind::Message { .. }
            | CompletionResponseOutputKind::FunctionCall { .. } => None,
        }
    }

    pub fn as_message(&self) -> Option<&str> {
        match &self.kind {
            CompletionResponseOutputKind::Message { text } => Some(text.as_str()),
            CompletionResponseOutputKind::Reasoning { .. }
            | CompletionResponseOutputKind::FunctionCall { .. } => None,
        }
    }

    pub fn as_function_call(&self) -> Option<CompletionResponseFunctionCallSnapshot<'_>> {
        match &self.kind {
            CompletionResponseOutputKind::FunctionCall {
                call_id,
                name,
                arguments,
                raw_arguments,
            } => Some(CompletionResponseFunctionCallSnapshot {
                call_id,
                name,
                arguments,
                raw_arguments,
            }),
            CompletionResponseOutputKind::Reasoning { .. }
            | CompletionResponseOutputKind::Message { .. } => None,
        }
    }
}

/// 从模型完成响应生成结构化宿主快照。
pub fn completion_response_snapshot(
    response: &pl_model::CompletionResponse,
) -> CompletionResponseSnapshot {
    let mut output = Vec::new();
    if let Some(reasoning) = response
        .reasoning_content
        .as_deref()
        .filter(|text| !text.trim().is_empty())
    {
        output.push(CompletionResponseOutputSnapshot::reasoning(reasoning));
    }
    if let Some(content) = response
        .content
        .as_deref()
        .filter(|text| !text.trim().is_empty())
    {
        output.push(CompletionResponseOutputSnapshot::message(content));
    }
    output.extend(response.tool_calls.iter().map(|call| {
        CompletionResponseOutputSnapshot::function_call(
            &call.call_id,
            call.name.clone(),
            call.arguments_for_tool(),
            call.payload_text(),
        )
    }));

    CompletionResponseSnapshot {
        id: response.response_id.clone(),
        model: response.model.clone(),
        output,
        usage: ModelTokenUsageSnapshot::from(&response.usage),
    }
}

/// 提取模型完成响应中的助手可见文本输出。
///
/// reasoning 和 tool call 不属于普通助手消息文本；宿主产品需要标题、摘要等
/// 纯文本用途时应使用该 helper，而不是重复解析 `CompletionResponse`。
pub fn completion_response_message_text(response: &pl_model::CompletionResponse) -> String {
    completion_response_snapshot(response)
        .output()
        .iter()
        .filter_map(CompletionResponseOutputSnapshot::as_message)
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("")
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
        tool_calls: None,
        tool_result: None,
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
        tool_calls: None,
        tool_result: None,
        metadata: Default::default(),
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
            tool_calls: None,
            tool_result: None,
            metadata: HashMap::new(),
        };

        assert_eq!(
            super::append_message_fragment_text("hello".to_string(), Some(&fragment)),
            "hello\n\nskill one\nskill two"
        );
        let empty_fragment = Message {
            role: MessageRole::User,
            content: MessageContent::Text("   ".to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: HashMap::new(),
        };

        assert_eq!(
            super::append_message_fragment_text("hello".to_string(), Some(&empty_fragment)),
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
            tool_calls: None,
            tool_result: None,
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
    fn completion_response_snapshot_projects_model_response_shape() {
        let response = pl_model::CompletionResponse {
            response_id: Some("resp_1".to_string()),
            content: Some("answer".to_string()),
            reasoning_content: Some("thinking".to_string()),
            tool_calls: vec![
                pl_model::ToolCall::function(
                    "call_item",
                    "read_file",
                    serde_json::json!({"path": "Cargo.toml"}),
                    "call_1",
                ),
                pl_model::ToolCall::custom(
                    "custom_item",
                    "apply_patch",
                    "*** Begin Patch",
                    "custom_item",
                ),
            ],
            responses_context_items: Vec::new(),
            orchestration: Default::default(),
            usage: pl_model::TokenUsage {
                prompt_tokens: 10,
                cached_prompt_tokens: 4,
                cache_write_tokens: 1,
                completion_tokens: 3,
                reasoning_tokens: 2,
                total_tokens: 13,
            },
            model: "test-model".to_string(),
        };

        let snapshot = super::completion_response_snapshot(&response);
        assert_eq!(snapshot.id(), Some("resp_1"));
        assert_eq!(
            snapshot.output().to_vec(),
            vec![
                super::CompletionResponseOutputSnapshot::reasoning("thinking"),
                super::CompletionResponseOutputSnapshot::message("answer"),
                super::CompletionResponseOutputSnapshot::function_call(
                    "call_1",
                    "read_file",
                    serde_json::json!({"path": "Cargo.toml"}),
                    r#"{"path":"Cargo.toml"}"#,
                ),
                super::CompletionResponseOutputSnapshot::function_call(
                    "custom_item",
                    "apply_patch",
                    serde_json::json!({ "input": "*** Begin Patch" }),
                    "*** Begin Patch",
                ),
            ]
        );
        assert_eq!(snapshot.usage().input_tokens(), 10);
        assert_eq!(snapshot.usage().cached_input_tokens(), 4);
        assert_eq!(snapshot.usage().output_tokens(), 3);
        assert_eq!(snapshot.usage().reasoning_output_tokens(), 2);
        assert_eq!(snapshot.usage().total_tokens(), 13);
    }

    #[test]
    fn completion_response_message_text_uses_only_visible_message_output() {
        let response = pl_model::CompletionResponse {
            response_id: Some("resp_1".to_string()),
            content: Some("Task title".to_string()),
            reasoning_content: Some("hidden chain of thought".to_string()),
            tool_calls: vec![pl_model::ToolCall::function(
                "call_item",
                "read_file",
                serde_json::json!({"path": "Cargo.toml"}),
                "call_1",
            )],
            responses_context_items: Vec::new(),
            orchestration: Default::default(),
            usage: pl_model::TokenUsage::default(),
            model: "test-model".to_string(),
        };

        assert_eq!(
            super::completion_response_message_text(&response),
            "Task title"
        );
    }

    #[test]
    fn text_preview_chars_trims_and_truncates_by_char_count() {
        assert_eq!(super::text_preview_chars("  hello  ", 10), "hello");
        assert_eq!(super::text_preview_chars("你好世界", 2), "你好...");
        assert_eq!(super::text_preview_chars("abc", 0), "...");
    }
}
