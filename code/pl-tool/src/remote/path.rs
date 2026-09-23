//! Host-independent POSIX path handling for remote workspaces.

use std::path::Path;

/// 远端绝对路径不满足跨端 POSIX 契约。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RemotePathError {
    #[error("remote path must not be empty")]
    Empty,
    #[error("remote path must be absolute: {path}")]
    Relative { path: String },
    #[error("remote path must not contain `..`: {path}")]
    ParentComponent { path: String },
}

/// 把跨端边界的远端绝对路径规范化为 canonical POSIX 字符串。
///
/// 宿主形态（含 Windows 路径分隔符）在提交给远端 helper 之前统一为 POSIX，重复分隔符
/// 与 `.` 被折叠，`..` 则被拒绝。workspace 打开、远端 Git、durable lease 与身份比较
/// 必须共用该结果，不能各自解释宿主 `Path`。
pub fn normalize_remote_absolute_path(path: &str) -> Result<String, RemotePathError> {
    if path.is_empty() {
        return Err(RemotePathError::Empty);
    }
    let normalized = path.replace('\\', "/");
    if !normalized.starts_with('/') {
        return Err(RemotePathError::Relative {
            path: path.to_string(),
        });
    }
    let components =
        normalized_components(&normalized).map_err(|()| RemotePathError::ParentComponent {
            path: path.to_string(),
        })?;
    Ok(if components.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", components.join("/"))
    })
}

pub(crate) fn relative_workspace_path(root: &str, path: &Path) -> Result<String, ()> {
    let root = normalize_remote_absolute_path(root).map_err(|_| ())?;
    let path = path.to_string_lossy().replace('\\', "/");
    let relative = if path.starts_with('/') {
        let path = normalize_remote_absolute_path(&path).map_err(|_| ())?;
        if path == root {
            String::new()
        } else if root == "/" {
            path.trim_start_matches('/').to_string()
        } else {
            path.strip_prefix(&format!("{root}/"))
                .ok_or(())?
                .to_string()
        }
    } else {
        path
    };
    normalize_relative(&relative)
}

fn normalize_relative(path: &str) -> Result<String, ()> {
    let components = normalized_components(path)?;
    Ok(if components.is_empty() {
        ".".to_string()
    } else {
        components.join("/")
    })
}

fn normalized_components(path: &str) -> Result<Vec<&str>, ()> {
    path.split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .map(|component| {
            if component == ".." {
                Err(())
            } else {
                Ok(component)
            }
        })
        .collect()
}
