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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use pretty_assertions::assert_eq;

    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("pure-workspace-{name}-{stamp}"))
    }

    #[test]
    fn rejects_missing_directory() {
        let path = Path::new("/nonexistent/dir/abc123");
        let result = load_workspace_instruction_documents(path, path, 4096, &[]);

        assert!(result.is_err());
    }

    #[test]
    fn returns_empty_for_missing_agents_md() {
        let dir = temp_dir("missing-agents");
        fs::create_dir_all(&dir).unwrap();

        let result = load_workspace_instruction_documents(&dir, &dir, 4096, &[]).unwrap();

        assert_eq!(result.content(), "");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_agents_md_content() {
        let dir = temp_dir("with-agents");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Agents.md"), "# Test Project\nRules here").unwrap();

        let result = load_workspace_instruction_documents(&dir, &dir, 4096, &[]).unwrap();

        assert_eq!(result.content(), "# Test Project\nRules here");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_override_before_agents_md() {
        let dir = temp_dir("with-uppercase-agents");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("AGENTS.override.md"), "# Override").unwrap();
        fs::write(dir.join("AGENTS.md"), "# Upper").unwrap();

        let result = load_workspace_instruction_documents(&dir, &dir, 4096, &[]).unwrap();

        assert_eq!(result.content(), "# Override");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_root_to_current_dir_chain() {
        let dir = temp_dir("chain");
        let child = dir.join("code").join("crate");
        fs::create_dir_all(&child).unwrap();
        fs::write(dir.join("AGENTS.md"), "# Root").unwrap();
        fs::write(child.join("AGENTS.md"), "# Crate").unwrap();

        let result = load_workspace_instruction_documents(&dir, &child, 4096, &[]).unwrap();

        assert_eq!(result.content(), "# Root\n\n# Crate");
        assert_eq!(
            result
                .documents
                .iter()
                .map(|document| document.path.file_name().unwrap().to_string_lossy())
                .collect::<Vec<_>>(),
            vec!["AGENTS.md", "AGENTS.md"]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn uses_fallback_filenames_after_defaults() {
        let dir = temp_dir("fallback");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("PURE.md"), "# Pure").unwrap();

        let result =
            load_workspace_instruction_documents(&dir, &dir, 4096, &["PURE.md".to_string()])
                .unwrap();

        assert_eq!(result.content(), "# Pure");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn truncates_by_total_byte_limit_and_records_sources() {
        let dir = temp_dir("truncate");
        let child = dir.join("crate");
        fs::create_dir_all(&child).unwrap();
        fs::write(dir.join("AGENTS.md"), "abcdef").unwrap();
        fs::write(child.join("AGENTS.md"), "ghijkl").unwrap();

        let result = load_workspace_instruction_documents(&dir, &child, 8, &[]).unwrap();

        assert_eq!(result.content(), "abcdef\n\ngh");
        assert_eq!(
            result
                .documents
                .iter()
                .map(|document| document.bytes)
                .collect::<Vec<_>>(),
            vec![6, 2]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn falls_back_to_root_when_current_dir_is_outside_workspace() {
        let dir = temp_dir("outside");
        let outside = temp_dir("outside-other");
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(dir.join("AGENTS.md"), "# Root").unwrap();
        fs::write(outside.join("AGENTS.md"), "# Outside").unwrap();

        let result = load_workspace_instruction_documents(&dir, &outside, 4096, &[]).unwrap();

        assert_eq!(result.content(), "# Root");
        fs::remove_dir_all(dir).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn resolves_git_workspace_root_from_child_directory() {
        let dir = temp_dir("git-root");
        let child = dir.join("code").join("crate");
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();
        fs::create_dir_all(&child).unwrap();

        let result = resolve_workspace_root(&child).unwrap();

        assert_eq!(result, fs::canonicalize(&dir).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ignores_an_empty_git_directory_in_an_ancestor() {
        let dir = temp_dir("invalid-git-root");
        let child = dir.join("project");
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::create_dir_all(&child).unwrap();

        let result = resolve_workspace_root(&child).unwrap();

        assert_eq!(result, fs::canonicalize(&child).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ignores_a_dangling_gitdir_pointer_in_an_ancestor() {
        let dir = temp_dir("dangling-gitdir-root");
        let child = dir.join("project");
        fs::create_dir_all(&child).unwrap();
        fs::write(dir.join(".git"), "gitdir: missing-metadata\n").unwrap();

        let result = resolve_workspace_root(&child).unwrap();

        assert_eq!(result, fs::canonicalize(&child).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }
}
