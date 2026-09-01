use std::fmt;
use std::future::Future;
use std::sync::Arc;

use pl_protocol::Result;

use crate::tool::ToolWorkspace;

/// workspace 文件工具的统一后端。
///
/// 工具层负责解析统一输入、生成统一 JSON 输出并执行 patch 匹配逻辑；
/// backend 只表达“在某个 workspace 中读、写、删、列”的能力。本地目录、
/// Docker 容器或远程沙箱都应通过实现该 trait 接入，避免为同名 file 工具维护多套协议。
pub trait WorkspaceFileBackend: fmt::Debug + Send + Sync {
    fn default_cwd(&self) -> impl Future<Output = Result<String>> + Send;

    fn stat(
        &self,
        request: WorkspaceFileStatRequest,
    ) -> impl Future<Output = Result<WorkspaceFileStat>> + Send;

    fn read_text(
        &self,
        request: WorkspaceFileReadRequest,
    ) -> impl Future<Output = Result<String>> + Send;

    fn read_bytes(
        &self,
        request: WorkspaceFileReadBytesRequest,
    ) -> impl Future<Output = Result<Vec<u8>>> + Send;

    fn write_text(
        &self,
        request: WorkspaceFileWriteRequest,
    ) -> impl Future<Output = Result<()>> + Send;

    fn remove_file(
        &self,
        request: WorkspaceFileRemoveRequest,
    ) -> impl Future<Output = Result<()>> + Send;

    /// Lists matching descendants without returning the requested directory itself.
    fn list(
        &self,
        request: WorkspaceFileListRequest,
    ) -> impl Future<Output = Result<WorkspaceFileListResult>> + Send;
}

/// 为 host-provided workspace backend 统一施加冻结的 Agent 文件写策略。
#[derive(Debug, Clone)]
pub struct WorkspacePolicyBackend<B> {
    backend: Arc<B>,
    workspace: ToolWorkspace,
}

impl<B> WorkspacePolicyBackend<B> {
    pub fn new(backend: Arc<B>, workspace: ToolWorkspace) -> Self {
        Self { backend, workspace }
    }
}

impl<B> WorkspaceFileBackend for WorkspacePolicyBackend<B>
where
    B: WorkspaceFileBackend,
{
    async fn default_cwd(&self) -> Result<String> {
        self.backend.default_cwd().await
    }

    async fn stat(&self, request: WorkspaceFileStatRequest) -> Result<WorkspaceFileStat> {
        self.backend.stat(request).await
    }

    async fn read_text(&self, request: WorkspaceFileReadRequest) -> Result<String> {
        self.backend.read_text(request).await
    }

    async fn read_bytes(&self, request: WorkspaceFileReadBytesRequest) -> Result<Vec<u8>> {
        self.backend.read_bytes(request).await
    }

    async fn write_text(&self, request: WorkspaceFileWriteRequest) -> Result<()> {
        self.workspace
            .ensure_relative_path_writable(request.cwd.as_deref(), &request.path)?;
        self.backend.write_text(request).await
    }

    async fn remove_file(&self, request: WorkspaceFileRemoveRequest) -> Result<()> {
        self.workspace
            .ensure_relative_path_writable(request.cwd.as_deref(), &request.path)?;
        self.backend.remove_file(request).await
    }

    async fn list(&self, request: WorkspaceFileListRequest) -> Result<WorkspaceFileListResult> {
        self.backend.list(request).await
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileStatRequest {
    pub path: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileStat {
    pub path: String,
    pub is_file: bool,
    pub is_dir: bool,
    pub len: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileReadRequest {
    pub path: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileReadBytesRequest {
    pub path: String,
    pub cwd: Option<String>,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileWriteRequest {
    pub path: String,
    pub cwd: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileRemoveRequest {
    pub path: String,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileListRequest {
    pub path: String,
    pub cwd: Option<String>,
    pub glob: String,
    pub max_files: usize,
    pub include_dirs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileListResult {
    pub files: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceFileListEntry {
    pub path: String,
    pub is_dir: bool,
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::tool::AgentWorkspace;

    #[derive(Debug, Default)]
    struct RecordingBackend {
        writes: Mutex<Vec<String>>,
    }

    impl WorkspaceFileBackend for RecordingBackend {
        async fn default_cwd(&self) -> Result<String> {
            Ok(".".to_string())
        }

        async fn stat(&self, _request: WorkspaceFileStatRequest) -> Result<WorkspaceFileStat> {
            unreachable!("policy regression does not read")
        }

        async fn read_text(&self, _request: WorkspaceFileReadRequest) -> Result<String> {
            unreachable!("policy regression does not read")
        }

        async fn read_bytes(&self, _request: WorkspaceFileReadBytesRequest) -> Result<Vec<u8>> {
            unreachable!("policy regression does not read")
        }

        async fn write_text(&self, request: WorkspaceFileWriteRequest) -> Result<()> {
            self.writes.lock().unwrap().push(request.path);
            Ok(())
        }

        async fn remove_file(&self, _request: WorkspaceFileRemoveRequest) -> Result<()> {
            unreachable!("policy regression does not remove")
        }

        async fn list(
            &self,
            _request: WorkspaceFileListRequest,
        ) -> Result<WorkspaceFileListResult> {
            unreachable!("policy regression does not list")
        }
    }

    #[tokio::test]
    async fn host_backend_preserves_directory_policy_independently_of_session_permissions() {
        let root = std::path::PathBuf::from("/remote/project");
        let backend = Arc::new(RecordingBackend::default());
        let restricted = WorkspacePolicyBackend::new(
            backend.clone(),
            ToolWorkspace::new(AgentWorkspace::directory(
                root.clone(),
                Some(vec![root.join("allowed")]),
            )),
        );

        restricted
            .write_text(WorkspaceFileWriteRequest {
                path: "ok.txt".to_string(),
                cwd: Some("allowed".to_string()),
                content: "ok".to_string(),
            })
            .await
            .expect("allowed host-backed write");
        let denied = restricted
            .write_text(WorkspaceFileWriteRequest {
                path: "denied.txt".to_string(),
                cwd: None,
                content: "denied".to_string(),
            })
            .await
            .expect_err("directory policy must reject an out-of-scope host-backed write");
        let empty = WorkspacePolicyBackend::new(
            backend.clone(),
            ToolWorkspace::new(AgentWorkspace::directory(root.clone(), Some(Vec::new()))),
        )
        .write_text(WorkspaceFileWriteRequest {
            path: "also-denied.txt".to_string(),
            cwd: None,
            content: "denied".to_string(),
        })
        .await
        .expect_err("empty writablePaths must keep a host-backed project read-only");
        WorkspacePolicyBackend::new(
            backend.clone(),
            ToolWorkspace::new(AgentWorkspace::local(root)),
        )
        .write_text(WorkspaceFileWriteRequest {
            path: "unrestricted.txt".to_string(),
            cwd: None,
            content: "ok".to_string(),
        })
        .await
        .expect("unrestricted host-backed write");

        assert!(denied.to_string().contains("writablePaths"));
        assert!(empty.to_string().contains("writablePaths"));
        assert_eq!(
            *backend.writes.lock().unwrap(),
            vec!["ok.txt".to_string(), "unrestricted.txt".to_string()]
        );
    }
}
