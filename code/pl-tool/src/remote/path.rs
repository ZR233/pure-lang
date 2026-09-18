//! Host-independent POSIX path handling for remote workspaces.

use std::path::Path;

/// 跨端边界的远端路径文本归一化。
///
/// 宿主形态（含 Windows 路径分隔符）在提交给远端 helper 之前必须统一为 POSIX 字符串，
/// 否则 helper 端 canonicalize 会失败或把同一目录当成不同目标。这是远端路径的唯一文本
/// 归一化表达，供 workspace 打开、相对路径计算与越界拒绝共用。
pub(crate) fn normalize_remote_path_text(path: &str) -> String {
    path.replace('\\', "/")
}

pub(crate) fn relative_workspace_path(root: &str, path: &Path) -> Result<String, ()> {
    let root = normalize_root(root)?;
    let path = normalize_remote_path_text(&path.to_string_lossy());
    let relative = if path.starts_with('/') {
        if path.trim_end_matches('/') == root {
            ""
        } else if root == "/" {
            path.trim_start_matches('/')
        } else {
            path.strip_prefix(&format!("{root}/")).ok_or(())?
        }
    } else {
        &path
    };
    normalize_relative(relative)
}

fn normalize_root(root: &str) -> Result<String, ()> {
    let root = normalize_remote_path_text(root);
    if !root.starts_with('/') {
        return Err(());
    }
    let components = normalized_components(&root)?;
    Ok(if components.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", components.join("/"))
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_paths_are_independent_from_host_path_semantics() {
        assert_eq!(
            relative_workspace_path("/srv/project", Path::new("/srv/project/src/lib.rs"))
                .expect("child"),
            "src/lib.rs"
        );
        assert_eq!(
            relative_workspace_path("/srv/project", Path::new(r"\srv\project\src\lib.rs"))
                .expect("backslash child"),
            "src/lib.rs"
        );
        assert!(relative_workspace_path("/srv/project", Path::new("/srv/project-other")).is_err());
        assert!(relative_workspace_path("/srv/project", Path::new("../outside")).is_err());
    }

    /// 跨端入口归一化：宿主形态与 POSIX 形态的同一远端目标必须归一化成同一个字符串，
    /// 从而在 `SshManager::open_workspace_host` 的缓存键与 helper 打开参数上只对应一个
    /// 远端 workspace。归一化函数是纯字符串操作，可在无 SSH 连接时确定性验证；连接建立
    /// 后的重复打开复用由 helper 的 canonical workspace id 保证。
    #[test]
    fn host_shaped_and_posix_paths_normalize_to_one_target() {
        let posix = "/srv/project/.anywork/worktrees/thread-1/session";
        assert_eq!(normalize_remote_path_text(posix), posix);
        assert_eq!(
            normalize_remote_path_text(r"\srv\project\.anywork\worktrees\thread-1\session"),
            posix
        );
        // 与现场日志同形：仓库根 POSIX、分隔符混用的 Windows join 结果。
        assert_eq!(
            normalize_remote_path_text(r"/srv/project\.anywork/worktrees\thread-1\session"),
            posix
        );
    }
}
