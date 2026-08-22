use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::redaction::{trace_preview_output, trace_preview_value};

mod state;

pub use state::*;

/// 从 pl-core trace 中抽出的工具生命周期通用视图。
///
/// 产品层可以把它映射到自身的 store、Web 事件或日志格式；pl-core 负责统一
/// call id、参数 JSON、预览截断、输出 artifact 和耗时计算。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolLifecycleProjection {
    call_id: String,
    tool_name: String,
    arguments: Value,
    arguments_preview: String,
    started_at_unix: i64,
    state: ToolLifecycleState,
}

impl ToolLifecycleProjection {
    pub fn state(&self) -> &ToolLifecycleState {
        &self.state
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

    pub fn output(&self) -> Option<&str> {
        self.state.output()
    }

    pub fn output_preview(&self) -> Option<&str> {
        self.state.output_preview()
    }

    pub fn output_artifacts(&self) -> &[Value] {
        self.state.output_artifacts()
    }

    pub fn output_metrics(&self) -> Option<&pl_trace::TraceToolOutputMetrics> {
        self.state.output_metrics()
    }

    pub fn duration_ms(&self) -> Option<u64> {
        self.state.duration_ms()
    }

    pub fn started_at_unix(&self) -> i64 {
        self.started_at_unix
    }

    pub fn completed_at_unix(&self) -> Option<i64> {
        self.state.completed_at_unix()
    }

    /// 返回工具完成时间；缺失时回退到开始时间。
    pub fn completed_at_unix_or_started(&self) -> i64 {
        self.completed_at_unix().unwrap_or(self.started_at_unix)
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
        self.output_artifacts()
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
        for tool_call in message.tool_calls.iter().flatten() {
            if tool_call_matches(tool_call, call_id) {
                if tool_name.is_none() {
                    tool_name = Some(tool_call.name.clone());
                }
                if arguments.is_none() {
                    arguments = Some(tool_call.arguments.clone());
                }
            }
        }

        if message.role == pl_protocol::MessageRole::Tool
            && let Some(record) = &message.tool_result
            && tool_result_matches(record, call_id)
        {
            if tool_name.is_none() && !record.name.is_empty() {
                tool_name = Some(record.name.clone());
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
            projection_from_trace_part(item, preview_chars)
        }
        pl_trace::TraceEventKind::TracePartCompleted { item }
        | pl_trace::TraceEventKind::TracePartFailed { item } => {
            projection_from_trace_part(item, preview_chars)
        }
        pl_trace::TraceEventKind::TracePartDelta { event: _event } => None,
        pl_trace::TraceEventKind::InteractionChanged { event: _event } => None,
        pl_trace::TraceEventKind::EnabledToolsRecorded { event: _event } => None,
        pl_trace::TraceEventKind::SkillActivated {
            activation: _activation,
        } => None,
    }
}

fn projection_from_trace_part(
    item: &pl_trace::TracePart,
    preview_chars: usize,
) -> Option<ToolLifecycleProjection> {
    let tool = item.tool()?;
    let invocation = tool.invocation();
    let arguments = arguments_value(invocation.arguments());
    let arguments_preview = trace_preview_value(&arguments, preview_chars);
    let state = match tool.state() {
        pl_trace::TraceToolState::Started(_) | pl_trace::TraceToolState::Streaming(_) => {
            ToolLifecycleState::Started(StartedToolLifecycle {})
        }
        pl_trace::TraceToolState::AwaitingApproval(_)
        | pl_trace::TraceToolState::Approved(_)
        | pl_trace::TraceToolState::Running(_) => {
            ToolLifecycleState::Running(RunningToolLifecycle {})
        }
        pl_trace::TraceToolState::Succeeded(state) => {
            let output = state.output().result().to_string();
            ToolLifecycleState::Succeeded(SucceededToolLifecycle {
                output_preview: trace_preview_output(&output, preview_chars),
                output,
                output_artifacts: state.output().output_artifacts().to_vec(),
                output_metrics: state.output().metrics().cloned(),
                completed_at_unix: item.updated_at(),
                duration_ms: duration_ms(item.created_at(), item.updated_at()),
            })
        }
        pl_trace::TraceToolState::Failed(state) => {
            let output = state.output().map_or_else(
                || state.failure().message().to_string(),
                |output| output.result().to_string(),
            );
            ToolLifecycleState::Failed(FailedToolLifecycle {
                output_preview: trace_preview_output(&output, preview_chars),
                output,
                output_artifacts: state
                    .output()
                    .map_or_else(Vec::new, |output| output.output_artifacts().to_vec()),
                output_metrics: state.output().and_then(|output| output.metrics()).cloned(),
                completed_at_unix: item.updated_at(),
                duration_ms: duration_ms(item.created_at(), item.updated_at()),
            })
        }
        pl_trace::TraceToolState::Denied(state) => {
            let reason = state.reason().to_string();
            ToolLifecycleState::Denied(DeniedToolLifecycle {
                reason_preview: trace_preview_output(&reason, preview_chars),
                reason,
                completed_at_unix: item.updated_at(),
                duration_ms: duration_ms(item.created_at(), item.updated_at()),
            })
        }
        pl_trace::TraceToolState::Cancelled(state) => {
            let cause = format!("{:?}", state.cause());
            ToolLifecycleState::Cancelled(CancelledToolLifecycle {
                cause_preview: trace_preview_output(&cause, preview_chars),
                cause,
                completed_at_unix: item.updated_at(),
                duration_ms: duration_ms(item.created_at(), item.updated_at()),
            })
        }
    };
    Some(ToolLifecycleProjection {
        call_id: tool_call_id(tool),
        tool_name: invocation.name().to_string(),
        arguments,
        arguments_preview,
        started_at_unix: item.created_at(),
        state,
    })
}

fn arguments_value(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|error| {
        let _error = error;
        json!(arguments)
    })
}

fn tool_call_matches(tool_call: &pl_protocol::ToolCallRecord, call_id: &str) -> bool {
    tool_call.call_id == call_id || tool_call.item_id == call_id
}

fn tool_result_matches(record: &pl_protocol::ToolResultRecord, call_id: &str) -> bool {
    record.call_id == call_id || record.item_id == call_id
}

fn tool_call_id(tool: &pl_trace::TraceToolPart) -> String {
    tool.invocation()
        .call_id()
        .unwrap_or_else(|| tool.invocation().tool_call_id())
        .to_string()
}

fn duration_ms(created_at: i64, updated_at: i64) -> u64 {
    updated_at
        .saturating_sub(created_at)
        .try_into()
        .unwrap_or(0_u64)
        .saturating_mul(1000)
}

#[cfg(test)]
mod tests {
    use pl_trace::{
        TraceEvent, TraceEventKind, TracePart, TracePartAction, TracePartCommand,
        TracePartCompletion, TracePartSource, TracePartState, TraceToolInvocation, TraceToolOutput,
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
                    item: tool_part(None, Vec::new()),
                },
            },
            TraceEvent {
                session_id: "session".to_string(),
                sequence: 2,
                timestamp: 12,
                kind: TraceEventKind::TracePartCompleted {
                    item: tool_part(
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
                    call_id: "call-1".to_string(),
                    tool_name: "exec".to_string(),
                    arguments: json!({"token": "secret", "path": "src"}),
                    arguments_preview: "{\n  \"path\": \"src\",\n  \"token\": \"<redacted>\"\n}"
                        .to_string(),
                    started_at_unix: 10,
                    state: ToolLifecycleState::Started(StartedToolLifecycle {}),
                },
                ToolLifecycleProjection {
                    call_id: "call-1".to_string(),
                    tool_name: "exec".to_string(),
                    arguments: json!({"token": "secret", "path": "src"}),
                    arguments_preview: "{\n  \"path\": \"src\",\n  \"token\": \"<redacted>\"\n}"
                        .to_string(),
                    started_at_unix: 10,
                    state: ToolLifecycleState::Succeeded(SucceededToolLifecycle {
                        output: r#"{"ok":true,"api_key":"secret"}"#.to_string(),
                        output_preview: "{\n  \"api_key\": \"<redacted>\",\n  \"ok\": true\n}"
                            .to_string(),
                        output_artifacts: vec![json!({"id": "artifact-1"})],
                        output_metrics: None,
                        duration_ms: 2_000,
                        completed_at_unix: 12,
                    }),
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

    fn tool_part(result: Option<&str>, output_artifacts: Vec<serde_json::Value>) -> TracePart {
        let invocation = TraceToolInvocation::new(
            "trace-call".to_string(),
            "exec".to_string(),
            r#"{"token":"secret","path":"src"}"#.to_string(),
        )
        .with_provider_identity(Some("call-1".to_string()), None);
        let mut item = TracePart::new(
            "turn".to_string(),
            "item".to_string(),
            1,
            10,
            TracePartSource::Runtime,
            TracePartState::Tool(TraceToolPart::started(invocation)),
        );
        if let Some(result) = result {
            let output = TraceToolOutput::new(result.to_string()).with_details(
                None,
                output_artifacts,
                Vec::new(),
                None,
            );
            let command = TracePartCommand {
                item_id: item.item_id().to_string(),
                expected_revision: item.revision(),
                updated_at: 12,
                action: TracePartAction::Complete(TracePartCompletion::Tool { output }),
            };
            item.apply(command).expect("valid completed tool part");
        }
        item
    }

    fn tool_call_message(
        id: &str,
        call_id: Option<&str>,
        name: &str,
        arguments: serde_json::Value,
    ) -> pl_protocol::Message {
        pl_protocol::Message {
            role: pl_protocol::MessageRole::Assistant,
            content: pl_protocol::MessageContent::Text(String::new()),
            reasoning_content: None,
            tool_calls: Some(vec![pl_protocol::ToolCallRecord {
                item_id: id.to_string(),
                call_id: call_id.unwrap_or(id).to_string(),
                name: name.to_string(),
                kind: pl_protocol::ToolCallKind::Function,
                arguments,
                caller: None,
            }]),
            tool_result: None,
            metadata: Default::default(),
        }
    }

    fn tool_result_message(
        id: &str,
        call_id: Option<&str>,
        name: &str,
        _raw_arguments: &str,
        output: &str,
    ) -> pl_protocol::Message {
        pl_protocol::Message {
            role: pl_protocol::MessageRole::Tool,
            content: pl_protocol::MessageContent::Text(output.to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_result: Some(pl_protocol::ToolResultRecord {
                item_id: id.to_string(),
                call_id: call_id.unwrap_or(id).to_string(),
                name: name.to_string(),
                kind: pl_protocol::ToolCallKind::Function,
            }),
            metadata: Default::default(),
        }
    }
}
