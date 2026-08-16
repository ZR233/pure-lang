use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::redaction::{trace_preview_output, trace_preview_value};

/// 工具生命周期投影阶段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolLifecyclePhase {
    Started,
    Finished { success: bool },
}

/// 从 pl-core trace 中抽出的工具生命周期通用视图。
///
/// 产品层可以把它映射到自身的 store、Web 事件或日志格式；pl-core 负责统一
/// call id、参数 JSON、预览截断、输出 artifact 和耗时计算。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolLifecycleProjection {
    phase: ToolLifecyclePhase,
    call_id: String,
    tool_name: String,
    arguments: Value,
    arguments_preview: String,
    output: String,
    output_preview: String,
    output_artifacts: Vec<Value>,
    output_metrics: Option<pl_trace::TraceToolOutputMetrics>,
    duration_ms: Option<u64>,
    started_at_unix: i64,
    completed_at_unix: Option<i64>,
}

impl ToolLifecycleProjection {
    pub fn phase(&self) -> &ToolLifecyclePhase {
        &self.phase
    }

    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    pub fn arguments(&self) -> &Value {
        &self.arguments
    }

    pub fn arguments_preview(&self) -> &str {
        &self.arguments_preview
    }

    pub fn output(&self) -> &str {
        &self.output
    }

    pub fn output_preview(&self) -> &str {
        &self.output_preview
    }

    pub fn output_artifacts(&self) -> &[Value] {
        &self.output_artifacts
    }

    pub fn output_metrics(&self) -> Option<&pl_trace::TraceToolOutputMetrics> {
        self.output_metrics.as_ref()
    }

    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }

    pub fn started_at_unix(&self) -> i64 {
        self.started_at_unix
    }

    pub fn completed_at_unix(&self) -> Option<i64> {
        self.completed_at_unix
    }

    /// 返回工具完成时间；缺失时回退到开始时间。
    pub fn completed_at_unix_or_started(&self) -> i64 {
        self.completed_at_unix.unwrap_or(self.started_at_unix)
    }

    /// 将 trace 中保存的 artifact JSON 解码为产品层的 artifact 类型。
    ///
    /// pl-core 统一负责生命周期投影里的 JSON 解码策略；产品层只需要选择自身
    /// 持久化或 UI 协议使用的目标类型。无法解码的条目会被忽略，和 trace
    /// artifact 作为附加信息的容错语义保持一致。
    pub fn output_artifacts_as<T>(&self) -> Vec<T>
    where
        T: DeserializeOwned,
    {
        self.output_artifacts
            .iter()
            .filter_map(|value| serde_json::from_value(value.clone()).ok())
            .collect()
    }
}

/// 从会话历史中抽出的工具调用详情。
///
/// 该投影用于产品层在持久化 trace 缺失时从 `pl_protocol::Message` 历史
/// 恢复工具名、参数和模型可见输出；产品层仍负责补充 agent/session/turn
/// 等业务标识和持久化事件 metadata。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolHistoryProjection {
    call_id: String,
    tool_name: String,
    arguments: Value,
    arguments_preview: String,
    output: String,
    output_preview: String,
}

impl ToolHistoryProjection {
    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    pub fn arguments(&self) -> &Value {
        &self.arguments
    }

    pub fn arguments_preview(&self) -> &str {
        &self.arguments_preview
    }

    pub fn output(&self) -> &str {
        &self.output
    }

    pub fn output_preview(&self) -> &str {
        &self.output_preview
    }

    /// 根据历史投影中恢复出的模型可见输出推断工具是否成功。
    pub fn inferred_success(&self) -> bool {
        !self.output.is_empty()
    }
}

pub fn tool_history_projection(
    messages: &[pl_protocol::Message],
    call_id: &str,
    preview_chars: usize,
) -> Option<ToolHistoryProjection> {
    let mut tool_name = None;
    let mut arguments = None;
    let mut output = None;

    for message in messages {
        if let Some(metadata) =
            pl_protocol::ToolCallHistoryMetadata::from_metadata(&message.metadata)
            && let Ok(tool_calls) = serde_json::from_str::<Value>(&metadata.tool_calls_json)
            && let Some(tool_calls) = tool_calls.as_array()
        {
            for tool_call in tool_calls {
                if tool_call_matches(tool_call, call_id) {
                    if tool_name.is_none() {
                        tool_name = tool_call
                            .get("name")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned);
                    }
                    if arguments.is_none() {
                        arguments = tool_call_arguments(tool_call);
                    }
                }
            }
        }

        if message.role == pl_protocol::MessageRole::Tool
            && let Ok(metadata) = pl_protocol::ToolResultMetadata::from_metadata(&message.metadata)
            && tool_result_matches(&metadata, call_id)
        {
            if tool_name.is_none() && !metadata.tool_name.is_empty() {
                tool_name = Some(metadata.tool_name.clone());
            }
            if arguments.is_none()
                && let Some(raw_arguments) = metadata.tool_call_arguments.as_deref()
            {
                arguments = Some(arguments_value(raw_arguments));
            }
            output = Some(crate::message_content_text(&message.content));
        }
    }

    let tool_name = tool_name?;
    let arguments = arguments.unwrap_or_else(|| json!({}));
    let output = output.unwrap_or_default();
    Some(ToolHistoryProjection {
        call_id: call_id.to_string(),
        tool_name,
        arguments_preview: trace_preview_value(&arguments, preview_chars),
        arguments,
        output_preview: trace_preview_output(&output, preview_chars),
        output,
    })
}

pub fn tool_lifecycle_projections(
    events: &[pl_trace::TraceEvent],
    preview_chars: usize,
) -> Vec<ToolLifecycleProjection> {
    events
        .iter()
        .filter_map(|event| tool_lifecycle_projection(event, preview_chars))
        .collect()
}

pub fn tool_lifecycle_projection(
    event: &pl_trace::TraceEvent,
    preview_chars: usize,
) -> Option<ToolLifecycleProjection> {
    match &event.kind {
        pl_trace::TraceEventKind::TracePartStarted { item } => {
            if item.status == pl_trace::TracePartStatus::Started {
                projection_from_trace_part(item, ToolLifecyclePhase::Started, preview_chars)
            } else {
                None
            }
        }
        pl_trace::TraceEventKind::TracePartCompleted { item } => projection_from_trace_part(
            item,
            ToolLifecyclePhase::Finished { success: true },
            preview_chars,
        ),
        pl_trace::TraceEventKind::TracePartFailed {
            item,
            error: _error,
        } => projection_from_trace_part(
            item,
            ToolLifecyclePhase::Finished { success: false },
            preview_chars,
        ),
        pl_trace::TraceEventKind::TracePartDelta { event: _event } => None,
        pl_trace::TraceEventKind::PlanLifecycleChanged { event: _event } => None,
        pl_trace::TraceEventKind::InteractionChanged { event: _event } => None,
        pl_trace::TraceEventKind::EnabledToolsRecorded { event: _event } => None,
        pl_trace::TraceEventKind::SkillActivated {
            activation: _activation,
        } => None,
    }
}

fn projection_from_trace_part(
    item: &pl_trace::TracePart,
    phase: ToolLifecyclePhase,
    preview_chars: usize,
) -> Option<ToolLifecycleProjection> {
    let tool = item.tool.as_ref()?;
    let arguments = arguments_value(&tool.arguments);
    let arguments_preview = trace_preview_value(&arguments, preview_chars);
    let (output, output_preview, output_artifacts, duration_ms, completed_at_unix) = match &phase {
        ToolLifecyclePhase::Started => (String::new(), String::new(), Vec::new(), None, None),
        ToolLifecyclePhase::Finished { success: _success } => {
            let output = tool.result.clone().unwrap_or_default();
            (
                output.clone(),
                trace_preview_output(&output, preview_chars),
                tool.output_artifacts.clone(),
                duration_ms(item.created_at, item.updated_at),
                Some(item.updated_at),
            )
        }
    };
    Some(ToolLifecycleProjection {
        phase,
        call_id: tool_call_id(tool),
        tool_name: tool.name.clone(),
        arguments,
        arguments_preview,
        output,
        output_preview,
        output_artifacts,
        output_metrics: tool.output_metrics.clone(),
        duration_ms,
        started_at_unix: item.created_at,
        completed_at_unix,
    })
}

fn arguments_value(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|error| {
        let _error = error;
        json!(arguments)
    })
}

fn tool_call_matches(tool_call: &Value, call_id: &str) -> bool {
    tool_call
        .get("call_id")
        .and_then(Value::as_str)
        .is_some_and(|value| value == call_id)
        || tool_call
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|value| value == call_id)
}

fn tool_result_matches(metadata: &pl_protocol::ToolResultMetadata, call_id: &str) -> bool {
    metadata.tool_call_id == call_id || metadata.tool_call_call_id.as_deref() == Some(call_id)
}

fn tool_call_arguments(tool_call: &Value) -> Option<Value> {
    let payload = tool_call.get("payload")?;
    match payload.get("kind").and_then(Value::as_str) {
        Some("function") => payload.get("arguments").cloned(),
        Some("custom") => payload
            .get("input")
            .and_then(Value::as_str)
            .map(|input| json!({ "input": input })),
        Some(_) | None => payload.get("arguments").cloned(),
    }
}

fn tool_call_id(tool: &pl_trace::TraceToolPart) -> String {
    tool.call_id
        .clone()
        .unwrap_or_else(|| tool.tool_call_id.clone())
}

fn duration_ms(created_at: i64, updated_at: i64) -> Option<u64> {
    updated_at
        .saturating_sub(created_at)
        .try_into()
        .ok()
        .map(|seconds: u64| seconds.saturating_mul(1000))
}

#[cfg(test)]
mod tests {
    use pl_trace::{
        TraceEvent, TraceEventKind, TracePart, TracePartKind, TracePartSource, TracePartStatus,
        TraceToolPart,
    };
    use pretty_assertions::assert_eq;
    use serde::Deserialize;
    use serde_json::json;

    use super::*;

    #[test]
    fn tool_lifecycle_projection_extracts_tool_events_and_artifacts() {
        let events = vec![
            TraceEvent {
                session_id: "session".to_string(),
                sequence: 1,
                timestamp: 10,
                kind: TraceEventKind::TracePartStarted {
                    item: tool_part(TracePartStatus::Started, None, Vec::new()),
                },
            },
            TraceEvent {
                session_id: "session".to_string(),
                sequence: 2,
                timestamp: 12,
                kind: TraceEventKind::TracePartCompleted {
                    item: tool_part(
                        TracePartStatus::Completed,
                        Some(r#"{"ok":true,"api_key":"secret"}"#),
                        vec![json!({"id": "artifact-1"})],
                    ),
                },
            },
        ];

        let projections = tool_lifecycle_projections(&events, 120);

        assert_eq!(
            projections,
            vec![
                ToolLifecycleProjection {
                    phase: ToolLifecyclePhase::Started,
                    call_id: "call-1".to_string(),
                    tool_name: "exec".to_string(),
                    arguments: json!({"token": "secret", "path": "src"}),
                    arguments_preview: "{\n  \"path\": \"src\",\n  \"token\": \"<redacted>\"\n}"
                        .to_string(),
                    output: String::new(),
                    output_preview: String::new(),
                    output_artifacts: Vec::new(),
                    output_metrics: None,
                    duration_ms: None,
                    started_at_unix: 10,
                    completed_at_unix: None,
                },
                ToolLifecycleProjection {
                    phase: ToolLifecyclePhase::Finished { success: true },
                    call_id: "call-1".to_string(),
                    tool_name: "exec".to_string(),
                    arguments: json!({"token": "secret", "path": "src"}),
                    arguments_preview: "{\n  \"path\": \"src\",\n  \"token\": \"<redacted>\"\n}"
                        .to_string(),
                    output: r#"{"ok":true,"api_key":"secret"}"#.to_string(),
                    output_preview: "{\n  \"api_key\": \"<redacted>\",\n  \"ok\": true\n}"
                        .to_string(),
                    output_artifacts: vec![json!({"id": "artifact-1"})],
                    output_metrics: None,
                    duration_ms: Some(2_000),
                    started_at_unix: 10,
                    completed_at_unix: Some(12),
                },
            ]
        );

        #[derive(Debug, Deserialize, PartialEq, Eq)]
        struct ArtifactRecord {
            id: String,
        }

        assert_eq!(
            projections[1].output_artifacts_as::<ArtifactRecord>(),
            vec![ArtifactRecord {
                id: "artifact-1".to_string(),
            }]
        );
        assert_eq!(projections[0].completed_at_unix_or_started(), 10);
        assert_eq!(projections[1].completed_at_unix_or_started(), 12);
    }

    #[test]
    fn tool_history_projection_recovers_call_arguments_and_output() {
        let messages = vec![
            tool_call_message(
                "provider-call",
                Some("call-1"),
                "exec",
                json!({"command": "pwd", "token": "secret"}),
            ),
            tool_result_message(
                "provider-call",
                Some("call-1"),
                "exec",
                r#"{"command":"pwd","token":"secret"}"#,
                r#"{"status":0,"stdout":"/workspace\n","stderr":""}"#,
            ),
        ];

        let projection =
            tool_history_projection(&messages, "call-1", 160).expect("history projection");

        assert_eq!(
            projection,
            ToolHistoryProjection {
                call_id: "call-1".to_string(),
                tool_name: "exec".to_string(),
                arguments: json!({"command": "pwd", "token": "secret"}),
                arguments_preview: "{\n  \"command\": \"pwd\",\n  \"token\": \"<redacted>\"\n}"
                    .to_string(),
                output: r#"{"status":0,"stdout":"/workspace\n","stderr":""}"#.to_string(),
                output_preview:
                    "{\n  \"status\": 0,\n  \"stderr\": \"\",\n  \"stdout\": \"/workspace\\n\"\n}"
                        .to_string(),
            }
        );
    }

    #[test]
    fn tool_history_projection_reports_inferred_success() {
        let successful = ToolHistoryProjection {
            call_id: "call-1".to_string(),
            tool_name: "exec".to_string(),
            arguments: json!({}),
            arguments_preview: "{}".to_string(),
            output: "visible output".to_string(),
            output_preview: "visible output".to_string(),
        };
        let empty = ToolHistoryProjection {
            output: String::new(),
            output_preview: String::new(),
            ..successful.clone()
        };

        assert!(successful.inferred_success());
        assert!(!empty.inferred_success());
    }

    fn tool_part(
        status: TracePartStatus,
        result: Option<&str>,
        output_artifacts: Vec<serde_json::Value>,
    ) -> TracePart {
        TracePart {
            turn_id: "turn".to_string(),
            item_id: "item".to_string(),
            started_sequence: 1,
            revision: 0,
            kind: TracePartKind::Tool,
            status,
            created_at: 10,
            updated_at: 12,
            source: TracePartSource::Runtime,
            text_channel: None,
            content: String::new(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            reasoning_content_chunks: Vec::new(),
            tool: Some(TraceToolPart {
                tool_call_id: "trace-call".to_string(),
                call_id: Some("call-1".to_string()),
                provider_item_id: None,
                name: "exec".to_string(),
                arguments: r#"{"token":"secret","path":"src"}"#.to_string(),
                result: result.map(ToString::to_string),
                exit_code: None,
                timed_out: false,
                output_artifacts,
                audit_metadata: Vec::new(),
                output_metrics: None,
                working_directory: None,
                denial_reason: None,
            }),
            agent: None,
            inference: None,
            usage: None,
        }
    }

    fn tool_call_message(
        id: &str,
        call_id: Option<&str>,
        name: &str,
        arguments: serde_json::Value,
    ) -> pl_protocol::Message {
        let tool_calls = vec![pl_model::ToolCall::function(
            id,
            name,
            arguments,
            call_id.unwrap_or(id),
        )];
        let mut metadata = Default::default();
        pl_protocol::ToolCallHistoryMetadata::new(
            serde_json::to_string(&tool_calls).expect("tool calls json"),
        )
        .insert_into(&mut metadata);
        pl_protocol::Message {
            role: pl_protocol::MessageRole::Assistant,
            content: pl_protocol::MessageContent::Text(String::new()),
            reasoning_content: None,
            metadata,
        }
    }

    fn tool_result_message(
        id: &str,
        call_id: Option<&str>,
        name: &str,
        raw_arguments: &str,
        output: &str,
    ) -> pl_protocol::Message {
        let mut metadata = Default::default();
        pl_protocol::ToolResultMetadata::new(
            id.to_string(),
            call_id.map(ToString::to_string),
            name.to_string(),
            pl_protocol::ToolCallKind::Function,
            raw_arguments.to_string(),
        )
        .insert_into(&mut metadata);
        pl_protocol::Message {
            role: pl_protocol::MessageRole::Tool,
            content: pl_protocol::MessageContent::Text(output.to_string()),
            reasoning_content: None,
            metadata,
        }
    }
}
