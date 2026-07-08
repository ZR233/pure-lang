use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

const TOKEN_ESTIMATE_BYTES: usize = 4;

pub const DEFAULT_MODEL_TOOL_OUTPUT_TOKENS: usize = 10_000;

pub fn model_visible_tool_output(output: &str) -> String {
    model_visible_tool_output_with_tokens(output, DEFAULT_MODEL_TOOL_OUTPUT_TOKENS)
}

pub fn model_visible_tool_output_with_tokens(output: &str, max_output_tokens: usize) -> String {
    let max_bytes = max_output_tokens
        .saturating_mul(TOKEN_ESTIMATE_BYTES)
        .max(1);
    if output.len() <= max_bytes {
        return output.to_string();
    }
    if let Ok(value) = serde_json::from_str::<Value>(output) {
        return bounded_json_tool_output(value, max_bytes).to_string();
    }
    let (text, truncated, bytes_omitted, next_offset) = bounded_text(output, max_bytes, 0);
    json!({
        "truncated": truncated,
        "bytesReturned": text.len(),
        "bytesOmitted": bytes_omitted,
        "nextOffset": next_offset,
        "text": text,
    })
    .to_string()
}

pub fn trace_preview_value(value: &Value, max: usize) -> String {
    let redacted = redacted_trace_preview_value(value);
    let serialized =
        serde_json::to_string_pretty(&redacted).unwrap_or_else(|_| redacted.to_string());
    preview(&serialized, max)
}

pub fn trace_preview_output(output: &str, max: usize) -> String {
    serde_json::from_str::<Value>(output)
        .map(|value| trace_preview_value(&value, max))
        .unwrap_or_else(|_| preview(&redact_preview_string(output), max))
}

pub fn redacted_trace_preview_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                if is_sensitive_key(key) {
                    out.insert(key.clone(), Value::String("<redacted>".to_string()));
                } else {
                    out.insert(key.clone(), redacted_trace_preview_value(value));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(20)
                .map(redacted_trace_preview_value)
                .chain(
                    (items.len() > 20)
                        .then(|| Value::String(format!("<{} more items>", items.len() - 20))),
                )
                .collect(),
        ),
        Value::String(value) => Value::String(redact_preview_string(value)),
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
    }
}

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
    pub phase: ToolLifecyclePhase,
    pub call_id: String,
    pub tool_name: String,
    pub arguments: Value,
    pub arguments_preview: String,
    pub output: String,
    pub output_preview: String,
    pub output_artifacts: Vec<Value>,
    pub duration_ms: Option<u64>,
    pub started_at_unix: i64,
    pub completed_at_unix: Option<i64>,
}

impl ToolLifecycleProjection {
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
    pub call_id: String,
    pub tool_name: String,
    pub arguments: Value,
    pub arguments_preview: String,
    pub output: String,
    pub output_preview: String,
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
            output = Some(message_content_text(&message.content));
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
        Some(_other) => payload.get("arguments").cloned(),
        None => payload.get("arguments").cloned(),
    }
}

fn message_content_text(content: &pl_protocol::MessageContent) -> String {
    match content {
        pl_protocol::MessageContent::Text(text) => text.clone(),
        pl_protocol::MessageContent::MultiPart(parts) => parts
            .iter()
            .filter_map(|part| match part {
                pl_protocol::ContentPart::Text { text } => Some(text.as_str()),
                pl_protocol::ContentPart::Image {
                    source: _source,
                    media_type: _media_type,
                    filename: _filename,
                } => None,
            })
            .collect::<Vec<_>>()
            .join(""),
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

/// 为捕获工具 stdout/stderr 输出准备文件路径。
///
/// 宿主负责生成调用 id 与 artifact id；pl-core 只统一安全文件名、目录布局和
/// 空流清理语义，避免本地/容器 backend 各自维护一套路径协议。
#[derive(Debug, Clone, Copy)]
pub struct ToolOutputCaptureRequest<'a> {
    pub artifact_files_root: &'a Path,
    pub namespace: Option<&'a str>,
    pub call_id: &'a str,
    pub stdout_id: &'a str,
    pub stderr_id: &'a str,
    pub command: &'a str,
}

/// 工具输出 artifact 的路径计算请求。
#[derive(Debug, Clone, Copy)]
pub struct ToolOutputArtifactPathRequest<'a> {
    pub artifact_files_root: &'a Path,
    pub namespace: Option<&'a str>,
    pub call_id: &'a str,
    pub artifact_id: &'a str,
    pub name: &'a str,
}

/// stdout/stderr 捕获文件集合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutputCapture {
    pub call_id: String,
    pub stdout: ToolOutputStreamCapture,
    pub stderr: ToolOutputStreamCapture,
}

/// 单个输出流的捕获文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutputStreamCapture {
    pub id: String,
    pub name: String,
    pub stream: ToolOutputStream,
    pub path: PathBuf,
}

/// 工具输出流名称。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolOutputStream {
    Stdout,
    Stderr,
}

impl ToolOutputStream {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

/// 工具输出流的实际写入字节数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolOutputStreamSizes {
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
}

/// 与产品无关的工具输出 artifact 描述。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutputArtifactDescriptor {
    pub id: String,
    pub call_id: String,
    pub name: String,
    pub stream: ToolOutputStream,
    pub path: PathBuf,
    pub size_bytes: u64,
}

impl ToolOutputCapture {
    pub async fn prepare(request: ToolOutputCaptureRequest<'_>) -> crate::Result<Self> {
        let stdout_name = tool_output_file_name(request.command, ToolOutputStream::Stdout);
        let stderr_name = tool_output_file_name(request.command, ToolOutputStream::Stderr);
        let stdout_path = tool_output_artifact_file_path(ToolOutputArtifactPathRequest {
            artifact_files_root: request.artifact_files_root,
            namespace: request.namespace,
            call_id: request.call_id,
            artifact_id: request.stdout_id,
            name: &stdout_name,
        });
        let stderr_path = tool_output_artifact_file_path(ToolOutputArtifactPathRequest {
            artifact_files_root: request.artifact_files_root,
            namespace: request.namespace,
            call_id: request.call_id,
            artifact_id: request.stderr_id,
            name: &stderr_name,
        });
        if let Some(parent) = stdout_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if let Some(parent) = stderr_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        Ok(Self {
            call_id: request.call_id.to_string(),
            stdout: ToolOutputStreamCapture {
                id: request.stdout_id.to_string(),
                name: stdout_name,
                stream: ToolOutputStream::Stdout,
                path: stdout_path,
            },
            stderr: ToolOutputStreamCapture {
                id: request.stderr_id.to_string(),
                name: stderr_name,
                stream: ToolOutputStream::Stderr,
                path: stderr_path,
            },
        })
    }

    pub async fn collect_artifacts(
        &self,
        sizes: ToolOutputStreamSizes,
    ) -> crate::Result<Vec<ToolOutputArtifactDescriptor>> {
        let mut artifacts = Vec::new();
        push_or_remove_artifact(
            &mut artifacts,
            &self.call_id,
            &self.stdout,
            sizes.stdout_bytes,
        )
        .await?;
        push_or_remove_artifact(
            &mut artifacts,
            &self.call_id,
            &self.stderr,
            sizes.stderr_bytes,
        )
        .await?;
        Ok(artifacts)
    }
}

pub fn tool_output_artifact_file_path(request: ToolOutputArtifactPathRequest<'_>) -> PathBuf {
    let mut dir = request.artifact_files_root.join("tool-output");
    if let Some(namespace) = request
        .namespace
        .map(safe_path_component)
        .filter(|value| !value.is_empty())
    {
        dir = dir.join(namespace);
    }
    dir.join(safe_path_component_or(request.call_id, "tool-call"))
        .join(safe_path_component_or(request.artifact_id, "artifact"))
        .join(safe_path_component_or(request.name, "output.txt"))
}

async fn push_or_remove_artifact(
    artifacts: &mut Vec<ToolOutputArtifactDescriptor>,
    call_id: &str,
    capture: &ToolOutputStreamCapture,
    size_bytes: u64,
) -> crate::Result<()> {
    if size_bytes > 0 {
        artifacts.push(ToolOutputArtifactDescriptor {
            id: capture.id.clone(),
            call_id: call_id.to_string(),
            name: capture.name.clone(),
            stream: capture.stream,
            path: capture.path.clone(),
            size_bytes,
        });
    } else {
        let _ = tokio::fs::remove_file(&capture.path).await;
    }
    Ok(())
}

fn tool_output_file_name(command: &str, stream: ToolOutputStream) -> String {
    let command = command
        .split_whitespace()
        .next()
        .map(safe_path_component)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "command".to_string());
    format!("{command}-{}.txt", stream.as_str())
}

fn safe_path_component(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('.')
        .trim_matches('_')
        .to_string()
}

fn safe_path_component_or(raw: &str, fallback: &str) -> String {
    let safe = safe_path_component(raw);
    if safe.is_empty() {
        fallback.to_string()
    } else {
        safe
    }
}

fn bounded_json_tool_output(mut value: Value, max_bytes: usize) -> Value {
    match &mut value {
        Value::Object(map) => {
            for key in [
                "stdout",
                "stderr",
                "body",
                "text",
                "tarBase64",
                "contentBase64",
            ] {
                if let Some(Value::String(text)) = map.get_mut(key) {
                    let (bounded, truncated, bytes_omitted, next_offset) =
                        bounded_text(text, max_bytes, 0);
                    if truncated {
                        let bytes_returned = bounded.len();
                        *text = bounded;
                        map.insert("truncated".to_string(), Value::Bool(true));
                        map.insert("bytesReturned".to_string(), json!(bytes_returned));
                        map.insert("bytesOmitted".to_string(), json!(bytes_omitted));
                        map.insert("nextOffset".to_string(), json!(next_offset));
                        break;
                    }
                }
            }
            if value.to_string().len() > max_bytes {
                json_preview(value, max_bytes)
            } else {
                value
            }
        }
        Value::Array(_) | Value::String(_) | Value::Bool(_) | Value::Number(_) | Value::Null => {
            let serialized = value.to_string();
            if serialized.len() <= max_bytes {
                value
            } else {
                json_preview(Value::String(serialized), max_bytes)
            }
        }
    }
}

fn json_preview(value: Value, max_bytes: usize) -> Value {
    let serialized = match value {
        Value::String(text) => text,
        other => other.to_string(),
    };
    let (text, _, bytes_omitted, next_offset) = bounded_text(&serialized, max_bytes, 0);
    json!({
        "truncated": true,
        "bytesReturned": text.len(),
        "bytesOmitted": bytes_omitted,
        "nextOffset": next_offset,
        "jsonPreview": text,
    })
}

fn bounded_text(
    value: &str,
    max_bytes: usize,
    offset: usize,
) -> (String, bool, usize, Option<usize>) {
    if value.len() <= max_bytes {
        return (value.to_string(), false, 0, None);
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let text = value[..end].to_string();
    let omitted = value.len().saturating_sub(end);
    (text, true, omitted, Some(offset.saturating_add(end)))
}

fn preview(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &value[..end])
}

fn redact_preview_string(value: &str) -> String {
    if value.len() > 240 && looks_like_base64(value) {
        return format!("<base64 elided: {} chars>", value.len());
    }
    if value.len() > 800 {
        return format!("{}...", value.chars().take(800).collect::<String>());
    }
    value.to_string()
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("authorization")
        || key.contains("api_key")
        || key.ends_with("_key")
        || key.contains("base64")
}

fn looks_like_base64(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() > 240
        && trimmed.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=' | b'\n' | b'\r')
        })
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
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn tool_output_capture_keeps_non_empty_streams_and_removes_empty_files() {
        let dir = test_temp_dir();
        let capture = super::ToolOutputCapture::prepare(super::ToolOutputCaptureRequest {
            artifact_files_root: &dir,
            namespace: None,
            call_id: "call/id",
            stdout_id: "stdout-id",
            stderr_id: "stderr-id",
            command: "cargo test",
        })
        .await
        .expect("capture");
        tokio::fs::write(&capture.stdout.path, b"ok")
            .await
            .expect("stdout");
        tokio::fs::write(&capture.stderr.path, b"")
            .await
            .expect("stderr");

        let artifacts = capture
            .collect_artifacts(super::ToolOutputStreamSizes {
                stdout_bytes: 2,
                stderr_bytes: 0,
            })
            .await
            .expect("artifacts");

        assert_eq!(
            artifacts,
            vec![super::ToolOutputArtifactDescriptor {
                id: "stdout-id".to_string(),
                call_id: "call/id".to_string(),
                name: "cargo-stdout.txt".to_string(),
                stream: super::ToolOutputStream::Stdout,
                path: dir
                    .join("tool-output")
                    .join("call_id")
                    .join("stdout-id")
                    .join("cargo-stdout.txt"),
                size_bytes: 2,
            }]
        );
        assert!(capture.stdout.path.exists());
        assert!(!capture.stderr.path.exists());
        assert_eq!(
            super::tool_output_artifact_file_path(super::ToolOutputArtifactPathRequest {
                artifact_files_root: &dir,
                namespace: Some("agent/id"),
                call_id: "call/id",
                artifact_id: "artifact-id",
                name: "cargo-stdout.txt",
            }),
            dir.join("tool-output")
                .join("agent_id")
                .join("call_id")
                .join("artifact-id")
                .join("cargo-stdout.txt")
        );
        assert_eq!(
            super::tool_output_artifact_file_path(super::ToolOutputArtifactPathRequest {
                artifact_files_root: &dir,
                namespace: Some("../agent"),
                call_id: "...",
                artifact_id: "artifact/id",
                name: "../stdout.txt",
            }),
            dir.join("tool-output")
                .join("agent")
                .join("tool-call")
                .join("artifact_id")
                .join("stdout.txt")
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

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

        let projections = super::tool_lifecycle_projections(&events, 120);

        assert_eq!(
            projections,
            vec![
                super::ToolLifecycleProjection {
                    phase: super::ToolLifecyclePhase::Started,
                    call_id: "call-1".to_string(),
                    tool_name: "container_exec".to_string(),
                    arguments: json!({"token": "secret", "path": "src"}),
                    arguments_preview: "{\n  \"path\": \"src\",\n  \"token\": \"<redacted>\"\n}"
                        .to_string(),
                    output: String::new(),
                    output_preview: String::new(),
                    output_artifacts: Vec::new(),
                    duration_ms: None,
                    started_at_unix: 10,
                    completed_at_unix: None,
                },
                super::ToolLifecycleProjection {
                    phase: super::ToolLifecyclePhase::Finished { success: true },
                    call_id: "call-1".to_string(),
                    tool_name: "container_exec".to_string(),
                    arguments: json!({"token": "secret", "path": "src"}),
                    arguments_preview: "{\n  \"path\": \"src\",\n  \"token\": \"<redacted>\"\n}"
                        .to_string(),
                    output: r#"{"ok":true,"api_key":"secret"}"#.to_string(),
                    output_preview: "{\n  \"api_key\": \"<redacted>\",\n  \"ok\": true\n}"
                        .to_string(),
                    output_artifacts: vec![json!({"id": "artifact-1"})],
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
    }

    #[test]
    fn tool_history_projection_recovers_call_arguments_and_output() {
        let messages = vec![
            tool_call_message(
                "provider-call",
                Some("call-1"),
                "container_exec",
                json!({"command": "pwd", "token": "secret"}),
            ),
            tool_result_message(
                "provider-call",
                Some("call-1"),
                "container_exec",
                r#"{"command":"pwd","token":"secret"}"#,
                r#"{"status":0,"stdout":"/workspace\n","stderr":""}"#,
            ),
        ];

        let projection =
            super::tool_history_projection(&messages, "call-1", 160).expect("history projection");

        assert_eq!(
            projection,
            super::ToolHistoryProjection {
                call_id: "call-1".to_string(),
                tool_name: "container_exec".to_string(),
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

    fn test_temp_dir() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "pl-core-output-capture-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
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
            tool: Some(TraceToolPart {
                tool_call_id: "trace-call".to_string(),
                call_id: Some("call-1".to_string()),
                provider_item_id: None,
                name: "container_exec".to_string(),
                arguments: r#"{"token":"secret","path":"src"}"#.to_string(),
                result: result.map(ToString::to_string),
                exit_code: None,
                timed_out: false,
                output_artifacts,
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
            call_id.map(ToString::to_string),
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
