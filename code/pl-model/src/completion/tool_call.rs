//! 工具调用的 canonical 表示与身份。

use serde::{Deserialize, Serialize};

use pl_protocol::ToolCallCaller;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// provider 返回的工具调用 item id（`item.id`）。
    pub id: String,
    pub name: String,
    pub payload: ToolCallPayload,
    /// 跨协议回放的 canonical 调用 id；在协议解码边界一次性确定，必填。
    pub call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_arguments: Option<InvalidToolArguments>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<ToolCallCaller>,
}

/// 一次工具调用的必填 typed 身份。
///
/// `item_id` 是 provider 返回的工具调用 item id；`call_id` 是跨协议回放使用的
/// canonical 调用 id。两者在解码边界确定，不存在 optional 回落路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallIdentity {
    pub item_id: String,
    pub call_id: String,
}

/// Provider 返回的 function tool 参数无法解析为 JSON 时保留的诊断信息。
///
/// 该信息让执行层把模型输出错误作为失败的工具调用反馈给模型，而不是把整次
/// completion 误判为 provider 传输失败。原始参数同时用于历史回放与 trace。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvalidToolArguments {
    pub raw: String,
    pub error: String,
}

impl ToolCall {
    pub fn identity(&self) -> ToolCallIdentity {
        ToolCallIdentity {
            item_id: self.id.clone(),
            call_id: self.call_id.clone(),
        }
    }

    pub fn function(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: serde_json::Value,
        call_id: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            payload: ToolCallPayload::Function { arguments },
            call_id: call_id.into(),
            invalid_arguments: None,
            caller: None,
        }
    }

    /// 构造 provider 已给出稳定身份、但 function 参数不是合法 JSON 的工具调用。
    pub fn invalid_function(
        id: impl Into<String>,
        name: impl Into<String>,
        raw: impl Into<String>,
        error: impl Into<String>,
        call_id: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            payload: ToolCallPayload::Function {
                arguments: serde_json::Value::Null,
            },
            call_id: call_id.into(),
            invalid_arguments: Some(InvalidToolArguments {
                raw: raw.into(),
                error: error.into(),
            }),
            caller: None,
        }
    }

    pub fn custom(
        id: impl Into<String>,
        name: impl Into<String>,
        input: impl Into<String>,
        call_id: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            payload: ToolCallPayload::Custom {
                input: input.into(),
            },
            call_id: call_id.into(),
            invalid_arguments: None,
            caller: None,
        }
    }

    pub fn with_caller(mut self, caller: Option<ToolCallCaller>) -> Self {
        self.caller = caller;
        self
    }

    pub fn kind(&self) -> pl_protocol::ToolCallKind {
        match self.payload {
            ToolCallPayload::Function { .. } => pl_protocol::ToolCallKind::Function,
            ToolCallPayload::Custom { .. } => pl_protocol::ToolCallKind::Custom,
        }
    }

    pub fn arguments_for_tool(&self) -> serde_json::Value {
        match &self.payload {
            ToolCallPayload::Function { arguments } => arguments.clone(),
            ToolCallPayload::Custom { input } => serde_json::json!({ "input": input }),
        }
    }

    pub fn arguments_for_display(&self) -> serde_json::Value {
        if let Some(invalid) = &self.invalid_arguments {
            return serde_json::json!({
                "raw": invalid.raw,
                "parse_error": invalid.error,
            });
        }
        match &self.payload {
            ToolCallPayload::Function { arguments } => arguments.clone(),
            ToolCallPayload::Custom { input } => serde_json::json!({ "input": input }),
        }
    }

    pub fn payload_text(&self) -> String {
        if let Some(invalid) = &self.invalid_arguments {
            return invalid.raw.clone();
        }
        match &self.payload {
            ToolCallPayload::Function { arguments } => {
                serde_json::to_string(arguments).unwrap_or_default()
            }
            ToolCallPayload::Custom { input } => input.clone(),
        }
    }

    /// 返回可直接反馈给模型的非法参数诊断；合法调用返回 `None`。
    pub fn invalid_arguments_message(&self) -> Option<String> {
        let invalid = self.invalid_arguments.as_ref()?;
        Some(format!(
            "Invalid JSON arguments for function tool {}: {}. Call the tool again with exactly one valid JSON object.",
            self.name, invalid.error
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ToolCallPayload {
    Function { arguments: serde_json::Value },
    Custom { input: String },
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn tool_call_identity_exposes_item_and_call_ids() {
        let call = ToolCall::function("item-1", "read_file", serde_json::json!({}), "call-1");

        assert_eq!(
            call.identity(),
            ToolCallIdentity {
                item_id: "item-1".to_string(),
                call_id: "call-1".to_string(),
            }
        );
    }
}
