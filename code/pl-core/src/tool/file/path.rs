use std::path::{Path, PathBuf};

use pl_protocol::PureError;

use crate::tool::ToolPathPolicy;

#[derive(Debug, Clone)]
pub struct WorkspacePaths {
    policy: ToolPathPolicy,
}

impl WorkspacePaths {
    pub async fn new(root: PathBuf, allow_workspace_escape: bool) -> Result<Self, PureError> {
        Ok(Self {
            policy: ToolPathPolicy::new(root, allow_workspace_escape, "file")?,
        })
    }

    pub async fn resolve_existing(&self, path: &str) -> Result<PathBuf, PureError> {
        self.policy.resolve_existing(path)
    }

    pub async fn resolve_for_write(&self, path: &str) -> Result<PathBuf, PureError> {
        self.policy.resolve_for_write(path)
    }

    pub async fn reject_symlink_write(&self, path: &Path) -> Result<(), PureError> {
        self.reject_symlink(path, "write").await
    }

    /// 拒绝通过符号链接读取（在 canonicalize 前检查 candidate，对齐 codex）。
    ///
    /// 接受相对路径（工具输入的 `path`），须在 `resolve_existing` 之前调用，
    /// 否则符号链接会被 canonicalize 解析到 target 而漏检。
    pub async fn reject_symlink_read(&self, path: &str) -> Result<(), PureError> {
        let candidate = self.policy.candidate(Path::new(path));
        self.reject_symlink(&candidate, "read").await
    }

    async fn reject_symlink(&self, path: &Path, refusing_verb: &str) -> Result<(), PureError> {
        match tokio::fs::symlink_metadata(path).await {
            Ok(metadata) if metadata.file_type().is_symlink() => Err(self.error(format!(
                "refusing to {refusing_verb} through symbolic link '{}'",
                path.display()
            ))),
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(self.error(format!("failed to inspect '{}': {error}", path.display())))
            }
        }
    }

    pub fn display_relative(&self, path: &Path) -> String {
        self.policy.display_relative(path)
    }

    fn error(&self, error: String) -> PureError {
        PureError::ToolExecutionFailed {
            tool: "file".to_string(),
            error,
        }
    }
}

pub fn matches_pattern(path: &str, pattern: Option<&str>) -> bool {
    let Some(pattern) = pattern.filter(|pattern| !pattern.is_empty()) else {
        return true;
    };
    if matches_pattern_once(path, pattern) {
        return true;
    }
    if !pattern.contains("**/") {
        return false;
    }

    let mut variants = vec![pattern.to_string()];
    let mut index = 0;
    while index < variants.len() {
        if let Some(offset) = variants[index].find("**/") {
            let mut variant = variants[index].clone();
            variant.replace_range(offset..offset + 3, "");
            if !variants.contains(&variant) && matches_pattern_once(path, &variant) {
                return true;
            }
            if !variants.contains(&variant) {
                variants.push(variant);
            }
        }
        index += 1;
    }
    false
}

fn matches_pattern_once(path: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return path.contains(pattern);
    }

    let mut remainder = path;
    let mut first = true;
    for part in pattern.split('*').filter(|part| !part.is_empty()) {
        if first && !pattern.starts_with('*') {
            let Some(stripped) = remainder.strip_prefix(part) else {
                return false;
            };
            remainder = stripped;
        } else {
            let Some(index) = remainder.find(part) else {
                return false;
            };
            remainder = &remainder[index + part.len()..];
        }
        first = false;
    }

    pattern.ends_with('*') || remainder.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "pure-{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[tokio::test]
    async fn workspace_write_rejects_paths_outside_workspace() {
        let workspace = unique_temp_dir("workspace-boundary");
        let outside = unique_temp_dir("workspace-outside");
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();
        tokio::fs::write(outside.join("secret.txt"), "no")
            .await
            .unwrap();
        let paths = WorkspacePaths::new(workspace.clone(), false).await.unwrap();

        let result = paths
            .resolve_existing(outside.join("secret.txt").to_str().unwrap())
            .await;

        assert!(result.is_err());
        let _ = tokio::fs::remove_dir_all(workspace).await;
        let _ = tokio::fs::remove_dir_all(outside).await;
    }

    #[tokio::test]
    async fn full_access_allows_paths_outside_workspace() {
        let workspace = unique_temp_dir("full-access-workspace");
        let outside = unique_temp_dir("full-access-outside");
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();
        let outside_file = outside.join("allowed.txt");
        tokio::fs::write(&outside_file, "yes").await.unwrap();
        let paths = WorkspacePaths::new(workspace.clone(), true).await.unwrap();

        let resolved = paths
            .resolve_existing(outside_file.to_str().unwrap())
            .await
            .unwrap();

        assert_eq!(
            resolved,
            tokio::fs::canonicalize(&outside_file).await.unwrap()
        );
        let _ = tokio::fs::remove_dir_all(workspace).await;
        let _ = tokio::fs::remove_dir_all(outside).await;
    }

    #[tokio::test]
    async fn relative_write_target_is_resolved_from_workspace() {
        let workspace = unique_temp_dir("relative-write");
        tokio::fs::create_dir_all(&workspace).await.unwrap();
        let paths = WorkspacePaths::new(workspace.clone(), false).await.unwrap();

        let resolved = paths.resolve_for_write("src/new.rs").await.unwrap();

        assert_eq!(
            resolved,
            tokio::fs::canonicalize(&workspace)
                .await
                .unwrap()
                .join("src/new.rs")
        );
        let _ = tokio::fs::remove_dir_all(workspace).await;
    }
}
