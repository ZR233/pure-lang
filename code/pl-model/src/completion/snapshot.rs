//! Read-only views of normalized model output for host consumers.

/// 模型完成响应面向宿主产品的结构化快照。
///
/// 产品层可以把该快照投影到自己的 Web/API DTO，但不应重复解释
/// `crate::completion::CompletionResponse` 的 reasoning、文本、tool call 和 usage 语义。
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionResponseSnapshot {
    id: Option<String>,
    model: String,
    output: Vec<CompletionResponseOutputSnapshot>,
    accounting: pl_protocol::InferenceAccounting,
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

    /// Final accounting, including missing usage and disabled pricing states.
    pub fn accounting(&self) -> &pl_protocol::InferenceAccounting {
        &self.accounting
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
    response: &crate::completion::CompletionResponse,
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
        accounting: response.accounting.clone(),
    }
}

/// 提取模型完成响应中的助手可见文本输出。
///
/// reasoning 和 tool call 不属于普通助手消息文本；宿主产品需要标题、摘要等
/// 纯文本用途时应使用该 helper，而不是重复解析 `CompletionResponse`。
pub fn completion_response_message_text(
    response: &crate::completion::CompletionResponse,
) -> String {
    completion_response_snapshot(response)
        .output()
        .iter()
        .filter_map(CompletionResponseOutputSnapshot::as_message)
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    #[test]
    fn completion_response_snapshot_projects_model_response_shape() {
        let response = crate::completion::CompletionResponse {
            response_id: Some("resp_1".to_string()),
            content: Some("answer".to_string()),
            reasoning_content: Some("thinking".to_string()),
            tool_calls: vec![
                crate::completion::ToolCall::function(
                    "call_item",
                    "read_file",
                    serde_json::json!({"path": "Cargo.toml"}),
                    "call_1",
                ),
                crate::completion::ToolCall::custom(
                    "custom_item",
                    "apply_patch",
                    "*** Begin Patch",
                    "custom_item",
                ),
            ],
            responses_context_items: Vec::new(),
            orchestration: Default::default(),
            timing: None,
            accounting: pl_protocol::InferenceAccounting {
                usage: pl_protocol::UsageReport {
                    input_tokens: Some(10),
                    cache_read_tokens: Some(4),
                    cache_write_tokens: Some(1),
                    output_tokens: Some(3),
                    reasoning_tokens: Some(2),
                    total_tokens: Some(13),
                },
                ..Default::default()
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
        assert_eq!(snapshot.accounting().usage.totals().prompt_tokens, 10);
        assert_eq!(snapshot.accounting().usage.totals().cached_prompt_tokens, 4);
        assert_eq!(snapshot.accounting().usage.totals().completion_tokens, 3);
        assert_eq!(snapshot.accounting().usage.totals().reasoning_tokens, 2);
        assert_eq!(snapshot.accounting().usage.totals().total_tokens, 13);
    }

    #[test]
    fn completion_response_message_text_uses_only_visible_message_output() {
        let response = crate::completion::CompletionResponse {
            response_id: Some("resp_1".to_string()),
            content: Some("Task title".to_string()),
            reasoning_content: Some("hidden chain of thought".to_string()),
            tool_calls: vec![crate::completion::ToolCall::function(
                "call_item",
                "read_file",
                serde_json::json!({"path": "Cargo.toml"}),
                "call_1",
            )],
            responses_context_items: Vec::new(),
            orchestration: Default::default(),
            timing: None,
            accounting: pl_protocol::InferenceAccounting::default(),
            model: "test-model".to_string(),
        };

        assert_eq!(
            super::completion_response_message_text(&response),
            "Task title"
        );
    }
}
