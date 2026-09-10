//! Host-supplied workspace and file mutation boundaries.
use super::path_policy::path_is_inside_workspace;
use std::path::PathBuf;

/// Agent workspace 是否允许宿主权限策略访问 root 之外的路径。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceBoundary {
    #[default]
    Confined,
    HostPermitted,
}

impl WorkspaceBoundary {
    /// Whether the assigned boundary permits explicitly approved host paths.
    pub fn allows_host_paths(self) -> bool {
        matches!(self, Self::HostPermitted)
    }
}

/// Agent workspace 的修改能力。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceMutability {
    ReadOnly,
    #[default]
    ReadWrite,
}

/// 单个 Agent 的 canonical workspace 边界。
///
/// 宿主负责根据 durable owner 构造该值；所有内置路径工具、命令 cwd、Git、LSP 与项目
/// skills 必须消费同一个 root。`Confined` 不会被 turn 的 `full-access` 权限放宽。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentWorkspace {
    root: PathBuf,
    project_root: PathBuf,
    boundary: WorkspaceBoundary,
    mutability: WorkspaceMutability,
    project_writable_paths: Option<Vec<PathBuf>>,
}

impl AgentWorkspace {
    pub fn local(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            project_root: root.clone(),
            root,
            boundary: WorkspaceBoundary::HostPermitted,
            mutability: WorkspaceMutability::ReadWrite,
            project_writable_paths: None,
        }
    }

    /// 构造仅由 Pure 内置文件 mutation 工具实施目录写策略的 workspace。
    ///
    /// `None` 表示整个项目可写，`Some([])` 表示项目内只读。该策略不约束 shell、Git 或 MCP。
    pub fn directory(
        project_root: impl Into<PathBuf>,
        writable_paths: Option<Vec<PathBuf>>,
    ) -> Self {
        let project_root = project_root.into();
        Self {
            root: project_root.clone(),
            project_root,
            boundary: WorkspaceBoundary::HostPermitted,
            mutability: WorkspaceMutability::ReadWrite,
            project_writable_paths: writable_paths,
        }
    }

    /// 构造物理隔离的 Git worktree workspace。
    pub fn worktree(project_root: impl Into<PathBuf>, root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            project_root: project_root.into(),
            boundary: WorkspaceBoundary::Confined,
            mutability: WorkspaceMutability::ReadWrite,
            project_writable_paths: None,
        }
    }

    pub fn confined(root: impl Into<PathBuf>, mutability: WorkspaceMutability) -> Self {
        let root = root.into();
        Self {
            project_root: root.clone(),
            root,
            boundary: WorkspaceBoundary::Confined,
            mutability,
            project_writable_paths: None,
        }
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub fn boundary(&self) -> WorkspaceBoundary {
        self.boundary
    }

    pub fn mutability(&self) -> WorkspaceMutability {
        self.mutability
    }

    pub fn project_root(&self) -> &std::path::Path {
        &self.project_root
    }

    pub fn project_writable_paths(&self) -> Option<&[PathBuf]> {
        self.project_writable_paths.as_deref()
    }

    /// Validates a resolved write target against the assigned workspace boundary.
    pub fn ensure_path_writable(&self, path: &std::path::Path) -> pl_protocol::Result<()> {
        if self.mutability == WorkspaceMutability::ReadOnly {
            return Err(pl_protocol::PureError::ToolExecutionFailed {
                tool: "workspace".to_string(),
                error: "agent workspace is read-only".to_string(),
            });
        }
        let Some(writable_paths) = &self.project_writable_paths else {
            return Ok(());
        };
        let project_root = dunce::canonicalize(&self.project_root).map_err(|error| {
            pl_protocol::PureError::ToolExecutionFailed {
                tool: "workspace".to_string(),
                error: format!("failed to resolve Agent project root: {error}"),
            }
        })?;
        if !path_is_inside_workspace(path, &project_root) {
            return Ok(());
        }
        let is_allowed = writable_paths.iter().any(|allowed| {
            allowed
                .strip_prefix(&self.project_root)
                .is_ok_and(|relative| path_is_inside_workspace(path, &project_root.join(relative)))
        });
        if is_allowed {
            return Ok(());
        }
        Err(pl_protocol::PureError::ToolExecutionFailed {
            tool: "workspace".to_string(),
            error: format!(
                "project path '{}' is outside the directory Agent writablePaths boundary",
                path.display()
            ),
        })
    }

    /// Validates a backend-relative write target against the assigned workspace boundary.
    pub fn ensure_relative_path_writable(
        &self,
        cwd: Option<&str>,
        path: &str,
    ) -> pl_protocol::Result<()> {
        if self.mutability == WorkspaceMutability::ReadOnly {
            return Err(pl_protocol::PureError::ToolExecutionFailed {
                tool: "workspace".to_string(),
                error: "agent workspace is read-only".to_string(),
            });
        }
        let Some(writable_paths) = &self.project_writable_paths else {
            return Ok(());
        };
        let relative = normalize_workspace_relative_path(cwd, path)?;
        let project_root = comparable_policy_path(&self.project_root);
        let target = if relative == "." {
            project_root.clone()
        } else if project_root == "/" {
            format!("/{relative}")
        } else {
            format!("{project_root}/{relative}")
        };
        let allowed = writable_paths.iter().any(|path| {
            let path = comparable_policy_path(path);
            path == target
                || target
                    .strip_prefix(&path)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        });
        if allowed {
            return Ok(());
        }
        Err(pl_protocol::PureError::ToolExecutionFailed {
            tool: "workspace".to_string(),
            error: format!(
                "project path '{relative}' is outside the directory Agent writablePaths boundary"
            ),
        })
    }
}

fn normalize_workspace_relative_path(cwd: Option<&str>, path: &str) -> pl_protocol::Result<String> {
    let mut parts = Vec::new();
    for source in cwd
        .filter(|cwd| !cwd.trim().is_empty() && *cwd != ".")
        .into_iter()
        .chain(std::iter::once(path))
    {
        if source.starts_with('/') || source.contains('\\') {
            return Err(pl_protocol::PureError::ToolExecutionFailed {
                tool: "workspace".to_string(),
                error: "workspace mutation paths must be project-relative".to_string(),
            });
        }
        for component in source.split('/') {
            match component {
                "" | "." => {}
                ".." => {
                    return Err(pl_protocol::PureError::ToolExecutionFailed {
                        tool: "workspace".to_string(),
                        error: "workspace mutation path must not escape the project".to_string(),
                    });
                }
                value => parts.push(value),
            }
        }
    }
    Ok(if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    })
}

fn comparable_policy_path(path: &std::path::Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if value == "/" {
        value
    } else {
        value.trim_end_matches('/').to_string()
    }
}
