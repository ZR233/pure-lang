use std::collections::VecDeque;
use std::path::PathBuf;

use pl_protocol::Result;
use pl_protocol::remote::{
    RemoteCopyRequest, RemoteDirectoryListing, RemotePathRequest, RemoteReadRequest,
    RemoteRemoveRequest, RemoteRenameRequest, RemoteRequest, RemoteResponse,
};

use crate::tool::{
    WorkspaceFileBackend, WorkspaceFileListRequest, WorkspaceFileListResult,
    WorkspaceFileReadBytesRequest, WorkspaceFileReadRequest, WorkspaceFileRemoveRequest,
    WorkspaceFileStat, WorkspaceFileStatRequest, WorkspaceFileWriteRequest, matches_pattern,
    tool_error,
};
use crate::workspace::{WorkspaceInstructionDocument, WorkspaceInstructions};

use super::client::{RemoteClient, RemoteClientError, expect_ack};

const DEFAULT_INSTRUCTION_FILENAMES: &[&str] = &["AGENTS.override.md", "AGENTS.md", "Agents.md"];

/// 将现有 workspace 文件端口映射到远端 helper 原语的 backend。
#[derive(Debug, Clone)]
pub struct RemoteWorkspaceFileBackend {
    client: RemoteClient,
    workspace_id: String,
    canonical_path: String,
}

impl RemoteWorkspaceFileBackend {
    pub(crate) fn new(client: RemoteClient, workspace_id: String, canonical_path: String) -> Self {
        Self {
            client,
            workspace_id,
            canonical_path,
        }
    }

    /// 返回当前 SSH channel 内的 opaque workspace id。
    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    /// 返回 helper 依据远端文件系统解析出的 canonical POSIX 根路径。
    pub fn canonical_path(&self) -> &str {
        &self.canonical_path
    }

    pub(crate) fn client(&self) -> &RemoteClient {
        &self.client
    }

    fn path_request(&self, path: String, cwd: Option<String>) -> Result<RemotePathRequest> {
        Ok(RemotePathRequest {
            workspace_id: self.workspace_id.clone(),
            path: resolve_remote_path(path, cwd)?,
        })
    }

    /// 读取目录的一层 typed entry，供本地 Skills/LSP 等编排消费。
    pub async fn list_directory_entries(&self, path: String) -> Result<RemoteDirectoryListing> {
        let reply = self
            .client
            .request(
                RemoteRequest::ListDirectory(RemotePathRequest {
                    workspace_id: self.workspace_id.clone(),
                    path,
                }),
                &[],
            )
            .await
            .map_err(remote_file_error)?;
        match reply.response {
            RemoteResponse::Directory(listing) => Ok(listing),
            response => Err(tool_error(
                "file",
                format!("unexpected remote directory response: {response:?}"),
            )),
        }
    }

    /// 读取路径元数据；不存在时返回 `None`。
    pub async fn stat_optional(
        &self,
        path: String,
        cwd: Option<String>,
    ) -> Result<Option<WorkspaceFileStat>> {
        let path = self.path_request(path, cwd)?;
        let reply = self.client.request(RemoteRequest::Stat(path), &[]).await;
        match reply {
            Ok(reply) => match reply.response {
                RemoteResponse::Stat(stat) => Ok(Some(WorkspaceFileStat {
                    path: stat.path,
                    is_file: stat.is_file,
                    is_dir: stat.is_directory,
                    len: stat.len,
                })),
                response => Err(tool_error(
                    "stat_path",
                    format!("unexpected remote stat response: {response:?}"),
                )),
            },
            Err(RemoteClientError::Remote {
                code: pl_protocol::remote::RemoteErrorCode::PathNotFound,
                ..
            }) => Ok(None),
            Err(error) => Err(remote_file_error(error)),
        }
    }

    /// 递归创建 workspace 内的目录。
    pub async fn create_directory(&self, path: String, cwd: Option<String>) -> Result<()> {
        let request = self.path_request(path, cwd)?;
        let reply = self
            .client
            .request(RemoteRequest::CreateDirectory(request), &[])
            .await
            .map_err(remote_file_error)?;
        expect_ack(reply).map_err(remote_file_error)
    }

    /// 原子写入远端 workspace 中的原始字节。
    pub async fn write_bytes_atomic(
        &self,
        path: String,
        cwd: Option<String>,
        body: &[u8],
    ) -> Result<()> {
        let path = self.path_request(path, cwd)?;
        let reply = self
            .client
            .request(RemoteRequest::WriteAtomic(path), body)
            .await
            .map_err(remote_file_error)?;
        expect_ack(reply).map_err(remote_file_error)
    }

    /// 删除 workspace 内文件或目录，不跟随越界链接。
    pub async fn remove_path(
        &self,
        path: String,
        cwd: Option<String>,
        recursive: bool,
    ) -> Result<()> {
        let path = self.path_request(path, cwd)?;
        let reply = self
            .client
            .request(
                RemoteRequest::RemovePath(RemoteRemoveRequest {
                    workspace_id: path.workspace_id,
                    path: path.path,
                    recursive,
                }),
                &[],
            )
            .await
            .map_err(remote_file_error)?;
        expect_ack(reply).map_err(remote_file_error)
    }

    /// 在同一 workspace 内重命名路径。
    pub async fn rename_path(
        &self,
        source: String,
        target: String,
        cwd: Option<String>,
    ) -> Result<()> {
        let source = self.path_request(source, cwd.clone())?;
        let target = self.path_request(target, cwd)?;
        let reply = self
            .client
            .request(
                RemoteRequest::RenamePath(RemoteRenameRequest {
                    workspace_id: source.workspace_id,
                    source: source.path,
                    target: target.path,
                }),
                &[],
            )
            .await
            .map_err(remote_file_error)?;
        expect_ack(reply).map_err(remote_file_error)
    }

    /// 在同一 workspace 内复制文件或目录。
    pub async fn copy_path(
        &self,
        source: String,
        target: String,
        cwd: Option<String>,
        recursive: bool,
    ) -> Result<()> {
        let source = self.path_request(source, cwd.clone())?;
        let target = self.path_request(target, cwd)?;
        let reply = self
            .client
            .request(
                RemoteRequest::CopyPath(RemoteCopyRequest {
                    workspace_id: source.workspace_id,
                    source: source.path,
                    target: target.path,
                    recursive,
                }),
                &[],
            )
            .await
            .map_err(remote_file_error)?;
        expect_ack(reply).map_err(remote_file_error)
    }
}

/// 在本地 core 中选择、解码并截断远端 workspace 根目录的项目说明文档。
pub async fn load_remote_workspace_instructions(
    backend: &RemoteWorkspaceFileBackend,
    max_bytes: usize,
    fallback_filenames: &[String],
) -> Result<WorkspaceInstructions> {
    if max_bytes == 0 {
        return Ok(WorkspaceInstructions {
            documents: Vec::new(),
        });
    }
    let mut seen = std::collections::HashSet::new();
    let candidates = DEFAULT_INSTRUCTION_FILENAMES
        .iter()
        .map(|name| (*name).to_string())
        .chain(
            fallback_filenames
                .iter()
                .map(|name| name.trim())
                .filter(|name| !name.is_empty())
                .map(ToOwned::to_owned),
        )
        .filter(|name| seen.insert(name.clone()));
    for path in candidates {
        let Some(stat) = backend.stat_optional(path.clone(), None).await? else {
            continue;
        };
        if !stat.is_file {
            continue;
        }
        let bytes = backend
            .read_bytes(WorkspaceFileReadBytesRequest {
                path: path.clone(),
                cwd: None,
                max_bytes: pl_protocol::remote::REMOTE_MAX_BODY_BYTES,
            })
            .await?;
        let take = bytes.len().min(max_bytes);
        return Ok(WorkspaceInstructions {
            documents: vec![WorkspaceInstructionDocument {
                path: PathBuf::from(backend.canonical_path()).join(&path),
                content: String::from_utf8_lossy(&bytes[..take]).into_owned(),
                bytes: take,
            }],
        });
    }
    Ok(WorkspaceInstructions {
        documents: Vec::new(),
    })
}

impl WorkspaceFileBackend for RemoteWorkspaceFileBackend {
    async fn default_cwd(&self) -> Result<String> {
        Ok(".".to_string())
    }

    async fn stat(&self, request: WorkspaceFileStatRequest) -> Result<WorkspaceFileStat> {
        self.stat_optional(request.path.clone(), request.cwd)
            .await?
            .ok_or_else(|| tool_error("file", format!("path not found: {}", request.path)))
    }

    async fn read_text(&self, request: WorkspaceFileReadRequest) -> Result<String> {
        let path = self.path_request(request.path, request.cwd)?;
        let reply = self
            .client
            .request(
                RemoteRequest::ReadBytes(RemoteReadRequest {
                    workspace_id: path.workspace_id,
                    path: path.path,
                    max_bytes: pl_protocol::remote::REMOTE_MAX_BODY_BYTES,
                }),
                &[],
            )
            .await
            .map_err(remote_file_error)?;
        if !matches!(reply.response, RemoteResponse::Bytes) {
            return Err(tool_error(
                "read_file",
                format!("unexpected remote read response: {:?}", reply.response),
            ));
        }
        String::from_utf8(reply.body)
            .map_err(|error| tool_error("read_file", format!("remote file is not UTF-8: {error}")))
    }

    async fn read_bytes(&self, request: WorkspaceFileReadBytesRequest) -> Result<Vec<u8>> {
        let path = self.path_request(request.path, request.cwd)?;
        let reply = self
            .client
            .request(
                RemoteRequest::ReadBytes(RemoteReadRequest {
                    workspace_id: path.workspace_id,
                    path: path.path,
                    max_bytes: request.max_bytes,
                }),
                &[],
            )
            .await
            .map_err(remote_file_error)?;
        match reply.response {
            RemoteResponse::Bytes => Ok(reply.body),
            response => Err(tool_error(
                "view_image",
                format!("unexpected remote read response: {response:?}"),
            )),
        }
    }

    async fn write_text(&self, request: WorkspaceFileWriteRequest) -> Result<()> {
        self.write_bytes_atomic(request.path, request.cwd, request.content.as_bytes())
            .await
    }

    async fn remove_file(&self, request: WorkspaceFileRemoveRequest) -> Result<()> {
        self.remove_path(request.path, request.cwd, false).await
    }

    async fn list(&self, request: WorkspaceFileListRequest) -> Result<WorkspaceFileListResult> {
        let root = resolve_remote_path(request.path, request.cwd)?;
        let mut queue = VecDeque::from([root.clone()]);
        let mut files = Vec::new();
        while let Some(path) = queue.pop_front() {
            let listing = self.list_directory_entries(path).await?;
            for entry in listing.entries {
                if entry.is_symlink {
                    continue;
                }
                if entry.is_directory {
                    if matches!(entry.name.as_str(), ".git" | "target" | "node_modules") {
                        continue;
                    }
                    if request.include_dirs && matches_pattern(&entry.path, Some(&request.glob)) {
                        files.push(format!("{}/", entry.path));
                    }
                    queue.push_back(entry.path);
                } else if matches_pattern(&entry.path, Some(&request.glob)) {
                    files.push(entry.path);
                }
                if files.len() > request.max_files {
                    files.sort();
                    files.truncate(request.max_files);
                    return Ok(WorkspaceFileListResult {
                        files,
                        truncated: true,
                    });
                }
            }
        }
        files.sort();
        Ok(WorkspaceFileListResult {
            files,
            truncated: false,
        })
    }
}

fn resolve_remote_path(path: String, cwd: Option<String>) -> Result<String> {
    let mut components = Vec::new();
    for source in cwd
        .filter(|cwd| !cwd.trim().is_empty() && cwd != ".")
        .into_iter()
        .chain(std::iter::once(path))
    {
        if source.starts_with('/') || source.contains('\\') {
            return Err(tool_error(
                "file",
                "remote workspace paths must be relative POSIX paths",
            ));
        }
        for component in source.split('/') {
            match component {
                "" | "." => {}
                ".." => {
                    return Err(tool_error("file", "remote path must not escape workspace"));
                }
                value => components.push(value.to_string()),
            }
        }
    }
    Ok(if components.is_empty() {
        ".".to_string()
    } else {
        components.join("/")
    })
}

fn remote_file_error(error: RemoteClientError) -> pl_protocol::PureError {
    tool_error("file", error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_paths_are_posix_relative_and_confined() {
        assert_eq!(
            resolve_remote_path("lib.rs".to_string(), Some("src".to_string()))
                .expect("remote path"),
            "src/lib.rs"
        );
        assert!(resolve_remote_path("../secret".to_string(), None).is_err());
        assert!(resolve_remote_path("C:\\secret".to_string(), None).is_err());
    }
}
