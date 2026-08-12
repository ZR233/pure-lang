use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 为捕获工具 stdout/stderr 输出准备文件路径。
///
/// 宿主负责生成调用 id 与 artifact id；pl-core 只统一安全文件名、目录布局和
/// 空流清理语义，避免本地/容器 backend 各自维护一套路径协议。
#[derive(Debug, Clone, Copy)]
pub struct ToolOutputCaptureRequest<'a> {
    artifact_files_root: &'a Path,
    namespace: Option<&'a str>,
    call_id: &'a str,
    stdout_id: &'a str,
    stderr_id: &'a str,
    command: &'a str,
}

impl<'a> ToolOutputCaptureRequest<'a> {
    pub fn new(
        artifact_files_root: &'a Path,
        call_id: &'a str,
        stdout_id: &'a str,
        stderr_id: &'a str,
        command: &'a str,
    ) -> Self {
        Self {
            artifact_files_root,
            namespace: None,
            call_id,
            stdout_id,
            stderr_id,
            command,
        }
    }

    pub fn with_namespace(mut self, namespace: &'a str) -> Self {
        self.namespace = Some(namespace);
        self
    }
}

/// 工具输出 artifact 的路径计算请求。
#[derive(Debug, Clone, Copy)]
pub struct ToolOutputArtifactPathRequest<'a> {
    artifact_files_root: &'a Path,
    namespace: Option<&'a str>,
    call_id: &'a str,
    artifact_id: &'a str,
    name: &'a str,
}

impl<'a> ToolOutputArtifactPathRequest<'a> {
    pub fn new(
        artifact_files_root: &'a Path,
        call_id: &'a str,
        artifact_id: &'a str,
        name: &'a str,
    ) -> Self {
        Self {
            artifact_files_root,
            namespace: None,
            call_id,
            artifact_id,
            name,
        }
    }

    pub fn with_namespace(mut self, namespace: &'a str) -> Self {
        self.namespace = Some(namespace);
        self
    }
}

/// stdout/stderr 捕获文件集合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutputCapture {
    call_id: String,
    stdout: ToolOutputStreamCapture,
    stderr: ToolOutputStreamCapture,
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
    stdout_bytes: u64,
    stderr_bytes: u64,
}

impl ToolOutputStreamSizes {
    pub fn new(stdout_bytes: u64, stderr_bytes: u64) -> Self {
        Self {
            stdout_bytes,
            stderr_bytes,
        }
    }

    pub fn stdout_bytes(&self) -> u64 {
        self.stdout_bytes
    }

    pub fn stderr_bytes(&self) -> u64 {
        self.stderr_bytes
    }
}

/// 与产品无关的工具输出 artifact 描述。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutputArtifactDescriptor {
    id: String,
    call_id: String,
    name: String,
    stream: ToolOutputStream,
    path: PathBuf,
    size_bytes: u64,
}

impl ToolOutputArtifactDescriptor {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn stream(&self) -> ToolOutputStream {
        self.stream
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
}

impl ToolOutputCapture {
    pub fn stdout_path(&self) -> &Path {
        &self.stdout.path
    }

    pub fn stderr_path(&self) -> &Path {
        &self.stderr.path
    }

    pub async fn prepare(request: ToolOutputCaptureRequest<'_>) -> crate::Result<Self> {
        let stdout_name = tool_output_file_name(request.command, ToolOutputStream::Stdout);
        let stderr_name = tool_output_file_name(request.command, ToolOutputStream::Stderr);
        let mut stdout_request = ToolOutputArtifactPathRequest::new(
            request.artifact_files_root,
            request.call_id,
            request.stdout_id,
            &stdout_name,
        );
        let mut stderr_request = ToolOutputArtifactPathRequest::new(
            request.artifact_files_root,
            request.call_id,
            request.stderr_id,
            &stderr_name,
        );
        if let Some(namespace) = request.namespace {
            stdout_request = stdout_request.with_namespace(namespace);
            stderr_request = stderr_request.with_namespace(namespace);
        }
        let stdout_path = tool_output_artifact_file_path(stdout_request);
        let stderr_path = tool_output_artifact_file_path(stderr_request);
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
            sizes.stdout_bytes(),
        )
        .await?;
        push_or_remove_artifact(
            &mut artifacts,
            &self.call_id,
            &self.stderr,
            sizes.stderr_bytes(),
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

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use pretty_assertions::assert_eq;

    use super::*;

    #[tokio::test]
    async fn tool_output_capture_keeps_non_empty_streams_and_removes_empty_files() {
        let dir = test_temp_dir();
        let capture = ToolOutputCapture::prepare(ToolOutputCaptureRequest::new(
            &dir,
            "call/id",
            "stdout-id",
            "stderr-id",
            "cargo test",
        ))
        .await
        .expect("capture");
        tokio::fs::write(capture.stdout_path(), b"ok")
            .await
            .expect("stdout");
        tokio::fs::write(capture.stderr_path(), b"")
            .await
            .expect("stderr");

        let artifacts = capture
            .collect_artifacts(ToolOutputStreamSizes::new(2, 0))
            .await
            .expect("artifacts");

        assert_eq!(
            artifacts,
            vec![ToolOutputArtifactDescriptor {
                id: "stdout-id".to_string(),
                call_id: "call/id".to_string(),
                name: "cargo-stdout.txt".to_string(),
                stream: ToolOutputStream::Stdout,
                path: dir
                    .join("tool-output")
                    .join("call_id")
                    .join("stdout-id")
                    .join("cargo-stdout.txt"),
                size_bytes: 2,
            }]
        );
        assert!(capture.stdout_path().exists());
        assert!(!capture.stderr_path().exists());
        assert_eq!(
            tool_output_artifact_file_path(
                ToolOutputArtifactPathRequest::new(
                    &dir,
                    "call/id",
                    "artifact-id",
                    "cargo-stdout.txt"
                )
                .with_namespace("agent/id")
            ),
            dir.join("tool-output")
                .join("agent_id")
                .join("call_id")
                .join("artifact-id")
                .join("cargo-stdout.txt")
        );
        assert_eq!(
            tool_output_artifact_file_path(
                ToolOutputArtifactPathRequest::new(&dir, "...", "artifact/id", "../stdout.txt")
                    .with_namespace("../agent")
            ),
            dir.join("tool-output")
                .join("agent")
                .join("tool-call")
                .join("artifact_id")
                .join("stdout.txt")
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    fn test_temp_dir() -> PathBuf {
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
