use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
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
    use pretty_assertions::assert_eq;
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
}
