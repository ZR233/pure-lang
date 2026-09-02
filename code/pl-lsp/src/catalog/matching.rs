//! Workspace 检测规则的文件系统匹配。
//!
//! 规则是相对 workspace root 的文件名或 glob：不含 `*` 时直接检查
//! `root/<rule>` 是否存在；含 `*` 时只对 root 第一层条目做单段通配匹配。
//! 不支持 `**` 递归与字符类，保持检测语义廉价、可静态判断。

use std::path::Path;

/// 任一规则命中即视为该 workspace 适用；规则列表为空表示适用所有 workspace。
pub(crate) fn workspace_matches(rules: &[String], workspace_root: &Path) -> bool {
    rules.is_empty() || rules.iter().any(|rule| rule_matches(rule, workspace_root))
}

fn rule_matches(rule: &str, workspace_root: &Path) -> bool {
    if !rule.contains('*') {
        return workspace_root.join(rule).exists();
    }
    let Ok(entries) = std::fs::read_dir(workspace_root) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| glob_match(rule, name))
    })
}

/// `*` 通配匹配：`*` 匹配任意（含空）字符序列，其余字符精确比较。
pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    let parts = pattern.split('*').collect::<Vec<_>>();
    let Some((&first, middle)) = parts.split_first() else {
        return true;
    };
    let Some((&last, middle)) = middle.split_last() else {
        return first == text;
    };
    if text.len() < first.len() + last.len() || !text.starts_with(first) || !text.ends_with(last) {
        return false;
    }
    let mut cursor = &text[first.len()..text.len() - last.len()];
    for part in middle {
        if part.is_empty() {
            continue;
        }
        match cursor.find(part) {
            Some(index) => cursor = &cursor[index + part.len()..],
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_detection_honors_exact_and_single_segment_glob_rules() {
        let workspace = tempfile::tempdir().expect("temporary workspace");
        std::fs::write(workspace.path().join("package.json"), "{}\n")
            .expect("write detection fixture");

        assert!(workspace_matches(&[], workspace.path()));
        assert!(workspace_matches(
            &["package.json".to_string()],
            workspace.path()
        ));
        assert!(workspace_matches(
            &["pack*.json".to_string()],
            workspace.path()
        ));
        assert!(!workspace_matches(
            &["Cargo.toml".to_string()],
            workspace.path()
        ));
    }
}
