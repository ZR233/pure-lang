use std::path::Path;

use pl_protocol::{PureError, Result};

/// 读取工作区项目记忆。
///
/// 当前约定读取工作区根目录下的 `Agents.md`。当目录存在但文件缺失时返回空字符串；
/// 当目录不存在或读取失败时返回配置错误。
pub fn load_workspace_instructions(workspace_dir: &Path) -> Result<String> {
    let agents_file = workspace_dir.join("Agents.md");
    match std::fs::read_to_string(&agents_file) {
        Ok(content) => Ok(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if workspace_dir.is_dir() {
                Ok(String::new())
            } else {
                Err(PureError::ConfigError(format!(
                    "workspace directory not found: {}",
                    workspace_dir.display()
                )))
            }
        }
        Err(e) => Err(PureError::ConfigError(format!(
            "failed to read workspace instructions: {e}"
        ))),
    }
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
}
