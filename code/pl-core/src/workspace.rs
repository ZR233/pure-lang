use std::path::{Path, PathBuf};

use pl_protocol::{PureError, Result};

/// 解析 Studio 运行时的有效工作区根目录。
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
        if dir.join(".git").exists() {
            return Ok(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    Ok(canonical)
}

/// 读取工作区项目记忆。
///
/// 优先读取工作区根目录下的 `AGENTS.md`，兼容读取旧命名 `Agents.md`。
/// 当目录存在但文件缺失时返回空字符串；当目录不存在或读取失败时返回配置错误。
pub fn load_workspace_instructions(workspace_dir: &Path) -> Result<String> {
    if !workspace_dir.is_dir() {
        return Err(PureError::ConfigError(format!(
            "workspace directory not found: {}",
            workspace_dir.display()
        )));
    }

    for file_name in ["AGENTS.md", "Agents.md"] {
        let agents_file = workspace_dir.join(file_name);
        match std::fs::read_to_string(&agents_file) {
            Ok(content) => return Ok(content),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(PureError::ConfigError(format!(
                    "failed to read workspace instructions: {e}"
                )));
            }
        }
    }
    Ok(String::new())
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
        let result = load_workspace_instructions(Path::new("/nonexistent/dir/abc123"));

        assert!(result.is_err());
    }

    #[test]
    fn returns_empty_for_missing_agents_md() {
        let dir = temp_dir("missing-agents");
        fs::create_dir_all(&dir).unwrap();

        let result = load_workspace_instructions(&dir).unwrap();

        assert_eq!(result, "");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_agents_md_content() {
        let dir = temp_dir("with-agents");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Agents.md"), "# Test Project\nRules here").unwrap();

        let result = load_workspace_instructions(&dir).unwrap();

        assert_eq!(result, "# Test Project\nRules here");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_uppercase_agents_md_first() {
        let dir = temp_dir("with-uppercase-agents");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("AGENTS.md"), "# Upper").unwrap();

        let result = load_workspace_instructions(&dir).unwrap();

        assert_eq!(result, "# Upper");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn resolves_git_workspace_root_from_child_directory() {
        let dir = temp_dir("git-root");
        let child = dir.join("code").join("crate");
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::create_dir_all(&child).unwrap();

        let result = resolve_workspace_root(&child).unwrap();

        assert_eq!(result, fs::canonicalize(&dir).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }
}
