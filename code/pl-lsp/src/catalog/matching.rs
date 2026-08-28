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

    fn temp_root(name: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("pure-lsp-detect-{name}-{stamp}"));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn empty_rules_match_every_workspace() {
        let root = temp_root("empty");

        assert!(workspace_matches(&[], &root));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn exact_rule_matches_existing_file_only() {
        let root = temp_root("exact");
        std::fs::write(root.join("pure.toml"), "schema = 1\n").unwrap();

        assert!(workspace_matches(&["pure.toml".to_string()], &root));
        assert!(!workspace_matches(&["Cargo.toml".to_string()], &root));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn glob_rule_matches_single_segment_entries() {
        let root = temp_root("glob");
        std::fs::write(root.join("package.json"), "{}\n").unwrap();

        assert!(workspace_matches(&["*.json".to_string()], &root));
        assert!(!workspace_matches(&["*.toml".to_string()], &root));
        assert!(workspace_matches(&["pack*.json".to_string()], &root));
        assert!(workspace_matches(&["*".to_string()], &root));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn glob_match_supports_prefix_suffix_and_inner_literals() {
        assert!(glob_match("*.pure", "hello.pure"));
        assert!(glob_match("*.pure", ".pure"));
        assert!(!glob_match("*.pure", "hello.rs"));
        assert!(glob_match("a*c*e", "abcde"));
        assert!(glob_match("a*c*e", "acbde"));
        assert!(!glob_match("a*c*e", "abde"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "exactly"));
    }
}
