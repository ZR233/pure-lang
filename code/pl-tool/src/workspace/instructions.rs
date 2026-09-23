use std::collections::HashSet;
use std::path::{Path, PathBuf};

use pl_protocol::{PureError, Result};

const DEFAULT_PROJECT_DOC_FILENAMES: &[&str] = &["AGENTS.override.md", "AGENTS.md", "Agents.md"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInstructionDocument {
    pub path: PathBuf,
    pub content: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceInstructions {
    pub documents: Vec<WorkspaceInstructionDocument>,
}

impl WorkspaceInstructions {
    pub fn content(&self) -> String {
        self.documents
            .iter()
            .map(|document| document.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// 解析 agent host 的有效工作区根目录。
///
/// 如果输入目录位于 Git 仓库中，返回最近的 Git 仓库根；否则返回输入目录的
/// 规范化路径。这样从子 crate 打开的项目仍能让工具看到完整仓库。
pub fn resolve_workspace_root(project_dir: &Path) -> Result<PathBuf> {
    let canonical = std::fs::canonicalize(project_dir).map_err(|error| {
        PureError::ConfigError(format!(
            "workspace directory not found: {} ({error})",
            project_dir.display()
        ))
    })?;
    if !canonical.is_dir() {
        return Err(PureError::ConfigError(format!(
            "workspace path is not a directory: {}",
            canonical.display()
        )));
    }

    let mut cursor = Some(canonical.as_path());
    while let Some(dir) = cursor {
        if is_git_worktree_marker(&dir.join(".git")) {
            return Ok(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    Ok(canonical)
}

fn is_git_worktree_marker(path: &Path) -> bool {
    if path.is_dir() {
        return path.join("HEAD").is_file();
    }
    if !path.is_file() {
        return false;
    }
    let Some(git_dir) = std::fs::read_to_string(path)
        .ok()
        .and_then(|content| content.lines().next().map(str::trim).map(str::to_string))
        .and_then(|line| {
            line.strip_prefix("gitdir:")
                .map(str::trim)
                .map(str::to_string)
        })
        .filter(|git_dir| !git_dir.is_empty())
    else {
        return false;
    };
    let git_dir = Path::new(&git_dir);
    let git_dir = if git_dir.is_absolute() {
        git_dir.to_path_buf()
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .join(git_dir)
    };
    git_dir.join("HEAD").is_file()
}

/// 按 Codex 风格从 workspace root 到 current dir 链式读取项目记忆。
///
/// 每一层目录按候选文件优先级选择一个文档：`AGENTS.override.md`、
/// `AGENTS.md`、`Agents.md`，再尝试配置提供的 fallback 文件名。
/// 内容按总字节上限截断，并保留实际注入的 source path。
pub fn load_workspace_instruction_documents(
    workspace_dir: &Path,
    current_dir: &Path,
    max_bytes: usize,
    fallback_filenames: &[String],
) -> Result<WorkspaceInstructions> {
    if !workspace_dir.is_dir() {
        return Err(PureError::ConfigError(format!(
            "workspace directory not found: {}",
            workspace_dir.display()
        )));
    }
    if max_bytes == 0 {
        return Ok(WorkspaceInstructions {
            documents: Vec::new(),
        });
    }

    let workspace_root = std::fs::canonicalize(workspace_dir).map_err(|error| {
        PureError::ConfigError(format!(
            "workspace directory not found: {} ({error})",
            workspace_dir.display()
        ))
    })?;
    let current = std::fs::canonicalize(current_dir).unwrap_or_else(|_| workspace_root.clone());
    let current = if current.starts_with(&workspace_root) && current.is_dir() {
        current
    } else {
        workspace_root.clone()
    };

    let candidates = candidate_filenames(fallback_filenames);
    let mut remaining = max_bytes;
    let mut documents = Vec::new();
    for directory in root_to_current_dirs(&workspace_root, &current) {
        let Some(path) = first_existing_instruction_file(&directory, &candidates) else {
            continue;
        };
        let bytes = std::fs::read(&path).map_err(|error| {
            PureError::ConfigError(format!(
                "failed to read workspace instructions {}: {error}",
                path.display()
            ))
        })?;
        let take = bytes.len().min(remaining);
        if take == 0 {
            break;
        }
        let content = String::from_utf8_lossy(&bytes[..take]).to_string();
        documents.push(WorkspaceInstructionDocument {
            path,
            content,
            bytes: take,
        });
        remaining -= take;
        if remaining == 0 {
            break;
        }
    }
    Ok(WorkspaceInstructions { documents })
}

fn candidate_filenames(fallback_filenames: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    DEFAULT_PROJECT_DOC_FILENAMES
        .iter()
        .map(|name| (*name).to_string())
        .chain(
            fallback_filenames
                .iter()
                .map(|name| name.trim())
                .filter(|name| !name.is_empty())
                .map(ToOwned::to_owned),
        )
        .filter(|name| seen.insert(name.clone()))
        .collect()
}

fn root_to_current_dirs(root: &Path, current: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut cursor = Some(current);
    while let Some(directory) = cursor {
        dirs.push(directory.to_path_buf());
        if directory == root {
            break;
        }
        cursor = directory.parent();
    }
    dirs.reverse();
    dirs
}

fn first_existing_instruction_file(directory: &Path, candidates: &[String]) -> Option<PathBuf> {
    candidates
        .iter()
        .map(|file_name| directory.join(file_name))
        .find(|path| path.is_file())
}
